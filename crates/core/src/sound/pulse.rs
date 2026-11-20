// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The connection to the sound server through libpulse's threaded main loop.
//!
//! libpulse runs its own thread, which calls back with the answers. Every call into libpulse holds
//! the main loop's lock; the callbacks run with that lock held and only send what they got into a
//! channel, which an async method awaits on the caller's executor. So the core stays free of an
//! executor, and the GUI's main loop never blocks on the sound server.
//!
//! Without a server, or when the server goes away (a restart of pipewire-pulse), the connection
//! fails; the next [`Sound::snapshot`] tries again, at most once a second, so a reader that asks
//! now and then (the page does every few seconds while it shows no server) finds the server when
//! it is back. [`Sound::changes`] ticks on every state change and every event the server reports,
//! so a reader of [`Sound::snapshot`] follows all of it.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use async_channel::{Receiver, Sender};
use futures_lite::{FutureExt, Stream};
use libpulse_binding as pulse;
use pulse::callbacks::ListResult;
use pulse::context::introspect::{
    CardInfo, ServerInfo, SinkInfo, SinkInputInfo, SourceInfo, SourceOutputInfo,
};
use pulse::context::subscribe::InterestMaskSet;
use pulse::context::{Context, FlagSet, State};
use pulse::def::PortAvailable;
use pulse::mainloop::threaded::Mainloop;
use pulse::proplist::{Proplist, properties};
use pulse::volume::{ChannelVolumes, Volume as PaVolume};

use super::{
    Availability, Card, Device, Error, Port, Profile, Server, Snapshot, Stream as AppStream,
    Volume, sort_by_description,
};
use crate::i18n::{Msg, Text};
use crate::msg;

/// Why a list couldn't be read; outside the macro that uses it, where xgettext finds it.
const LIST_FAILED: Msg = msg!("the sound server didn't answer the list");

/// How long an answer of the server may take before the call counts as failed.
const ANSWER_TIME: Duration = Duration::from_secs(5);

/// How long a snapshot waits for a connection that is being made.
const CONNECT_TIME: Duration = Duration::from_secs(2);

/// How long after a failed connection the next one is tried, at the soonest.
const RETRY_TIME: Duration = Duration::from_secs(1);

/// The app's id, which the server shows next to our streams and which marks our own level meter.
const APP_ID: &str = "io.github.mowi12.SwayThings";

/// The callback of a list call: collects what `$convert` keeps of each item and sends the list at
/// its end on `$send`.
macro_rules! collect {
    ($send:expr, $convert:expr $(,)?) => {{
        let send = $send;
        let convert = $convert;
        let mut items = Vec::new();
        move |result: ListResult<&_>| match result {
            ListResult::Item(item) => items.extend(convert(item)),
            ListResult::End => {
                let _ = send.try_send(Ok(std::mem::take(&mut items)));
            }
            ListResult::Error => {
                let _ = send.try_send(Err(Error::Failed(LIST_FAILED.into())));
            }
        }
    }};
}

/// How the connection to the server is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Link {
    /// Connected and subscribed.
    Up,
    /// Being made; ticks follow.
    Connecting,
    /// Failed; tried again after [`RETRY_TIME`].
    Down,
}

/// A connection to the sound server. Cheap to clone; the last clone dropped disconnects.
#[derive(Clone)]
pub struct Sound {
    inner: Rc<Inner>,
}

impl std::fmt::Debug for Sound {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Sound").finish_non_exhaustive()
    }
}

pub(super) struct Inner {
    // Dropped in this order: the context before the main loop it belongs to.
    pub(super) context: RefCell<Context>,
    pub(super) mainloop: RefCell<Mainloop>,
    changes: Receiver<()>,
    /// The tick sender, for subscriptions made later.
    ticks: Sender<()>,
    /// The server to connect to, or `None` for the session's.
    server: Option<String>,
    /// The client index of the connection our subscriptions were made on.
    subscribed: Cell<Option<u32>>,
    /// When the current connection was started.
    attempt: Cell<Instant>,
    /// Counts the connections made, so a holder of something tied to one (a level meter) sees
    /// that it was replaced.
    generation: Cell<u64>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        let mainloop = self.mainloop.get_mut();
        mainloop.lock();
        self.context.get_mut().disconnect();
        mainloop.unlock();
        mainloop.stop();
    }
}

impl Sound {
    /// Connects to the sound server of the session; without one, later snapshots try again.
    ///
    /// # Errors
    ///
    /// [`Error::Failed`] if libpulse can't set up its main loop or context.
    pub fn connect() -> Result<Self, Error> {
        Self::connect_to(None)
    }

    /// Connects to the sound server at `server` (like `PULSE_SERVER`), or the session's with `None`.
    ///
    /// # Errors
    ///
    /// [`Error::Failed`] if libpulse can't set up its main loop or context.
    pub fn connect_to(server: Option<&str>) -> Result<Self, Error> {
        let mut mainloop = Mainloop::new().ok_or_else(setup)?;
        // A tick only says "look again": one waiting is enough, and a holder that never reads
        // `changes` (like the CLI's doctor) can't build up a backlog.
        let (ticks, changes) = async_channel::bounded(1);
        let server = server.map(str::to_owned);
        let context = new_context(&mainloop, &ticks, server.as_deref())?;
        mainloop.start().map_err(failed)?;
        Ok(Self {
            inner: Rc::new(Inner {
                context: RefCell::new(context),
                mainloop: RefCell::new(mainloop),
                changes,
                ticks,
                server,
                subscribed: Cell::new(None),
                attempt: Cell::new(Instant::now()),
                generation: Cell::new(0),
            }),
        })
    }

    /// A tick whenever something may have changed: the connection's state, or an event of the
    /// server (a device, card, stream or default changed). Ticks come in
    /// bursts; read one [`snapshot`](Self::snapshot) after a burst.
    pub fn changes(&self) -> impl Stream<Item = ()> + Unpin + use<> {
        Box::pin(self.inner.changes.clone())
    }

    /// Which connection is current: it changes whenever a new one replaces a failed one, so a
    /// level meter of the old one can be started again.
    pub fn generation(&self) -> u64 {
        self.inner.generation.get()
    }

    /// Runs `f` on the context with the main loop's lock held. The lock is let go even if `f`
    /// panics (the binding panics when libpulse refuses to start an operation), so a refused call
    /// can't leave libpulse's thread waiting forever.
    pub(super) fn locked<T>(&self, f: impl FnOnce(&mut Context) -> T) -> T {
        struct Unlock<'a>(&'a RefCell<Mainloop>);
        impl Drop for Unlock<'_> {
            fn drop(&mut self) {
                self.0.borrow_mut().unlock();
            }
        }
        self.inner.mainloop.borrow_mut().lock();
        let _unlock = Unlock(&self.inner.mainloop);
        f(&mut self.inner.context.borrow_mut())
    }

    /// Starts operations with [`locked`](Self::locked): [`Error::Unavailable`] if the connection
    /// isn't ready (the server went away since it was checked), else the context's error if
    /// libpulse refuses the operation anyway.
    ///
    /// The state is checked first: with the lock held it can't change, so libpulse doesn't refuse
    /// an operation for a broken connection. Any other refusal is a panic of the binding (over a
    /// null operation), caught here. That relies on panics unwinding, as in every profile of the
    /// workspace; built with `panic = "abort"`, such a refusal would end the app.
    fn start<T>(&self, start: impl FnOnce(&mut Context) -> T) -> Result<T, Error> {
        self.locked(|context| {
            if context.get_state() != State::Ready {
                return Err(Error::Unavailable);
            }
            let started = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| start(context)));
            started.map_err(|_| {
                if context.get_state() == State::Ready {
                    failed(context.errno())
                } else {
                    Error::Unavailable
                }
            })
        })
    }

    /// How the connection is; subscribes to the server's events once per connection. A
    /// connection that failed (no server, or it went away) is replaced by a new one, at most once
    /// every [`RETRY_TIME`].
    fn link(&self) -> Link {
        self.locked(|context| {
            match context.get_state() {
                State::Ready => {}
                State::Failed | State::Terminated => {
                    if self.inner.attempt.get().elapsed() < RETRY_TIME {
                        return Link::Down;
                    }
                    self.inner.attempt.set(Instant::now());
                    let mainloop = self.inner.mainloop.borrow();
                    let server = self.inner.server.as_deref();
                    return match new_context(&mainloop, &self.inner.ticks, server) {
                        Ok(fresh) => {
                            *context = fresh;
                            self.inner.subscribed.set(None);
                            self.inner.generation.set(self.inner.generation.get() + 1);
                            Link::Connecting
                        }
                        Err(_) => Link::Down,
                    };
                }
                _ => return Link::Connecting,
            }
            let client = context.get_index();
            if client.is_some() && self.inner.subscribed.get() != client {
                self.inner.subscribed.set(client);
                let interest = InterestMaskSet::SINK
                    | InterestMaskSet::SOURCE
                    | InterestMaskSet::SINK_INPUT
                    | InterestMaskSet::SOURCE_OUTPUT
                    | InterestMaskSet::CARD
                    | InterestMaskSet::SERVER;
                context.subscribe(interest, |_| {});
            }
            Link::Up
        })
    }

    /// Waits until the connection is up, for a moment at most; without a server, it says so at
    /// once.
    async fn wait_ready(&self) -> Result<(), Error> {
        let deadline = async_io::Timer::after(CONNECT_TIME);
        let taken = Cell::new(false);
        let wait = async {
            loop {
                match self.link() {
                    Link::Up => return true,
                    Link::Down => return false,
                    Link::Connecting => {}
                }
                if self.inner.changes.recv().await.is_err() {
                    return false;
                }
                taken.set(true);
            }
        };
        let ready = wait
            .or(async {
                deadline.await;
                false
            })
            .await;
        // A tick taken while waiting is given back, so the reader of `changes` looks again; none
        // is made up, or every snapshot would ask for the next one.
        if taken.get() {
            let _ = self.inner.ticks.try_send(());
        }
        if ready {
            Ok(())
        } else {
            Err(Error::Unavailable)
        }
    }

    /// The server and its defaults, without the rest of a snapshot.
    ///
    /// # Errors
    ///
    /// [`Error::Unavailable`] while no server runs.
    pub async fn server(&self) -> Result<Server, Error> {
        self.wait_ready().await?;
        let receive = self.start(|context| {
            let (send, receive) = async_channel::bounded(1);
            context.introspect().get_server_info(move |info| {
                let _ = send.try_send(server_from(info));
            });
            receive
        })?;
        answer(receive).await
    }

    /// What the server knows now.
    ///
    /// # Errors
    ///
    /// [`Error::Unavailable`] while no server runs, [`Error::Failed`] if a list can't be read.
    pub async fn snapshot(&self) -> Result<Snapshot, Error> {
        self.wait_ready().await?;
        let own = self.locked(|context| context.get_index());
        let (server, outputs, inputs, cards, playing, recording) = self.start(|context| {
            let introspect = context.introspect();
            let (server_tx, server) = async_channel::bounded(1);
            introspect.get_server_info(move |info| {
                let _ = server_tx.try_send(server_from(info));
            });
            let outputs = list(|send| {
                introspect
                    .get_sink_info_list(collect!(send, |info: &SinkInfo<'_>| Some(sink(info))));
            });
            let inputs = list(|send| {
                introspect.get_source_info_list(collect!(send, |info: &SourceInfo<'_>| {
                    // The monitor of an output records what it plays: not an input to offer.
                    info.monitor_of_sink.is_none().then(|| source(info))
                }));
            });
            let cards = list(|send| {
                introspect
                    .get_card_info_list(collect!(send, |info: &CardInfo<'_>| Some(card(info))));
            });
            let playing = list(|send| {
                introspect.get_sink_input_info_list(collect!(send, |info: &SinkInputInfo<'_>| {
                    Some(playing(info))
                }));
            });
            let recording = list(|send| {
                introspect.get_source_output_info_list(collect!(
                    send,
                    move |info: &SourceOutputInfo<'_>| {
                        // Our own level meter isn't an app recording.
                        (own.is_none() || info.client != own).then(|| recording(info))
                    },
                ));
            });
            (server, outputs, inputs, cards, playing, recording)
        })?;
        let server = answer(server).await?;
        let mut outputs = answer(outputs).await??;
        let mut inputs = answer(inputs).await??;
        let mut cards = answer(cards).await??;
        let mut playing = answer(playing).await??;
        let mut recording = answer(recording).await??;
        sort_by_description(&mut outputs, |device| (&device.description, &device.name));
        sort_by_description(&mut inputs, |device| (&device.description, &device.name));
        sort_by_description(&mut cards, |card| (&card.description, &card.name));
        for streams in [&mut playing, &mut recording] {
            streams.sort_by(|a, b| {
                a.app
                    .to_lowercase()
                    .cmp(&b.app.to_lowercase())
                    .then(a.index.cmp(&b.index))
            });
        }
        Ok(Snapshot {
            server,
            outputs,
            inputs,
            cards,
            playing,
            recording,
        })
    }

    /// Makes the output `name` the default. The streams that follow the default move along by
    /// themselves (PipeWire, and PulseAudio since 15.0); moving them here would pin them to it.
    ///
    /// # Errors
    ///
    /// [`Error::Failed`] if the server refuses.
    pub async fn set_default_output(&self, name: &str) -> Result<(), Error> {
        self.wait_ready().await?;
        let name = name.to_owned();
        self.done(|context, send| {
            context.set_default_sink(&name, move |ok| {
                let _ = send.try_send(ok);
            });
        })
        .await
    }

    /// Makes the input `name` the default (see [`set_default_output`](Self::set_default_output)).
    ///
    /// # Errors
    ///
    /// [`Error::Failed`] if the server refuses.
    pub async fn set_default_input(&self, name: &str) -> Result<(), Error> {
        self.wait_ready().await?;
        let name = name.to_owned();
        self.done(|context, send| {
            context.set_default_source(&name, move |ok| {
                let _ = send.try_send(ok);
            });
        })
        .await
    }

    /// Sets the volume of the output with the server's `index`.
    ///
    /// # Errors
    ///
    /// [`Error::Failed`] if the server refuses.
    pub async fn set_output_volume(&self, index: u32, volume: &Volume) -> Result<(), Error> {
        let volume = channel_volumes(volume);
        self.done(|context, send| {
            context
                .introspect()
                .set_sink_volume_by_index(index, &volume, Some(reply(send)));
        })
        .await
    }

    /// Mutes or unmutes the output with the server's `index`.
    ///
    /// # Errors
    ///
    /// [`Error::Failed`] if the server refuses.
    pub async fn set_output_muted(&self, index: u32, muted: bool) -> Result<(), Error> {
        self.done(|context, send| {
            context
                .introspect()
                .set_sink_mute_by_index(index, muted, Some(reply(send)));
        })
        .await
    }

    /// Switches the output with the server's `index` to its port `port`.
    ///
    /// # Errors
    ///
    /// [`Error::Failed`] if the server refuses.
    pub async fn set_output_port(&self, index: u32, port: &str) -> Result<(), Error> {
        self.done(|context, send| {
            context
                .introspect()
                .set_sink_port_by_index(index, port, Some(reply(send)));
        })
        .await
    }

    /// Sets the volume of the input with the server's `index`.
    ///
    /// # Errors
    ///
    /// [`Error::Failed`] if the server refuses.
    pub async fn set_input_volume(&self, index: u32, volume: &Volume) -> Result<(), Error> {
        let volume = channel_volumes(volume);
        self.done(|context, send| {
            context
                .introspect()
                .set_source_volume_by_index(index, &volume, Some(reply(send)));
        })
        .await
    }

    /// Mutes or unmutes the input with the server's `index`.
    ///
    /// # Errors
    ///
    /// [`Error::Failed`] if the server refuses.
    pub async fn set_input_muted(&self, index: u32, muted: bool) -> Result<(), Error> {
        self.done(|context, send| {
            context
                .introspect()
                .set_source_mute_by_index(index, muted, Some(reply(send)));
        })
        .await
    }

    /// Switches the input with the server's `index` to its port `port`.
    ///
    /// # Errors
    ///
    /// [`Error::Failed`] if the server refuses.
    pub async fn set_input_port(&self, index: u32, port: &str) -> Result<(), Error> {
        self.done(|context, send| {
            context
                .introspect()
                .set_source_port_by_index(index, port, Some(reply(send)));
        })
        .await
    }

    /// Switches the card with the server's `index` to its profile `profile`.
    ///
    /// # Errors
    ///
    /// [`Error::Failed`] if the server refuses.
    pub async fn set_card_profile(&self, index: u32, profile: &str) -> Result<(), Error> {
        self.done(|context, send| {
            context
                .introspect()
                .set_card_profile_by_index(index, profile, Some(reply(send)));
        })
        .await
    }

    /// Sets the volume of the playing stream with the server's `index`.
    ///
    /// # Errors
    ///
    /// [`Error::Failed`] if the server refuses.
    pub async fn set_playing_volume(&self, index: u32, volume: &Volume) -> Result<(), Error> {
        let volume = channel_volumes(volume);
        self.done(|context, send| {
            context
                .introspect()
                .set_sink_input_volume(index, &volume, Some(reply(send)));
        })
        .await
    }

    /// Mutes or unmutes the playing stream with the server's `index`.
    ///
    /// # Errors
    ///
    /// [`Error::Failed`] if the server refuses.
    pub async fn set_playing_muted(&self, index: u32, muted: bool) -> Result<(), Error> {
        self.done(|context, send| {
            context
                .introspect()
                .set_sink_input_mute(index, muted, Some(reply(send)));
        })
        .await
    }

    /// Sets the volume of the recording stream with the server's `index`.
    ///
    /// # Errors
    ///
    /// [`Error::Failed`] if the server refuses.
    pub async fn set_recording_volume(&self, index: u32, volume: &Volume) -> Result<(), Error> {
        let volume = channel_volumes(volume);
        self.done(|context, send| {
            context
                .introspect()
                .set_source_output_volume(index, &volume, Some(reply(send)));
        })
        .await
    }

    /// Mutes or unmutes the recording stream with the server's `index`.
    ///
    /// # Errors
    ///
    /// [`Error::Failed`] if the server refuses.
    pub async fn set_recording_muted(&self, index: u32, muted: bool) -> Result<(), Error> {
        self.done(|context, send| {
            context
                .introspect()
                .set_source_output_mute(index, muted, Some(reply(send)));
        })
        .await
    }

    /// Starts an operation with the lock held and waits for its success flag.
    async fn done(&self, start: impl FnOnce(&mut Context, Sender<bool>)) -> Result<(), Error> {
        let (send, receive) = async_channel::bounded(1);
        self.start(|context| start(context, send))?;
        if answer(receive).await? {
            Ok(())
        } else {
            Err(self.locked(|context| failed(context.errno())))
        }
    }
}

fn setup() -> Error {
    Error::Failed(msg!("libpulse can't be set up").into())
}

/// A context of `mainloop` that ticks `ticks` on every change of its state and every event of the
/// server, connecting to `server` (the session's with `None`). It never starts a server itself.
fn new_context(
    mainloop: &Mainloop,
    ticks: &Sender<()>,
    server: Option<&str>,
) -> Result<Context, Error> {
    let mut proplist = Proplist::new().ok_or_else(setup)?;
    // Failing to set a property only leaves it out of pavucontrol's list.
    let _ = proplist.set_str(properties::APPLICATION_NAME, "Sway Things");
    let _ = proplist.set_str(properties::APPLICATION_ID, APP_ID);
    let _ = proplist.set_str(properties::APPLICATION_ICON_NAME, APP_ID);
    let mut context =
        Context::new_with_proplist(mainloop, "Sway Things", &proplist).ok_or_else(setup)?;
    let tick = ticks.clone();
    context.set_state_callback(Some(Box::new(move || {
        let _ = tick.try_send(());
    })));
    let tick = ticks.clone();
    context.set_subscribe_callback(Some(Box::new(move |_, _, _| {
        let _ = tick.try_send(());
    })));
    context
        .connect(server, FlagSet::NOAUTOSPAWN, None)
        .map_err(failed)?;
    Ok(context)
}

/// The callback of an introspection call that answers with a success flag.
fn reply(send: Sender<bool>) -> Box<dyn FnMut(bool) + 'static> {
    Box::new(move |ok| {
        let _ = send.try_send(ok);
    })
}

/// Starts a list call through `start`, which gets the sender of the collected list.
fn list<T>(start: impl FnOnce(Sender<Result<Vec<T>, Error>>)) -> Receiver<Result<Vec<T>, Error>> {
    let (send, receive) = async_channel::bounded(1);
    start(send);
    receive
}

/// Waits for the answer on `receive`, for [`ANSWER_TIME`] at most.
async fn answer<T>(receive: Receiver<T>) -> Result<T, Error> {
    let timeout = async {
        async_io::Timer::after(ANSWER_TIME).await;
        Err(Error::Failed(
            msg!("the sound server didn't answer in time").into(),
        ))
    };
    async { receive.recv().await.map_err(|_| Error::Unavailable) }
        .or(timeout)
        .await
}

fn failed(error: pulse::error::PAErr) -> Error {
    Error::Failed(Text::data(error.to_string().unwrap_or_default()))
}

fn text(value: Option<&str>) -> String {
    value.unwrap_or_default().to_owned()
}

fn volume(volumes: &ChannelVolumes) -> Volume {
    Volume {
        channels: volumes.get().iter().map(|volume| volume.0).collect(),
    }
}

fn channel_volumes(volume: &Volume) -> ChannelVolumes {
    let mut volumes = ChannelVolumes::default();
    let channels = volume
        .channels
        .len()
        .clamp(1, usize::from(pulse::sample::Spec::CHANNELS_MAX));
    volumes.set_len(u8::try_from(channels).unwrap_or(1));
    for (target, &level) in volumes.get_mut().iter_mut().zip(&volume.channels) {
        *target = PaVolume(level);
    }
    volumes
}

fn availability(available: PortAvailable) -> Availability {
    match available {
        PortAvailable::Yes => Availability::Yes,
        PortAvailable::No => Availability::No,
        PortAvailable::Unknown => Availability::Unknown,
    }
}

fn icon(proplist: &Proplist, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| proplist.get_str(key))
        .filter(|icon| !icon.is_empty())
}

macro_rules! device_from {
    ($info:expr) => {{
        let info = $info;
        let mut ports: Vec<(u32, Port)> = info
            .ports
            .iter()
            .map(|port| {
                (
                    port.priority,
                    Port {
                        name: text(port.name.as_deref()),
                        description: text(port.description.as_deref()),
                        available: availability(port.available),
                    },
                )
            })
            .collect();
        ports.sort_by_key(|(priority, _)| std::cmp::Reverse(*priority));
        Device {
            index: info.index,
            name: text(info.name.as_deref()),
            description: info
                .description
                .as_deref()
                .filter(|description| !description.is_empty())
                .or(info.name.as_deref())
                .unwrap_or_default()
                .to_owned(),
            volume: volume(&info.volume),
            muted: info.mute,
            ports: ports.into_iter().map(|(_, port)| port).collect(),
            active_port: info
                .active_port
                .as_ref()
                .and_then(|port| port.name.as_deref())
                .map(str::to_owned),
            card: info.card,
            icon: icon(&info.proplist, &[properties::DEVICE_ICON_NAME]),
        }
    }};
}

fn server_from(info: &ServerInfo<'_>) -> Server {
    Server {
        name: text(info.server_name.as_deref()),
        version: text(info.server_version.as_deref()),
        default_output: info.default_sink_name.as_deref().map(str::to_owned),
        default_input: info.default_source_name.as_deref().map(str::to_owned),
    }
}

fn sink(info: &SinkInfo<'_>) -> Device {
    device_from!(info)
}

fn source(info: &SourceInfo<'_>) -> Device {
    device_from!(info)
}

fn card(info: &CardInfo<'_>) -> Card {
    let mut profiles: Vec<(u32, Profile)> = info
        .profiles
        .iter()
        .map(|profile| {
            let name = text(profile.name.as_deref());
            // "Off" sorts last, whatever its priority.
            let priority = if name == "off" {
                0
            } else {
                profile.priority.saturating_add(1)
            };
            (
                priority,
                Profile {
                    description: profile
                        .description
                        .as_deref()
                        .filter(|description| !description.is_empty())
                        .unwrap_or(&name)
                        .to_owned(),
                    name,
                    available: profile.available,
                },
            )
        })
        .collect();
    profiles.sort_by_key(|(priority, _)| std::cmp::Reverse(*priority));
    let name = text(info.name.as_deref());
    Card {
        index: info.index,
        description: info
            .proplist
            .get_str(properties::DEVICE_DESCRIPTION)
            .filter(|description| !description.is_empty())
            .unwrap_or_else(|| name.clone()),
        name,
        profiles: profiles.into_iter().map(|(_, profile)| profile).collect(),
        active_profile: info
            .active_profile
            .as_ref()
            .and_then(|profile| profile.name.as_deref())
            .map(str::to_owned),
    }
}

macro_rules! stream_from {
    ($info:expr, $device:expr) => {{
        let info = $info;
        let proplist = &info.proplist;
        let app = proplist
            .get_str(properties::APPLICATION_NAME)
            .filter(|app| !app.is_empty())
            .or_else(|| info.name.as_deref().map(str::to_owned))
            .unwrap_or_default();
        let title = proplist
            .get_str(properties::MEDIA_NAME)
            .or_else(|| info.name.as_deref().map(str::to_owned))
            .filter(|title| !title.is_empty() && *title != app);
        AppStream {
            index: info.index,
            title,
            icon: icon(
                proplist,
                &[
                    properties::APPLICATION_ICON_NAME,
                    properties::APPLICATION_PROCESS_BINARY,
                ],
            ),
            volume: (info.has_volume && info.volume_writable).then(|| volume(&info.volume)),
            muted: info.mute,
            device: $device,
            paused: info.corked,
            app,
        }
    }};
}

fn playing(info: &SinkInputInfo<'_>) -> AppStream {
    stream_from!(info, info.sink)
}

fn recording(info: &SourceOutputInfo<'_>) -> AppStream {
    stream_from!(info, info.source)
}
