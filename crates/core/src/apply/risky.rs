// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The safety net for settings that can lock the user out (a keyboard layout, a device switched
//! off): they are tried live, and stored only once the user keeps them.
//!
//! [`Applier::try_risky`] checks the change, refuses one that would leave no keyboard or no
//! pointing device, notes what is in effect now, sends the new value and returns a
//! [`Countdown`]. [`Applier::try_risky_reset`] does the same for removing the stored value: it
//! tries the value in effect without it. [`Applier::keep`] stores the value (or removes it) like
//! any other change; [`Applier::revert`], which the caller runs when the countdown has elapsed
//! too, sends the old one again. Nothing is stored before `keep`, so if the app or the command
//! line ends in between, the next reload of sway brings back the old value.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use super::{Applied, Applier, ApplyError, Effect, render};
use crate::command::input::InputType;
use crate::command::{SwayCmd, Target};
use crate::conflict::Known;
use crate::devices::{Devices, Lockout, Sending};
use crate::generate::Validate;
use crate::include::{IncludeStatus, WriteAccess};
use crate::ipc::{Client, Compositor};
use crate::outputs::Outputs;
use crate::registry::{Apply, Block, Scope, Setting, Value};
use crate::state::{State, StateError};
use crate::view::{SettingView, Source};

/// The preference that says how long a risky change waits to be kept.
const TIMEOUT: &str = "app.safety_net_timeout";

/// When a risky change that wasn't kept is reverted. It only measures time: the caller reverts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Countdown {
    deadline: Instant,
}

impl Countdown {
    /// When the change is reverted.
    pub fn deadline(self) -> Instant {
        self.deadline
    }

    /// The time left, for the banner or the prompt.
    pub fn remaining(self) -> Duration {
        self.deadline.saturating_duration_since(Instant::now())
    }

    /// Waits until the deadline. It works with any executor (`async_io` keeps the time), so the
    /// app and the command line wait for it alike.
    pub async fn elapsed(self) {
        async_io::Timer::at(self.deadline).await;
    }
}

/// One setting of a risky change: its key, and the value it is tried with.
#[derive(Debug, Clone, PartialEq)]
pub struct TriedValue {
    /// The key of the setting.
    pub key: &'static str,
    /// The new value. For a reset, the value in effect without the stored one.
    pub value: Value,
    /// Whether the change removes the stored value.
    pub reset: bool,
}

/// What [`Applier::try_risky`] and its relatives did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tried {
    /// The change was sent and waits to be [kept](Applier::keep) or
    /// [reverted](Applier::revert).
    Waits(Trial),
    /// The change needed no safety net and was stored at once: it sends nothing (a value sway is
    /// to choose again, which it does at its next reload), or nothing could bring back what was
    /// before (the mode of an output that is off).
    Stored(Applied),
    /// Nothing changes: only resets of settings without a stored value. Nothing was sent.
    Unchanged,
}

impl Tried {
    /// The trial, if the change waits to be kept or reverted.
    pub fn trial(self) -> Option<Trial> {
        match self {
            Self::Waits(trial) => Some(trial),
            Self::Stored(_) | Self::Unchanged => None,
        }
    }
}

/// A risky change that was sent and waits to be kept or reverted. It changes one setting, or
/// several of one device at once ([`Applier::try_risky_all`]).
#[derive(Debug, Clone)]
pub struct Pending {
    device: Option<String>,
    first: TriedValue,
    more: Vec<TriedValue>,
    sent: Vec<String>,
    undo: Vec<SwayCmd>,
    countdown: Countdown,
}

impl Pending {
    /// The key of the setting, the first one for a change of several.
    pub fn key(&self) -> &'static str {
        self.first.key
    }

    /// The device the change is for, or `None` for the whole session or input type.
    pub fn device(&self) -> Option<&str> {
        self.device.as_deref()
    }

    /// The new value of the first setting. For a reset, the value in effect without the stored
    /// one.
    pub fn value(&self) -> &Value {
        &self.first.value
    }

    /// Whether the change of the first setting removes the stored value
    /// ([`Applier::try_risky_reset`]).
    pub fn is_reset(&self) -> bool {
        self.first.reset
    }

    /// Every setting the change tries, the first one first.
    pub fn values(&self) -> impl Iterator<Item = &TriedValue> {
        std::iter::once(&self.first).chain(&self.more)
    }

    /// When it is reverted unless it is kept.
    pub fn countdown(&self) -> Countdown {
        self.countdown
    }
}

/// What [`Applier::try_risky`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trial {
    /// The commands that were sent, as sent over IPC.
    pub commands: Vec<String>,
    /// When the change is reverted unless it is kept.
    pub countdown: Countdown,
}

impl<V: Validate> Applier<V> {
    /// The risky change that waits to be kept or reverted, if there is one.
    pub fn pending(&self) -> Option<&Pending> {
        self.pending.as_ref()
    }

    /// Tries `value` for the risky setting `key`, for the whole session or input type, or for
    /// one input `device`: sends it to the session without storing it, and returns when it is
    /// reverted unless [kept](Applier::keep). What is in effect now (as in
    /// [`SettingView`], with the user's config from `status`) is what [`Applier::revert`]
    /// restores.
    ///
    /// # Errors
    ///
    /// Nothing was sent then:
    /// [`ApplyError::NotRisky`] for a setting without the safety net (use [`Applier::set`]),
    /// [`ApplyError::PendingChange`] while another change waits to be kept or reverted,
    /// [`ApplyError::NoSession`] without a session to try it in, [`ApplyError::Lockout`] if it
    /// would leave no keyboard, no pointing device or no output that is on, and the errors of
    /// [`Applier::set`]. If a command fails, the commands sent before it are undone.
    pub async fn try_risky(
        &mut self,
        key: &str,
        device: Option<&str>,
        value: Value,
        status: &IncludeStatus,
        access: &WriteAccess,
    ) -> Result<Tried, ApplyError> {
        self.try_risky_all(device, &[(key, Some(value))], status, access)
            .await
    }

    /// Tries removing the stored value of the risky setting `key`, for the whole session or input
    /// type, or for one input `device`: sends the value in effect without it, as a row would show
    /// it then (what the user's config sets, else the default; for one device, the value of its
    /// type), and returns when it is reverted unless [kept](Applier::keep). Keeping removes the
    /// stored value. [`Tried::Unchanged`] if no value is stored: nothing was sent then.
    ///
    /// # Errors
    ///
    /// Like [`Applier::try_risky`].
    pub async fn try_risky_reset(
        &mut self,
        key: &str,
        device: Option<&str>,
        status: &IncludeStatus,
        access: &WriteAccess,
    ) -> Result<Tried, ApplyError> {
        self.try_risky_all(device, &[(key, None)], status, access)
            .await
    }

    /// Tries several risky settings of one input `device` (or of the whole session or input
    /// type) as one change, which is kept or reverted as a whole: the layouts with their
    /// variants. `changes` gives each key its new value, or `None` to remove the stored value as
    /// [`Applier::try_risky_reset`] does. The directives are sent in the order of the registry;
    /// a setting that [clears another one first](Setting::clears_first) that is in the change
    /// too sends that one at its default before. `None` if nothing changes (only resets of
    /// settings without a stored value): nothing was sent then.
    ///
    /// # Errors
    ///
    /// Like [`Applier::try_risky`], for any of the settings.
    pub async fn try_risky_all(
        &mut self,
        device: Option<&str>,
        changes: &[(&str, Option<Value>)],
        status: &IncludeStatus,
        _access: &WriteAccess,
    ) -> Result<Tried, ApplyError> {
        self.nothing_pending()?;
        let (tried, settings_changed) = self.as_tried(device, changes)?;
        if settings_changed.is_empty() {
            return Ok(Tried::Unchanged);
        }
        for (setting, _) in &settings_changed {
            self.check_supported(setting)?;
        }
        let settings: Vec<&'static Setting> = settings_changed
            .iter()
            .map(|(setting, _)| *setting)
            .collect();
        let key = settings.first().map_or("", |setting| setting.key());
        let outputs = self.live_outputs(key, &settings).await?;
        let known = Known {
            devices: None,
            outputs: outputs.as_ref(),
        };
        let (values, over) = targets(
            &settings_changed,
            device,
            &tried,
            status,
            self.compositor,
            known,
        );
        let (session, runs_as_tried) =
            as_running(&self.state, &settings_changed, device, outputs.as_ref())?;
        let snapshot = snapshot(&settings, device, &session, status, self.compositor, known);
        let now = (&session, &snapshot);
        let then = (&tried, &over);
        let commands = sequence(now, then, &settings, device)?;
        let undo = sequence(then, now, &settings, device)?;
        if runs_as_tried || commands.is_empty() {
            // Nothing to try: the output runs with the new values already, or nothing changes
            // in the session (a mode sway chose is no longer stored), which then takes effect
            // after a reload of sway. The change is stored.
            let effect = if runs_as_tried {
                Effect::Live {
                    commands: Vec::new(),
                }
            } else {
                Effect::AfterReload
            };
            self.store_untried(tried, &settings_changed, device, &effect);
            return Ok(Tried::Stored(Applied {
                changed: true,
                effect,
                not_copied: Vec::new(),
            }));
        }
        let switches = settings_changed.iter().find_map(|(setting, _)| {
            let value = values.iter().find(|value| value.key == setting.key())?;
            switches(setting, &value.value)
        });
        let Some(client) = &mut self.client else {
            return Err(ApplyError::NoSession { key });
        };
        let sent = send_guarded(client, key, device, switches, &commands, &undo).await?;
        if undo.is_empty() {
            // Nothing can bring back what was before (the mode of an output that is off, which
            // sway doesn't report): there is nothing to revert to, and nothing the change could
            // lock the user out of. It is stored at once.
            let effect = Effect::Live { commands: sent };
            self.store_untried(tried, &settings_changed, device, &effect);
            return Ok(Tried::Stored(Applied {
                changed: true,
                effect,
                not_copied: Vec::new(),
            }));
        }

        let countdown = Countdown {
            deadline: Instant::now() + self.timeout(),
        };
        let mut values = values.into_iter();
        let Some(first) = values.next() else {
            return Ok(Tried::Unchanged);
        };
        self.pending = Some(Pending {
            device: device.map(str::to_owned),
            first,
            more: values.collect(),
            sent: sent.clone(),
            undo,
            countdown,
        });
        Ok(Tried::Waits(Trial {
            commands: sent,
            countdown,
        }))
    }

    /// The state as it would be with `changes`, which also checks the keys, the device and the
    /// values, and the settings that change.
    #[allow(
        clippy::type_complexity,
        reason = "a state and the changes that make it, used once"
    )]
    fn as_tried(
        &self,
        device: Option<&str>,
        changes: &[(&str, Option<Value>)],
    ) -> Result<(State, Vec<(&'static Setting, Option<Value>)>), ApplyError> {
        let mut tried = self.state.clone();
        let mut settings_changed = Vec::new();
        for (key, change) in changes {
            let setting = self.risky_setting(key)?;
            match change {
                Some(value) => {
                    tried.set(key, device, value.clone(), None)?;
                    settings_changed.push((setting, Some(value.clone())));
                }
                None => {
                    if tried.reset(key, device)? {
                        settings_changed.push((setting, None));
                    }
                }
            }
        }
        Ok((tried, settings_changed))
    }

    /// The outputs of the session, if one of `settings` is a setting of outputs: a value of an
    /// output that nothing sets is what the output runs with. `None` for other settings.
    ///
    /// # Errors
    ///
    /// [`ApplyError::NoSession`] without a session, [`ApplyError::Ipc`] if sway can't list its
    /// outputs.
    async fn live_outputs(
        &mut self,
        key: &'static str,
        settings: &[&'static Setting],
    ) -> Result<Option<Outputs>, ApplyError> {
        if !settings
            .iter()
            .any(|setting| setting.scope() == Scope::PerOutput)
        {
            return Ok(None);
        }
        let Some(client) = &mut self.client else {
            return Err(ApplyError::NoSession { key });
        };
        let found = client.outputs().await.map_err(ApplyError::Ipc)?;
        Ok(Some(Outputs::new(&found)))
    }

    /// Stores a risky change that needs no safety net, as [`Applier::keep`] would, and notes how
    /// it takes `effect`: there is nothing to keep or revert.
    fn store_untried(
        &mut self,
        tried: State,
        changed: &[(&'static Setting, Option<Value>)],
        device: Option<&str>,
        effect: &Effect,
    ) {
        let before = std::mem::replace(&mut self.state, tried);
        self.unsaved_since = Some(Instant::now());
        for (setting, _) in changed {
            let was = before.get(setting.key(), device).cloned();
            if effect != &Effect::AfterReload {
                self.waiting.forget(setting.key(), device);
            }
            self.note(setting, device, effect, was);
        }
    }

    /// Keeps the pending risky change: stores its value (or for a reset, removes the stored one)
    /// like any other change, to be saved with [`Applier::persist`]. Nothing is sent; the value
    /// is in effect already.
    ///
    /// # Errors
    ///
    /// [`ApplyError::NothingPending`] if no change waits, and [`ApplyError::State`] if the value
    /// can't be stored (the change still waits then).
    pub fn keep(&mut self, _access: &WriteAccess) -> Result<Applied, ApplyError> {
        let pending = self.pending.take().ok_or(ApplyError::NothingPending)?;
        let device = pending.device.as_deref();
        let mut state = self.state.clone();
        let stored = pending.values().try_for_each(|tried| {
            if tried.reset {
                state.reset(tried.key, device).map(drop)
            } else {
                state.set(tried.key, device, tried.value.clone(), None)
            }
        });
        if let Err(error) = stored {
            self.pending = Some(pending);
            return Err(error.into());
        }
        self.state = state;
        self.unsaved_since = Some(Instant::now());
        // The kept value is in effect now, whatever waited for a reload before.
        for tried in pending.values() {
            self.waiting.forget(tried.key, device);
        }
        Ok(Applied {
            changed: true,
            effect: Effect::Live {
                commands: pending.sent,
            },
            not_copied: Vec::new(),
        })
    }

    /// Reverts the pending risky change: sends the value that was in effect before it again.
    /// Call it when the user says so and when the [`Countdown`] has elapsed.
    ///
    /// # Errors
    ///
    /// [`ApplyError::NothingPending`] if no change waits, [`ApplyError::NoSession`] if the
    /// session is gone, and [`ApplyError::Ipc`] if sway rejects a command; the change no longer
    /// waits then, and a reload of sway brings back the stored value.
    pub async fn revert(&mut self) -> Result<Effect, ApplyError> {
        let pending = self.pending.take().ok_or(ApplyError::NothingPending)?;
        let Some(client) = &mut self.client else {
            return Err(ApplyError::NoSession {
                key: pending.first.key,
            });
        };
        let mut sent = Vec::with_capacity(pending.undo.len());
        for command in &pending.undo {
            client.run(command).await.map_err(ApplyError::Ipc)?;
            sent.push(command.render(Target::Ipc).map_err(ApplyError::Command)?);
        }
        Ok(Effect::Live { commands: sent })
    }

    /// Changes the risky setting `key` without the safety net, for `sway-things set --yes` in
    /// scripts: sends the value if there is a session, and stores it. The lockout protection
    /// still applies: against the connected devices, or without a session, to switching off a
    /// whole type of keyboards or pointing devices.
    ///
    /// # Errors
    ///
    /// [`ApplyError::NotRisky`], [`ApplyError::PendingChange`] and [`ApplyError::Lockout`] as for
    /// [`Applier::try_risky`], and the errors of [`Applier::set`]. Nothing is stored if sway
    /// rejects the value.
    pub async fn set_risky_unchecked(
        &mut self,
        key: &str,
        device: Option<&str>,
        value: Value,
        _access: &WriteAccess,
    ) -> Result<Applied, ApplyError> {
        self.nothing_pending()?;
        let setting = self.risky_setting(key)?;
        let mut changed = self.state.clone();
        changed.set(key, device, value.clone(), None)?;
        self.check_supported(setting)?;
        let commands = render(&changed, &BTreeMap::new(), setting, device, Target::Ipc)?;
        let effect = self
            .send_unchecked(setting, device, &value, &commands)
            .await?;
        let stored = self.state.get(key, device) != Some(&value);
        self.state = changed;
        if stored {
            self.unsaved_since = Some(Instant::now());
        }
        Ok(Applied {
            changed: stored,
            effect,
            not_copied: Vec::new(),
        })
    }

    /// Removes the stored value of the risky setting `key` without the safety net, for
    /// `sway-things reset --yes` in scripts: sends the value in effect without it if there is a
    /// session (as [`Applier::try_risky_reset`] would try it), and removes it. The lockout
    /// protection applies as for [`Applier::set_risky_unchecked`]. A setting the compositor
    /// doesn't support is removed without sending anything.
    ///
    /// # Errors
    ///
    /// Like [`Applier::set_risky_unchecked`]. Nothing is removed if sway rejects the value.
    pub async fn reset_risky_unchecked(
        &mut self,
        key: &str,
        device: Option<&str>,
        status: &IncludeStatus,
        _access: &WriteAccess,
    ) -> Result<Applied, ApplyError> {
        self.nothing_pending()?;
        let setting = self.risky_setting(key)?;
        let mut changed = self.state.clone();
        if !changed.reset(key, device)? {
            return Ok(Applied {
                changed: false,
                effect: Effect::NotStored,
                not_copied: Vec::new(),
            });
        }
        let effect =
            if let Err(ApplyError::Unsupported { unmet, .. }) = self.check_supported(setting) {
                Effect::Unsupported(unmet)
            } else {
                let outputs = match self.client {
                    Some(_) => self.live_outputs(setting.key(), &[setting]).await?,
                    None => None,
                };
                let known = Known {
                    devices: None,
                    outputs: outputs.as_ref(),
                };
                let over = in_effect(
                    setting,
                    device,
                    &changed,
                    status,
                    self.compositor,
                    known,
                    Session::Ignored,
                );
                let value = over
                    .get(setting.key())
                    .cloned()
                    .unwrap_or_else(|| setting.default_value());
                let commands = render(&changed, &over, setting, device, Target::Ipc)?;
                self.send_unchecked(setting, device, &value, &commands)
                    .await?
            };
        self.state = changed;
        self.unsaved_since = Some(Instant::now());
        Ok(Applied {
            changed: true,
            effect,
            not_copied: Vec::new(),
        })
    }

    /// Sends `commands`, which set `setting` to `value`, without the safety net: guarded against
    /// a lockout, and without a session only checked against it.
    async fn send_unchecked(
        &mut self,
        setting: &'static Setting,
        device: Option<&str>,
        value: &Value,
        commands: &[SwayCmd],
    ) -> Result<Effect, ApplyError> {
        let switches = switches(setting, value);
        let Some(client) = &mut self.client else {
            if let Some(lockout) = switches.and_then(|switch| switch.lockout_unknown(device)) {
                return Err(ApplyError::Lockout {
                    key: setting.key(),
                    lockout,
                });
            }
            return Ok(Effect::NoSession);
        };
        let undo =
            render(&self.state, &BTreeMap::new(), setting, device, Target::Ipc).unwrap_or_default();
        let sent = send_guarded(client, setting.key(), device, switches, commands, &undo).await?;
        Ok(Effect::Live { commands: sent })
    }

    /// Refuses a risky change while another one waits.
    fn nothing_pending(&self) -> Result<(), ApplyError> {
        match &self.pending {
            Some(pending) => Err(ApplyError::PendingChange { key: pending.key() }),
            None => Ok(()),
        }
    }

    /// Refuses a setting the compositor doesn't support.
    fn check_supported(&self, setting: &'static Setting) -> Result<(), ApplyError> {
        let requirements = setting.requirements();
        self.compositor
            .check(requirements.min_sway, requirements.min_swayfx)
            .map_err(|unmet| ApplyError::Unsupported {
                key: setting.key(),
                unmet,
            })
    }

    /// The risky setting `key`.
    fn risky_setting(&self, key: &str) -> Result<&'static Setting, ApplyError> {
        let setting = self
            .state
            .registry()
            .get(key)
            .ok_or_else(|| StateError::UnknownKey {
                key: key.to_owned(),
            })?;
        if setting.apply() == Apply::Risky {
            Ok(setting)
        } else {
            Err(ApplyError::NotRisky { key: setting.key() })
        }
    }

    /// How long a risky change waits to be kept: the user's preference, or its default.
    fn timeout(&self) -> Duration {
        let seconds = self.state.get(TIMEOUT, None).cloned().or_else(|| {
            self.state
                .registry()
                .get(TIMEOUT)
                .map(Setting::default_value)
        });
        match seconds {
            Some(Value::Integer(seconds)) => {
                Duration::from_secs(u64::try_from(seconds).unwrap_or(0))
            }
            _ => Duration::from_secs(15),
        }
    }
}

/// What is in effect now for every setting of the directives of `settings`, which a revert
/// restores. Where nothing sets an output's value, that is what the output runs with (the mode
/// sway chose).
fn snapshot(
    settings: &[&'static Setting],
    device: Option<&str>,
    state: &State,
    status: &IncludeStatus,
    compositor: Compositor,
    known: Known<'_>,
) -> BTreeMap<String, Value> {
    settings
        .iter()
        .flat_map(|setting| {
            in_effect(
                setting,
                device,
                state,
                status,
                compositor,
                known,
                Session::Counts,
            )
        })
        .collect()
}

/// `state` as the session runs: where an output runs with something else than what is stored
/// (an output stored as off that `hook displays` switched on), what it runs with, so a revert
/// brings that back and never the stored value. And whether the output runs with every value of
/// `changed` already.
fn as_running(
    state: &State,
    changed: &[(&'static Setting, Option<Value>)],
    device: Option<&str>,
    outputs: Option<&Outputs>,
) -> Result<(State, bool), ApplyError> {
    let mut session = state.clone();
    let mut runs_as_tried = true;
    for (setting, change) in changed {
        let live = device
            .and_then(|device| outputs?.find(device))
            .and_then(|output| output.session_value(setting.key()));
        if let (Some(live), Some(stored)) = (&live, session.get(setting.key(), device)) {
            if live != stored {
                session.set(setting.key(), device, live.clone(), None)?;
            }
        }
        runs_as_tried &= live.is_some() && live.as_ref() == change.as_ref();
    }
    Ok((session, runs_as_tried))
}

/// The value each setting of a risky change is tried with, and the values in effect of the
/// directives of the settings whose stored value is removed: what the user's config sets, else
/// the default. Without the stored value, sway chooses again at its next reload, so what an output
/// runs with now doesn't count.
fn targets(
    changed: &[(&'static Setting, Option<Value>)],
    device: Option<&str>,
    tried: &State,
    status: &IncludeStatus,
    compositor: Compositor,
    known: Known<'_>,
) -> (Vec<TriedValue>, BTreeMap<String, Value>) {
    let mut over = BTreeMap::new();
    let mut values = Vec::with_capacity(changed.len());
    for (setting, change) in changed {
        let value = if let Some(value) = change {
            value.clone()
        } else {
            let effect = in_effect(
                setting,
                device,
                tried,
                status,
                compositor,
                known,
                Session::Ignored,
            );
            let value = effect
                .get(setting.key())
                .cloned()
                .unwrap_or_else(|| setting.default_value());
            over.extend(effect);
            value
        };
        values.push(TriedValue {
            key: setting.key(),
            value,
            reset: change.is_none(),
        });
    }
    (values, over)
}

/// Whether what an output runs with now counts as the value in effect where nothing sets it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Session {
    /// It does: the value to go back to.
    Counts,
    /// It doesn't: the value without a stored one, which sway chooses again at its next reload.
    Ignored,
}

/// The value in effect for each setting of the directive of `setting` in `state`, as a row shows
/// it ([`SettingView`]).
fn in_effect(
    setting: &'static Setting,
    device: Option<&str>,
    state: &State,
    status: &IncludeStatus,
    compositor: Compositor,
    known: Known<'_>,
    session: Session,
) -> BTreeMap<String, Value> {
    state
        .registry()
        .group(setting)
        .map(|member| {
            let view = SettingView::of(member, device, state, status, compositor, known);
            let value = if session == Session::Ignored && view.source == Source::Session {
                member.default_value()
            } else {
                view.value
            };
            (member.key().to_owned(), value)
        })
        .collect()
}

/// A state, and the values to use in place of its own.
type Values<'a> = (&'a State, &'a BTreeMap<String, Value>);

/// The commands that change `settings` from the values in `from` to those in `to`: each
/// directive once, in the order of the registry. A setting that [clears another one
/// first](Setting::clears_first) that is among them sends that one at its default before, as it
/// fits the values in `from`.
fn sequence(
    (from, from_over): Values<'_>,
    (to, to_over): Values<'_>,
    settings: &[&'static Setting],
    device: Option<&str>,
) -> Result<Vec<SwayCmd>, ApplyError> {
    let registry = to.registry();
    let among = |key: &str| settings.iter().any(|setting| setting.key() == key);
    let mut commands = Vec::new();
    for setting in settings {
        let Some(cleared) = setting
            .clears_first()
            .filter(|key| among(key))
            .and_then(|key| registry.get(key))
        else {
            continue;
        };
        let mut over = from_over.clone();
        over.insert(cleared.key().to_owned(), cleared.default_value());
        commands.extend(render(from, &over, cleared, device, Target::Ipc)?);
    }
    let mut sent: Vec<&str> = Vec::new();
    for setting in registry.iter().filter(|setting| among(setting.key())) {
        if sent.contains(&setting.key()) {
            continue;
        }
        sent.extend(registry.group(setting).map(Setting::key));
        commands.extend(render(to, to_over, setting, device, Target::Ipc)?);
    }
    Ok(commands)
}

/// Refuses a change that `switches` devices or outputs if it would leave the session without a
/// keyboard, a pointing device or an output that is on, then sends `commands`. After a command failed, `undo` restores what was
/// sent before it, as far as sway allows. Returns the commands as sent.
async fn send_guarded(
    client: &mut Client,
    key: &'static str,
    device: Option<&str>,
    switches: Option<Switch>,
    commands: &[SwayCmd],
    undo: &[SwayCmd],
) -> Result<Vec<String>, ApplyError> {
    let lockout = match switches {
        Some(Switch::Inputs(kind, sending)) => {
            let devices = Devices::new(&client.inputs().await.map_err(ApplyError::Ipc)?);
            devices.lockout(kind, device, sending)
        }
        Some(Switch::Outputs(enabled)) => {
            let outputs = Outputs::new(&client.outputs().await.map_err(ApplyError::Ipc)?);
            outputs.lockout(device, enabled)
        }
        None => None,
    };
    if let Some(lockout) = lockout {
        return Err(ApplyError::Lockout { key, lockout });
    }
    let mut sent = Vec::with_capacity(commands.len());
    for command in commands {
        if let Err(error) = client.run(command).await {
            if !sent.is_empty() {
                // Best effort: the error of the command that failed is what counts.
                let _ = client.run_all(undo).await;
            }
            return Err(ApplyError::Ipc(error));
        }
        sent.push(command.render(Target::Ipc).map_err(ApplyError::Command)?);
    }
    Ok(sent)
}

/// What a change switches on or off, for the lockout protection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Switch {
    /// Input devices of a type, to a sending mode (sway's `events` option).
    Inputs(InputType, Sending),
    /// Outputs, on or off.
    Outputs(bool),
}

impl Switch {
    /// What the change could leave the user without when the session's devices and outputs
    /// aren't known.
    fn lockout_unknown(self, device: Option<&str>) -> Option<Lockout> {
        match self {
            Self::Inputs(kind, sending) => Lockout::without_devices(kind, device, sending),
            Self::Outputs(enabled) => Outputs::lockout_unknown(device, enabled),
        }
    }
}

/// What `value` of `setting` switches on or off: input devices (sway's `events` option) or
/// outputs (`enable`, `disable`).
fn switches(setting: &Setting, value: &Value) -> Option<Switch> {
    let mapping = setting.sway()?;
    let directive = mapping.directive();
    if directive.block == Block::Output && directive.name == "enable" {
        return match value {
            Value::Bool(enabled) => Some(Switch::Outputs(*enabled)),
            _ => None,
        };
    }
    if directive.block != Block::Input || directive.name != "events" {
        return None;
    }
    let Scope::PerDevice(kind) = setting.scope() else {
        return None;
    };
    let sending = match value {
        Value::Bool(true) => Sending::Enabled,
        Value::Bool(false) => Sending::Disabled,
        Value::Choice(id) => match id.as_str() {
            "disabled" => Sending::Disabled,
            "disabled_on_external_mouse" => Sending::DisabledOnExternalMouse,
            _ => Sending::Enabled,
        },
        _ => return None,
    };
    Some(Switch::Inputs(kind, sending))
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::Countdown;

    #[test]
    fn a_countdown_elapses_at_its_deadline() {
        let countdown = Countdown {
            deadline: Instant::now() + Duration::from_millis(20),
        };
        assert!(countdown.remaining() > Duration::ZERO);
        async_io::block_on(countdown.elapsed());
        assert_eq!(countdown.remaining(), Duration::ZERO);
        assert!(Instant::now() >= countdown.deadline());
    }
}
