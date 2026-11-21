// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The notification daemon in the checkup: none runs and nothing starts one, two are started,
//! mako's config without our include, swaync without our files.

use super::{Checkup, Code, Finding, Severity};
use crate::generate::Validate;
use crate::i18n::Text;
use crate::msg;
use crate::notifications::files::{Files, Inclusion};
use crate::notifications::starts::{self, Origin};
use crate::notifications::{Daemon, Server, mako};
use crate::registry::Value;
use crate::registry::notifications::START;

/// What the checkup knows of the notification daemon: what the caller asked the bus.
#[derive(Debug, Clone)]
pub struct DaemonLook {
    /// The files for the daemons.
    pub files: Files,
    /// The daemon that owns `org.freedesktop.Notifications`, if one does.
    pub server: Option<Server>,
    /// The daemons installed, of mako, swaync and dunst.
    pub installed: Vec<Daemon>,
    /// The daemons the bus starts on demand ([`starts::activatable`]).
    pub activatable: Vec<Daemon>,
    /// The config the running daemon was given with `-c` ([`starts::config_arg`]).
    pub running_config: Option<std::path::PathBuf>,
}

impl<V: Validate> Checkup<'_, V> {
    /// The findings about the notification daemon.
    pub(super) fn notification_daemon(&self, findings: &mut Vec<Finding>) {
        let Some(look) = &self.notifications else {
            return;
        };
        let chosen = match self.state.get(START, None) {
            Some(Value::Choice(program)) => Daemon::from_program(program),
            _ => None,
        };
        let lines = starts::start_lines(self.status.config(), self.locations.directory(), chosen);
        let mut started: Vec<Daemon> = lines.iter().map(|line| line.daemon).collect();
        started.extend(chosen);
        started.sort_by_key(|daemon| daemon.program());
        started.dedup();
        match &look.server {
            Some(server) => {
                let origin = match starts::origin(server.daemon, server.pid, &lines) {
                    Origin::Line(line) => Text::new(msg!("started by {file}:{line}"))
                        .arg("file", line.file.display())
                        .arg("line", line.line),
                    Origin::Unit(unit) => {
                        Text::new(msg!("started as the systemd unit {unit}")).arg("unit", unit)
                    }
                    Origin::Elsewhere => msg!("started by D-Bus activation or a program").into(),
                };
                let name = match server.daemon {
                    Daemon::Other => server.name.clone(),
                    daemon => daemon.title().to_owned(),
                };
                findings.push(Finding::new(
                    Code::NotificationDaemon,
                    Severity::Ok,
                    Text::new(msg!("{daemon} shows the notifications, {origin}."))
                        .arg("daemon", name)
                        .text("origin", origin),
                ));
            }
            None if started.is_empty() && !look.activatable.is_empty() => {
                let names: Vec<&str> = look
                    .activatable
                    .iter()
                    .map(|daemon| daemon.title())
                    .collect();
                findings.push(Finding::new(
                    Code::NotificationDaemon,
                    Severity::Ok,
                    Text::new(msg!(
                        "No notification daemon runs yet; the bus starts {daemons} with the first \
                         notification."
                    ))
                    .arg("daemons", names.join(", ")),
                ));
            }
            None if started.is_empty() => findings.push(
                Finding::new(
                    Code::NoNotificationDaemon,
                    Severity::Warning,
                    msg!("No notification daemon runs, and your sway config starts none."),
                )
                .with_hint(msg!(
                    "Choose one under Start with sway on the Notifications page, or add exec mako \
                     to your sway config."
                )),
            ),
            None => {}
        }
        if started.len() > 1 {
            let names: Vec<&str> = started.iter().map(|daemon| daemon.title()).collect();
            findings.push(
                Finding::new(
                    Code::TwoNotificationDaemons,
                    Severity::Warning,
                    Text::new(msg!(
                        "{daemons} are all started; only the first to start shows notifications."
                    ))
                    .arg("daemons", names.join(", ")),
                )
                .with_details(
                    lines
                        .iter()
                        .map(|line| {
                            Text::new(msg!("{file}:{line}: {text}"))
                                .arg("file", line.file.display())
                                .arg("line", line.line)
                                .arg("text", &line.text)
                        })
                        .collect(),
                )
                .with_hint(msg!("Remove the lines of all but one.")),
            );
        }
        let running = look.server.as_ref().map(|server| server.daemon);
        self.mako_include(look, running, chosen, findings);
        self.swaync_handover(look, running, findings);
        self.swaync_own_config(look, running, chosen, findings);
    }

    /// Whether a setting of the page whose key starts with `prefix` is stored.
    fn stores(&self, prefix: &str) -> bool {
        self.state
            .entries()
            .iter()
            .any(|entry| entry.setting.key().starts_with(prefix))
    }

    /// mako's config without our include (or with it where mako refuses it), while mako is
    /// chosen on the page or something is set for it. A mako started with `-c` reads that
    /// config.
    fn mako_include(
        &self,
        look: &DaemonLook,
        running: Option<Daemon>,
        chosen: Option<Daemon>,
        findings: &mut Vec<Finding>,
    ) {
        if chosen != Some(Daemon::Mako) && !self.stores("notifications.mako.") {
            return;
        }
        let files = match (&look.running_config, running) {
            (Some(config), Some(Daemon::Mako)) => {
                look.files.clone().with_mako_config(config.clone())
            }
            _ => look.files.clone(),
        };
        let inclusion = files.mako_inclusion();
        let reported = matches!(
            inclusion,
            Inclusion::Missing | Inclusion::InSection | Inclusion::Malformed
        ) || (inclusion == Inclusion::NoConfig && !files.may_create_mako_config());
        if !reported {
            return;
        }
        let own_dnd = std::fs::read_to_string(&files.mako_config)
            .is_ok_and(|text| mako::has_dnd_section(&text));
        let message = if inclusion == Inclusion::Malformed {
            Text::new(msg!(
                "{file} names the file of Sway Things in a way mako refuses, and with it the \
                 whole config: without blanks around =, and with a path that starts with / or ~/."
            ))
        } else if inclusion == Inclusion::NoConfig {
            Text::new(msg!(
                "{file} doesn't exist, and mako runs with a config of its own or there is a \
                 system-wide one, so none is created; what is set on the Notifications page \
                 doesn't reach mako."
            ))
        } else if inclusion == Inclusion::InSection {
            Text::new(msg!(
                "{file} includes the file of Sway Things after a [section], where mako refuses \
                 it and with it the whole config."
            ))
        } else if own_dnd {
            Text::new(msg!(
                "{file} doesn't include the file of Sway Things, so what is set on the \
                 Notifications page doesn't reach mako."
            ))
        } else {
            Text::new(msg!(
                "{file} doesn't include the file of Sway Things, so neither Do Not Disturb nor \
                 what is set on the Notifications page reaches mako."
            ))
        };
        findings.push(
            Finding::new(
                Code::MakoIncludeMissing,
                Severity::Warning,
                message.arg("file", files.mako_config.display()),
            )
            .in_file(&files.mako_config)
            .with_hint(
                Text::new(msg!(
                    "Add {line} to it, after your own options and before the first [section]; \
                     then run makoctl reload."
                ))
                .arg("line", files.mako_include_line()),
            ),
        );
    }

    /// Whether the running swaync reads our config: it was given ours with `-c`.
    fn swaync_runs_ours(look: &DaemonLook) -> bool {
        let canonical = |path: &std::path::Path| std::fs::canonicalize(path).ok();
        look.running_config.as_deref().is_some_and(|config| {
            config == look.files.swaync_config
                || canonical(config)
                    .is_some_and(|config| canonical(&look.files.swaync_config) == Some(config))
        })
    }

    /// swaync that runs without our files while something is set for it (choosing it to start
    /// with sway alone doesn't count).
    fn swaync_handover(
        &self,
        look: &DaemonLook,
        running: Option<Daemon>,
        findings: &mut Vec<Finding>,
    ) {
        if running != Some(Daemon::Swaync) || Self::swaync_runs_ours(look) {
            return;
        }
        let stored = self.state.entries().iter().any(|entry| {
            let key = entry.setting.key();
            key.starts_with("notifications.") && key != START && entry.setting.sway().is_none()
        });
        if !stored {
            return;
        }
        let files = &look.files;
        findings.push(
            Finding::new(
                Code::SwayncNotHandedOver,
                Severity::Warning,
                msg!(
                    "swaync runs with its own config, so what is set on the Notifications page \
                     doesn't reach it."
                ),
            )
            .with_hint(
                Text::new(msg!("Start it with: swaync -c {config} -s {style}"))
                    .arg("config", files.shown(&files.swaync_config))
                    .arg("style", files.shown(&files.swaync_style)),
            ),
        );
    }

    /// swaync on our config while the user has a swaync config of their own that was never
    /// imported: their look and widgets aren't in effect.
    fn swaync_own_config(
        &self,
        look: &DaemonLook,
        running: Option<Daemon>,
        chosen: Option<Daemon>,
        findings: &mut Vec<Finding>,
    ) {
        let ours = (running == Some(Daemon::Swaync) && Self::swaync_runs_ours(look))
            || (running.is_none() && chosen == Some(Daemon::Swaync));
        let files = &look.files;
        if !ours || !files.swaync_user_config.is_file() || self.stores("notifications.swaync.") {
            return;
        }
        findings.push(
            Finding::new(
                Code::SwayncOwnConfigUnused,
                Severity::Warning,
                Text::new(msg!(
                    "swaync runs with the config of Sway Things, so your own {file} isn't in \
                     effect."
                ))
                .arg("file", files.shown(&files.swaync_user_config)),
            )
            .in_file(&files.swaync_user_config)
            .with_hint(msg!(
                "Import it as a starting point: Import My swaync Config on the Notifications \
                 page, or sway-things notifications import-swaync."
            )),
        );
    }
}
