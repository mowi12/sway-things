// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::io::{ErrorKind, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;

use async_io::block_on;

use super::types::{Event, EventType, ShutdownChange};
use super::{Client, Compositor, IpcError, Unmet, socket_from, wire};
use crate::command::input::{DragLock, Input, InputSelector, InputType};
use crate::command::{CommandError, Target, behavior, effects, window};
use crate::version::{SwayFxVersion, SwayVersion};

pub(crate) const SWAY_1_10: &str = r#"{"human_readable": "1.10.1", "variant": "sway", "major": 1,
    "minor": 10, "patch": 1, "loaded_config_file_name": "/home/me/.config/sway/config"}"#;
const SWAYFX_0_5: &str = r#"{"human_readable": "0.5.3", "sway_original_version": "1.11.0",
    "variant": "sway", "major": 0, "minor": 5, "patch": 3,
    "loaded_config_file_name": "/etc/sway/config"}"#;

/// An answer that starts with this is sent without it, and then the connection is closed.
pub(crate) const HANG_UP: &[u8] = b"<hang up>";

/// A message as sway sends it.
pub(crate) fn message(kind: u32, payload: &str) -> Vec<u8> {
    let mut message = b"i3-ipc".to_vec();
    message.extend_from_slice(&u32::try_from(payload.len()).unwrap().to_ne_bytes());
    message.extend_from_slice(&kind.to_ne_bytes());
    message.extend_from_slice(payload.as_bytes());
    message
}

/// A stand-in for sway: answers every request on its socket with what `answer` returns for the
/// message type and payload, and records the requests.
pub(crate) struct FakeSway {
    _dir: tempfile::TempDir,
    pub(super) socket: PathBuf,
    requests: Arc<Mutex<Vec<(u32, String)>>>,
}

impl FakeSway {
    pub(crate) fn new(answer: impl Fn(u32, &str) -> Vec<u8> + Send + Sync + 'static) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("sway-ipc.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let (recorded, answer) = (Arc::clone(&requests), Arc::new(answer));
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { return };
                let (recorded, answer) = (Arc::clone(&recorded), Arc::clone(&answer));
                thread::spawn(move || serve(stream, &recorded, &*answer));
            }
        });
        Self {
            _dir: dir,
            socket,
            requests,
        }
    }

    /// A sway that reports `version` and answers everything else with `answer`.
    pub(crate) fn with_version(
        version: &'static str,
        answer: impl Fn(u32, &str) -> Vec<u8> + Send + Sync + 'static,
    ) -> Self {
        Self::new(move |kind, payload| match kind {
            wire::GET_VERSION => message(kind, version),
            _ => answer(kind, payload),
        })
    }

    pub(crate) fn connect(&self) -> Client {
        block_on(Client::connect_to(&self.socket)).unwrap()
    }

    /// The commands received so far, as sent.
    pub(crate) fn commands(&self) -> Vec<String> {
        self.requests()
            .into_iter()
            .filter(|(kind, _)| *kind == wire::RUN_COMMAND)
            .map(|(_, payload)| payload)
            .collect()
    }

    /// The requests received so far, without `GET_VERSION`.
    fn requests(&self) -> Vec<(u32, String)> {
        let mut requests = self.requests.lock().unwrap().clone();
        requests.retain(|(kind, _)| *kind != wire::GET_VERSION);
        requests
    }
}

fn serve(
    mut stream: UnixStream,
    recorded: &Mutex<Vec<(u32, String)>>,
    answer: &dyn Fn(u32, &str) -> Vec<u8>,
) {
    loop {
        let mut header = [0; 14];
        if stream.read_exact(&mut header).is_err() {
            return;
        }
        let length = u32::from_ne_bytes(header[6..10].try_into().unwrap());
        let kind = u32::from_ne_bytes(header[10..14].try_into().unwrap());
        let mut payload = vec![0; length as usize];
        stream.read_exact(&mut payload).unwrap();
        let payload = String::from_utf8(payload).unwrap();
        recorded.lock().unwrap().push((kind, payload.clone()));
        let answer = answer(kind, &payload);
        let (answer, hang_up) = match answer.strip_prefix(HANG_UP) {
            Some(answer) => (answer, true),
            None => (&answer[..], false),
        };
        if stream.write_all(answer).is_err() || hang_up {
            return;
        }
    }
}

/// A sway 1.10.1 that answers each `GET_INPUTS` with the next of `inputs` (the last one again
/// after that), and sends one input event per reply after the first to a subscriber before it
/// closes that connection.
pub(crate) fn with_inputs(inputs: &'static [&'static str]) -> FakeSway {
    let asked = std::sync::atomic::AtomicUsize::new(0);
    FakeSway::with_version(SWAY_1_10, move |kind, _| match kind {
        wire::SUBSCRIBE => {
            let mut answer = HANG_UP.to_vec();
            answer.extend(message(kind, r#"{"success": true}"#));
            for _ in inputs.iter().skip(1) {
                let event = r#"{"change": "added", "input":
                    {"identifier": "0:0:event", "name": "event", "type": "keyboard"}}"#;
                answer.extend(message(wire::EVENT | 0x15, event));
            }
            answer
        }
        wire::GET_INPUTS => {
            let index = asked.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            message(kind, inputs[index.min(inputs.len() - 1)])
        }
        _ => message(kind, "{}"),
    })
}

/// A sway on which every command succeeds, except those that contain `fail`.
pub(crate) fn obedient(version: &'static str) -> FakeSway {
    FakeSway::with_version(version, |kind, payload| {
        let reply = if payload.contains("fail") {
            r#"[{"success": false, "parse_error": true, "error": "Unknown/invalid command 'fail'"}]"#
        } else {
            r#"[{"success": true}]"#
        };
        message(kind, reply)
    })
}

#[test]
fn connecting_detects_the_compositor() {
    let sway = FakeSway::with_version(SWAY_1_10, |kind, _| message(kind, "{}"));
    let mut client = sway.connect();
    assert_eq!(
        client.compositor(),
        Compositor::Sway {
            version: SwayVersion::new(1, 10, 1)
        }
    );
    assert_eq!(client.socket(), sway.socket);
    let version = block_on(client.version()).unwrap();
    assert_eq!(version.compositor, client.compositor());
    assert_eq!(version.human_readable, "1.10.1");
    assert_eq!(
        version.loaded_config_file_name,
        "/home/me/.config/sway/config"
    );

    let swayfx = FakeSway::with_version(SWAYFX_0_5, |kind, _| message(kind, "{}"));
    assert_eq!(
        swayfx.connect().compositor(),
        Compositor::SwayFx {
            version: SwayFxVersion::new(0, 5, 3),
            base: SwayVersion::new(1, 11, 0)
        }
    );
}

#[test]
fn a_compositor_that_is_too_old_is_refused() {
    let sway = FakeSway::new(|kind, _| {
        message(
            kind,
            r#"{"human_readable": "1.9", "major": 1, "minor": 9, "patch": 0,
                "loaded_config_file_name": ""}"#,
        )
    });
    let error = block_on(Client::connect_to(&sway.socket)).unwrap_err();
    assert!(matches!(
        error,
        IpcError::Unsupported {
            compositor: Compositor::Sway { .. },
            unmet: Unmet::Sway { .. }
        }
    ));
    assert_eq!(
        error.to_string(),
        "sway 1.9 is not supported: Sway Things requires sway 1.10 or newer"
    );
}

#[test]
fn no_session_and_no_sway_are_different_errors() {
    assert!(matches!(socket_from(None), Err(IpcError::NoSession)));
    assert!(matches!(
        socket_from(Some("".into())),
        Err(IpcError::NoSession)
    ));
    assert_eq!(
        socket_from(Some("/run/user/1000/sway-ipc.1000.1234.sock".into())).unwrap(),
        PathBuf::from("/run/user/1000/sway-ipc.1000.1234.sock")
    );

    let dir = tempfile::tempdir().unwrap();
    // No socket at all, and one left over from a sway that is gone.
    let missing = dir.path().join("missing.sock");
    let stale = dir.path().join("stale.sock");
    drop(UnixListener::bind(&stale).unwrap());
    for socket in [missing, stale] {
        let error = block_on(Client::connect_to(&socket)).unwrap_err();
        assert!(
            matches!(&error, IpcError::NotRunning { socket: path, .. } if *path == socket),
            "{error:?}"
        );
        assert_eq!(
            error.to_string(),
            format!("sway is not running (no answer on {})", socket.display())
        );
    }
}

#[test]
fn commands_are_sent_one_by_one_as_rendered_for_ipc() {
    let sway = obedient(SWAY_1_10);
    let mut client = sway.connect();
    let touchpad = Input::new(InputSelector::Type(InputType::Touchpad));
    let font = window::font("Dell \"U2415\" $1", false).unwrap();
    block_on(client.run(&touchpad.tap(true))).unwrap();
    block_on(client.run(&font)).unwrap();
    assert_eq!(
        sway.requests(),
        [
            (
                wire::RUN_COMMAND,
                "input type:touchpad tap enabled".to_owned()
            ),
            (wire::RUN_COMMAND, font.render(Target::Ipc).unwrap()),
        ]
    );
}

#[test]
fn a_failed_command_carries_sways_message() {
    let sway = FakeSway::with_version(SWAY_1_10, |kind, payload| {
        let reply = match payload {
            "tiling_drag enable" => r#"[{"success": true}]"#,
            "gaps inner all set 8" => {
                r#"[{"success": false, "parse_error": false, "error": "No workspaces"}]"#
            }
            _ => r#"[{"success": false, "parse_error": true, "error": "Unknown command"}]"#,
        };
        message(kind, reply)
    });
    let mut client = sway.connect();
    let commands = [
        behavior::tiling_drag(true),
        window::gaps_runtime(window::Gaps::Inner, window::GapsScope::All, 8).unwrap(),
        behavior::tiling_drag(false),
    ];
    let results = block_on(client.run_all(&commands));
    assert!(matches!(results[0], Ok(())));
    assert!(matches!(
        &results[1],
        Err(IpcError::CommandFailed { command, message, parse_error: false })
            if command == "gaps inner all set 8" && message == "No workspaces"
    ));
    // The command after a failed one still runs.
    assert!(matches!(
        &results[2],
        Err(IpcError::CommandFailed {
            parse_error: true,
            ..
        })
    ));
    assert_eq!(sway.requests().len(), 3);
    assert_eq!(
        results[1].as_ref().unwrap_err().to_string(),
        "sway rejected `gaps inner all set 8`: No workspaces"
    );
}

#[test]
fn commands_the_compositor_does_not_know_are_not_sent() {
    let sway = obedient(SWAY_1_10);
    let mut client = sway.connect();
    let touchpad = Input::new(InputSelector::Type(InputType::Touchpad));

    // sway 1.10 would read this as `disabled` without an error.
    let error = block_on(client.run(&touchpad.drag_lock(DragLock::EnabledSticky))).unwrap_err();
    assert!(matches!(
        &error,
        IpcError::CommandUnsupported { command, unmet: Unmet::Sway { .. } }
            if command == "input type:touchpad drag_lock enabled_sticky"
    ));
    assert_eq!(
        error.to_string(),
        "`input type:touchpad drag_lock enabled_sticky` requires sway 1.11 or newer"
    );
    assert!(matches!(
        block_on(client.run(&effects::corner_radius(8).unwrap())),
        Err(IpcError::CommandUnsupported {
            unmet: Unmet::SwayFx { .. },
            ..
        })
    ));
    // A command that only works in config files.
    assert!(matches!(
        block_on(client.run(&behavior::workspace_layout(
            behavior::WorkspaceLayout::Tabbed
        ))),
        Err(IpcError::Render(CommandError::Unavailable {
            target: Target::Ipc
        }))
    ));
    assert_eq!(sway.requests(), []);

    let swayfx = obedient(SWAYFX_0_5);
    let mut client = swayfx.connect();
    block_on(client.run(&effects::corner_radius(8).unwrap())).unwrap();
    block_on(client.run(&touchpad.drag_lock(DragLock::EnabledSticky))).unwrap();
    assert_eq!(swayfx.requests().len(), 2);
}

#[test]
fn small_replies_are_typed() {
    let sway = FakeSway::with_version(SWAY_1_10, |kind, payload| {
        let reply = match kind {
            wire::GET_BINDING_MODES => r#"["default", "resize"]"#,
            wire::GET_BINDING_STATE => r#"{"name": "resize"}"#,
            wire::GET_CONFIG => r#"{"config": "include ~/.config/sway/sway-things/*.conf\n"}"#,
            wire::GET_SEATS => {
                r#"[{"name": "seat0", "capabilities": 3, "focus": 4, "devices": []}]"#
            }
            wire::SEND_TICK if payload == "ping" => r#"{"success": true}"#,
            wire::SEND_TICK => r#"{"success": false}"#,
            _ => "[]",
        };
        message(kind, reply)
    });
    let mut client = sway.connect();
    assert_eq!(
        block_on(client.binding_modes()).unwrap(),
        ["default", "resize"]
    );
    assert_eq!(block_on(client.binding_state()).unwrap(), "resize");
    assert_eq!(
        block_on(client.config()).unwrap(),
        "include ~/.config/sway/sway-things/*.conf\n"
    );
    let seats = block_on(client.seats()).unwrap();
    assert_eq!(seats[0].name, "seat0");
    assert_eq!(block_on(client.inputs()).unwrap().len(), 0);
    assert_eq!(block_on(client.outputs()).unwrap().len(), 0);
    block_on(client.send_tick("ping")).unwrap();
    assert!(matches!(
        block_on(client.send_tick("pong")),
        Err(IpcError::Protocol(_))
    ));
    assert_eq!(
        sway.requests()[6..],
        [
            (wire::SEND_TICK, "ping".to_owned()),
            (wire::SEND_TICK, "pong".to_owned())
        ]
    );
}

#[test]
fn answers_that_are_not_ipc_replies_are_errors() {
    let broken = |answer: fn(u32) -> Vec<u8>| {
        let sway = FakeSway::with_version(SWAY_1_10, move |kind, _| answer(kind));
        let mut client = sway.connect();
        block_on(client.binding_modes()).unwrap_err()
    };
    // Not the magic string.
    let error = broken(|_| b"HTTP/1.1 400 Bad Request\r\n\r\n".to_vec());
    assert!(matches!(error, IpcError::Protocol(_)), "{error:?}");
    // The reply to another request.
    let error = broken(|_| message(wire::GET_TREE, "[]"));
    assert!(matches!(error, IpcError::Protocol(_)), "{error:?}");
    // A length nobody should allocate.
    let error = broken(|kind| {
        let mut message = b"i3-ipc".to_vec();
        message.extend_from_slice(&u32::MAX.to_ne_bytes());
        message.extend_from_slice(&kind.to_ne_bytes());
        message
    });
    assert!(matches!(error, IpcError::Protocol(_)), "{error:?}");
    // Not the JSON of the reply.
    let error = broken(|kind| message(kind, r#"{"modes": 3}"#));
    assert!(
        matches!(
            error,
            IpcError::InvalidReply {
                request: "GET_BINDING_MODES",
                ..
            }
        ),
        "{error:?}"
    );
    assert!(
        error
            .to_string()
            .starts_with("can't read sway's reply to GET_BINDING_MODES: ")
    );

    // sway goes away in the middle of a reply.
    let sway = FakeSway::with_version(SWAY_1_10, |kind, _| {
        let mut answer = HANG_UP.to_vec();
        answer.extend_from_slice(&message(kind, r#"["default"]"#)[..16]);
        answer
    });
    let mut client = sway.connect();
    let error = block_on(client.binding_modes()).unwrap_err();
    assert!(
        matches!(&error, IpcError::Io(source) if source.kind() == ErrorKind::UnexpectedEof),
        "{error:?}"
    );

    // A version that isn't one.
    let sway = FakeSway::new(|kind, _| {
        message(
            kind,
            r#"{"human_readable": "?", "major": -1, "minor": 0, "patch": 0,
                "loaded_config_file_name": ""}"#,
        )
    });
    assert!(matches!(
        block_on(Client::connect_to(&sway.socket)),
        Err(IpcError::Protocol(_))
    ));
}

#[test]
fn subscribed_events_arrive_in_order() {
    let sway = FakeSway::with_version(SWAY_1_10, |kind, payload| {
        assert_eq!(kind, wire::SUBSCRIBE);
        assert_eq!(payload, r#"["tick","mode","shutdown","input"]"#);
        let mut answer = HANG_UP.to_vec();
        answer.extend(message(kind, r#"{"success": true}"#));
        for (event, payload) in [
            (0x07, r#"{"first": true, "payload": ""}"#),
            (0x02, r#"{"change": "resize", "pango_markup": false}"#),
            // An event that can't be read doesn't end the subscription.
            (0x07, r#"{"first": "no"}"#),
            // Neither does one this client doesn't know.
            (0x63, r#"{"future": true}"#),
            (0x07, r#"{"first": false, "payload": "ping"}"#),
            (0x06, r#"{"change": "exit"}"#),
        ] {
            answer.extend(message(wire::EVENT | event, payload));
        }
        answer
    });
    let client = sway.connect();
    let mut events = block_on(client.subscribe(&[
        EventType::Tick,
        EventType::Mode,
        EventType::Shutdown,
        EventType::Input,
    ]))
    .unwrap();
    let mut next = || block_on(events.next());
    assert!(matches!(next(), Some(Ok(Event::Tick(tick))) if tick.first));
    assert!(matches!(next(), Some(Ok(Event::Mode(mode))) if mode.change == "resize"));
    assert!(matches!(
        next(),
        Some(Err(IpcError::InvalidReply {
            request: "an event",
            ..
        }))
    ));
    assert!(matches!(next(), Some(Ok(Event::Tick(tick))) if tick.payload == "ping"));
    assert!(matches!(
        next(),
        Some(Ok(Event::Shutdown(shutdown))) if shutdown.change == ShutdownChange::Exit
    ));
    // sway closed the connection.
    assert!(next().is_none());
    assert!(next().is_none());
}

#[test]
fn a_refused_or_broken_subscription_is_an_error() {
    let sway = FakeSway::with_version(SWAY_1_10, |kind, _| message(kind, r#"{"success": false}"#));
    assert!(matches!(
        block_on(sway.connect().subscribe(&[EventType::Tick])),
        Err(IpcError::Protocol(_))
    ));

    // A reply where an event should be ends the events.
    let sway = FakeSway::with_version(SWAY_1_10, |kind, _| {
        let mut answer = message(kind, r#"{"success": true}"#);
        answer.extend(message(wire::GET_TREE, "{}"));
        answer.extend(message(
            wire::EVENT | 0x07,
            r#"{"first": true, "payload": ""}"#,
        ));
        answer
    });
    let mut events = block_on(sway.connect().subscribe(&[EventType::Tick])).unwrap();
    assert!(matches!(
        block_on(events.next()),
        Some(Err(IpcError::Protocol(_)))
    ));
    assert!(block_on(events.next()).is_none());
}
