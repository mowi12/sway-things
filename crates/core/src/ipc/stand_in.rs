// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A stand-in for sway on a Unix socket, for the tests of this crate and of the crates built on
//! it (the `test-support` feature). It answers each request with what a closure returns and
//! records the requests.

#![allow(
    clippy::unwrap_used,
    clippy::missing_panics_doc,
    reason = "a test helper: a stand-in that breaks fails the test"
)]

use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;

use async_io::block_on;

use super::{Client, wire};

/// The reply of sway 1.10.1 to `GET_VERSION`.
pub const SWAY_1_10: &str = r#"{"human_readable": "1.10.1", "variant": "sway", "major": 1,
    "minor": 10, "patch": 1, "loaded_config_file_name": "/home/me/.config/sway/config"}"#;

/// An answer that starts with this is sent without it, and then the connection is closed.
pub const HANG_UP: &[u8] = b"<hang up>";

/// A message as sway sends it.
pub fn message(kind: u32, payload: &str) -> Vec<u8> {
    let mut message = b"i3-ipc".to_vec();
    message.extend_from_slice(&u32::try_from(payload.len()).unwrap().to_ne_bytes());
    message.extend_from_slice(&kind.to_ne_bytes());
    message.extend_from_slice(payload.as_bytes());
    message
}

/// A stand-in for sway: answers every request on its socket with what `answer` returns for the
/// message type and payload, and records the requests.
pub struct FakeSway {
    _dir: tempfile::TempDir,
    socket: PathBuf,
    requests: Arc<Mutex<Vec<(u32, String)>>>,
}

impl FakeSway {
    /// A sway that answers every request with what `answer` returns for its message type and
    /// payload.
    pub fn new(answer: impl Fn(u32, &str) -> Vec<u8> + Send + Sync + 'static) -> Self {
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
    pub fn with_version(
        version: impl Into<String>,
        answer: impl Fn(u32, &str) -> Vec<u8> + Send + Sync + 'static,
    ) -> Self {
        let version = version.into();
        Self::new(move |kind, payload| match kind {
            wire::GET_VERSION => message(kind, &version),
            _ => answer(kind, payload),
        })
    }

    /// The path of the socket, for `$SWAYSOCK`.
    pub fn socket(&self) -> &std::path::Path {
        &self.socket
    }

    /// A client connected to the stand-in.
    pub fn connect(&self) -> Client {
        block_on(Client::connect_to(&self.socket)).unwrap()
    }

    /// The commands received so far, as sent.
    pub fn commands(&self) -> Vec<String> {
        self.requests()
            .into_iter()
            .filter(|(kind, _)| *kind == wire::RUN_COMMAND)
            .map(|(_, payload)| payload)
            .collect()
    }

    /// The requests received so far, without `GET_VERSION`.
    pub fn requests(&self) -> Vec<(u32, String)> {
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
pub fn with_inputs(inputs: &'static [&'static str]) -> FakeSway {
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

/// A sway on which every command succeeds, except those that contain `fail`, and that has no
/// input devices.
pub fn obedient(version: impl Into<String>) -> FakeSway {
    obedient_with_inputs(version, "[]")
}

/// A sway on which every command succeeds, except those that contain `fail`, and that answers
/// `GET_INPUTS` with `inputs`.
pub fn obedient_with_inputs(version: impl Into<String>, inputs: impl Into<String>) -> FakeSway {
    let inputs = inputs.into();
    FakeSway::with_version(version, move |kind, payload| {
        if kind == wire::GET_INPUTS {
            return message(kind, &inputs);
        }
        let reply = if payload.contains("fail") {
            r#"[{"success": false, "parse_error": true, "error": "Unknown/invalid command 'fail'"}]"#
        } else {
            r#"[{"success": true}]"#
        };
        message(kind, reply)
    })
}
