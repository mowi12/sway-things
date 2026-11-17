// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The settings behind the window: the core's [`Applier`], the include status, and the watchers
//! that keep both current.
//!
//! Rows read what to show from a copy of the state ([`Model::view`]), so they never wait for a
//! change in progress. A change ([`Model::set`]) is queued and sent by one task at a time; for a
//! setting that changes again before its turn (a slider being dragged) only the newest value is
//! sent. The settings files and the generated files are written [`SAVE_DELAY`] after the last
//! change. The model tells the rows when to read again (`changed`), when a change failed
//! (`failed`) and when something went wrong that isn't one row's (`problem`).
//!
//! A risky setting (an input device switched off, the keyboard layout) changes through the core's
//! safety net: it is tried live, and only stored when the user keeps it ([`Model::keep`]).
//! [`Model::revert`], or the end of the countdown, puts the old value back. While the change
//! waits ([`Model::pending_change`]), its row shows the tried value. [`Model::try_risky_all`]
//! tries several settings of one device as one such change: the layouts with their variants.
//! [`Model::switch_layout`] switches the keyboard layout in use, which stores nothing.
//!
//! A row's indicators act through the model too: [`Model::reset`] removes a stored value (a risky
//! one through the safety net), [`Model::move_to`] moves it between the shared file and the host
//! overlay, [`Model::let_config_win`] hands a setting back to the user's config, and
//! [`Model::reset_page`] removes every stored value of a page that can't lock the user out.
//! [`Model::report`] says where the user's config sets a setting too.
//!
//! Settings that sway only reads while it loads its config wait for a reload
//! ([`Model::waiting`]); [`Model::reload_sway`] saves and asks sway for one. A reload from
//! elsewhere (`swaymsg reload`, a key binding) counts too: sway reports each one.
//!
//! The outputs are kept current the same way ([`Model::outputs`]): an [`OutputWatch`] reads them
//! again on each `output` event, and a value of an output that nothing sets shows what it runs
//! with.
//!
//! In a session, the model also keeps the input devices current ([`Model::devices`]): a
//! [`DeviceWatch`] reads them again on each `input` event of sway, and the devices that only have
//! stored values are listed too. Without a session only those are known.

use std::path::PathBuf;
use std::time::Instant;

use crate::i18n::{Msg, msg, tr, trf};
use gtk::glib;
use gtk::glib::closure_local;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use sway_things_core::apply::{
    Applier, ApplyError, Countdown, Pending, Persisted, SAVE_DELAY, Waiting,
};
use sway_things_core::command::input::{InputSelector, XkbSwitch};
use sway_things_core::conflict::{Known, Report};
use sway_things_core::devices::{DeviceWatch, Devices, switch_layout};
use sway_things_core::generate::{self, GeneratedFile, Locations, OnDisk};
use sway_things_core::include::{IncludeStatus, IncludeWatch, ReadOnly, WriteAccess};
use sway_things_core::ipc::{Client, Compositor, Reloads};
use sway_things_core::outputs::profiles::Profiles;
use sway_things_core::outputs::{OutputWatch, Outputs};
use sway_things_core::registry::{Apply, ConfigFile, Page, Scope, Setting, Value, Values};
use sway_things_core::state::{Layer, State};
use sway_things_core::ui_state::UiState;
use sway_things_core::view::{SettingView, Source};

// Messages used inside `glib::clone!` blocks, which xgettext can't look into: they are marked
// out here.
/// The problem when the connected devices can't be followed.
const NO_DEVICES: Msg = msg!("Connected devices can't be listed: {error}");
/// The problem when the reloads of sway can't be followed.
const NO_RELOADS: Msg = msg!("Reloads of sway from elsewhere won’t show: {error}");

/// The preference whether collapsed sections are remembered across restarts.
const REMEMBER_SECTIONS: &str = "app.remember_collapsed_sections";

/// Whether `state` asks to remember collapsed sections (`app.remember_collapsed_sections`).
fn remembers_sections(state: &State) -> bool {
    let value = state.get(REMEMBER_SECTIONS, None).cloned().or_else(|| {
        state
            .registry()
            .get(REMEMBER_SECTIONS)
            .map(Setting::default_value)
    });
    value == Some(Value::Bool(true))
}

/// What the model does next, one at a time.
#[derive(Debug, Clone, PartialEq)]
enum Op {
    /// Changes a setting; a risky one is tried with the safety net.
    Set {
        key: &'static str,
        device: Option<String>,
        value: Value,
    },
    /// Tries several risky settings of one device as one change with the safety net: each
    /// gets a new value, or with `None` its stored value is removed.
    TryRiskyAll {
        device: Option<String>,
        changes: Vec<(&'static str, Option<Value>)>,
    },
    /// Switches the active keyboard layout of a keyboard, or of all of them, to the one at an
    /// index.
    SwitchLayout { device: Option<String>, index: u8 },
    /// Removes a stored value; a risky one is tried with the safety net.
    Reset {
        key: &'static str,
        device: Option<String>,
    },
    /// Moves a stored value to another settings file.
    MoveTo {
        key: &'static str,
        device: Option<String>,
        layer: Layer,
    },
    /// Removes the stored values of a setting, so the user's config decides it.
    LetConfigWin(Report),
    /// Removes every stored value of a page, except those of risky settings.
    ResetPage(Page),
    /// Keeps the risky change that waits.
    Keep,
    /// Reverts the risky change that waits.
    Revert,
    /// Saves the settings files and writes the generated files.
    Persist,
    /// Has a display show another one, or its own content again.
    SetMirror {
        target: String,
        mirror: Option<sway_things_core::outputs::mirror::Mirror>,
    },
    /// Places the workspaces on the displays, or not; with a placement for the displays
    /// connected now, or with the one they have now.
    PlaceWorkspaces { place: bool, placement: Placement },
    /// Loads the settings files that changed on disk.
    Reload,
    /// Saves, then asks sway to reload its config.
    ReloadSway,
    /// Notes that sway reloaded its config.
    SwayReloaded,
    /// Writes a generated file that was changed outside Sway Things.
    Overwrite(ConfigFile),
}

/// Where the workspaces go, for [`Model::place_workspaces`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Placement {
    /// As remembered for the displays connected now.
    Kept,
    /// This placement, by workspace name and display name.
    Of(std::collections::BTreeMap<String, String>),
    /// Where the workspaces are now.
    AsNow,
}

/// What the model works with once it is loaded.
#[derive(Debug)]
struct Core {
    /// `None` while a task uses it.
    applier: Option<Applier>,
    /// The displays remembered per combination, as of the last change.
    profiles: Option<Profiles>,
    /// The state as of the last change, for the rows to read.
    state: State,
    /// The risky change that waits to be kept or reverted, as of the last change.
    pending: Option<Pending>,
    /// The stored changes that wait for a reload of sway or the next login, as of the last
    /// change.
    waiting: Waiting,
    status: IncludeStatus,
    compositor: Compositor,
    locations: Locations,
    /// The input devices: connected ones as of the last event, and those with stored values.
    devices: Devices,
    /// Whether the connected devices are known: in a session, once the device watch runs.
    devices_known: bool,
    /// The outputs: connected ones as of the last event, and those with stored values.
    outputs: Outputs,
    /// Whether the connected outputs are known: in a session, once the output watch runs.
    outputs_known: bool,
    /// Whether changes are saved by themselves (not in tests, where saving would run sway).
    autosave: bool,
    /// Which sections are collapsed, saved while `app.remember_collapsed_sections` is on.
    ui: UiState,
}

impl Core {
    /// The devices and outputs that the lines of the user's config are matched with.
    fn known(&self) -> Known<'_> {
        Known {
            devices: Some(&self.devices),
            outputs: Some(&self.outputs),
        }
    }
}

mod imp {
    use std::cell::{Cell, RefCell};
    use std::collections::VecDeque;
    use std::sync::OnceLock;

    use gtk::glib;
    use gtk::glib::subclass::Signal;
    use gtk::prelude::*;
    use gtk::subclass::prelude::*;

    #[derive(Debug, Default)]
    pub struct Model {
        pub(super) core: RefCell<Option<super::Core>>,
        pub(super) error: RefCell<Option<String>>,
        pub(super) queue: RefCell<VecDeque<super::Op>>,
        pub(super) busy: Cell<bool>,
        /// Who set up the displays when last looked.
        pub(super) displays: Cell<sway_things_core::generate::Displays>,
        /// When the displays page last looked for kanshi.
        pub(super) kanshi_looked: Cell<Option<std::time::Instant>>,
        /// Whether to look for kanshi at all: not for a model made for tests, so they don't
        /// depend on the machine they run on.
        pub(super) looks_for_kanshi: Cell<bool>,
        pub(super) save_planned: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Model {
        const NAME: &'static str = "StModel";
        type Type = super::Model;
    }

    impl ObjectImpl for Model {
        fn signals() -> &'static [Signal] {
            static SIGNALS: OnceLock<Vec<Signal>> = OnceLock::new();
            SIGNALS.get_or_init(|| {
                vec![
                    // The state or the include status changed: read again what to show.
                    Signal::builder("changed").build(),
                    // A change failed: key, device ("" for none) and the message.
                    Signal::builder("failed")
                        .param_types([
                            String::static_type(),
                            String::static_type(),
                            String::static_type(),
                        ])
                        .build(),
                    // Something went wrong that isn't one setting's: the message.
                    Signal::builder("problem")
                        .param_types([String::static_type()])
                        .build(),
                ]
            })
        }
    }
}

glib::wrapper! {
    /// The settings behind the window.
    pub struct Model(ObjectSubclass<imp::Model>);
}

impl Default for Model {
    fn default() -> Self {
        glib::Object::new()
    }
}

impl Model {
    /// Loads the settings files, connects to sway (or finds the installed one without a
    /// session), reads the user's config and starts watching both. If that fails, the model has
    /// an [error](Model::error) and no settings.
    pub async fn load() -> Self {
        let model = Self::default();
        model.imp().looks_for_kanshi.set(true);
        match Self::connect().await {
            Ok((applier, watch)) => {
                let state_watch = applier.state().watch();
                model.imp().displays.set(applier.displays());
                model.start(applier, watch.status().clone(), true);
                // kanshi started or stopped since the files were written: the lines for the
                // displays go or come back. Only a file as Sway Things wrote it is replaced.
                let outdated = model.generated().is_some_and(|files| {
                    files.iter().any(|(file, on_disk)| {
                        on_disk.file == ConfigFile::Output
                            && on_disk.ownership == Ok(generate::Ownership::Ours)
                            && on_disk.content.as_deref() != Some(file.text().as_str())
                    })
                });
                if outdated {
                    model.queue(Op::Persist);
                }
                model.watch_config(watch);
                match state_watch {
                    Ok(watcher) => model.watch_state(watcher),
                    Err(error) => model.emit_problem(&trf(
                        msg!("Changes to the settings files from outside won't show: {error}"),
                        &[("error", &error.text().translated())],
                    )),
                }
            }
            Err(error) => {
                model.imp().error.replace(Some(error));
            }
        }
        model
    }

    async fn connect() -> Result<(Applier, IncludeWatch), String> {
        let (mut applier, main) = Applier::open()
            .await
            .map_err(|error| error.text().translated())?;
        let watch = IncludeWatch::new(main, applier.locations().clone())
            .map_err(|error| error.text().translated())?;
        // While kanshi sets up the displays, no output lines of ours contradict it.
        applier.set_displays(generate::Displays::detect(
            watch.status(),
            applier.locations(),
        ));
        Ok((applier, watch))
    }

    /// A model of `applier` with the include status `status`, which never changes, and nothing
    /// saved by itself: for tests.
    pub fn with_applier(applier: Applier, status: IncludeStatus) -> Self {
        let model = Self::default();
        model.start(applier, status, false);
        model
    }

    fn start(&self, mut applier: Applier, status: IncludeStatus, autosave: bool) {
        let socket = applier.client().map(|client| client.socket().to_path_buf());
        let mut devices = Devices::default();
        devices.set_stored(applier.state());
        let mut outputs = Outputs::default();
        outputs.set_stored(applier.state());
        let ui_path = applier.state().paths().ui();
        let ui = if remembers_sections(applier.state()) {
            UiState::load(ui_path)
        } else {
            UiState::empty(ui_path)
        };
        let core = Core {
            ui,
            profiles: applier.profiles().cloned(),
            state: applier.state().clone(),
            pending: applier.pending().cloned(),
            waiting: applier.waiting().clone(),
            compositor: applier.compositor(),
            locations: applier.locations().clone(),
            applier: Some(applier),
            status,
            devices,
            devices_known: false,
            outputs,
            outputs_known: false,
            autosave,
        };
        self.imp().core.replace(Some(core));
        if let Some(socket) = socket {
            self.watch_devices(socket.clone());
            self.watch_outputs(socket.clone());
            self.watch_reloads(socket);
        }
    }

    /// Why the settings couldn't be loaded, if they couldn't.
    pub fn error(&self) -> Option<String> {
        self.imp().error.borrow().clone()
    }

    /// What a row shows for `setting`, for the whole session or input type, or for one input
    /// `device`. `None` if the settings couldn't be loaded.
    pub fn view(&self, setting: &'static Setting, device: Option<&str>) -> Option<SettingView> {
        let core = self.imp().core.borrow();
        let core = core.as_ref()?;
        let mut view = SettingView::of(
            setting,
            device,
            &core.state,
            &core.status,
            core.compositor,
            core.known(),
        );
        // A risky change that waits isn't stored, but it is in effect.
        let pending = core
            .pending
            .as_ref()
            .filter(|pending| pending.device() == device);
        if let Some((pending, value)) = pending.and_then(|pending| {
            let tried = pending.values().find(|tried| tried.key == setting.key())?;
            Some((pending, &tried.value))
        }) {
            view.value = value.clone();
            let tried = Tried {
                values: pending
                    .values()
                    .map(|tried| (tried.key, &tried.value))
                    .collect(),
                under: core.state.values(device),
            };
            if setting.sway().is_some() {
                view.line = core
                    .state
                    .registry()
                    .config_line(setting, &tried, device)
                    .ok();
            }
        }
        if self.kanshi_runs() && setting.scope() == Scope::PerOutput {
            // kanshi sets up the displays: what they run with counts, not what was stored
            // before kanshi ran.
            let live = device
                .and_then(|device| core.outputs.get(device))
                .and_then(|output| output.session_value(setting.key()));
            if let Some(live) = live {
                view.value = live;
                view.source = Source::Session;
            }
            // No line of ours is in the sway config meanwhile.
            view.line = None;
        }
        Some(view)
    }

    /// Who sets up the displays: kanshi if it is set up for it (for a model that looks).
    fn displays(&self) -> generate::Displays {
        if !self.imp().looks_for_kanshi.get() {
            return generate::Displays::Sway;
        }
        let core = self.imp().core.borrow();
        core.as_ref().map_or(generate::Displays::Sway, |core| {
            generate::Displays::detect(&core.status, &core.locations)
        })
    }

    /// Whether the model looks at the machine for kanshi at all (not one made for tests).
    pub fn looks_for_kanshi(&self) -> bool {
        self.imp().looks_for_kanshi.get()
    }

    /// Who set up the displays when the model last looked.
    pub fn displays_backend(&self) -> generate::Displays {
        self.imp().displays.get()
    }

    /// Whether kanshi set up the displays when the model last looked.
    pub fn kanshi_runs(&self) -> bool {
        self.imp().displays.get().is_kanshi()
    }

    /// Looks again whether kanshi sets up the displays, as when displays come and go; says
    /// whether that changed.
    pub fn look_for_kanshi(&self) -> bool {
        // At most every few seconds: the model changes often while a display is dragged.
        let now = std::time::Instant::now();
        let recently = self
            .imp()
            .kanshi_looked
            .get()
            .is_some_and(|then| now.duration_since(then) < std::time::Duration::from_secs(2));
        if recently {
            return false;
        }
        self.imp().kanshi_looked.set(Some(now));
        let displays = self.displays();
        let changed = self.imp().displays.replace(displays) != displays;
        if changed {
            // The rows show what the displays run with, or what is stored, and the lines for
            // the displays go from the generated files or come back.
            self.queue(Op::Persist);
        }
        changed
    }

    /// The input devices: in a session the connected ones, then those that only have stored
    /// values. `None` if the settings couldn't be loaded.
    pub fn devices(&self) -> Option<Devices> {
        self.imp()
            .core
            .borrow()
            .as_ref()
            .map(|core| core.devices.clone())
    }

    /// The outputs: in a session the connected ones, then those that only have stored values.
    /// `None` if the settings couldn't be loaded.
    pub fn outputs(&self) -> Option<Outputs> {
        self.imp()
            .core
            .borrow()
            .as_ref()
            .map(|core| core.outputs.clone())
    }

    /// Whether the connected outputs are known. Without a session, or before the output watch
    /// has read them, only the outputs with stored values are listed.
    pub fn outputs_known(&self) -> bool {
        self.imp()
            .core
            .borrow()
            .as_ref()
            .is_some_and(|core| core.outputs_known)
    }

    /// Whether the connected devices are known. Without a session, or before the device watch
    /// has read them, only the devices with stored values are listed.
    pub fn devices_known(&self) -> bool {
        self.imp()
            .core
            .borrow()
            .as_ref()
            .is_some_and(|core| core.devices_known)
    }

    /// The connected outputs, each its name and a label to show (make and model), as of the
    /// last change. Empty without a session.
    pub fn output_choices(&self) -> Vec<(String, String)> {
        let core = self.imp().core.borrow();
        let Some(core) = core.as_ref() else {
            return Vec::new();
        };
        core.outputs
            .iter()
            .filter(|output| output.is_connected())
            .map(|output| {
                let product = [output.make(), output.model()]
                    .into_iter()
                    .filter(|part| !part.is_empty() && *part != "Unknown")
                    .collect::<Vec<_>>()
                    .join(" ");
                let label = if product.is_empty() {
                    output.name().to_owned()
                } else {
                    trf(
                        msg!("{product} ({output})"),
                        &[("product", &product), ("output", &output.name())],
                    )
                };
                (output.name().to_owned(), label)
            })
            .collect()
    }

    /// The risky change that waits to be kept or reverted, if there is one.
    pub fn pending_change(&self) -> Option<Pending> {
        self.imp()
            .core
            .borrow()
            .as_ref()
            .and_then(|core| core.pending.clone())
    }

    /// The stored changes that wait for a reload of sway or the next login. `None` if the
    /// settings couldn't be loaded.
    pub fn waiting(&self) -> Option<Waiting> {
        self.imp()
            .core
            .borrow()
            .as_ref()
            .map(|core| core.waiting.clone())
    }

    /// Saves the changes and asks sway to reload its config, so the changes that wait for a
    /// reload take effect. A failure is shown in the banner above the page.
    pub fn reload_sway(&self) {
        self.queue(Op::ReloadSway);
    }

    /// What Sway Things writes to each generated file for the current settings, and the file as
    /// it is on disk, read now. Nothing is written. `None` if the settings couldn't be loaded.
    pub fn generated(&self) -> Option<Vec<(GeneratedFile, OnDisk)>> {
        let core = self.imp().core.borrow();
        let core = core.as_ref()?;
        let files = generate::generate_for(
            core.state.registry(),
            &core.state,
            core.compositor,
            self.imp().displays.get(),
        );
        let on_disk = generate::on_disk(&files, &core.locations);
        Some(files.into_iter().zip(on_disk).collect())
    }

    /// Writes the generated `file` although it was changed outside Sway Things, after the user
    /// agreed. A failure is shown in the banner above the page.
    pub fn overwrite(&self, file: ConfigFile) {
        self.queue(Op::Overwrite(file));
    }

    /// Whether the section `section` (a [`section_id`](sway_things_core::ui_state::section_id))
    /// of `page` is collapsed.
    pub fn is_collapsed(&self, page: Page, section: &str) -> bool {
        self.imp()
            .core
            .borrow()
            .as_ref()
            .is_some_and(|core| core.ui.is_collapsed(page, section))
    }

    /// Collapses or expands the section `section` of `page`, and saves that while
    /// `app.remember_collapsed_sections` is on. A failure to save is shown in the banner above
    /// the page.
    pub fn set_collapsed(&self, page: Page, section: &str, collapsed: bool) {
        let saved = {
            let mut core = self.imp().core.borrow_mut();
            let Some(core) = core.as_mut() else { return };
            if !core.ui.set_collapsed(page, section, collapsed) || !remembers_sections(&core.state)
            {
                return;
            }
            core.ui.save()
        };
        if let Err(error) = saved {
            self.emit_problem(&trf(
                msg!("Collapsed sections can’t be remembered: {error}"),
                &[("error", &error)],
            ));
        }
    }

    /// The file the value of `setting` for `device` (`None`: the whole session or input type) is
    /// stored in, or `None` if Sway Things stores no value of its own for it.
    pub fn stored_in(&self, setting: &Setting, device: Option<&str>) -> Option<Layer> {
        self.imp()
            .core
            .borrow()
            .as_ref()
            .and_then(|core| core.state.layer(setting.key(), device))
    }

    /// The lines of the user's config that set `setting` for `device`, as last read. `None` for
    /// a preference of Sway Things, or if the settings couldn't be loaded.
    pub fn report(&self, setting: &'static Setting, device: Option<&str>) -> Option<Report> {
        let core = self.imp().core.borrow();
        let core = core.as_ref()?;
        // kanshi sets up the displays: no line of ours competes with the user's.
        if self.kanshi_runs() && setting.scope() == Scope::PerOutput {
            return None;
        }
        Report::check(&core.status, setting, device, core.known())
    }

    /// Removes the stored value of `key` for `device`, so the user's config or sway's default
    /// decides it again. A risky setting is tried with the safety net first.
    pub fn reset(&self, key: &'static str, device: Option<&str>) {
        self.queue(Op::Reset {
            key,
            device: device.map(str::to_owned),
        });
    }

    /// Tries several risky settings of one input `device` (or of the whole input type) as one
    /// change with the safety net, like the layouts with their variants: each key gets its new
    /// value, or with `None` its stored value is removed. A failure is reported for the first key.
    pub fn try_risky_all(&self, device: Option<&str>, changes: Vec<(&'static str, Option<Value>)>) {
        self.queue(Op::TryRiskyAll {
            device: device.map(str::to_owned),
            changes,
        });
    }

    /// Switches the active keyboard layout of the keyboard `device`, or of every keyboard, to
    /// the one at `index`. Nothing is stored: sway starts with the first layout again.
    pub fn switch_layout(&self, device: Option<&str>, index: u8) {
        self.queue(Op::SwitchLayout {
            device: device.map(str::to_owned),
            index,
        });
    }

    /// Moves the stored value of `key` for `device` to the settings file `layer`.
    pub fn move_to(&self, key: &'static str, device: Option<&str>, layer: Layer) {
        self.queue(Op::MoveTo {
            key,
            device: device.map(str::to_owned),
            layer,
        });
    }

    /// Lets the user's config decide the setting of `report`: removes its stored values and
    /// sends the user's value.
    pub fn let_config_win(&self, report: Report) {
        self.queue(Op::LetConfigWin(report));
    }

    /// What a reset of `page` does: the number of stored values it removes, and the settings
    /// it leaves alone because they can lock the user out, by their stored values.
    pub fn page_reset(&self, page: Page) -> (usize, Vec<&'static Setting>) {
        let core = self.imp().core.borrow();
        let Some(core) = core.as_ref() else {
            return (0, Vec::new());
        };
        let mut removed = 0;
        let mut kept = Vec::new();
        for entry in core.state.entries() {
            if entry.setting.page() != page {
                continue;
            }
            if entry.setting.apply() == Apply::Risky {
                if !kept
                    .iter()
                    .any(|setting: &&Setting| setting.key() == entry.setting.key())
                {
                    kept.push(entry.setting);
                }
            } else {
                removed += 1;
            }
        }
        (removed, kept)
    }

    /// Removes every stored value of `page`, except those of settings that can lock the user
    /// out ([`Model::page_reset`]).
    pub fn reset_page(&self, page: Page) {
        self.queue(Op::ResetPage(page));
    }

    /// Keeps the risky change that waits: stores it, to be saved like any other change.
    pub fn keep(&self) {
        self.queue(Op::Keep);
    }

    /// Reverts the risky change that waits: puts the value from before it back.
    pub fn revert(&self) {
        self.queue(Op::Revert);
    }

    /// Whether and where the user's config includes the generated files, as last read. `None`
    /// if the settings couldn't be loaded.
    pub fn status(&self) -> Option<IncludeStatus> {
        self.imp()
            .core
            .borrow()
            .as_ref()
            .map(|core| core.status.clone())
    }

    /// Replaces the include status, after the user's config changed, and tells the rows.
    pub fn set_status(&self, status: IncludeStatus) {
        if let Some(core) = self.imp().core.borrow_mut().as_mut() {
            core.status = status;
        }
        self.emit_by_name::<()>("changed", &[]);
    }

    /// The compositor the settings are checked against: the running one, else the installed
    /// one. `None` if the settings couldn't be loaded.
    pub fn compositor(&self) -> Option<Compositor> {
        self.imp()
            .core
            .borrow()
            .as_ref()
            .map(|core| core.compositor)
    }

    /// Where the generated files go.
    pub fn locations(&self) -> Option<Locations> {
        self.imp()
            .core
            .borrow()
            .as_ref()
            .map(|core| core.locations.clone())
    }

    /// Whether the workspaces are placed on the displays, and where they go for the displays
    /// connected now. `None` without remembered displays.
    pub fn placement(&self) -> Option<(bool, std::collections::BTreeMap<String, String>)> {
        let core = self.imp().core.borrow();
        let core = core.as_ref()?;
        let profiles = core.profiles.as_ref()?;
        let names: Vec<&str> = core
            .outputs
            .iter()
            .filter(|output| output.is_connected())
            .map(sway_things_core::outputs::Output::identifier)
            .collect();
        let placement = profiles
            .of(&names)
            .map(|profile| profile.workspaces().clone())
            .unwrap_or_default();
        Some((profiles.places_workspaces(), placement))
    }

    /// The display each display of the connected combination mirrors, by name. `None` without
    /// remembered displays.
    pub fn mirrors(
        &self,
    ) -> Option<std::collections::BTreeMap<String, sway_things_core::outputs::mirror::Mirror>> {
        let core = self.imp().core.borrow();
        let core = core.as_ref()?;
        let profiles = core.profiles.as_ref()?;
        let names: Vec<&str> = core
            .outputs
            .iter()
            .filter(|output| output.is_connected())
            .map(sway_things_core::outputs::Output::identifier)
            .collect();
        Some(
            profiles
                .of(&names)
                .map_or_else(Default::default, |profile| {
                    profile
                        .outputs()
                        .filter_map(|(name, setup)| Some((name.to_owned(), setup.mirror.clone()?)))
                        .collect()
                }),
        )
    }

    /// Has the display `target` show `mirror`, or with `None` its own content again.
    pub fn set_mirror(
        &self,
        target: &str,
        mirror: Option<sway_things_core::outputs::mirror::Mirror>,
    ) {
        self.queue(Op::SetMirror {
            target: target.to_owned(),
            mirror,
        });
    }

    /// Places the workspaces on the displays (`place`) as `placement` says, or not.
    pub fn place_workspaces(&self, place: bool, placement: Placement) {
        self.queue(Op::PlaceWorkspaces { place, placement });
    }

    /// Whether a change is queued or in progress.
    pub fn is_busy(&self) -> bool {
        self.imp().busy.get()
    }

    /// Whether a change of `key` for `device` is queued.
    pub fn is_pending(&self, key: &str, device: Option<&str>) -> bool {
        self.imp().queue.borrow().iter().any(|op| match op {
            Op::Set {
                key: queued_key,
                device: queued_device,
                ..
            } => *queued_key == key && queued_device.as_deref() == device,
            Op::TryRiskyAll {
                device: queued_device,
                changes,
            } => {
                queued_device.as_deref() == device
                    && changes.iter().any(|(queued_key, _)| *queued_key == key)
            }
            Op::SwitchLayout { .. }
            | Op::Reset { .. }
            | Op::MoveTo { .. }
            | Op::LetConfigWin(_)
            | Op::ResetPage(_)
            | Op::Keep
            | Op::Revert
            | Op::Persist
            | Op::Reload
            | Op::ReloadSway
            | Op::SwayReloaded
            | Op::Overwrite(_)
            | Op::PlaceWorkspaces { .. }
            | Op::SetMirror { .. } => false,
        })
    }

    /// Changes `key` to `value`, for the whole session or input type, or for one input `device`.
    /// A change of the same setting that is still queued is replaced.
    pub fn set(&self, key: &'static str, device: Option<&str>, value: Value) {
        let op = Op::Set {
            key,
            device: device.map(str::to_owned),
            value,
        };
        {
            let mut queue = self.imp().queue.borrow_mut();
            let same = queue.iter_mut().find(|queued| match (queued, &op) {
                (
                    Op::Set {
                        key: queued_key,
                        device: queued_device,
                        ..
                    },
                    Op::Set { key, device, .. },
                ) => queued_key == key && queued_device == device,
                _ => false,
            });
            match same {
                Some(queued) => *queued = op,
                None => queue.push_back(op),
            }
        }
        self.run_queue();
    }

    fn queue(&self, op: Op) {
        {
            let mut queue = self.imp().queue.borrow_mut();
            if !queue.contains(&op) {
                queue.push_back(op);
            }
        }
        self.run_queue();
    }

    /// Runs the queued operations one by one, unless that is already happening.
    fn run_queue(&self) {
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

    async fn run(&self, op: Op) {
        let (applier, status) = {
            let mut core = self.imp().core.borrow_mut();
            let Some(core) = core.as_mut() else { return };
            let Some(applier) = core.applier.take() else {
                return;
            };
            (applier, core.status.clone())
        };
        let mut applier = applier;
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
            (Op::Overwrite(file), Ok(access)) => match applier.persist(&[file], &access).await {
                Ok(persisted) => self.emit_persisted(&persisted),
                Err(error) => self.emit_problem(&error.text().translated()),
            },
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
                self.reset_page_now(&mut applier, page, &access).await;
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
            (Op::Persist, Ok(access)) => match applier.persist(&[], &access).await {
                Ok(persisted) => self.emit_persisted(&persisted),
                Err(error) => self.emit_problem(&error.text().translated()),
            },
            (Op::Reload, Ok(access)) => {
                applier.reload(&access).await;
            }
            (Op::ReloadSway, Ok(access)) => match applier.reload_sway(&access).await {
                Ok(persisted) => self.emit_persisted(&persisted),
                Err(error) => self.emit_problem(&trf(
                    msg!("Sway wasn’t reloaded: {error}."),
                    &[("error", &error.text().translated())],
                )),
            },
            (Op::SwayReloaded, _) => applier.reloaded(),
            (op @ (Op::SetMirror { .. } | Op::PlaceWorkspaces { .. }), _) => {
                self.displays_now(&mut applier, op).await;
            }
            // Nothing can be saved or loaded without the include line; it is shown elsewhere.
            (Op::Persist | Op::Reload, Err(_)) => {}
        }
        self.give_back(applier, was_pending);
    }

    /// Takes `applier` back after an operation: copies what the rows read, plans the revert of a
    /// risky change that started waiting (none waited with the countdown `was_pending`), and
    /// tells the rows.
    fn give_back(&self, mut applier: Applier, was_pending: Option<Countdown>) {
        if let Some(problem) = applier.take_displays_problem() {
            self.emit_problem(&problem);
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

    /// Follows the input devices of the session on `socket`: reads them, and again on each
    /// `input` event, until sway closes the connection.
    fn watch_devices(&self, socket: PathBuf) {
        glib::spawn_future_local(glib::clone!(
            #[weak(rename_to = model)]
            self,
            async move {
                let started = match Client::connect_to(socket).await {
                    Ok(client) => DeviceWatch::start(&client).await,
                    Err(error) => Err(error),
                };
                let mut watch = match started {
                    Ok(watch) => watch,
                    Err(error) => {
                        model.emit_problem(&trf(
                            NO_DEVICES,
                            &[("error", &error.text().translated())],
                        ));
                        return;
                    }
                };
                model.take_devices(watch.devices());
                // An error leaves the list as it was; the events after it still come.
                while watch.changed().await.is_some() {
                    model.take_devices(watch.devices());
                }
            }
        ));
    }

    /// Follows the outputs of the session on `socket`: reads them, and again on each `output`
    /// event, until sway closes the connection.
    fn watch_outputs(&self, socket: PathBuf) {
        glib::spawn_future_local(glib::clone!(
            #[weak(rename_to = model)]
            self,
            async move {
                let started = match Client::connect_to(socket).await {
                    Ok(client) => OutputWatch::start(&client).await,
                    Err(error) => Err(error),
                };
                let mut watch = match started {
                    Ok(watch) => watch,
                    Err(error) => {
                        model.emit_problem(&format!("Displays can't be listed: {error}"));
                        return;
                    }
                };
                model.take_outputs(watch.outputs());
                // An error leaves the list as it was; the events after it still come.
                while watch.changed().await.is_some() {
                    model.take_outputs(watch.outputs());
                }
                // sway is gone: what it reported is no longer so.
                model.forget_outputs();
            }
        ));
    }

    /// Forgets the connected outputs once sway is gone; those with stored values stay listed.
    fn forget_outputs(&self) {
        if let Some(core) = self.imp().core.borrow_mut().as_mut() {
            let mut outputs = Outputs::default();
            outputs.set_stored(&core.state);
            core.outputs = outputs;
            core.outputs_known = false;
        }
        self.emit_by_name::<()>("changed", &[]);
    }

    /// Takes the connected outputs of `outputs`, and lists those with stored values too.
    fn take_outputs(&self, outputs: &Outputs) {
        if let Some(core) = self.imp().core.borrow_mut().as_mut() {
            let mut outputs = outputs.clone();
            outputs.set_stored(&core.state);
            core.outputs = outputs;
            core.outputs_known = true;
        }
        self.emit_by_name::<()>("changed", &[]);
    }

    /// Follows the reloads of sway's config in the session on `socket`, whoever asks for them, so
    /// the changes that waited for one count as in effect.
    fn watch_reloads(&self, socket: PathBuf) {
        glib::spawn_future_local(glib::clone!(
            #[weak(rename_to = model)]
            self,
            async move {
                let started = match Client::connect_to(socket).await {
                    Ok(client) => Reloads::start(&client).await,
                    Err(error) => Err(error),
                };
                let mut reloads = match started {
                    Ok(reloads) => reloads,
                    Err(error) => {
                        model.emit_problem(&trf(
                            NO_RELOADS,
                            &[("error", &error.text().translated())],
                        ));
                        return;
                    }
                };
                // An event that can't be read is skipped; the events after it still come.
                while let Some(reloaded) = reloads.next().await {
                    if reloaded.is_ok() {
                        model.queue(Op::SwayReloaded);
                    }
                }
            }
        ));
    }

    /// Takes the connected devices of `devices`, and lists those with stored values too.
    fn take_devices(&self, devices: &Devices) {
        if let Some(core) = self.imp().core.borrow_mut().as_mut() {
            let mut devices = devices.clone();
            devices.set_stored(&core.state);
            core.devices = devices;
            core.devices_known = true;
        }
        self.emit_by_name::<()>("changed", &[]);
    }

    /// Changes `key` for `device` to `value`; a risky setting is tried with the safety net.
    async fn set_now(
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
                .map(|_| ())
        } else {
            applier.set(key, device, value, access).await.map(|_| ())
        };
        if let Err(error) = result {
            self.emit_failed(key, device, &error.text().translated());
        }
    }

    /// Removes the stored value of `key` for `device`; a risky setting is tried with the safety
    /// net.
    async fn reset_now(
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
                .map(|_| ())
        } else {
            applier.reset(key, device, access).await.map(|_| ())
        };
        if let Err(error) = result {
            self.emit_failed(key, device, &error.text().translated());
        }
    }

    /// Tries several risky settings of `device` as one change; a failure is reported for the
    /// first key.
    async fn try_risky_all_now(
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
    async fn switch_layout_now(&self, applier: &mut Applier, device: Option<&str>, index: u8) {
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
    fn follow_displays(&self, applier: &mut Applier, persisting: bool) {
        let displays = self.displays();
        applier.set_displays(displays);
        if self.imp().displays.replace(displays) != displays && !persisting {
            self.queue(Op::Persist);
        }
    }

    /// Runs an operation of the displays that aren't settings: a mirror, the workspaces.
    async fn displays_now(&self, applier: &mut Applier, op: Op) {
        match op {
            Op::SetMirror { target, mirror } => {
                if let Err(error) = applier.set_mirror(&target, mirror).await {
                    self.emit_problem(&format!("The display wasn\u{2019}t mirrored: {error}."));
                }
            }
            Op::PlaceWorkspaces { place, placement } => {
                self.place_workspaces_now(applier, place, placement).await;
            }
            _ => {}
        }
    }

    /// Places the workspaces (`place`) as `placement` says, or not.
    async fn place_workspaces_now(&self, applier: &mut Applier, place: bool, placement: Placement) {
        let placement = match placement {
            Placement::Kept => Ok(None),
            Placement::Of(placement) => Ok(Some(placement)),
            Placement::AsNow => applier.current_placement().await.map(Some),
        };
        let placed = match placement {
            Ok(placement) => applier.place_workspaces(place, placement).await,
            Err(error) => Err(error),
        };
        if let Err(error) = placed {
            self.emit_problem(&format!("The workspaces weren\u{2019}t placed: {error}."));
        }
    }

    /// Removes every stored value of `page`, except those of risky settings.
    async fn reset_page_now(&self, applier: &mut Applier, page: Page, access: &WriteAccess) {
        let stored: Vec<(&'static str, Option<String>)> = applier
            .state()
            .entries()
            .into_iter()
            .filter(|entry| entry.setting.page() == page && entry.setting.apply() != Apply::Risky)
            .map(|entry| (entry.setting.key(), entry.device.map(str::to_owned)))
            .collect();
        for (key, device) in stored {
            if let Err(error) = applier.reset(key, device.as_deref(), access).await {
                self.emit_failed(key, device.as_deref(), &error.text().translated());
            }
        }
    }

    /// Reverts the change that waits when `countdown` elapses, unless it was kept or reverted
    /// before.
    fn revert_at(&self, countdown: Countdown) {
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
    fn plan_save(&self) {
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

    fn watch_config(&self, mut watch: IncludeWatch) {
        glib::spawn_future_local(glib::clone!(
            #[weak(rename_to = model)]
            self,
            async move {
                while let Some(status) = watch.changed().await {
                    model.set_status(status.clone());
                }
            }
        ));
    }

    fn watch_state(&self, watcher: sway_things_core::watch::Watcher) {
        glib::spawn_future_local(glib::clone!(
            #[weak(rename_to = model)]
            self,
            async move {
                while watcher.changed().await.is_some() {
                    model.queue(Op::Reload);
                }
            }
        ));
    }

    fn emit_failed(&self, key: &str, device: Option<&str>, message: &str) {
        self.emit_by_name::<()>("failed", &[&key, &device.unwrap_or_default(), &message]);
    }

    /// Reports the generated files that weren't written.
    fn emit_persisted(&self, persisted: &Persisted) {
        for (file, outcome) in persisted.problems() {
            if let Some(problem) = outcome.problem(file) {
                self.emit_problem(&problem.translated());
            }
        }
    }

    fn emit_problem(&self, message: &str) {
        self.emit_by_name::<()>("problem", &[&message]);
    }

    /// Calls `callback` when the state or the include status changed.
    pub fn connect_changed<F: Fn(&Self) + 'static>(&self, callback: F) -> glib::SignalHandlerId {
        self.connect_closure(
            "changed",
            false,
            closure_local!(move |model: &Self| callback(model)),
        )
    }

    /// Calls `callback` with the key, the device and the message when a change failed.
    pub fn connect_failed<F: Fn(&Self, &str, Option<&str>, &str) + 'static>(
        &self,
        callback: F,
    ) -> glib::SignalHandlerId {
        self.connect_closure(
            "failed",
            false,
            closure_local!(
                move |model: &Self, key: String, device: String, message: String| {
                    let device = (!device.is_empty()).then_some(device.as_str());
                    callback(model, &key, device, &message);
                }
            ),
        )
    }

    /// Calls `callback` with the message when something went wrong that isn't one setting's.
    pub fn connect_problem<F: Fn(&Self, &str) + 'static>(
        &self,
        callback: F,
    ) -> glib::SignalHandlerId {
        self.connect_closure(
            "problem",
            false,
            closure_local!(move |model: &Self, message: String| callback(model, &message)),
        )
    }
}

/// Whether `key` is a setting that only changes through the safety net.
fn is_risky(applier: &Applier, key: &str) -> bool {
    applier
        .state()
        .registry()
        .get(key)
        .is_some_and(|setting| setting.apply() == Apply::Risky)
}

/// The tried values of a risky change, and the stored values for the other keys.
struct Tried<'a, V> {
    values: Vec<(&'static str, &'a Value)>,
    under: V,
}

impl<V: Values> Values for Tried<'_, V> {
    fn get(&self, key: &str) -> Option<&Value> {
        self.values
            .iter()
            .find(|(tried, _)| *tried == key)
            .map(|(_, value)| *value)
            .or_else(|| self.under.get(key))
    }
}

/// Why settings can't be changed, in the user's language.
fn explained(read_only: &ReadOnly) -> String {
    read_only.explanation().translated()
}
