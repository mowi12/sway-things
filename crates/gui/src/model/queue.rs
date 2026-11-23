// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The queue of the model's operations: each change runs with the applier in turn, on the main
//! loop, and its outcome is said through the model's signals.
use std::time::Instant;

use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use sway_things_core::apply::{Applier, ApplyError, Countdown, Pending, SAVE_DELAY};
use sway_things_core::command::input::{InputSelector, XkbSwitch};
use sway_things_core::conflict::Known;
use sway_things_core::devices::{Devices, switch_layout};
use sway_things_core::gsettings::GSettingsError;
use sway_things_core::include::{IncludeStatus, ReadOnly, WriteAccess};
use sway_things_core::outputs::Outputs;
use sway_things_core::registry::{ConfigFile, Page, Value};

use super::{Model, Op, explained, is_risky};
use crate::i18n::{msg, tr, trf};

impl Model {
    /// Changes `key` for `device` to `value`; a risky setting is tried with the safety net.
    pub(super) async fn set_now(
        &self,
        applier: &mut Applier,
        key: &'static str,
        device: Option<&str>,
        value: Value,
        status: &IncludeStatus,
        access: &WriteAccess,
    ) {
        let result = if is_risky(applier, key) {
            applier
                .try_risky(key, device, value, status, access)
                .await
                .map(|_| Vec::new())
        } else {
            applier
                .set(key, device, value, access)
                .await
                .map(|applied| applied.not_copied)
        };
        self.report_change(key, device, result);
    }

    /// Removes the stored value of `key` for `device`: back to the user's line where one sets
    /// it ([`Applier::reset_to_config`]); a risky setting is tried with the safety net.
    pub(super) async fn reset_now(
        &self,
        applier: &mut Applier,
        key: &'static str,
        device: Option<&str>,
        status: &IncludeStatus,
        access: &WriteAccess,
    ) {
        let result = if is_risky(applier, key) {
            applier
                .try_risky_reset(key, device, status, access)
                .await
                .map(|_| Vec::new())
        } else {
            // Copies, so nothing stays borrowed while the reset runs.
            let Some((devices, outputs)) = self.known_copies() else {
                return;
            };
            let known = Known {
                devices: Some(&devices),
                outputs: Some(&outputs),
            };
            applier
                .reset_to_config(key, device, status, known, access)
                .await
                .map(|applied| applied.not_copied)
        };
        self.report_change(key, device, result);
    }

    /// Copies of the known input devices and outputs, for a [`Known`]; `None` before the model
    /// has a core.
    fn known_copies(&self) -> Option<(Devices, Outputs)> {
        let core = self.imp().core.borrow();
        let core = core.as_ref()?;
        Some((core.devices.clone(), core.outputs.clone()))
    }

    /// Reports a change of `key` that failed, or that was made but couldn't reach GSettings, so
    /// the apps that read it there keep their own value.
    pub(super) fn report_change(
        &self,
        key: &'static str,
        device: Option<&str>,
        result: Result<Vec<GSettingsError>, ApplyError>,
    ) {
        match result {
            Err(error) => self.emit_failed(key, device, &error.text().translated()),
            Ok(not_copied) => {
                if let Some(error) = not_copied.first() {
                    self.emit_failed(key, device, &error.text().translated());
                }
            }
        }
    }

    /// Tries several risky settings of `device` as one change; a failure is reported for the
    /// first key.
    pub(super) async fn try_risky_all_now(
        &self,
        applier: &mut Applier,
        device: Option<&str>,
        changes: &[(&'static str, Option<Value>)],
        status: &IncludeStatus,
        access: Result<WriteAccess, ReadOnly>,
    ) {
        let Some((key, _)) = changes.first() else {
            return;
        };
        let result = match access {
            Ok(access) => applier
                .try_risky_all(device, changes, status, &access)
                .await
                .map(drop)
                .map_err(|error| error.text().translated()),
            Err(read_only) => Err(explained(&read_only)),
        };
        if let Err(message) = result {
            self.emit_failed(key, device, &message);
        }
    }

    /// Switches the active layout of the keyboard `device`, or of every keyboard, to the one at
    /// `index`. A failure is shown in the banner above the page.
    pub(super) async fn switch_layout_now(
        &self,
        applier: &mut Applier,
        device: Option<&str>,
        index: u8,
    ) {
        let selector = match device.map(InputSelector::device).transpose() {
            Ok(selector) => selector,
            Err(error) => {
                self.emit_problem(&trf(
                    msg!("The layout can't be switched: {error}"),
                    &[("error", &error.text().translated())],
                ));
                return;
            }
        };
        let Some(client) = applier.client() else {
            self.emit_problem(tr(msg!(
                "The layout can only be switched in a running sway session."
            )));
            return;
        };
        if let Err(error) = switch_layout(client, selector.as_ref(), XkbSwitch::Index(index)).await
        {
            self.emit_problem(&trf(
                msg!("The layout can't be switched: {error}"),
                &[("error", &error.text().translated())],
            ));
        }
    }

    /// Tells `applier` who sets up the displays now: kanshi may have been set up or gone since
    /// the app opened. When that changed, the files are written again (unless `persisting`
    /// already), so the lines for the displays go or come back.
    pub(super) fn follow_displays(&self, applier: &mut Applier, persisting: bool) {
        let before = self.imp().displays.get();
        // Not before every change: a look reads `/proc` and kanshi's config.
        self.look_for_kanshi();
        let displays = self.imp().displays.get();
        applier.set_displays(displays);
        if displays != before && persisting {
            // Written with this very change: once is enough.
            self.imp()
                .queue
                .borrow_mut()
                .retain(|op| !matches!(op, Op::Persist));
        }
    }

    /// Runs an operation of the displays that isn't a change of a setting: a mirror, the
    /// workspaces, following the combination kanshi set up.
    pub(super) async fn displays_now(&self, applier: &mut Applier, op: Op) {
        match op {
            Op::SetMirror { target, mirror } => {
                match applier.set_mirror(&target, mirror).await {
                    Ok(()) => {
                        // While a rule places the workspaces, the display takes none now, or
                        // takes them again. sway keeps the displays it was told for each
                        // workspace until it reloads, and a number that still led onto the
                        // picture would hide it: reloaded at once, without asking.
                        let places =
                            sway_things_core::outputs::workspaces::Rule::of(applier.state())
                                .places();
                        if places {
                            self.queue(Op::ReloadSway);
                        }
                    }
                    Err(error) => self.emit_problem(&trf(
                        msg!("The display wasn\u{2019}t mirrored: {error}."),
                        &[("error", &error.text().translated())],
                    )),
                }
            }
            Op::MoveAlong(places) => {
                // Nothing waits any more (kept or reverted meanwhile): nothing to move along.
                match applier.move_along(&places).await {
                    Ok(()) | Err(ApplyError::NothingPending) => {}
                    Err(error) => self.emit_problem(&trf(
                        msg!("The other displays weren\u{2019}t moved along: {error}."),
                        &[("error", &error.text().translated())],
                    )),
                }
            }
            // Best effort, quietly: the hooks of kanshi and sway try again.
            Op::HealMirrors => {
                let _ = applier.heal_mirrors().await;
            }
            Op::FollowCombination => {
                if let Err(error) = applier.follow_combination().await {
                    self.emit_problem(&trf(
                        msg!("The settings of the displays weren\u{2019}t updated: {error}."),
                        &[("error", &error.text().translated())],
                    ));
                }
            }
            _ => {}
        }
    }

    /// Removes every stored value of `page`, except those of risky settings.
    pub(super) async fn reset_page_now(
        &self,
        applier: &mut Applier,
        page: Page,
        status: &IncludeStatus,
        access: &WriteAccess,
    ) {
        // Copies, so nothing stays borrowed while the reset runs.
        let Some((devices, outputs)) = self.known_copies() else {
            return;
        };
        let known = Known {
            devices: Some(&devices),
            outputs: Some(&outputs),
        };
        let errors = applier.reset_page(page, status, known, access).await;
        for (key, device, error) in errors.failed {
            self.emit_failed(key, device.as_deref(), &error.text().translated());
        }
        if let Some(error) = errors.displays {
            self.emit_problem(&trf(
                msg!("The displays weren\u{2019}t reset: {error}."),
                &[("error", &error.text().translated())],
            ));
        }
    }

    /// Reverts the change that waits when `countdown` elapses, unless it was kept or reverted
    /// before.
    pub(super) fn revert_at(&self, countdown: Countdown) {
        glib::timeout_add_local_once(
            countdown.remaining(),
            glib::clone!(
                #[weak(rename_to = model)]
                self,
                move || {
                    let still = model
                        .pending_change()
                        .is_some_and(|pending| pending.countdown() == countdown);
                    if still {
                        model.revert();
                    }
                }
            ),
        );
    }

    /// Plans the save of the changes that aren't saved yet, unless one is planned.
    pub(super) fn plan_save(&self) {
        let imp = self.imp();
        let due = {
            let core = imp.core.borrow();
            core.as_ref()
                .filter(|core| core.autosave)
                .and_then(|core| core.applier.as_ref())
                .and_then(Applier::save_due)
        };
        let Some(due) = due else { return };
        if imp.save_planned.replace(true) {
            return;
        }
        let wait = due
            .saturating_duration_since(Instant::now())
            .max(SAVE_DELAY / 10);
        glib::timeout_add_local_once(
            wait,
            glib::clone!(
                #[weak(rename_to = model)]
                self,
                move || {
                    model.imp().save_planned.set(false);
                    model.queue(Op::Persist);
                }
            ),
        );
    }
}

impl Model {
    pub(super) fn queue(&self, op: Op) {
        {
            let mut queue = self.imp().queue.borrow_mut();
            if !queue.contains(&op) {
                queue.push_back(op);
            }
        }
        self.run_queue();
    }

    /// Runs the queued operations one by one, unless that is already happening.
    pub(super) fn run_queue(&self) {
        let imp = self.imp();
        if imp.busy.replace(true) {
            return;
        }
        glib::spawn_future_local(glib::clone!(
            #[strong(rename_to = model)]
            self,
            async move {
                loop {
                    let op = model.imp().queue.borrow_mut().pop_front();
                    let Some(op) = op else { break };
                    model.run(op).await;
                }
                model.imp().busy.set(false);
                // Who waited for the queue to be done (`is_busy`) looks again.
                model.emit_by_name::<()>("changed", &[]);
                model.plan_save();
            }
        ));
    }

    pub(super) async fn run(&self, op: Op) {
        let (applier, status) = {
            let mut core = self.imp().core.borrow_mut();
            let Some(core) = core.as_mut() else { return };
            let Some(applier) = core.applier.take() else {
                return;
            };
            (applier, core.status.clone())
        };
        let mut applier = applier;
        // A change of the bindings is worked out against the user's config as last read.
        applier.set_user_config(&status);
        self.follow_displays(&mut applier, matches!(op, Op::Persist));
        let was_pending = applier.pending().map(Pending::countdown);
        match (op, status.write_access()) {
            (
                Op::Set { key, device, .. }
                | Op::Reset { key, device }
                | Op::MoveTo { key, device, .. },
                Err(read_only),
            ) => {
                self.emit_failed(key, device.as_deref(), &explained(&read_only));
            }
            (Op::TryRiskyAll { device, changes }, access) => {
                self.try_risky_all_now(&mut applier, device.as_deref(), &changes, &status, access)
                    .await;
            }
            // Switching the layout stores nothing, so it needs no include line.
            (Op::SwitchLayout { device, index }, _) => {
                self.switch_layout_now(&mut applier, device.as_deref(), index)
                    .await;
            }
            (Op::LetConfigWin(report), Err(read_only)) => {
                self.emit_failed(report.key(), report.device(), &explained(&read_only));
            }
            (Op::Overwrite(file), Ok(access)) => {
                self.persist_now(&mut applier, &[file], &access).await;
            }
            (Op::ResetPage(_) | Op::ReloadSway | Op::Overwrite(_), Err(read_only)) => {
                self.emit_problem(&explained(&read_only));
            }
            (Op::Reset { key, device }, Ok(access)) => {
                self.reset_now(&mut applier, key, device.as_deref(), &status, &access)
                    .await;
            }
            (Op::MoveTo { key, device, layer }, Ok(access)) => {
                if let Err(error) = applier.move_to(key, device.as_deref(), layer, &access) {
                    self.emit_failed(key, device.as_deref(), &error.text().translated());
                }
            }
            (Op::LetConfigWin(report), Ok(access)) => {
                if let Err(error) = applier.let_config_win(&report, &access).await {
                    self.emit_failed(report.key(), report.device(), &error.text().translated());
                }
            }
            (Op::ResetPage(page), Ok(access)) => {
                self.reset_page_now(&mut applier, page, &status, &access)
                    .await;
            }
            (Op::Set { key, device, value }, Ok(access)) => {
                self.set_now(
                    &mut applier,
                    key,
                    device.as_deref(),
                    value,
                    &status,
                    &access,
                )
                .await;
            }
            (Op::Keep, Ok(access)) => {
                if let Err(error) = applier.keep(&access) {
                    self.emit_problem(&error.text().translated());
                }
            }
            // Without the include line nothing can be kept: back to the old value.
            (Op::Keep | Op::Revert, _) => match applier.revert().await {
                Ok(_) | Err(ApplyError::NothingPending) => {}
                Err(error) => self.emit_problem(&trf(
                    msg!("{error}. The old value comes back when sway reloads its config."),
                    &[("error", &error.text().translated())],
                )),
            },
            (Op::Persist, Ok(access)) => self.persist_now(&mut applier, &[], &access).await,
            (Op::Reload, Ok(access)) => {
                applier.reload(&access).await;
            }
            (Op::ReloadSway, Ok(access)) => self.reload_sway_now(&mut applier, &access).await,
            (Op::SwayReloaded, _) => applier.reloaded(),
            (Op::Run(command), access) => self.run_now(&mut applier, &command, access.ok()).await,
            (op @ (Op::SetMirror { .. } | Op::FollowCombination | Op::MoveAlong(_)), _) => {
                self.displays_now(&mut applier, op).await;
            }
            (Op::HealMirrors, _) => self.displays_now(&mut applier, Op::HealMirrors).await,
            (Op::FixPlacement { workspace, display }, access) => {
                self.fix_placement_now(&mut applier, &workspace, &display, access)
                    .await;
            }
            // Nothing can be saved or loaded without the include line; it is shown elsewhere.
            (Op::Persist | Op::Reload, Err(_)) => {}
        }
        self.give_back(applier, was_pending);
    }

    /// Saves and writes the files, replacing the generated files `overwrite` changed outside.
    pub(super) async fn persist_now(
        &self,
        applier: &mut Applier,
        overwrite: &[ConfigFile],
        access: &WriteAccess,
    ) {
        match applier.persist(overwrite, access).await {
            Ok(persisted) => self.emit_persisted(&persisted),
            Err(error) => self.emit_problem(&error.text().translated()),
        }
    }

    /// Runs `command` in the session, after saving what isn't saved yet with `access`, and tells
    /// the window if it can't.
    async fn run_now(
        &self,
        applier: &mut Applier,
        command: &sway_things_core::command::SwayCmd,
        access: Option<WriteAccess>,
    ) {
        // The command may need what is only in memory yet: the unit of the systemd runner, an
        // entry just changed. Saved first, as before a reload, unless saving is left to the user.
        let autosave = self
            .imp()
            .core
            .borrow()
            .as_ref()
            .is_some_and(|core| core.autosave);
        if let Some(access) = access.as_ref().filter(|_| autosave) {
            if applier.save_due().is_some() {
                self.persist_now(applier, &[], access).await;
            }
        }
        let Some(client) = applier.client() else {
            self.emit_problem(tr(msg!("Only in a running sway session")));
            return;
        };
        if let Err(error) = client.run(command).await {
            self.emit_problem(&error.text().translated());
        }
    }

    /// Saves and writes the files, and has sway reload its config.
    pub(super) async fn reload_sway_now(&self, applier: &mut Applier, access: &WriteAccess) {
        match applier.reload_sway(access).await {
            Ok(persisted) => self.emit_persisted(&persisted),
            Err(error) => self.emit_problem(&trf(
                msg!("Sway wasn’t reloaded: {error}."),
                &[("error", &error.text().translated())],
            )),
        }
    }

    /// Takes `applier` back after an operation: copies what the rows read, plans the revert of a
    /// risky change that started waiting (none waited with the countdown `was_pending`), and
    /// tells the rows.
    pub(super) fn give_back(&self, mut applier: Applier, was_pending: Option<Countdown>) {
        if let Some(problem) = applier.take_displays_problem() {
            self.emit_problem(&problem.translated());
        }
        if let Some(problem) = applier.take_daemon_problem() {
            self.emit_problem(&problem.translated());
        }
        let pending = applier.pending().cloned();
        if let Some(countdown) = pending.as_ref().map(Pending::countdown) {
            if was_pending != Some(countdown) {
                self.revert_at(countdown);
            }
        }
        if let Some(core) = self.imp().core.borrow_mut().as_mut() {
            core.state = applier.state().clone();
            core.devices.set_stored(&core.state);
            core.outputs.set_stored(&core.state);
            core.pending = pending;
            core.waiting = applier.waiting().clone();
            core.profiles = applier.profiles().cloned();
            core.applier = Some(applier);
        }
        self.emit_by_name::<()>("changed", &[]);
    }
}
