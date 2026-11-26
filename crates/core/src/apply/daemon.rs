// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The notification daemon's part of persisting: our files for mako and swaync are written
//! from the stored settings, the daemon that runs reads them again, and a daemon chosen to start
//! with sway starts now if none runs.

use super::{Applier, Validate};
use crate::i18n::Text;
use crate::msg;
use crate::notifications::files::{Files, Inclusion};
use crate::notifications::starts;
use crate::notifications::{self, Daemon, Notifications, Server, Stored, mako, swaync};
use crate::registry::Value;
use crate::registry::notifications::START;

/// What changed for the notification daemon since its files were last written.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct Changes {
    /// The daemon chosen to start with sway.
    pub(super) start: bool,
    /// A setting its files hold: one of the Notifications page, or a window color it may
    /// follow.
    pub(super) settings: bool,
}

/// Where the files for the notification daemons go, and the bus to reach the daemon on.
#[derive(Debug, Clone)]
pub(super) struct DaemonFiles {
    pub(super) files: Files,
    pub(super) bus: Option<Notifications>,
}

impl<V: Validate> Applier<V> {
    /// Writes the files for the notification daemons to `files` from now on, and reaches the
    /// daemon on `bus`. An applier made for tests writes none until told.
    pub fn set_daemon_files(&mut self, files: Files, bus: Option<Notifications>) {
        self.daemon = Some(DaemonFiles { files, bus });
    }

    /// The files for the notification daemons, if they are written.
    pub fn daemon_files(&self) -> Option<&Files> {
        self.daemon.as_ref().map(|daemon| &daemon.files)
    }

    /// The session bus the notification daemon is reached on, if any.
    pub fn daemon_bus(&self) -> Option<&Notifications> {
        self.daemon.as_ref().and_then(|daemon| daemon.bus.as_ref())
    }

    /// What went wrong with the files of the notification daemon since last asked.
    pub fn take_daemon_problem(&mut self) -> Option<Text> {
        self.daemon_problem.take()
    }

    /// Has our files for mako and swaync say what is stored, when a setting of the page (or the
    /// window colors it follows) changed, or the daemon chosen on the page lacks them: swaync
    /// falls back to the user's own config when the file it was started with is missing, and
    /// mako rejects its whole config over an include it can't find. mako's file is written while
    /// mako is installed or has a config; the user's mako config is only created for a mako
    /// chosen on the page, and never where mako runs with a config of its own (`-c`) or a
    /// system-wide one exists. When a file changed, the daemon that runs reads its config again.
    /// Best effort: a problem is kept for [`Applier::take_daemon_problem`].
    ///
    /// # Blocking
    ///
    /// Waits for `swaync --version`.
    pub(super) async fn keep_daemon_files(&mut self) {
        let Some(daemon) = self.daemon.clone() else {
            return;
        };
        let Changes {
            settings: touched,
            start,
        } = std::mem::take(&mut self.daemon_changes);
        let chosen = chosen(&self.state);
        let files = &daemon.files;
        let mako_wanted = touched
            || (chosen == Some(Daemon::Mako) && !files.mako_ours.is_file())
            || (files.mako_inclusion() == Inclusion::Included && !files.mako_ours.is_file());
        let swaync_wanted = touched
            || (chosen == Some(Daemon::Swaync)
                && !(files.swaync_config.is_file() && files.swaync_style.is_file()));
        if !(mako_wanted || swaync_wanted || start) {
            return;
        }
        let server = match &daemon.bus {
            Some(bus) => bus.server().await.ok().flatten(),
            None => None,
        };
        // A mako started with `-c` reads that config.
        let files = match server
            .as_ref()
            .filter(|server| server.daemon == Daemon::Mako)
            .and_then(|server| server.pid)
            .and_then(starts::config_arg)
        {
            Some(config) => files.clone().with_mako_config(config),
            None => files.clone(),
        };
        let stored = Stored::new(&self.state);
        let mut written = false;
        let mut problem = None;
        let makes_mako =
            Daemon::Mako.installed() || files.mako_config.exists() || chosen == Some(Daemon::Mako);
        if mako_wanted && makes_mako {
            let create = chosen == Some(Daemon::Mako);
            match files.write_mako(&mako::generate(&stored), create) {
                Ok(changed) => written |= changed,
                Err(error) => problem = Some(error.text()),
            }
        }
        if swaync_wanted && (Daemon::Swaync.installed() || files.swaync_config.exists()) {
            let config = swaync::config(&stored, swaync::default_config());
            let gtk = match &server {
                Some(server) if server.daemon == Daemon::Swaync => {
                    swaync::Gtk::of_version(&server.version)
                }
                _ => swaync::Gtk::installed(),
            };
            match files.write_swaync(&config, &swaync::style(&stored, gtk)) {
                Ok(changed) => written |= changed,
                Err(error) => problem = Some(error.text()),
            }
        }
        if let (Some(bus), Some(server), true) = (&daemon.bus, &server, written) {
            if let Err(error) = bus.reload(server).await {
                problem = Some(error.text());
            }
        }
        if let (Some(bus), true) = (&daemon.bus, start) {
            if let Some(refused) = self.switch_daemon(bus, server.as_ref(), chosen).await {
                problem = Some(refused);
            }
        }
        self.daemon_problem = problem;
    }

    /// Has sway start the daemon `chosen` on the page at once (the line in our include only
    /// runs when sway starts or reloads), if none runs. A daemon that runs and isn't the one
    /// chosen is ended first only if our hook started it ([`notifications::started_by_us`]);
    /// one the user started (a line of their config, a systemd unit, D-Bus activation) is left
    /// alone, and the returned text says how to switch.
    ///
    /// # Blocking
    ///
    /// Waits for `kill` to end the daemon our hook started.
    async fn switch_daemon(
        &mut self,
        bus: &Notifications,
        running: Option<&Server>,
        chosen: Option<Daemon>,
    ) -> Option<Text> {
        let chosen = chosen?;
        if let Some(running) = running {
            if running.daemon == chosen {
                return None;
            }
            let ours = running.pid.filter(|pid| notifications::started_by_us(*pid));
            let Some(pid) = ours else {
                let name = match running.daemon {
                    Daemon::Other => running.name.clone(),
                    daemon => daemon.title().to_owned(),
                };
                return Some(
                    Text::new(msg!(
                        "{running} runs and wasn't started by Sway Things, so it is left alone: \
                         quit it and remove what starts it (a line of your sway config or a \
                         systemd unit), then {chosen} starts with the next login"
                    ))
                    .arg("running", name)
                    .arg("chosen", chosen.title()),
                );
            };
            let _ = std::process::Command::new("kill")
                .arg(pid.to_string())
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
            // It gives up the name as it ends.
            for _ in 0..30 {
                if matches!(bus.server().await, Ok(None)) {
                    break;
                }
                async_io::Timer::after(std::time::Duration::from_millis(100)).await;
            }
        }
        if let Some(client) = &mut self.client {
            let _ = client.run(&notifications::start_now()).await;
        }
        None
    }
}

/// The daemon chosen on the page to start with sway.
fn chosen(state: &crate::state::State) -> Option<Daemon> {
    match state.get(START, None) {
        Some(Value::Choice(program)) => Daemon::from_program(program),
        _ => None,
    }
}
