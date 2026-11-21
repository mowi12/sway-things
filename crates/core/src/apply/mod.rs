// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Changing a setting end to end: in the running session, in the settings files and in the
//! generated sway config.
//!
//! An [`Applier`] owns the [`State`] and, in a sway session, the connection to the compositor.
//! [`Applier::set`] and [`Applier::reset`] check the change, send the commands over IPC and update
//! the state in memory. [`Applier::persist`] saves the settings files, generates the sway config
//! files and writes them. The command line calls it right after a change; the app waits until
//! [`Applier::save_due`], so dragging a slider saves once.
//!
//! Every change needs the [`WriteAccess`] of an active
//! [`IncludeStatus`](crate::include::IncludeStatus), so nothing changes while the user's sway
//! config doesn't include the generated files.
//!
//! ```no_run
//! use sway_things_core::apply::Applier;
//! use sway_things_core::generate::Locations;
//! use sway_things_core::include::IncludeStatus;
//! use sway_things_core::registry::{Registry, Value};
//! use sway_things_core::state::{Paths, State};
//! use sway_things_core::sway_config::{Disk, default_path};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let locations = Locations::from_env()?;
//! let status = IncludeStatus::detect(default_path(&Disk).as_deref(), &Disk, &locations);
//! let access = status.write_access()?;
//!
//! let state = State::load(Registry::builtin(), Paths::from_env()?);
//! let mut applier = Applier::connect(state, locations).await?;
//! let applied = applier.set("windows.gaps.inner", None, Value::Integer(8), &access).await?;
//! println!("{:?}", applied.effect);
//! let persisted = applier.persist(&[], &access).await?;
//! assert!(persisted.is_complete());
//! # Ok(())
//! # }
//! ```

mod daemon;
mod risky;
#[cfg(test)]
mod tests;
mod waiting;

pub use risky::{Countdown, Pending, Trial, Tried, TriedValue};
pub use waiting::{Until, Waiting, WaitingChange};

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io;
use std::rc::Rc;
use std::time::{Duration, Instant};

use crate::command::{CommandError, SwayCmd, Target};
use crate::conflict;
use crate::devices::Lockout;
use crate::generate::{
    self, GeneratedFile, Locations, Outcome, Report, SwayValidator, Validate, WriteError,
};
use crate::gsettings::{self, GSettingsError, GSettingsStore, Gio, NoGSettings};
use crate::i18n::Text;
use crate::include::{MainConfig, WriteAccess};
use crate::ipc::{Client, Compositor, IpcError, Unmet};
use crate::msg;
use crate::outputs::kanshi::{self, KanshiFiles};
use crate::outputs::profiles::{Profile, Profiles, kanshi_config};
use crate::outputs::{ENABLED, MODE, Outputs, POSITION, SCALE, TRANSFORM};
use crate::registry::{
    Apply, ConfigFile, Registry, RenderError, Scope, Setting, SwayMapping, Value, Values,
    workspaces,
};
use crate::state::{ChangedKey, Changes, Layer, Paths, PathsError, State, StateError};

/// How long the app waits after the last change before it saves, so a burst of changes (a slider
/// being dragged) is saved once.
pub const SAVE_DELAY: Duration = Duration::from_millis(500);

/// How a change takes or took effect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// The running session was changed with these commands, as sent.
    Live {
        /// The commands, as sent over IPC.
        commands: Vec<String>,
    },
    /// There is no session to change. The value is in effect once sway loads the generated files.
    NoSession,
    /// sway only honors the setting while it loads its config: the value is in effect after a
    /// `reload` ([`command::reload`](crate::command::reload)), which the caller runs when it
    /// suits. A risky setting that changed in the settings files is reported like this too,
    /// because it is never applied live without the safety net.
    AfterReload,
    /// The tool behind the setting has to reload its config.
    AfterToolReload,
    /// The value is in effect after the next login.
    AfterRelogin,
    /// The setting is a preference of Sway Things itself, and there is nothing to send to sway.
    Preference,
    /// A reset of a setting that wasn't stored: nothing changed.
    NotStored,
    /// The compositor doesn't support the setting, so nothing was sent. A reset still removes the
    /// stored value.
    Unsupported(Unmet),
}

/// What [`Applier::set`] or [`Applier::reset`] did. The settings files and the generated files
/// are written by [`Applier::persist`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Applied {
    /// Whether the stored value changed.
    pub changed: bool,
    /// How the change takes effect.
    pub effect: Effect,
    /// The GSettings keys the change couldn't be copied to
    /// ([`gsettings`](crate::gsettings)). The change itself is made.
    pub not_copied: Vec<GSettingsError>,
}

/// What [`Applier::persist`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Persisted {
    /// Whether a settings file was written.
    pub saved: bool,
    /// What happened to each generated file.
    pub files: Report,
}

impl Persisted {
    /// Whether every generated file is in place with its new content.
    pub fn is_complete(&self) -> bool {
        self.problems().next().is_none()
    }

    /// The generated files that weren't written: changed outside (ask the user, then persist
    /// again with the file in `overwrite`), not generated by Sway Things, or rejected by sway.
    pub fn problems(&self) -> impl Iterator<Item = (ConfigFile, &Outcome)> {
        self.files
            .iter()
            .filter(|(_, outcome)| {
                matches!(
                    outcome,
                    Outcome::EditedOutside | Outcome::Foreign | Outcome::Invalid(_)
                )
            })
            .map(|(file, outcome)| (*file, outcome))
    }
}

/// What [`Applier::reload`] did.
#[derive(Debug)]
pub struct Reloaded {
    /// What changed in the settings files.
    pub changes: Changes,
    /// Each changed directive with how its new value takes effect, or why it couldn't be sent.
    /// Settings that are values of one directive are listed once, under the first changed key.
    pub applied: Vec<(ChangedKey, Result<Effect, ApplyError>)>,
    /// The GSettings keys the changes couldn't be copied to.
    pub not_copied: Vec<GSettingsError>,
}

/// Applies changes of settings: live, to the settings files and to the generated files.
#[derive(Debug)]
pub struct Applier<V = SwayValidator> {
    state: State,
    locations: Locations,
    validator: V,
    client: Option<Client>,
    compositor: Compositor,
    /// When the state last changed without being persisted.
    unsaved_since: Option<Instant>,
    /// The risky change that waits to be kept or reverted.
    pending: Option<Pending>,
    /// The stored changes that wait for a reload of sway or the next login.
    waiting: Waiting,
    /// Where settings with a GSettings key are copied to.
    gsettings: Rc<dyn GSettingsStore>,
    /// Who sets up the displays: kanshi means no `output` lines are generated.
    displays: generate::Displays,
    /// The setups of the displays per combination, for kanshi. `None` until loaded, or if
    /// they can't be: then nothing is remembered and nothing is written for kanshi.
    profiles: Option<Profiles>,
    /// kanshi's files, if kanshi is installed; `None` for an applier made for tests.
    kanshi_files: Option<KanshiFiles>,
    /// Whether a setting of the displays changed since they were last remembered: only then
    /// is what they run with remembered for their combination.
    displays_touched: bool,
    /// What went wrong with the remembered displays or kanshi's files, to tell the user once.
    displays_problem: Option<Text>,
    /// While kanshi sets up the displays: the combination of displays (their names, sorted)
    /// the stored values of the displays were made for; `None` until known.
    combination: Option<Vec<String>>,
    /// When that combination came.
    combination_since: Option<Instant>,
    /// Whether sway was asked to start kanshi already ([`Applier::remember_displays`]).
    kanshi_started: bool,
    /// The notification daemon's files and the session bus, or `None` for an applier made for
    /// tests: then nothing is written for the daemon and nothing is asked on the bus.
    daemon: Option<daemon::DaemonFiles>,
    /// What changed for the notification daemon since its files were last written.
    daemon_changes: daemon::Changes,
    /// What went wrong with the files of the notification daemon, to tell the user once.
    daemon_problem: Option<Text>,
}

impl Applier<SwayValidator> {
    /// Loads this user's settings and connects like [`Applier::connect`], with the files in
    /// their usual places ([`Paths::from_env`], [`Locations::from_env`]). Also returns the main
    /// sway config: the one the running sway loaded, or without a session the one sway would
    /// find. The app and the command line start with this, and read the
    /// [`IncludeStatus`](crate::include::IncludeStatus) of that config.
    ///
    /// # Errors
    ///
    /// [`ApplyError::Paths`] if the directories of the files can't be found, and the errors of
    /// [`Applier::connect`].
    pub async fn open() -> Result<(Self, MainConfig), ApplyError> {
        let paths = Paths::from_env().map_err(ApplyError::Paths)?;
        let locations = Locations::from_env().map_err(ApplyError::Paths)?;
        let state = State::load(Registry::builtin(), paths);
        let mut applier = Self::connect(state, locations).await?;
        match Profiles::load(applier.state.paths().displays()) {
            Ok(profiles) => applier.profiles = Some(profiles),
            Err(error) => {
                applier.displays_problem = Some(
                    Text::new(msg!(
                        "{error} ({file}); the displays are set up from the sway config meanwhile"
                    ))
                    .text("error", error.text())
                    .arg("file", applier.state.paths().displays().display()),
                );
            }
        }
        applier.kanshi_files = KanshiFiles::from_env().filter(|_| kanshi::installed());
        if let Some(files) = crate::notifications::files::Files::from_env() {
            let bus = crate::notifications::Notifications::session().await.ok();
            applier.set_daemon_files(files, bus);
        }
        let main = match applier.client() {
            Some(client) => match client.version().await {
                Ok(version) if !version.loaded_config_file_name.is_empty() => {
                    MainConfig::Path(version.loaded_config_file_name.into())
                }
                _ => MainConfig::Default,
            },
            None => MainConfig::Default,
        };
        Ok((applier, main))
    }

    /// An applier for this user: connected to the session in `$SWAYSOCK`, or without a session
    /// if there is none. Then the compositor to generate for is the installed `sway`. Settings
    /// with a GSettings key are copied to the user's GSettings ([`Gio`]).
    ///
    /// # Errors
    ///
    /// [`ApplyError::Ipc`] if sway runs but can't be talked to (also if it is too old), and
    /// [`ApplyError::NoCompositor`] if there is no session and no `sway` to ask either.
    pub async fn connect(state: State, locations: Locations) -> Result<Self, ApplyError> {
        let validator = SwayValidator::new();
        let mut applier = match Client::connect().await {
            Ok(client) => Self::with_session(state, locations, validator, client),
            Err(IpcError::NoSession | IpcError::NotRunning { .. }) => {
                let compositor = validator
                    .compositor()
                    .await
                    .map_err(|source| ApplyError::NoCompositor(Some(source)))?
                    .ok_or(ApplyError::NoCompositor(None))?;
                Self::without_session(state, locations, validator, compositor)
            }
            Err(error) => return Err(ApplyError::Ipc(error)),
        };
        applier.set_gsettings(Box::new(Gio));
        Ok(applier)
    }
}

impl<V: Validate> Applier<V> {
    /// An applier that changes the session of `client`. It copies nothing to GSettings until
    /// [`Applier::set_gsettings`] says where to.
    pub fn with_session(state: State, locations: Locations, validator: V, client: Client) -> Self {
        Self {
            compositor: client.compositor(),
            client: Some(client),
            state,
            locations,
            validator,
            unsaved_since: None,
            pending: None,
            waiting: Waiting::default(),
            gsettings: Rc::new(NoGSettings),
            displays: generate::Displays::Sway,
            profiles: None,
            kanshi_files: None,
            displays_touched: false,
            displays_problem: None,
            combination: None,
            combination_since: None,
            kanshi_started: false,
            daemon: None,
            daemon_changes: daemon::Changes::default(),
            daemon_problem: None,
        }
    }

    /// An applier without a session, e.g. for the command line at login: changes are saved and
    /// generated for `compositor`, and nothing is sent. Like [`Applier::with_session`], it copies
    /// nothing to GSettings until told where to.
    pub fn without_session(
        state: State,
        locations: Locations,
        validator: V,
        compositor: Compositor,
    ) -> Self {
        Self {
            compositor,
            client: None,
            state,
            locations,
            validator,
            unsaved_since: None,
            pending: None,
            waiting: Waiting::default(),
            gsettings: Rc::new(NoGSettings),
            displays: generate::Displays::Sway,
            profiles: None,
            kanshi_files: None,
            displays_touched: false,
            displays_problem: None,
            combination: None,
            combination_since: None,
            kanshi_started: false,
            daemon: None,
            daemon_changes: daemon::Changes::default(),
            daemon_problem: None,
        }
    }

    /// Copies settings with a GSettings key to `store` from now on.
    pub fn set_gsettings(&mut self, store: Box<dyn GSettingsStore>) {
        self.gsettings = Rc::from(store);
    }

    /// Where settings are copied to GSettings, shared, e.g. to read what GSettings has now
    /// ([`SettingView::with_desktop`](crate::view::SettingView::with_desktop)).
    pub fn gsettings(&self) -> Rc<dyn GSettingsStore> {
        Rc::clone(&self.gsettings)
    }

    /// Copies the settings of the directive of `setting` that have a GSettings key: the stored
    /// value for the whole session, or the schema default once nothing is stored. Returns the keys
    /// that couldn't be written.
    fn copy_to_gsettings(&self, setting: &Setting) -> Vec<GSettingsError> {
        self.state
            .registry()
            .group(setting)
            .filter_map(|member| {
                gsettings::copy(
                    self.gsettings.as_ref(),
                    member,
                    self.state.get(member.key(), None),
                )
                .err()
            })
            .collect()
    }

    /// Writes every stored value of a setting with a GSettings key to the store, as
    /// `sway-things hook gsettings` does ([`gsettings::copy_stored`]). Returns the keys that
    /// couldn't be written.
    pub fn copy_stored_to_gsettings(&self) -> Vec<(&'static str, GSettingsError)> {
        gsettings::copy_stored(&self.state, self.gsettings.as_ref())
    }

    /// The settings, with the changes that aren't persisted yet.
    pub fn state(&self) -> &State {
        &self.state
    }

    /// Where the generated files are.
    pub fn locations(&self) -> &Locations {
        &self.locations
    }

    /// What validates the generated files before they are written.
    pub fn validator(&self) -> &V {
        &self.validator
    }

    /// The compositor the settings are checked against and generated for.
    pub fn compositor(&self) -> Compositor {
        self.compositor
    }

    /// The connection to the session, for requests of the caller's own.
    pub fn client(&mut self) -> Option<&mut Client> {
        self.client.as_mut()
    }

    /// Replaces the connection, e.g. after it broke, or drops it with `None`.
    pub fn set_client(&mut self, client: Option<Client>) {
        if let Some(client) = &client {
            self.compositor = client.compositor();
        }
        self.client = client;
    }

    /// The contents of the generated files for the current settings, without writing them.
    pub fn generate(&self) -> Vec<GeneratedFile> {
        generate::generate_for(
            self.state.registry(),
            &self.state,
            self.compositor,
            self.displays,
        )
    }

    /// Generates for displays set up by `displays` from now on ([`generate::Displays::detect`]
    /// says who runs). Sway by default.
    pub fn set_displays(&mut self, displays: generate::Displays) {
        // Without the remembered displays (a file that can't be read), kanshi would get nothing
        // of what is set here: sway keeps setting them up from our lines.
        let unreadable = self.profiles.is_none() && self.kanshi_files.is_some();
        self.displays = displays.with_profiles(!unreadable);
    }

    /// Notes that `setting` changed, so the displays are remembered at the next save if it is
    /// one of theirs.
    pub(crate) fn touch(&mut self, setting: &Setting) {
        if setting.scope() == Scope::PerOutput {
            self.displays_touched = true;
        }
        if setting.key() == crate::registry::notifications::START {
            self.daemon_changes.start = true;
        }
        if setting.key().starts_with("notifications.")
            || setting.key().starts_with("windows.focused.")
        {
            self.daemon_changes.settings = true;
        }
    }

    /// What went wrong with the remembered displays or kanshi's files since last asked.
    pub fn take_displays_problem(&mut self) -> Option<Text> {
        self.displays_problem.take()
    }

    /// Who sets up the displays, as [`Applier::set_displays`] said.
    pub fn displays(&self) -> generate::Displays {
        self.displays
    }

    /// When to call [`Applier::persist`]: [`SAVE_DELAY`] after the last change that isn't
    /// persisted yet. `None` if there is nothing to persist. Each change moves it, so wait for it
    /// with a timer of the main loop and ask again when that fires.
    pub fn save_due(&self) -> Option<Instant> {
        self.unsaved_since.map(|since| since + SAVE_DELAY)
    }

    /// Sets `key` to `value`, for the whole session or input type, or for one `device`: checks
    /// the value, sends the commands to the session and stores the value in memory. A new value
    /// goes to the [default file](crate::state::Layer::default_for) of the setting, a changed one
    /// stays where it is.
    ///
    /// # Errors
    ///
    /// Nothing is stored then, and the session is as before (after a command failed, the commands
    /// sent before it are undone as far as sway allows):
    /// [`ApplyError::State`] if the arguments don't fit the registry, [`ApplyError::Risky`] for a
    /// setting that needs the safety net, [`ApplyError::Unsupported`] if the compositor doesn't
    /// support the setting or the value, [`ApplyError::Render`] or [`ApplyError::Command`] if the
    /// value can't be turned into a command, and [`ApplyError::Ipc`] if sway rejects a command or
    /// can't be reached.
    pub async fn set(
        &mut self,
        key: &str,
        device: Option<&str>,
        value: Value,
        access: &WriteAccess,
    ) -> Result<Applied, ApplyError> {
        self.displays_need_session(key)?;
        // While kanshi sets up the displays, each one's values are stored for the combination:
        // a value for every display is one for each display connected now, else their own
        // values would win over it.
        if let Some(each) = self.each_display(key, device).await? {
            let mut commands = Vec::new();
            let mut changed = false;
            let mut not_copied = Vec::new();
            for output in &each {
                let applied = self
                    .set_one(key, Some(output), value.clone(), access)
                    .await?;
                changed |= applied.changed;
                not_copied.extend(applied.not_copied);
                if let Effect::Live { commands: sent } = applied.effect {
                    commands.extend(sent);
                }
            }
            // One stored for every display before kanshi set them up would say something else
            // now: under kanshi a value stands for the displays connected, not for all.
            if self.state.reset(key, None)? {
                changed = true;
                self.unsaved_since = Some(Instant::now());
            }
            return Ok(Applied {
                changed,
                effect: Effect::Live { commands },
                not_copied,
            });
        }
        self.set_one(key, device, value, access).await
    }

    /// Refuses a change of the display setting `key` while kanshi sets up the displays and no
    /// session runs: it would be stored, but neither sway's lines nor a profile of kanshi's
    /// would take it, and the next look at the displays would put back what they run with.
    fn displays_need_session(&self, key: &str) -> Result<(), ApplyError> {
        let setting = self.setting(key)?;
        if self.displays.is_kanshi() && setting.scope() == Scope::PerOutput && self.client.is_none()
        {
            return Err(ApplyError::DisplaysNeedSession { key: setting.key() });
        }
        Ok(())
    }

    /// The displays connected now, for a value of the display setting `key` for every display
    /// (`device` is `None`) while kanshi sets them up; `None` otherwise, or without a session.
    async fn each_display(
        &mut self,
        key: &str,
        device: Option<&str>,
    ) -> Result<Option<Vec<String>>, ApplyError> {
        let setting = self.setting(key)?;
        // A position, a mode or switching off is for one display at a time anyway.
        let for_each = device.is_none()
            && self.displays.is_kanshi()
            && setting.scope() == Scope::PerOutput
            && !setting.is_one_at_a_time();
        let Some(client) = self.client.as_mut().filter(|_| for_each) else {
            return Ok(None);
        };
        let outputs = Outputs::new(&client.outputs().await.map_err(ApplyError::Ipc)?);
        Ok(Some(
            outputs
                .iter()
                .filter(|output| output.is_connected() && output.is_enabled())
                .map(|output| output.identifier().to_owned())
                .collect(),
        ))
    }

    /// [`Applier::set`] for one `device`, or the whole session or input type.
    async fn set_one(
        &mut self,
        key: &str,
        device: Option<&str>,
        value: Value,
        _access: &WriteAccess,
    ) -> Result<Applied, ApplyError> {
        let setting = self.setting(key)?;
        let requirements = setting.requirements();
        self.compositor
            .check(requirements.min_sway, requirements.min_swayfx)
            .map_err(|unmet| ApplyError::Unsupported {
                key: setting.key(),
                unmet,
            })?;
        self.touch(setting);
        let before = self.state.clone();
        let was = before.get(key, device).cloned();
        // A value that is stored already is sent again, but leaves nothing to save.
        let changed = was.as_ref() != Some(&value);
        if changed {
            self.state.set(key, device, value, None)?;
        }
        let applied = self.finish(setting, device, changed, before).await?;
        self.note(setting, device, &applied.effect, was);
        Ok(applied)
    }

    /// Removes the stored value of `key`, so Sway Things no longer manages the setting, and sends
    /// its default to the session (for one device: the value of its type). The user's own config
    /// may set another value, which is back after the next reload of sway.
    ///
    /// # Errors
    ///
    /// Like [`Applier::set`], except that a setting the compositor doesn't support can always be
    /// reset.
    pub async fn reset(
        &mut self,
        key: &str,
        device: Option<&str>,
        access: &WriteAccess,
    ) -> Result<Applied, ApplyError> {
        self.displays_need_session(key)?;
        // As for [`Applier::set`]: under kanshi, the value of every display is each one's.
        if let Some(each) = self.each_display(key, device).await? {
            // With one stored for every display before kanshi set them up, which goes too.
            let dropped = self.state.reset(key, None)?;
            if dropped {
                self.unsaved_since = Some(Instant::now());
            }
            let mut applied = Applied {
                changed: dropped,
                effect: Effect::NotStored,
                not_copied: Vec::new(),
            };
            for output in &each {
                let one = self.reset_one(key, Some(output), access).await?;
                if one.changed {
                    let mut not_copied = applied.not_copied;
                    not_copied.extend(one.not_copied);
                    applied = Applied {
                        changed: true,
                        effect: merged(applied.effect, one.effect),
                        not_copied,
                    };
                }
            }
            return Ok(applied);
        }
        self.reset_one(key, device, access).await
    }

    /// [`Applier::reset`] for one `device`, or the whole session or input type.
    async fn reset_one(
        &mut self,
        key: &str,
        device: Option<&str>,
        _access: &WriteAccess,
    ) -> Result<Applied, ApplyError> {
        let setting = self.setting(key)?;
        self.touch(setting);
        let before = self.state.clone();
        let was = before.get(key, device).cloned();
        if !self.state.reset(key, device)? {
            return Ok(Applied {
                changed: false,
                effect: Effect::NotStored,
                not_copied: Vec::new(),
            });
        }
        let applied = self.finish(setting, device, true, before).await?;
        self.note(setting, device, &applied.effect, was);
        Ok(applied)
    }

    /// Moves the stored value of `key` to the settings file `layer`: the host overlay for "this
    /// machine only", or the shared file. Nothing is sent and the generated files stay the same;
    /// [`Applier::persist`] saves the files. Returns whether there was a value to move.
    ///
    /// # Errors
    ///
    /// [`ApplyError::State`] if the arguments don't fit the registry or the file can't take the
    /// value; nothing changed then.
    pub fn move_to(
        &mut self,
        key: &str,
        device: Option<&str>,
        layer: Layer,
        _access: &WriteAccess,
    ) -> Result<bool, ApplyError> {
        let moved = self.state.move_to(key, device, layer)?;
        if self.state.is_dirty() {
            self.unsaved_since = Some(Instant::now());
        }
        Ok(moved)
    }

    /// "Let my config win": removes the stored values of the setting in `report`, so the user's
    /// own config decides it, and sends the user's value to the session. One line of the user's
    /// sets every setting of a directive (the five colors of `client.focused`), so they are all
    /// removed together.
    ///
    /// The value sent is the one `report` read from the user's config
    /// ([`conflict::Report::user_value`]); without a line there, sway's default. If a setting of the group
    /// can't be read from that line, nothing is sent and the effect is [`Effect::AfterReload`]:
    /// the caller reloads sway, which reads the user's line itself.
    ///
    /// # Errors
    ///
    /// Like [`Applier::reset`].
    pub async fn let_config_win(
        &mut self,
        report: &conflict::Report,
        _access: &WriteAccess,
    ) -> Result<Applied, ApplyError> {
        let setting = self.setting(report.key())?;
        let device = report.device();
        let registry = self.state.registry();
        let before = self.state.clone();
        let mut removed = false;
        for member in registry.group(setting) {
            removed |= self.state.reset(member.key(), device)?;
        }
        if !removed {
            return Ok(Applied {
                changed: false,
                effect: Effect::NotStored,
                not_copied: Vec::new(),
            });
        }
        let mut users = BTreeMap::new();
        if let Some(line) = report.user_value() {
            for member in registry.group(setting) {
                let Some(Ok(value)) = member.read(&line.directive) else {
                    self.unsaved_since = Some(Instant::now());
                    for member in registry.group(setting) {
                        let was = before.get(member.key(), device).cloned();
                        self.note(member, device, &Effect::AfterReload, was);
                    }
                    return Ok(Applied {
                        changed: true,
                        effect: Effect::AfterReload,
                        not_copied: self.copy_to_gsettings(setting),
                    });
                };
                users.insert(member.key().to_owned(), value);
            }
        }
        match self
            .apply_live(setting, device, Some(&before), &users)
            .await
        {
            Ok(effect) => {
                self.unsaved_since = Some(Instant::now());
                for member in registry.group(setting) {
                    let was = before.get(member.key(), device).cloned();
                    self.note(member, device, &effect, was);
                }
                Ok(Applied {
                    changed: true,
                    effect,
                    not_copied: self.copy_to_gsettings(setting),
                })
            }
            Err(error) => {
                self.state = before;
                Err(error)
            }
        }
    }

    /// The setting `key`, if it can be changed here.
    fn setting(&self, key: &str) -> Result<&'static Setting, ApplyError> {
        let setting = self
            .state
            .registry()
            .get(key)
            .ok_or_else(|| StateError::UnknownKey {
                key: key.to_owned(),
            })?;
        if setting.apply() == Apply::Risky {
            return Err(ApplyError::Risky { key: setting.key() });
        }
        Ok(setting)
    }

    /// The live step after the state changed in memory; `before` comes back if it fails.
    async fn finish(
        &mut self,
        setting: &'static Setting,
        device: Option<&str>,
        changed: bool,
        before: State,
    ) -> Result<Applied, ApplyError> {
        match self
            .apply_live(setting, device, Some(&before), &BTreeMap::new())
            .await
        {
            Ok(Effect::Unsupported(unmet)) if self.state.get(setting.key(), device).is_some() => {
                // A value needs more than its setting: `set` refuses that too.
                self.state = before;
                Err(ApplyError::Unsupported {
                    key: setting.key(),
                    unmet,
                })
            }
            Ok(effect) => {
                if self.state.is_dirty() {
                    self.unsaved_since = Some(Instant::now());
                }
                if [workspaces::PLACEMENT, workspaces::COUNT].contains(&setting.key()) {
                    self.write_profiles_for_kanshi().await;
                    // Best effort: the hook places them again when sway starts or reloads.
                    let _ = self.place_now().await;
                }
                let not_copied = self.copy_to_gsettings(setting);
                Ok(Applied {
                    changed,
                    effect,
                    not_copied,
                })
            }
            Err(error) => {
                self.state = before;
                Err(error)
            }
        }
    }

    /// Brings the session to the values in the state for the directive of `setting`, with those
    /// in `over` in place of the state's. After a command failed, the commands for `previous`
    /// undo those that were sent before it.
    async fn apply_live(
        &mut self,
        setting: &'static Setting,
        device: Option<&str>,
        previous: Option<&State>,
        over: &BTreeMap<String, Value>,
    ) -> Result<Effect, ApplyError> {
        if setting.sway().is_none() {
            // A setting a hook sends (the placement of the workspaces) can need a reload.
            return Ok(match setting.apply() {
                Apply::Reload => Effect::AfterReload,
                Apply::ToolReload => Effect::AfterToolReload,
                _ => Effect::Preference,
            });
        }
        let requirements = setting.requirements();
        if let Err(unmet) = self
            .compositor
            .check(requirements.min_sway, requirements.min_swayfx)
        {
            return Ok(Effect::Unsupported(unmet));
        }
        // What goes into the generated file must be a command too.
        let config = render(&self.state, over, setting, device, Target::Config)?;
        if let Some(unmet) = config
            .iter()
            .find_map(|command| self.compositor.supports(command).err())
        {
            return Ok(Effect::Unsupported(unmet));
        }
        match setting.apply() {
            Apply::Live => {}
            // Never live without the safety net.
            Apply::Reload | Apply::Risky => return Ok(Effect::AfterReload),
            Apply::ToolReload => return Ok(Effect::AfterToolReload),
            Apply::Relogin => return Ok(Effect::AfterRelogin),
        }
        let commands = render(&self.state, over, setting, device, Target::Ipc)?;
        if commands.is_empty() && setting.scope() == Scope::PerOutput {
            // sway has no command that hands an output's mode, position or scale back to its own
            // choice: that happens when it reads its config again.
            return Ok(Effect::AfterReload);
        }
        let undo = match previous {
            Some(previous) => {
                render(previous, &BTreeMap::new(), setting, device, Target::Ipc).unwrap_or_default()
            }
            None => Vec::new(),
        };
        // A value for every output (`output * scale 2`) takes the place of each output's own one
        // in sway, unlike in the generated file, where the outputs' own lines come after: they
        // are sent again, so the session is as sway will read the file.
        let own: Vec<SwayCmd> = if setting.scope() == Scope::PerOutput && device.is_none() {
            let outputs: Vec<String> = self
                .state
                .entries()
                .into_iter()
                .filter(|entry| entry.setting.key() == setting.key())
                .filter_map(|entry| entry.device.map(ToOwned::to_owned))
                .collect();
            let mut own = Vec::new();
            for output in &outputs {
                own.extend(render(
                    &self.state,
                    over,
                    setting,
                    Some(output),
                    Target::Ipc,
                )?);
            }
            own
        } else {
            Vec::new()
        };
        let Some(client) = &mut self.client else {
            return Ok(Effect::NoSession);
        };
        let mut sent = Vec::with_capacity(commands.len() + own.len());
        for command in &commands {
            if let Err(error) = client.run(command).await {
                if !sent.is_empty() {
                    // Best effort: the error of the command that failed is what counts.
                    let _ = client.run_all(&undo).await;
                }
                return Err(ApplyError::Ipc(error));
            }
            sent.push(command.render(Target::Ipc).map_err(ApplyError::Command)?);
        }
        for command in &own {
            // Best effort: the value for every output is in effect either way.
            if client.run(command).await.is_ok() {
                sent.push(command.render(Target::Ipc).map_err(ApplyError::Command)?);
            }
        }
        Ok(Effect::Live { commands: sent })
    }

    /// Saves the settings files, generates the sway config files and writes them. Files that
    /// didn't change are left alone, so this is cheap to call when in doubt.
    ///
    /// A generated file that was changed outside is left alone and reported, unless it is listed
    /// in `overwrite` (after asking the user).
    ///
    /// # Errors
    ///
    /// [`ApplyError::State`] if a settings file can't be saved; nothing is generated then.
    /// [`ApplyError::Write`] if a generated file or the manifest can't be written, or sway can't
    /// be run to validate. The settings are saved then.
    pub async fn persist(
        &mut self,
        overwrite: &[ConfigFile],
        access: &WriteAccess,
    ) -> Result<Persisted, ApplyError> {
        self.unsaved_since = None;
        let saved = self.state.is_dirty();
        self.state.save(access)?;
        self.remember_displays().await;
        self.keep_kanshi_file().await;
        self.keep_daemon_files().await;
        let files = self.generate();
        let files =
            generate::write(&files, &self.locations, &self.validator, overwrite, access).await?;
        let persisted = Persisted { saved, files };
        let unwritten: Vec<ConfigFile> = persisted.problems().map(|(file, _)| file).collect();
        self.waiting.written(&unwritten);
        Ok(persisted)
    }

    /// While kanshi sets up the displays: has the file of ours for kanshi there and saying what
    /// the remembered displays do, whatever changed or didn't. kanshi rejects its whole config
    /// over an include it can't find, so the file must be there before anything else works, also
    /// when no display was ever set up here; and one that someone emptied is filled again.
    async fn keep_kanshi_file(&mut self) {
        if !self.displays.is_kanshi() {
            return;
        }
        let places = crate::outputs::workspaces::Placing::of(&self.state);
        let (Some(files), Some(profiles)) = (self.kanshi_files.as_ref(), self.profiles.as_ref())
        else {
            return;
        };
        if let Some(problem) = Self::write_kanshi(Some(files), profiles, places).await {
            self.displays_problem = Some(problem);
        }
    }

    /// While kanshi sets up the displays: remembers how the connected ones are set up here (the
    /// stored values, and what they run with where nothing is stored) for their combination,
    /// writes the profiles for kanshi and has kanshi read them again. Not while a risky change
    /// waits (that isn't kept yet), nor without a session, nor for a combination other than the
    /// one the stored values were made for ([`Applier::follow_combination`]). Best effort: what
    /// can't be written is tried again at the next save.
    async fn remember_displays(&mut self) {
        if !self.displays.is_kanshi() || self.pending.is_some() || !self.displays_touched {
            return;
        }
        self.reread_profiles();
        let (Some(client), Some(profiles)) = (&mut self.client, &mut self.profiles) else {
            return;
        };
        let Ok(found) = client.outputs().await else {
            return;
        };
        let outputs = Outputs::new(&found);
        if self
            .combination
            .as_ref()
            .is_some_and(|combination| *combination != combination_of(&outputs))
        {
            return;
        }
        if let Some(profile) = Profile::stored(&outputs, &self.state) {
            if profiles.remember(profile) {
                if let Err(error) = profiles.save() {
                    self.displays_problem = Some(
                        Text::new(msg!("the displays can't be remembered in {file}: {error}"))
                            .arg("file", profiles.path().display())
                            .arg("error", error),
                    );
                    return;
                }
            }
        }
        self.displays_problem = Self::write_kanshi(
            self.kanshi_files.as_ref(),
            profiles,
            crate::outputs::workspaces::Placing::of(&self.state),
        )
        .await;
        // The generated config starts kanshi, but it doesn't run yet (the displays are set up
        // here for the first time): sway starts it now, so the arrangement doesn't wait for the
        // next login to come back with the displays.
        let starts = matches!(self.displays, generate::Displays::Kanshi { start: true });
        // Once: a kanshi the user stopped, or one that ends at once, isn't started again and again.
        if self.displays_problem.is_none()
            && starts
            && !self.kanshi_started
            && !crate::outputs::kanshi_runs()
        {
            self.kanshi_started = true;
            let _ = client.run(&SwayCmd::new("exec").keyword("kanshi")).await;
        }
        // Written: nothing more to remember until a setting of the displays changes; else tried
        // again at the next save.
        self.displays_touched = self.displays_problem.is_some();
    }

    /// While kanshi sets up the displays: has the stored values of the displays follow the
    /// combination that is connected, so the page shows what kanshi set up. When the
    /// combination changes, the values of its profile take the place of what was stored for the
    /// displays (for a combination never set up here: what the displays run with now), and the
    /// values of displays that aren't connected are removed. Until a setting of the displays
    /// changes here, the values keep following the combination's profile, or what the displays
    /// run with. Nothing is sent: kanshi sets the displays up. Not while a risky change waits.
    /// Returns whether the stored values changed.
    ///
    /// # Errors
    ///
    /// The IPC errors of asking sway, and [`ApplyError::State`] if a value can't be stored.
    pub async fn follow_combination(&mut self) -> Result<bool, ApplyError> {
        if !self.displays.is_kanshi() || self.pending.is_some() {
            return Ok(false);
        }
        let Some(client) = &mut self.client else {
            return Ok(false);
        };
        let outputs = Outputs::new(&client.outputs().await.map_err(ApplyError::Ipc)?);
        let combination = combination_of(&outputs);
        if combination.is_empty() {
            return Ok(false);
        }
        if self.combination.as_ref() == Some(&combination) {
            // Changed here, or long enough after the combination came that kanshi has set it up:
            // a change from elsewhere (`swaymsg`) is left as it is, as without kanshi.
            let settled = self
                .combination_since
                .is_none_or(|since| since.elapsed() > FOLLOW_TIME);
            if self.displays_touched || settled {
                return Ok(false);
            }
        } else {
            self.combination_since = Some(Instant::now());
            // A change made here for the displays connected before, not remembered yet (the
            // combination changed within the delay before saving): remembered for theirs now,
            // before the values follow the new one.
            let before = self.combination.replace(combination.clone());
            if let Some(before) = before.filter(|_| self.displays_touched) {
                self.remember_combination(&before).await;
            }
            self.displays_touched = false;
        }
        let names: Vec<&str> = combination.iter().map(String::as_str).collect();
        let profile = self
            .profiles
            .as_ref()
            .and_then(|profiles| profiles.of(&names))
            .cloned()
            .or_else(|| Profile::capture(&outputs));
        let Some(profile) = profile else {
            return Ok(false);
        };
        let mut changed = false;
        let stale: Vec<(&'static str, String)> = self
            .state
            .entries()
            .into_iter()
            .filter(|entry| entry.setting.scope() == Scope::PerOutput)
            .filter_map(|entry| Some((entry.setting.key(), entry.device?.to_owned())))
            .filter(|(_, device)| !combination.contains(device))
            // Only what a remembered combination holds: it comes back with it. The values of a
            // display set up before kanshi did it stay.
            .filter(|(_, device)| {
                self.profiles.as_ref().is_some_and(|profiles| {
                    profiles
                        .iter()
                        .any(|profile| profile.combination().contains(&device.as_str()))
                })
            })
            .collect();
        for (key, device) in &stale {
            changed |= self.state.reset(key, Some(device))?;
        }
        for (name, setup) in profile.outputs() {
            let values = setup.values();
            for key in [ENABLED, MODE, POSITION, SCALE, TRANSFORM] {
                let wanted = values
                    .iter()
                    .find(|(wanted, _)| *wanted == key)
                    .map(|(_, value)| value);
                match wanted {
                    Some(value) if self.state.get(key, Some(name)) != Some(value) => {
                        self.state.set(key, Some(name), value.clone(), None)?;
                        changed = true;
                    }
                    Some(_) => {}
                    None => changed |= self.state.reset(key, Some(name))?,
                }
            }
        }
        // Not saved for this alone: the values come back whenever the app follows again, and a
        // save per dock and undock would push the user's own changes out of the history.
        if changed {
            self.waiting.settle(&self.state);
        }
        Ok(changed)
    }

    /// Remembers the stored values of the displays `names`, a combination that isn't connected
    /// any more, for that combination, and writes the profiles for kanshi. Best effort, like
    /// [`Applier::remember_displays`].
    async fn remember_combination(&mut self, names: &[String]) {
        self.reread_profiles();
        let Some(profiles) = &mut self.profiles else {
            return;
        };
        let Some(profile) = Profile::stored_for(names, &self.state) else {
            return;
        };
        if profiles.remember(profile) {
            if let Err(error) = profiles.save() {
                self.displays_problem = Some(
                    Text::new(msg!("the displays can't be remembered in {file}: {error}"))
                        .arg("file", profiles.path().display())
                        .arg("error", error),
                );
                return;
            }
        }
        self.displays_problem = Self::write_kanshi(
            self.kanshi_files.as_ref(),
            profiles,
            crate::outputs::workspaces::Placing::of(&self.state),
        )
        .await;
    }

    /// Reads the remembered displays again: `sway-things hook mirror`, run by kanshi, may have
    /// noted something in them since (the workspaces a picture took along), which a save from
    /// what was read before would drop. Kept as they are if the file can't be read now.
    fn reread_profiles(&mut self) {
        let fresh = self
            .profiles
            .as_ref()
            .and_then(|profiles| Profiles::load(profiles.path()).ok());
        if let Some(fresh) = fresh {
            self.profiles = Some(fresh);
        }
    }

    /// Writes the profiles for kanshi and has kanshi read them again, if anything changed.
    /// Returns what went wrong.
    async fn write_kanshi(
        files: Option<&KanshiFiles>,
        profiles: &Profiles,
        places: Option<crate::outputs::workspaces::Placing>,
    ) -> Option<Text> {
        let files = files?;
        match files.write(&kanshi_config(profiles, places)) {
            Ok(true) => {
                kanshi::reload().await;
                None
            }
            Ok(false) => None,
            Err(error) => Some(error.text()),
        }
    }

    /// Has the display `target` (by its name in the settings) show another one instead of its
    /// own content (`mirror`), or with `None` its own again, for the combination connected now.
    /// Saved at once, written for kanshi, and the mirrors are started or ended now.
    ///
    /// # Errors
    ///
    /// [`ApplyError::NoSession`] without a session or profiles, and the IPC errors.
    pub async fn set_mirror(
        &mut self,
        target: &str,
        mirror: Option<crate::outputs::mirror::Mirror>,
    ) -> Result<(), ApplyError> {
        self.reread_profiles();
        let (Some(client), Some(profiles)) = (&mut self.client, &mut self.profiles) else {
            return Err(ApplyError::NoSession {
                key: "displays.mirror",
            });
        };
        let outputs = Outputs::new(&client.outputs().await.map_err(ApplyError::Ipc)?);
        let Some(captured) = Profile::stored(&outputs, &self.state) else {
            return Ok(());
        };
        let combination: Vec<String> = captured
            .combination()
            .into_iter()
            .map(str::to_owned)
            .collect();
        // Where the display stands among the others, to go back to from apart.
        let place = captured
            .outputs()
            .find(|(name, _)| *name == target)
            .and_then(|(_, setup)| setup.position);
        let names: Vec<&str> = combination.iter().map(String::as_str).collect();
        if profiles.of(&names).is_none() {
            profiles.remember(captured);
        }
        let gone = !names.contains(&target);
        let ending = mirror.is_none();
        // The workspaces that left the display when its picture started, to give back.
        let session = client.socket().display().to_string();
        let given_back = profiles
            .of(&names)
            .map(|profile| profile.gathered(target, &session))
            .filter(|_| ending)
            .unwrap_or_default();
        let set = profiles
            .of_mut(&names)
            .is_some_and(|profile| profile.set_mirror(target, mirror.clone()));
        if !set {
            // A display that went meanwhile shows nothing to end.
            if gone && ending {
                return Ok(());
            }
            return Err(ApplyError::Mirror);
        }
        // The workspaces of a display that shows another one's picture go to that one first, so
        // they stay within reach; noted (and saved before kanshi reads the profiles again and runs
        // its own hook), so they come back. Best effort, like the mirror itself.
        let _ = crate::outputs::mirror::gather_workspaces(client, profiles).await;
        // Saved onto the file as it is now: kanshi's hook may have noted workspaces in it while
        // sway was asked.
        let noted = profiles
            .of(&names)
            .map(|profile| profile.gathered(target, &session))
            .unwrap_or_default();
        if let Ok(mut fresh) = Profiles::load(profiles.path()) {
            if fresh.of(&names).is_none() {
                if let Some(profile) = profiles.of(&names) {
                    fresh.remember(profile.clone());
                }
            }
            if let Some(profile) = fresh.of_mut(&names) {
                profile.set_mirror(target, mirror);
                profile.note_gathered(target, &noted, &session);
            }
            *profiles = fresh;
        }
        let path = profiles.path().to_owned();
        profiles
            .save()
            .map_err(|source| ApplyError::Write(generate::WriteError::Io { path, source }))?;
        self.displays_problem = Self::write_kanshi(
            self.kanshi_files.as_ref(),
            profiles,
            crate::outputs::workspaces::Placing::of(&self.state),
        )
        .await;
        // The picture doesn't take the focus when it starts (the generated config says so from
        // the next reload of sway on).
        let _ = client
            .run(&crate::outputs::mirror::no_focus_command())
            .await;
        // Idempotent: kanshi's own `hook mirror` after the reload changes nothing more.
        crate::outputs::mirror::apply(profiles, &outputs);
        // Its own content again: its workspaces come back.
        if ending {
            let _ =
                crate::outputs::mirror::give_back_workspaces(client, target, &given_back, place)
                    .await;
        }
        // No workspace named after a picture that ended stays behind on its display.
        let _ = crate::outputs::mirror::leave_ended(client, profiles).await;
        // The display takes workspaces again, or no longer: placed anew. sway keeps the lists of
        // displays it was told before until it reloads, so the workspaces that aren't open
        // follow then.
        if crate::outputs::workspaces::Rule::of(&self.state).places() {
            let _ = self.place_now().await;
            if let Some(setting) = self
                .state
                .registry()
                .get(crate::registry::workspaces::PLACEMENT)
            {
                let was = self.state.get(setting.key(), None).cloned();
                self.note(setting, None, &Effect::AfterReload, was);
            }
        }
        Ok(())
    }

    /// Forgets everything set here for the displays connected now, while kanshi sets them up:
    /// their stored values (the risky ones too), the profile of their combination with its
    /// pictures and its placement of the workspaces. kanshi's file is written without it and
    /// kanshi reads it again, so what set the displays up before Sway Things did applies again:
    /// a profile of the user's own for these displays, or what the displays run with. The
    /// pictures end and their workspaces come back. Says whether anything was forgotten.
    ///
    /// # Errors
    ///
    /// [`ApplyError::PendingChange`] while a risky change waits, the IPC errors, and the errors
    /// of writing.
    pub async fn forget_displays(&mut self, access: &WriteAccess) -> Result<bool, ApplyError> {
        if let Some(pending) = &self.pending {
            return Err(ApplyError::PendingChange { key: pending.key() });
        }
        self.reread_profiles();
        let places = crate::outputs::workspaces::Placing::of(&self.state);
        let (Some(client), Some(profiles)) = (&mut self.client, &mut self.profiles) else {
            return Ok(false);
        };
        let outputs = Outputs::new(&client.outputs().await.map_err(ApplyError::Ipc)?);
        let connected: Vec<String> = outputs
            .iter()
            .filter(|output| output.is_connected())
            .map(|output| output.identifier().to_owned())
            .collect();
        let names: Vec<&str> = connected.iter().map(String::as_str).collect();
        let session = client.socket().display().to_string();
        // The workspaces each picture took along, to give back, and where its display stood.
        let pictures: Vec<Picture> = profiles
            .of(&names)
            .map(|profile| {
                profile
                    .outputs()
                    .filter(|(_, setup)| setup.mirror.is_some())
                    .map(|(name, setup)| {
                        (
                            name.to_owned(),
                            profile.gathered(name, &session),
                            setup.position,
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        let mut forgotten = profiles.forget(&names);
        if forgotten {
            let path = profiles.path().to_owned();
            profiles
                .save()
                .map_err(|source| ApplyError::Write(generate::WriteError::Io { path, source }))?;
        }
        // The stored values of these displays, whatever they are for.
        let stored: Vec<(&'static str, String)> = self
            .state
            .entries()
            .into_iter()
            .filter(|entry| entry.setting.scope() == Scope::PerOutput)
            .filter_map(|entry| Some((entry.setting.key(), entry.device?.to_owned())))
            .filter(|(_, device)| connected.contains(device))
            .collect();
        for (key, device) in stored {
            forgotten |= self.state.reset(key, Some(&device))?;
        }
        self.state.save(access)?;
        // Nothing made here is kept for this combination: it is read from the displays again
        // when it comes back.
        self.combination = None;
        self.displays_touched = false;
        let (Some(client), Some(profiles)) = (&mut self.client, &self.profiles) else {
            return Ok(forgotten);
        };
        // The pictures end, their displays go back among the others and take their workspaces.
        crate::outputs::mirror::apply(profiles, &outputs);
        for (target, gathered, place) in &pictures {
            let _ = crate::outputs::mirror::give_back_workspaces(client, target, gathered, *place)
                .await;
        }
        let _ = crate::outputs::mirror::leave_ended(client, profiles).await;
        // kanshi reads the file without the profile: another one that matches applies.
        self.displays_problem =
            Self::write_kanshi(self.kanshi_files.as_ref(), profiles, places).await;
        if let Some(client) = &mut self.client {
            let _ = crate::outputs::ensure_one_on(client).await;
        }
        Ok(forgotten)
    }

    /// kanshi's files, if kanshi is installed (not for an applier made for tests).
    pub fn kanshi_files(&self) -> Option<&KanshiFiles> {
        self.kanshi_files.as_ref()
    }

    /// Uses `profiles` as the setups of the displays remembered per combination.
    #[cfg(test)]
    pub(crate) fn set_profiles(&mut self, profiles: Profiles) {
        self.profiles = Some(profiles);
    }

    /// The setups of the displays remembered per combination, if they could be loaded.
    pub fn profiles(&self) -> Option<&Profiles> {
        self.profiles.as_ref()
    }

    /// Puts `workspace` on `display` (by its name in the settings) in the placement as it is now
    /// for the displays connected, remembers that as their fixed placement, saved at once, and
    /// makes the rule fixed, which places the workspaces.
    ///
    /// # Errors
    ///
    /// [`ApplyError::NoSession`] without a session or profiles, a write error of `displays.toml`,
    /// and the errors of the change and the IPC.
    pub async fn fix_placement(
        &mut self,
        workspace: &str,
        display: &str,
        access: &WriteAccess,
    ) -> Result<(), ApplyError> {
        self.reread_profiles();
        // No displays remembered yet: the file is made now. One that can't be read stays as it is.
        let path = self.state.paths().displays();
        if self.profiles.is_none() && !path.exists() {
            self.profiles = Some(Profiles::empty(path));
        }
        let (Some(client), Some(profiles)) = (&mut self.client, &mut self.profiles) else {
            return Err(ApplyError::NoSession {
                key: crate::registry::workspaces::PLACEMENT,
            });
        };
        let outputs = Outputs::new(&client.outputs().await.map_err(ApplyError::Ipc)?);
        // The move is made on the placement as it is now, so moves made one after the other
        // all count, however fast they come.
        let rule = crate::outputs::workspaces::Rule::of(&self.state);
        let mirrors = crate::outputs::workspaces::mirrored(Some(profiles), &outputs);
        let mut placement = crate::outputs::workspaces::place(
            if rule.places() {
                rule
            } else {
                crate::outputs::workspaces::Rule::RoundRobin
            },
            &outputs,
            &mirrors,
            crate::outputs::workspaces::count(&self.state),
            &crate::outputs::workspaces::fixed(Some(profiles), &outputs),
        );
        placement.insert(workspace.to_owned(), display.to_owned());
        let Some(mut profile) = Profile::stored(&outputs, &self.state) else {
            return Ok(());
        };
        let names: Vec<&str> = profile.combination();
        if let Some(known) = profiles.of(&names) {
            profile = known.clone();
        }
        profile.set_workspaces(placement);
        profiles.remember(profile);
        let path = profiles.path().to_owned();
        profiles
            .save()
            .map_err(|source| ApplyError::Write(generate::WriteError::Io { path, source }))?;
        // The rule becomes fixed, which places them; or it was, and a reload settles the
        // workspaces that aren't open (sway only appends to their displays).
        let fixed = Value::Choice("fixed".to_owned());
        if self.state.get(workspaces::PLACEMENT, None) == Some(&fixed) {
            self.write_profiles_for_kanshi().await;
            self.place_now().await?;
            if let Some(setting) = self.state.registry().get(workspaces::PLACEMENT) {
                self.note(setting, None, &Effect::AfterReload, Some(fixed));
            }
            Ok(())
        } else {
            self.set(workspaces::PLACEMENT, None, fixed, access)
                .await
                .map(drop)
        }
    }

    /// Writes kanshi's profiles again, after the placement of the workspaces changed: they run
    /// the hook only while it places.
    async fn write_profiles_for_kanshi(&mut self) {
        if let Some(profiles) = &self.profiles {
            let places = crate::outputs::workspaces::Placing::of(&self.state);
            self.displays_problem =
                Self::write_kanshi(self.kanshi_files.as_ref(), profiles, places).await;
        }
    }

    /// Puts the workspaces on the displays connected now as the Workspaces page says
    /// ([`crate::outputs::workspaces::apply`]). Nothing without a session.
    ///
    /// # Errors
    ///
    /// The IPC errors.
    pub async fn place_now(&mut self) -> Result<(), ApplyError> {
        let Some(client) = &mut self.client else {
            return Ok(());
        };
        crate::outputs::workspaces::apply(client, &self.state, self.profiles.as_ref())
            .await
            .map(drop)
            .map_err(ApplyError::Ipc)
    }

    /// The stored changes that wait for a reload of sway or the next login.
    pub fn waiting(&self) -> &Waiting {
        &self.waiting
    }

    /// Persists the changes ([`Applier::persist`]) and asks sway to reload its config, so the
    /// changes that wait for a reload take effect. A reload starts the bars again and runs the
    /// `exec_always` lines, so the caller asks the user first.
    ///
    /// # Errors
    ///
    /// Nothing is sent then: [`ApplyError::PendingChange`] while a risky change waits (a reload
    /// would put the stored value back), [`ApplyError::ReloadWithoutSession`] without a session,
    /// the errors of [`Applier::persist`], and [`ApplyError::Unwritten`] if a generated file with
    /// a waiting change couldn't be written. [`ApplyError::Ipc`] if sway refuses the reload or
    /// can't be reached.
    pub async fn reload_sway(&mut self, access: &WriteAccess) -> Result<Persisted, ApplyError> {
        if let Some(pending) = &self.pending {
            return Err(ApplyError::PendingChange { key: pending.key() });
        }
        if self.client.is_none() {
            return Err(ApplyError::ReloadWithoutSession);
        }
        let persisted = self.persist(&[], access).await?;
        let unwritten = self.waiting.unwritten();
        if !unwritten.is_empty() {
            return Err(ApplyError::Unwritten { files: unwritten });
        }
        let Some(client) = &mut self.client else {
            return Err(ApplyError::ReloadWithoutSession);
        };
        client
            .run(&crate::command::reload())
            .await
            .map_err(ApplyError::Ipc)?;
        self.waiting.reloaded();
        Ok(persisted)
    }

    /// Notes that sway reloaded its config, also when something else asked for it (sway's
    /// `workspace` event with the change `reload`): the waiting changes that are in the generated
    /// files are in effect now.
    pub fn reloaded(&mut self) {
        self.waiting.reloaded();
    }

    /// Notes a change of `setting` for `device` that took `effect`, with `was` its stored value
    /// before, if it waits for a reload or the next login in the running session. Then drops the
    /// waiting changes that are undone.
    fn note(
        &mut self,
        setting: &'static Setting,
        device: Option<&str>,
        effect: &Effect,
        was: Option<Value>,
    ) {
        // Without a session, sway reads the generated files when it starts.
        if self.client.is_none() {
            return;
        }
        let until = match effect {
            Effect::AfterReload => Until::Reload,
            Effect::AfterRelogin => Until::Relogin,
            _ => {
                self.waiting.settle(&self.state);
                return;
            }
        };
        let file = setting.sway().map(SwayMapping::file);
        self.waiting.note(setting.key(), device, until, file, was);
        self.waiting.settle(&self.state);
    }

    /// Loads the settings files that changed on disk (a hand edit, a `git pull` of the dotfiles;
    /// call it on each report of [`State::watch`]) and sends the changed settings to the session.
    /// Call [`Applier::persist`] afterwards to regenerate the files.
    ///
    /// The file on disk wins, so a command that fails undoes nothing; its error is in the result.
    /// Risky settings aren't sent: they are in effect after the next reload of sway
    /// ([`Effect::AfterReload`]).
    pub async fn reload(&mut self, _access: &WriteAccess) -> Reloaded {
        let before = self.state.clone();
        let changes = self.state.reload();
        let registry = self.state.registry();
        let mut seen = BTreeSet::new();
        let mut applied = Vec::new();
        let mut not_copied = Vec::new();
        for changed in &changes.keys {
            let Some(setting) = registry.get(changed.key) else {
                continue;
            };
            self.touch(setting);
            let directive = registry.group(setting).next().map(Setting::key);
            if !seen.insert((changed.device.clone(), directive)) {
                continue;
            }
            let effect = self
                .apply_live(setting, changed.device.as_deref(), None, &BTreeMap::new())
                .await;
            // A risky value from disk isn't offered for a reload: that would skip the safety net.
            if let Ok(effect) = &effect {
                if setting.apply() != Apply::Risky {
                    for member in registry.group(setting) {
                        let device = changed.device.as_deref();
                        let was = before.get(member.key(), device).cloned();
                        self.note(member, device, effect, was);
                    }
                }
            }
            if changed.device.is_none() {
                not_copied.extend(self.copy_to_gsettings(setting));
            }
            applied.push((changed.clone(), effect));
        }
        if !changes.is_empty() {
            self.unsaved_since = Some(Instant::now());
        }
        Reloaded {
            changes,
            applied,
            not_copied,
        }
    }
}

/// The values in `over`, and those of `under` for the other keys.
struct Over<'a, V> {
    over: &'a BTreeMap<String, Value>,
    under: V,
}

impl<V: Values> Values for Over<'_, V> {
    fn get(&self, key: &str) -> Option<&Value> {
        self.over.get(key).or_else(|| self.under.get(key))
    }
}

/// A display that showed a picture: its name in the settings, the workspaces the picture took
/// along, and where the display stood among the others.
type Picture = (String, Vec<String>, Option<(i32, i32)>);

/// The commands of two effects of one change, sent one after the other.
fn merged(first: Effect, then: Effect) -> Effect {
    match (first, then) {
        (Effect::Live { mut commands }, Effect::Live { commands: more }) => {
            commands.extend(more);
            Effect::Live { commands }
        }
        (_, then) => then,
    }
}

/// How long after a combination of displays came the stored values follow what the displays
/// run with, while kanshi sets them up.
const FOLLOW_TIME: Duration = Duration::from_secs(10);

/// The names of the connected displays of `outputs`, sorted: their combination.
fn combination_of(outputs: &Outputs) -> Vec<String> {
    let mut names: Vec<String> = outputs
        .iter()
        .filter(|output| output.is_connected())
        .map(|output| output.identifier().to_owned())
        .collect();
    names.sort_unstable();
    names
}

/// The commands that set the directive of `setting` to its values in `state`, with those in
/// `over` in place of the state's.
fn render(
    state: &State,
    over: &BTreeMap<String, Value>,
    setting: &Setting,
    device: Option<&str>,
    target: Target,
) -> Result<Vec<SwayCmd>, ApplyError> {
    if let Some(device) = device {
        setting
            .check_device(device)
            .map_err(|_| StateError::InvalidDevice {
                device: device.to_owned(),
            })?;
    }
    Ok(state.registry().render(
        setting,
        &Over {
            over,
            under: state.values(device),
        },
        device,
        target,
    )?)
}

/// Why a change wasn't applied or persisted.
#[derive(Debug)]
pub enum ApplyError {
    /// The directories of the settings files or the generated files can't be found.
    Paths(PathsError),
    /// The key, the device or the value doesn't fit the registry, or a settings file can't be
    /// changed or saved.
    State(StateError),
    /// The setting can lock the user out (a keyboard layout, a disabled device), so it only
    /// changes through the safety net.
    Risky {
        /// The key.
        key: &'static str,
    },
    /// The compositor doesn't support the setting, or this value of it.
    Unsupported {
        /// The key.
        key: &'static str,
        /// What the setting needs.
        unmet: Unmet,
    },
    /// The value can't be turned into sway commands.
    Render(RenderError),
    /// A command can't be sent over IPC.
    Command(CommandError),
    /// sway rejected a command or can't be reached.
    Ipc(IpcError),
    /// The generated files can't be written.
    Write(WriteError),
    /// There is no session, and the installed `sway` doesn't say what it is either: it can't be
    /// run (with the error), or it prints no version.
    NoCompositor(Option<io::Error>),
    /// The setting has no safety net: it changes with [`Applier::set`].
    NotRisky {
        /// The key.
        key: &'static str,
    },
    /// A risky change waits to be kept or reverted, and another one can't be tried before.
    PendingChange {
        /// The key of the change that waits.
        key: &'static str,
    },
    /// kanshi sets up the displays, which remembers a display's setting for the displays
    /// connected, and there is no session to say which those are.
    DisplaysNeedSession {
        /// The key.
        key: &'static str,
    },
    /// A display can't show another one that is shown elsewhere or shows another itself.
    Mirror,
    /// No risky change waits to be kept or reverted.
    NothingPending,
    /// A risky change can only be tried in a running session.
    NoSession {
        /// The key.
        key: &'static str,
    },
    /// The change would leave the user without a keyboard or a pointing device.
    Lockout {
        /// The key.
        key: &'static str,
        /// What would be left out.
        lockout: Lockout,
    },
    /// sway can only be asked to reload in a running session.
    ReloadWithoutSession,
    /// These generated files couldn't be written, so a reload of sway wouldn't read the changes
    /// that wait for it.
    Unwritten {
        /// The files.
        files: Vec<ConfigFile>,
    },
}

impl From<StateError> for ApplyError {
    fn from(error: StateError) -> Self {
        Self::State(error)
    }
}

impl From<RenderError> for ApplyError {
    fn from(error: RenderError) -> Self {
        Self::Render(error)
    }
}

impl From<WriteError> for ApplyError {
    fn from(error: WriteError) -> Self {
        Self::Write(error)
    }
}

impl ApplyError {
    /// What went wrong, to show.
    pub fn text(&self) -> Text {
        match self {
            Self::Paths(error) => error.text(),
            Self::State(error) => error.text(),
            Self::Risky { key } => {
                Text::new(msg!("{key} can only be changed with the safety net")).arg("key", key)
            }
            Self::Unsupported { key, unmet } => Text::new(msg!("{key} {requirement}"))
                .arg("key", key)
                .text("requirement", unmet.text()),
            Self::Render(error) => error.text(),
            Self::Command(error) => error.text(),
            Self::Ipc(error) => error.text(),
            Self::Write(error) => error.text(),
            Self::NoCompositor(Some(error)) => Text::new(msg!(
                "not in a sway session, and sway can't be run: {error}"
            ))
            .arg("error", error),
            Self::NoCompositor(None) => {
                msg!("not in a sway session, and `sway --version` prints no version").into()
            }
            Self::NotRisky { key } => {
                Text::new(msg!("{key} has no safety net; change it directly")).arg("key", key)
            }
            Self::PendingChange { key } => {
                Text::new(msg!("keep or revert the change of {key} first")).arg("key", key)
            }
            Self::Mirror => msg!(
                "a display can't show one that shows another display itself, or be shown \
                 elsewhere while it shows another"
            )
            .into(),
            Self::DisplaysNeedSession { key } => Text::new(msg!(
                "{key} wasn't changed: kanshi sets up the displays and remembers it for the \
                 displays connected, so it changes only in a running sway session"
            ))
            .arg("key", key),
            Self::NothingPending => msg!("no change waits to be kept or reverted").into(),
            Self::NoSession { key } => Text::new(msg!(
                "{key} can lock you out, so it is only tried in a running sway session, where it \
                 can be reverted"
            ))
            .arg("key", key),
            Self::Lockout { key, lockout } => Text::new(msg!("{key} wasn't changed: {reason}"))
                .arg("key", key)
                .text("reason", lockout.text()),
            Self::ReloadWithoutSession => {
                msg!("sway isn't running, so it can't reload its config").into()
            }
            Self::Unwritten { files } => {
                let names: Vec<&str> = files.iter().map(|file| file.file_name()).collect();
                Text::new(msg!(
                    "{files} couldn't be written, so sway wouldn't read the changes"
                ))
                .arg("files", names.join(", "))
            }
        }
    }
}

impl fmt::Display for ApplyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text().english())
    }
}

impl std::error::Error for ApplyError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Paths(error) => Some(error),
            Self::State(error) => Some(error),
            Self::Unsupported { unmet, .. } => Some(unmet),
            Self::Render(error) => Some(error),
            Self::Command(error) => Some(error),
            Self::Ipc(error) => Some(error),
            Self::Write(error) => Some(error),
            Self::NoCompositor(error) => error
                .as_ref()
                .map(|error| error as &(dyn std::error::Error + 'static)),
            Self::Risky { .. }
            | Self::NotRisky { .. }
            | Self::PendingChange { .. }
            | Self::NothingPending
            | Self::Mirror
            | Self::NoSession { .. }
            | Self::DisplaysNeedSession { .. }
            | Self::Lockout { .. }
            | Self::ReloadWithoutSession
            | Self::Unwritten { .. } => None,
        }
    }
}
