// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The level of an input, as a meter shows it: a recording stream that asks the server for the
//! peak only (25 values a second, one channel), the way pavucontrol does it. It wakes the device
//! while it runs: on PipeWire, a stream that lets the device sleep gets nothing from an input no
//! app records from. It doesn't move with the default.

use std::cell::RefCell;
use std::rc::Rc;

use async_channel::Receiver;
use futures_lite::Stream;
use libpulse_binding as pulse;
use pulse::def::BufferAttr;
use pulse::sample::{Format, Spec};
use pulse::stream::{FlagSet, PeekResult, Stream as PaStream};

use super::{Error, Sound};
use crate::msg;

/// How many levels a second the meter reports.
const RATE: u32 = 25;

/// A running level meter on one input. Dropping it stops it.
///
/// The stream is shared with its read callback, which runs on libpulse's thread, yet it is an
/// `Rc<RefCell<…>>`, which isn't `Send`. That is sound only because every touch of the stream,
/// and of the `Rc` itself, happens with the main loop's lock held: libpulse runs its callbacks
/// under that lock, [`Meter::start`] makes the stream inside [`Sound::locked`], and `Drop` lets it
/// go there too. So never two threads at once. Keep it that way: touching the stream (or cloning
/// or dropping the `Rc`) anywhere else is a data race.
pub struct Meter {
    // Before `sound`: the stream is let go under the lock of the main loop `sound` holds.
    stream: Option<Rc<RefCell<PaStream>>>,
    sound: Sound,
    levels: Receiver<f64>,
}

impl std::fmt::Debug for Meter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Meter").finish_non_exhaustive()
    }
}

impl Meter {
    /// Starts measuring the input with the server's name `input`.
    ///
    /// # Errors
    ///
    /// [`Error::Failed`] if the server refuses the recording stream.
    pub fn start(sound: &Sound, input: &str) -> Result<Self, Error> {
        let spec = Spec {
            format: Format::F32le,
            rate: RATE,
            channels: 1,
        };
        let refused = || Error::Failed(msg!("the level meter can't be started").into());
        let (send, levels) = async_channel::bounded(4);
        let stream = sound.locked(|context| {
            let stream = PaStream::new(context, "Level meter", &spec, None).ok_or_else(refused)?;
            let stream = Rc::new(RefCell::new(stream));
            let reader = Rc::clone(&stream);
            stream
                .borrow_mut()
                .set_read_callback(Some(Box::new(move |_| {
                    let Ok(mut stream) = reader.try_borrow_mut() else {
                        return;
                    };
                    let level = match stream.peek() {
                        Ok(PeekResult::Data(data)) => Some(peak(data)),
                        Ok(PeekResult::Hole(_)) => None,
                        Ok(PeekResult::Empty) | Err(_) => return,
                    };
                    let _ = stream.discard();
                    if let Some(level) = level {
                        // A reader that is behind misses a value, never the newest.
                        let _ = send.try_send(level);
                    }
                })));
            let attr = BufferAttr {
                maxlength: u32::MAX,
                tlength: u32::MAX,
                prebuf: u32::MAX,
                minreq: u32::MAX,
                // One sample (4 bytes) per read: the peak of each 40 ms.
                fragsize: 4,
            };
            let connected = stream.borrow_mut().connect_record(
                Some(input),
                Some(&attr),
                FlagSet::PEAK_DETECT | FlagSet::ADJUST_LATENCY | FlagSet::DONT_MOVE,
            );
            if connected.is_err() {
                // The callback holds the stream; without it, both go.
                stream.borrow_mut().set_read_callback(None);
                return Err(refused());
            }
            Ok(stream)
        })?;
        Ok(Self {
            stream: Some(stream),
            sound: sound.clone(),
            levels,
        })
    }

    /// The levels as they come, from 0.0 (silence) to 1.0 (full scale).
    pub fn levels(&self) -> impl Stream<Item = f64> + Unpin + use<> {
        Box::pin(self.levels.clone())
    }
}

impl Drop for Meter {
    fn drop(&mut self) {
        let Some(stream) = self.stream.take() else {
            return;
        };
        self.sound.locked(move |_| {
            {
                let mut stream = stream.borrow_mut();
                // Dropping the callback ends its hold on the stream.
                stream.set_read_callback(None);
                let _ = stream.disconnect();
            }
            // The last reference goes here, with the lock held.
            drop(stream);
        });
    }
}

/// The loudest sample of `data` (native-endian 32-bit floats), clamped to 0.0 … 1.0.
fn peak(data: &[u8]) -> f64 {
    data.chunks_exact(4)
        .filter_map(|bytes| bytes.try_into().ok())
        .map(|bytes: [u8; 4]| f64::from(f32::from_le_bytes(bytes)).abs())
        .filter(|level| level.is_finite())
        .fold(0.0, f64::max)
        .clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::peak;

    #[test]
    fn the_peak_is_the_loudest_sample() {
        let data: Vec<u8> = [0.1f32, -0.7, 0.3]
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect();
        assert!((peak(&data) - 0.7).abs() < 1e-6);
        assert!(peak(&[]).abs() < f64::EPSILON);
        assert!((peak(&2.0f32.to_le_bytes()) - 1.0).abs() < f64::EPSILON);
        assert!(peak(&f32::NAN.to_le_bytes()).abs() < f64::EPSILON);
    }
}
