// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A stand-in for logind's session, for tests: a private `dbus-daemon` and, on it, a service
//! that owns `org.freedesktop.login1` and notes what `SetBrightness` was asked. Built with the
//! `test-support` feature; it needs `dbus-daemon`.

use std::sync::{Arc, Mutex};

use zbus::Connection;

use super::{Logind, SESSION};

/// The error of a stand-in.
pub type Error = Box<dyn std::error::Error + Send + Sync>;

/// A call of `SetBrightness`: the subsystem, the device's name and the raw value.
pub type Call = (String, String, u32);

/// logind's session on the stand-in's bus.
struct Session {
    calls: Arc<Mutex<Vec<Call>>>,
}

#[zbus::interface(name = "org.freedesktop.login1.Session")]
impl Session {
    fn set_brightness(&self, subsystem: String, name: String, brightness: u32) {
        if let Ok(mut calls) = self.calls.lock() {
            calls.push((subsystem, name, brightness));
        }
    }
}

/// A private bus with logind's stand-in on it. Stops both when dropped.
pub struct StandIn {
    bus: crate::dbus::TestBus,
    server: Option<Connection>,
    calls: Arc<Mutex<Vec<Call>>>,
}

impl Drop for StandIn {
    fn drop(&mut self) {
        self.server = None;
        let _ = self.bus.daemon.kill();
        let _ = self.bus.daemon.wait();
    }
}

impl StandIn {
    /// Starts a bus and logind's session on it, at the path [`Logind::connect`] uses.
    ///
    /// # Errors
    ///
    /// If `dbus-daemon` can't be started or the name can't be taken.
    pub async fn start() -> Result<Self, Error> {
        let bus = crate::dbus::TestBus::start().await?;
        let calls = Arc::new(Mutex::new(Vec::new()));
        let server = zbus::connection::Builder::address(bus.address.as_str())?
            .name("org.freedesktop.login1")?
            .serve_at(
                SESSION,
                Session {
                    calls: Arc::clone(&calls),
                },
            )?
            .build()
            .await?;
        Ok(Self {
            bus,
            server: Some(server),
            calls,
        })
    }

    /// logind as the code under test finds it, on the stand-in's bus.
    pub fn logind(&self) -> Logind {
        Logind::with(self.bus.connection.clone(), SESSION)
    }

    /// The calls of `SetBrightness` so far, in order.
    pub fn calls(&self) -> Vec<Call> {
        self.calls
            .lock()
            .map(|calls| calls.clone())
            .unwrap_or_default()
    }
}
