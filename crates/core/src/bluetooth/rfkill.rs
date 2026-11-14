// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The kernel's radio kill switch for Bluetooth, through `/dev/rfkill`.
//!
//! A *soft* block comes from software (airplane mode, `rfkill block bluetooth`, some laptops' Fn
//! key) and can be lifted by writing to `/dev/rfkill`; systemd's udev rules give the user of the
//! active session write access (the `uaccess` tag), so no root is needed. A *hard* block is a
//! physical switch that no software can lift.
//!
//! The device speaks in events of 8 bytes (`struct rfkill_event` in `linux/rfkill.h`): the
//! radio's index (`u32`, native byte order), its type, the operation, and the soft and hard
//! flags. Reading it gives an event per radio first, then one per change. Newer kernels have a
//! longer event, but hand out the first 8 bytes to a reader that asks for 8.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::AsFd;
use std::path::Path;

use async_io::Async;

/// The device file.
pub const DEVICE: &str = "/dev/rfkill";

/// The size of an event.
const EVENT_SIZE: usize = 8;
/// `RFKILL_TYPE_BLUETOOTH`
const TYPE_BLUETOOTH: u8 = 2;
/// `RFKILL_TYPE_WLAN`, for the stand-ins' other radios.
#[cfg(any(test, feature = "test-support"))]
const TYPE_WLAN: u8 = 1;
/// `RFKILL_OP_ADD`: a radio and its state, when reading starts or a radio appears.
const OP_ADD: u8 = 0;
/// `RFKILL_OP_DEL`: a radio is gone.
const OP_DEL: u8 = 1;
/// `RFKILL_OP_CHANGE`: a radio's state changed.
const OP_CHANGE: u8 = 2;
/// `RFKILL_OP_CHANGE_ALL`: sets the state of every radio of a type.
const OP_CHANGE_ALL: u8 = 3;

/// Whether the Bluetooth radios are blocked: each flag is set if any radio has it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Block {
    /// Blocked by software; [`unblock`] lifts it.
    pub soft: bool,
    /// Blocked by a hardware switch.
    pub hard: bool,
}

impl Block {
    /// Whether the radio is blocked at all.
    pub fn any(self) -> bool {
        self.soft || self.hard
    }
}

/// The Bluetooth radios as rfkill reports them, kept up to date by [`changed`](Self::changed).
#[derive(Debug)]
pub struct Rfkill<F: AsFd = File> {
    file: Async<F>,
    /// The block of each Bluetooth radio, by index.
    radios: HashMap<u32, Block>,
}

impl Rfkill {
    /// Opens `/dev/rfkill` for reading and takes in the radios it reports.
    ///
    /// # Errors
    ///
    /// The I/O error if it can't be opened (no rfkill in the kernel, no access) or read.
    pub fn open() -> io::Result<Self> {
        Self::from_file(File::open(DEVICE)?)
    }
}

impl<F: AsFd> Rfkill<F>
where
    for<'a> &'a F: Read,
{
    /// Watches `file`, an rfkill device or a stand-in for one that speaks its events (a socket in
    /// tests), and takes in the events it already has.
    ///
    /// # Errors
    ///
    /// The I/O error if it can't be read.
    pub fn from_file(file: F) -> io::Result<Self> {
        let mut rfkill = Self {
            file: Async::new(file)?,
            radios: HashMap::new(),
        };
        rfkill.drain()?;
        Ok(rfkill)
    }

    /// Whether the Bluetooth radios are blocked now.
    pub fn block(&self) -> Block {
        self.radios
            .values()
            .fold(Block::default(), |all, radio| Block {
                soft: all.soft || radio.soft,
                hard: all.hard || radio.hard,
            })
    }

    /// Whether there is a Bluetooth radio at all.
    pub fn has_radio(&self) -> bool {
        !self.radios.is_empty()
    }

    /// Waits until rfkill reports something and returns the block after it. The block can be the
    /// same as before (another radio changed).
    ///
    /// # Errors
    ///
    /// The I/O error if the device can't be read anymore.
    pub async fn changed(&mut self) -> io::Result<Block> {
        loop {
            self.file.readable().await?;
            if self.drain()? {
                return Ok(self.block());
            }
        }
    }

    /// Reads every event there is without waiting; whether there was one.
    fn drain(&mut self) -> io::Result<bool> {
        let mut any = false;
        loop {
            let mut event = [0; EVENT_SIZE];
            // async-io made the file non-blocking, so this ends with `WouldBlock`.
            match self.file.get_ref().read(&mut event) {
                Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
                Ok(EVENT_SIZE) => {
                    apply(&mut self.radios, event);
                    any = true;
                }
                Ok(_) => return Err(io::ErrorKind::InvalidData.into()),
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(any),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => return Err(error),
            }
        }
    }
}

/// Takes `event` into `radios`.
fn apply(radios: &mut HashMap<u32, Block>, event: [u8; EVENT_SIZE]) {
    let [i0, i1, i2, i3, kind, op, soft, hard] = event;
    if kind != TYPE_BLUETOOTH {
        return;
    }
    let index = u32::from_ne_bytes([i0, i1, i2, i3]);
    let block = Block {
        soft: soft != 0,
        hard: hard != 0,
    };
    match op {
        OP_ADD | OP_CHANGE => {
            radios.insert(index, block);
        }
        OP_DEL => {
            radios.remove(&index);
        }
        _ => {}
    }
}

/// The event that lifts the soft block of every Bluetooth radio.
fn unblock_event() -> [u8; EVENT_SIZE] {
    let [i0, i1, i2, i3] = 0u32.to_ne_bytes();
    [i0, i1, i2, i3, TYPE_BLUETOOTH, OP_CHANGE_ALL, 0, 0]
}

/// Lifts the soft block of every Bluetooth radio. A hard block stays.
///
/// # Errors
///
/// The I/O error if `/dev/rfkill` can't be opened for writing (no access) or written.
pub fn unblock() -> io::Result<()> {
    unblock_at(Path::new(DEVICE))
}

/// [`unblock`] through the device at `path`.
///
/// # Errors
///
/// The I/O error if `path` can't be opened for writing or written.
pub fn unblock_at(path: &Path) -> io::Result<()> {
    OpenOptions::new()
        .write(true)
        .open(path)?
        .write_all(&unblock_event())
}

/// An event in the format of `/dev/rfkill`, for stand-ins in tests: radio `index` of type
/// Bluetooth (or Wi-Fi, for `bluetooth: false`) added or changed with these flags.
#[cfg(any(test, feature = "test-support"))]
pub fn event(index: u32, bluetooth: bool, added: bool, block: Block) -> [u8; EVENT_SIZE] {
    let [i0, i1, i2, i3] = index.to_ne_bytes();
    let kind = if bluetooth { TYPE_BLUETOOTH } else { TYPE_WLAN };
    let op = if added { OP_ADD } else { OP_CHANGE };
    [
        i0,
        i1,
        i2,
        i3,
        kind,
        op,
        u8::from(block.soft),
        u8::from(block.hard),
    ]
}

#[cfg(test)]
mod tests {
    use std::os::unix::net::UnixStream;

    use async_io::block_on;

    use super::*;

    const SOFT: Block = Block {
        soft: true,
        hard: false,
    };
    const HARD: Block = Block {
        soft: false,
        hard: true,
    };

    #[test]
    fn the_events_already_there_are_read_on_open() {
        let (mut device, file) = UnixStream::pair().unwrap();
        // A Wi-Fi radio, which doesn't count, and two Bluetooth radios.
        device.write_all(&event(0, false, true, HARD)).unwrap();
        device
            .write_all(&event(1, true, true, Block::default()))
            .unwrap();
        device.write_all(&event(2, true, true, SOFT)).unwrap();
        let rfkill = Rfkill::from_file(file).unwrap();
        assert!(rfkill.has_radio());
        assert_eq!(rfkill.block(), SOFT);
    }

    #[test]
    fn changes_and_removals_are_followed() {
        let (mut device, file) = UnixStream::pair().unwrap();
        device.write_all(&event(1, true, true, SOFT)).unwrap();
        let mut rfkill = Rfkill::from_file(file).unwrap();
        assert!(rfkill.block().soft);

        device.write_all(&event(1, true, false, HARD)).unwrap();
        assert_eq!(block_on(rfkill.changed()).unwrap(), HARD);

        let mut removed = event(1, true, false, Block::default());
        removed[5] = OP_DEL;
        device.write_all(&removed).unwrap();
        assert_eq!(block_on(rfkill.changed()).unwrap(), Block::default());
        assert!(!rfkill.has_radio());
    }

    #[test]
    fn a_closed_device_is_an_error() {
        let (device, file) = UnixStream::pair().unwrap();
        let mut rfkill = Rfkill::from_file(file).unwrap();
        drop(device);
        assert!(block_on(rfkill.changed()).is_err());
    }

    #[test]
    fn unblocking_writes_one_change_all_event() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rfkill");
        File::create(&path).unwrap();
        unblock_at(&path).unwrap();
        let written = std::fs::read(&path).unwrap();
        assert_eq!(written, unblock_event());
        assert_eq!(written[4], TYPE_BLUETOOTH);
        assert_eq!(written[5], OP_CHANGE_ALL);
        assert_eq!(&written[6..], [0, 0]);
    }

    #[test]
    fn unblocking_without_access_fails() {
        assert!(unblock_at(Path::new("/nonexistent/rfkill")).is_err());
    }
}
