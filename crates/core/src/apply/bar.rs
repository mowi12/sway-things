// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Keeping the bar set up as the settings say, after each save ([`Applier::persist`]).
//!
//! With waybar, the config and stylesheet of Sway Things are written where they changed, and
//! the waybar processes of this session that run them read them again (`SIGUSR2`). If nothing
//! else starts waybar and none of ours runs yet, sway starts it now through the hook, as
//! `75-bar.conf` will at the next login (once per run of the app). With swaybar, its `bar` lines
//! are sent over IPC when they changed, which makes the bar or changes it live. Handing the bar
//! back hides swaybar until sway reloads, and ends a waybar our hook started.

use super::{Applier, ApplyError};
use crate::bar::process::{self, Starter};
use crate::bar::{Backend, Modules, WaybarFiles, swaybar, waybar};
use crate::command::{SwayCmd, Target};
use crate::generate::Validate;
use crate::i18n::Text;
use crate::include::WriteAccess;
use crate::msg;
use crate::sway_config::SwayConfig;

/// What the applier knows and did about the bar.
#[derive(Debug, Default)]
pub(crate) struct BarRun {
    /// Whether processes are looked at, signaled and started: only for the user's own applier
    /// ([`Applier::open`]), never for one made for tests.
    pub(crate) processes: bool,
    /// The user's sway config, for the colors of the windows and for what starts waybar.
    pub(crate) sway: Option<SwayConfig>,
    /// The swaybar lines sent last in this run, so the same ones aren't sent again.
    pub(crate) sent: Option<Vec<String>>,
    /// Whether sway was asked to start waybar already.
    pub(crate) started: bool,
    /// What went wrong, to tell the user once.
    pub(crate) problem: Option<Text>,
}

impl<V: Validate> Applier<V> {
    /// Tells the applier the user's sway config: the bar's style can follow the colors it sets
    /// for the windows, and what starts waybar decides whether ours is started. Part of
    /// [`Applier::follow`].
    pub(crate) fn set_sway_config(&mut self, config: SwayConfig) {
        self.bar.sway = Some(config);
    }

    /// What went wrong with the bar since last asked.
    pub fn take_bar_problem(&mut self) -> Option<Text> {
        self.bar.problem.take()
    }

    /// The files Sway Things writes for waybar.
    pub fn waybar_files(&self) -> Option<WaybarFiles> {
        WaybarFiles::of(&self.state)
    }

    /// The module definitions of the bar, read from `bar.jsonc` again (it may have been edited
    /// by hand).
    ///
    /// # Errors
    ///
    /// [`ApplyError::BarModules`] if the file can't be read.
    pub fn bar_modules(&self) -> Result<Modules, ApplyError> {
        match Modules::path_of(&self.state) {
            Some(path) => Modules::load(&path).map_err(ApplyError::BarModules),
            None => Ok(Modules::default()),
        }
    }

    /// Stores `modules` in `bar.jsonc` and sets up the bar with them.
    ///
    /// # Errors
    ///
    /// [`ApplyError::BarModules`] if the file can't be written (or isn't ours).
    pub async fn set_bar_modules(
        &mut self,
        modules: &Modules,
        access: &WriteAccess,
    ) -> Result<(), ApplyError> {
        if let Some(path) = Modules::path_of(&self.state) {
            modules
                .save(&path, access)
                .map_err(ApplyError::BarModules)?;
        }
        self.keep_bar(access).await;
        Ok(())
    }

    /// Has the waybar processes of this session that run our config read it again; one that
    /// ends then is started again once. What went wrong is noted for
    /// [`Applier::take_bar_problem`].
    pub async fn reload_bar(&mut self) {
        if let (true, Some(files)) = (self.bar.processes, self.waybar_files()) {
            self.reload_waybar(&files).await;
        }
    }

    /// [`process::reload`], with what went wrong noted.
    async fn reload_waybar(&mut self, files: &WaybarFiles) {
        match process::reload(files).await {
            Ok(process::Reloaded::Restarted) => {
                self.bar.problem = Some(
                    msg!(
                        "waybar ended when it read its config again, and was started again; \
                         if it ends again, a module option may be one it can't take"
                    )
                    .into(),
                );
            }
            Ok(_) => process::note(None),
            Err(error) => {
                process::note(Some(error));
                self.bar.problem = Some(error.text());
            }
        }
    }

    /// How waybar is started outside Sway Things' files, as the user's sway config and the
    /// running processes say; `/proc` is read on the blocking pool.
    pub async fn bar_starter(&self) -> Starter {
        let (Some(config), Some(files)) = (self.bar.sway.as_ref(), self.waybar_files()) else {
            return Starter::Nothing;
        };
        let running = if self.bar.processes {
            process::running_unblocked().await
        } else {
            Vec::new()
        };
        Starter::detect(config, self.locations.directory(), &running, &files)
    }

    /// Sets up the bar as the settings say: see the module's description. What runs is looked
    /// at each time, not remembered, so a change from the command line or the first one after
    /// the app started hands the bar back as well.
    pub(crate) async fn keep_bar(&mut self, access: &WriteAccess) {
        let backend = Backend::of(&self.state);
        let Some(files) = self.waybar_files() else {
            return;
        };
        if backend == Backend::Waybar {
            self.keep_waybar(&files, access).await;
            self.note_waybar_hold().await;
        } else if self.bar.processes && !self.bar_starter().await.is_elsewhere() {
            // Handed back: a waybar of ours that our include started ends.
            if let Err(error) = process::stop_async(&files).await {
                self.bar.problem = Some(error.text());
            }
        }
        // Only the user's own applier looks for a swaybar of ours to hide; one made for tests
        // asks sway nothing it didn't ask before.
        if backend != Backend::Swaybar && !self.bar.processes {
            return;
        }
        let Some(client) = &mut self.client else {
            return;
        };
        let id = crate::command::bar::ID;
        let ours_exists = client
            .bar_ids()
            .await
            .is_ok_and(|ids| ids.iter().any(|known| known == id));
        if backend != Backend::Swaybar {
            // A swaybar of ours made in this session or from the last config stays until sway
            // reloads: hidden meanwhile.
            if ours_exists
                && client
                    .bar_mode(id)
                    .await
                    .is_ok_and(|mode| mode != "invisible")
            {
                let _ = client.run(&crate::command::bar::mode(false)).await;
            }
            self.bar.sent = None;
            return;
        }
        let commands: Vec<SwayCmd> = swaybar::lines(&self.state, None)
            .into_iter()
            .filter_map(|line| match line {
                swaybar::Line::Command(_, command) => Some(command),
                swaybar::Line::Skipped(..) => None,
            })
            .collect();
        let sent: Vec<String> = commands
            .iter()
            .filter_map(|command| command.render(Target::Ipc).ok())
            .collect();
        if !ours_exists || self.bar.sent.as_ref() != Some(&sent) {
            for command in &commands {
                // Best effort: a command sway refuses is a comment in the file too.
                let _ = client.run(command).await;
            }
        }
        self.bar.sent = Some(sent);
    }

    /// While waybar shows only while a key is held, sway needs the bar config waybar follows
    /// (`75-bar.conf` has it, but only a reload of sway reads it: over IPC its first line would
    /// start a swaybar). Until sway has it, the change waits for a reload, so the reload banner
    /// offers one; the hook has waybar ask for the bar config again then.
    async fn note_waybar_hold(&mut self) {
        use crate::registry::bar as keys;
        if !self.bar.processes {
            return;
        }
        let Some(client) = &mut self.client else {
            return;
        };
        let hold = crate::bar::hold_key(&self.state);
        let known = client
            .bar_ids()
            .await
            .is_ok_and(|ids| ids.iter().any(|id| id == crate::command::bar::WAYBAR_ID));
        if let (Some(key), true) = (hold, known) {
            // The bar config is there: another key reaches it at once (its last line; the
            // first one is config-only).
            let [_, _, modifier] = crate::command::bar::waybar_hold(key);
            let _ = client.run(&modifier).await;
        }
        if hold.is_some() && !known {
            self.waiting.note(
                keys::VISIBILITY,
                None,
                super::waiting::Until::Reload,
                Some(crate::registry::ConfigFile::Bar),
                Some(crate::registry::Value::Choice("always".to_owned())),
            );
        } else {
            self.waiting.forget(keys::VISIBILITY, None);
        }
    }

    /// Writes waybar's files and has ours read them, or has sway start it through the hook.
    async fn keep_waybar(&mut self, files: &WaybarFiles, access: &WriteAccess) {
        let modules = match self.bar_modules() {
            Ok(modules) => modules,
            Err(error) => {
                self.bar.problem = Some(error.text());
                return;
            }
        };
        match waybar::write(files, &self.state, &modules, self.bar.sway.as_ref(), access) {
            Ok(true) if self.bar.processes => self.reload_waybar(files).await,
            Ok(_) => {}
            Err(error) => self.bar.problem = Some(error.text()),
        }
        if !self.bar.processes || self.bar.started || self.client.is_none() {
            return;
        }
        if !process::ours_unblocked(files).await.is_empty()
            || self.bar_starter().await.is_elsewhere()
        {
            return;
        }
        if !process::installed() {
            self.bar.problem = Some(process::ProcessError::NotInstalled.text());
            return;
        }
        // The hook starts it under its lock, with sway's environment, as at the next login.
        self.bar.started = true;
        if let Some(client) = &mut self.client {
            let start = SwayCmd::new("exec")
                .keyword("sway-things")
                .keyword("hook")
                .keyword("waybar");
            let _ = client.run(&start).await;
        }
    }
}
