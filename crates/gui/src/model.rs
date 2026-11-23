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

mod kanshi;
mod queue;
mod watch;

pub use self::kanshi::KanshiLook;
use std::rc::Rc;

use crate::i18n::{Msg, msg, trf};
use gtk::glib;
use gtk::glib::closure_local;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use sway_things_core::apply::{Applier, Pending, Persisted, Waiting};
use sway_things_core::conflict::{Known, Report};
use sway_things_core::devices::Devices;
use sway_things_core::generate::{self, GeneratedFile, Locations, OnDisk};
use sway_things_core::gsettings::{GSettingsStore, INTERFACE};
use sway_things_core::include::{IncludeStatus, IncludeWatch, ReadOnly, WriteAccess};
use sway_things_core::ipc::Compositor;
use sway_things_core::outputs::Outputs;
use sway_things_core::outputs::profiles::Profiles;
use sway_things_core::registry::{Apply, ConfigFile, Page, Setting, Value};
use sway_things_core::state::{Layer, State};
use sway_things_core::ui_state::UiState;
use sway_things_core::view::SettingView;

// Messages used inside `glib::clone!` blocks, which xgettext can't look into: they are marked
// out here.
/// The problem when the connected devices can't be followed.
const NO_DEVICES: Msg = msg!("Connected devices can't be listed: {error}");
/// The problem when the reloads of sway can't be followed.
const NO_RELOADS: Msg = msg!("Reloads of sway from elsewhere won’t show: {error}");
/// The problem when the connected displays can't be followed.
const NO_DISPLAYS: Msg = msg!("Displays can't be listed: {error}");

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
    /// Puts a workspace on a display, by its name in the settings, in the fixed placement for
    /// the displays connected now, and places them.
    FixPlacement { workspace: String, display: String },
    /// While kanshi sets up the displays: has the stored values of the displays follow the
    /// combination connected now ([`Applier::follow_combination`]).
    FollowCombination,
    /// Moves other displays along with the trial of a display's new size, to these places.
    MoveAlong(Vec<(String, Value)>),
    /// Starts a picture of the combination connected now again that doesn't run
    /// ([`Applier::heal_mirrors`]).
    HealMirrors,
    /// Loads the settings files that changed on disk.
    Reload,
    /// Saves, then asks sway to reload its config.
    ReloadSway,
    /// Notes that sway reloaded its config.
    SwayReloaded,
    /// Writes a generated file that was changed outside Sway Things.
    Overwrite(ConfigFile),
    /// Runs a command in the session now: an autostart entry started by hand. It stores
    /// nothing, so it needs no include line.
    Run(sway_things_core::command::SwayCmd),
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
    /// Where settings are copied to GSettings, read for what a setting that lives only there has
    /// now.
    desktop: Rc<dyn GSettingsStore>,
    /// The files for the notification daemons and the session bus to reach the daemon on;
    /// neither for a model made for tests, unless the applier was given them.
    daemon_files: Option<sway_things_core::notifications::files::Files>,
    daemon_bus: Option<sway_things_core::notifications::Notifications>,
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
        /// What was seen of kanshi when last looked.
        pub(super) kanshi: RefCell<super::KanshiLook>,
        /// When the displays page last looked for kanshi.
        pub(super) kanshi_looked: Cell<Option<std::time::Instant>>,
        /// Whether to look for kanshi at all: not for a model made for tests, so they don't
        /// depend on the machine they run on.
        pub(super) looks_for_kanshi: Cell<bool>,
        pub(super) save_planned: Cell<bool>,
        /// Counts the changes of the outputs, so only the last one has the pictures looked at.
        pub(super) outputs_seen: Cell<u64>,
        /// The GSettings schema the Apps Look & Feel page shows, watched for changes made with
        /// other tools.
        pub(super) desktop_watch: RefCell<Option<gtk::gio::Settings>>,
        /// Where programs are looked for, a list like `$PATH`: `$PATH` itself unless a test
        /// sets its own.
        pub(super) search_path: RefCell<Option<std::ffi::OsString>>,
        /// The files of the Apps & Autostart page that weren't written when last saved, so each
        /// is told once.
        pub(super) apps_problems: RefCell<Vec<sway_things_core::apps::files::Problem>>,
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
                let outdated = model.generated().await.is_some_and(|files| {
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
                model.watch_kanshi();
                model.watch_desktop();
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
        let desktop = applier.gsettings();
        let core = Core {
            ui,
            profiles: applier.profiles().cloned(),
            state: applier.state().clone(),
            pending: applier.pending().cloned(),
            waiting: applier.waiting().clone(),
            compositor: applier.compositor(),
            locations: applier.locations().clone(),
            daemon_files: applier.daemon_files().cloned(),
            daemon_bus: applier.daemon_bus().cloned(),
            applier: Some(applier),
            status,
            devices,
            devices_known: false,
            outputs,
            outputs_known: false,
            autosave,
            desktop,
        };
        self.imp().core.replace(Some(core));
        if let Some(socket) = socket {
            self.watch_devices(socket.clone());
            self.watch_outputs(socket.clone());
            self.watch_reloads(socket);
        }
    }

    /// Follows `org.gnome.desktop.interface`, so a theme, font or style changed with another tool
    /// shows at once. Nothing if the schema isn't installed. Not in tests, which never read the
    /// user's GSettings.
    fn watch_desktop(&self) {
        let Some(schema) = gtk::gio::SettingsSchemaSource::default()
            .and_then(|source| source.lookup(INTERFACE, true))
        else {
            return;
        };
        let settings =
            gtk::gio::Settings::new_full(&schema, gtk::gio::SettingsBackend::NONE, None::<&str>);
        let model = self.downgrade();
        settings.connect_changed(None, move |_, _| {
            if let Some(model) = model.upgrade() {
                model.emit_by_name::<()>("changed", &[]);
            }
        });
        self.imp().desktop_watch.replace(Some(settings));
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
        let view = SettingView::with_desktop(
            setting,
            device,
            &core.state,
            &core.status,
            core.compositor,
            core.known(),
            core.desktop.as_ref(),
        );
        let effect = sway_things_core::apply::InEffect {
            pending: core.pending.as_ref(),
            kanshi: self.kanshi_runs().then_some(&core.outputs),
        };
        Some(sway_things_core::apply::in_effect(
            view,
            setting,
            device,
            &core.state,
            &effect,
        ))
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

    /// What the model saw of kanshi when it last looked.
    pub fn kanshi(&self) -> KanshiLook {
        self.imp().kanshi.borrow().clone()
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
                let product = output.label();
                let label = if product == output.name() {
                    product
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

    /// A copy of the stored settings. `None` if they couldn't be loaded.
    pub fn state(&self) -> Option<State> {
        self.imp()
            .core
            .borrow()
            .as_ref()
            .map(|core| core.state.clone())
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

    /// Runs `command` in the session now, after what is queued: an autostart entry started by
    /// hand. Changes not saved yet are saved first, so the files it needs are there. Without a
    /// session, the window says so.
    pub fn run_command(&self, command: sway_things_core::command::SwayCmd) {
        self.queue(Op::Run(command));
    }

    /// What Sway Things writes to each generated file for the current settings. Nothing is
    /// read or written. `None` if the settings couldn't be loaded.
    pub fn generated_files(&self) -> Option<Vec<GeneratedFile>> {
        let core = self.imp().core.borrow();
        let core = core.as_ref()?;
        Some(generate::generate_for(
            core.state.registry(),
            &core.state,
            core.compositor,
            self.imp().displays.get(),
        ))
    }

    /// What Sway Things writes to each generated file for the current settings, and the file as
    /// it is on disk, read now on a thread of its own, so the window never waits for the disk.
    /// Nothing is written. `None` if the settings couldn't be loaded.
    pub async fn generated(&self) -> Option<Vec<(GeneratedFile, OnDisk)>> {
        let files = self.generated_files()?;
        let locations = self.imp().core.borrow().as_ref()?.locations.clone();
        let read = files.clone();
        let on_disk = gtk::gio::spawn_blocking(move || generate::on_disk(&read, &locations))
            .await
            .ok()?;
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
        if self.kanshi_runs() && setting.sets_up_display() {
            return None;
        }
        Report::check(&core.status, setting, device, core.known())
    }

    /// Whether a reset of `setting` for `device` goes back to the value of the user's config
    /// (and sends it) rather than to sway's default, as [`Model::reset`] does it.
    pub fn resets_to_config(&self, setting: &'static Setting, device: Option<&str>) -> bool {
        let Some(report) = self.report(setting, device) else {
            return false;
        };
        let core = self.imp().core.borrow();
        core.as_ref().is_some_and(|core| {
            if setting.apply() == Apply::Risky {
                // The safety net sends the value in effect without ours: the user's, unless one
                // stored for all devices applies.
                report.user_value().is_some()
                    && (device.is_none() || core.state.get(setting.key(), None).is_none())
            } else {
                sway_things_core::apply::resets_to_users_line(&core.state, setting, device, &report)
            }
        })
    }

    /// Removes the stored value of `key` for `device`, so the user's config or sway's default
    /// decides it again, and sends that value: where a line of the user's config sets it, with
    /// the rest of its directive ([`Model::resets_to_config`]). A risky setting is tried with the
    /// safety net first.
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
        let plan = sway_things_core::apply::PageReset::plan(&core.state, page);
        (plan.removed.len(), plan.kept)
    }

    /// Puts `workspace` on `display` in the fixed placement and places the workspaces.
    async fn fix_placement_now(
        &self,
        applier: &mut Applier,
        workspace: &str,
        display: &str,
        access: Result<WriteAccess, ReadOnly>,
    ) {
        let placed = match access {
            Ok(access) => applier
                .fix_placement(workspace, display, &access)
                .await
                .map_err(|error| error.text().translated()),
            Err(read_only) => Err(explained(&read_only)),
        };
        if let Err(error) = placed {
            self.emit_problem(&trf(
                msg!("The workspaces weren\u{2019}t placed: {error}."),
                &[("error", &error)],
            ));
        }
    }

    /// Removes every stored value of `page`, except those of settings that can lock the user
    /// out ([`Model::page_reset`]).
    pub fn reset_page(&self, page: Page) {
        self.queue(Op::ResetPage(page));
    }

    /// Keeps the risky change that waits: stores it, to be saved like any other change.
    pub fn keep(&self) {
        self.queue(Op::Keep);
        self.follow_after_trial();
    }

    /// Reverts the risky change that waits: puts the value from before it back.
    pub fn revert(&self) {
        self.queue(Op::Revert);
        self.follow_after_trial();
    }

    /// Moves other displays along with the trial of a display's new size that waits, to
    /// `places` ("x,y" by display).
    pub fn move_along(&self, places: Vec<(String, Value)>) {
        self.queue(Op::MoveAlong(places));
    }

    /// After a risky change is kept or reverted: the stored values of the displays follow the
    /// combination again, which they don't while it waits (displays may have come or gone
    /// meanwhile). A follow queued before the change is done moves behind it.
    fn follow_after_trial(&self) {
        self.imp()
            .queue
            .borrow_mut()
            .retain(|op| *op != Op::FollowCombination);
        self.queue(Op::FollowCombination);
    }

    /// Every binding of the user's config and of Sway Things, as last read and stored; `keymap`
    /// compares keysym and keycode bindings. `None` if the settings couldn't be loaded.
    pub fn bindings(
        &self,
        keymap: Option<&dyn sway_things_core::bindings::Keymap>,
    ) -> Option<sway_things_core::bindings::Listing> {
        let core = self.imp().core.borrow();
        let core = core.as_ref()?;
        Some(sway_things_core::bindings::Listing::new(
            &core.status,
            &core.state,
            &core.locations.path(ConfigFile::Bindings),
            keymap,
        ))
    }

    /// The items of `shortcuts.bindings` stored now, or that a change in progress stores.
    pub fn binding_items(&self) -> Vec<String> {
        self.list(sway_things_core::bindings::ours::BINDINGS)
    }

    /// The items of the list setting `key` stored now (for the whole session), or that a change
    /// in progress stores: what a page that edits the list item by item starts from.
    pub fn list(&self, key: &'static str) -> Vec<String> {
        let queued = self
            .imp()
            .queue
            .borrow()
            .iter()
            .rev()
            .find_map(|op| match op {
                Op::Set {
                    key: queued,
                    device: None,
                    value: Value::List(items),
                } if *queued == key => Some(items.clone()),
                _ => None,
            });
        if let Some(items) = queued {
            return items;
        }
        self.imp()
            .core
            .borrow()
            .as_ref()
            .and_then(|core| match core.state.get(key, None) {
                Some(Value::List(items)) => Some(items.clone()),
                _ => None,
            })
            .unwrap_or_default()
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
        // The user's sway config may start kanshi now, or no longer.
        self.look_at_kanshi();
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

    /// Where programs are looked for, a list like `$PATH`: `$PATH` unless
    /// [set](Model::set_search_path).
    pub fn search_path(&self) -> Option<std::ffi::OsString> {
        self.imp()
            .search_path
            .borrow()
            .clone()
            .or_else(|| std::env::var_os("PATH"))
    }

    /// Looks for programs in `path` instead of `$PATH`, for tests, and reads everything again.
    pub fn set_search_path(&self, path: Option<std::ffi::OsString>) {
        self.imp().search_path.replace(path);
        self.emit_by_name::<()>("changed", &[]);
    }

    /// The files for the notification daemons, if they are written.
    pub fn daemon_files(&self) -> Option<sway_things_core::notifications::files::Files> {
        self.imp()
            .core
            .borrow()
            .as_ref()
            .and_then(|core| core.daemon_files.clone())
    }

    /// The session bus the notification daemon is reached on, if any.
    pub fn daemon_bus(&self) -> Option<sway_things_core::notifications::Notifications> {
        self.imp()
            .core
            .borrow()
            .as_ref()
            .and_then(|core| core.daemon_bus.clone())
    }

    /// Where the generated files go.
    pub fn locations(&self) -> Option<Locations> {
        self.imp()
            .core
            .borrow()
            .as_ref()
            .map(|core| core.locations.clone())
    }

    /// Where each workspace goes on the displays connected now, by the Workspaces page's rule,
    /// count and fixed placement, by display name; empty while sway places them. `None` before
    /// the settings are loaded.
    pub fn placement(&self) -> Option<std::collections::BTreeMap<String, String>> {
        use sway_things_core::outputs::workspaces;
        let core = self.imp().core.borrow();
        let core = core.as_ref()?;
        let profiles = core.profiles.as_ref();
        let mirrors = workspaces::mirrored(profiles, &core.outputs);
        Some(workspaces::place(
            workspaces::Rule::of(&core.state),
            &core.outputs,
            &mirrors,
            workspaces::count(&core.state),
            &workspaces::fixed(profiles, &core.outputs),
        ))
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

    /// Whether the displays connected now were set up here: a remembered combination with its
    /// displays, not only a placement of its workspaces. `None` if that isn't known.
    pub fn arranged_here(&self) -> Option<bool> {
        let core = self.imp().core.borrow();
        let core = core.as_ref()?;
        let profiles = core.profiles.as_ref()?;
        let names: Vec<&str> = core
            .outputs
            .iter()
            .filter(|output| output.is_connected())
            .map(sway_things_core::outputs::Output::identifier)
            .collect();
        Some(profiles.arranged(&names).is_some())
    }

    /// Whether the file for kanshi holds copies of the user's own profiles: while Sway Things
    /// places the workspaces, or a combination shows a picture.
    pub fn adopts_profiles(&self) -> bool {
        let core = self.imp().core.borrow();
        core.as_ref().is_some_and(|core| {
            sway_things_core::outputs::workspaces::Placing::of(&core.state).is_some()
                || core.profiles.as_ref().is_some_and(|profiles| {
                    profiles
                        .iter()
                        .any(sway_things_core::outputs::profiles::Profile::mirrors)
                })
        })
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

    /// Puts `workspace` on `display` (by its name in the settings) in the fixed placement for
    /// the displays connected now, and places the workspaces.
    pub fn fix_placement(&self, workspace: &str, display: &str) {
        self.queue(Op::FixPlacement {
            workspace: workspace.to_owned(),
            display: display.to_owned(),
        });
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
            | Op::FollowCombination
            | Op::MoveAlong(_)
            | Op::HealMirrors
            | Op::Overwrite(_)
            | Op::Run(_)
            | Op::FixPlacement { .. }
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
        // A file of the Apps & Autostart page that isn't Sway Things' own stays so: told once.
        let known = self.imp().apps_problems.replace(persisted.apps.clone());
        for problem in &persisted.apps {
            if !known.contains(problem) {
                self.emit_problem(&problem.text().translated());
            }
        }
    }

    /// Tells the window about a problem that isn't one setting's, such as an editor that
    /// can't start.
    pub fn emit_problem(&self, message: &str) {
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

/// Why settings can't be changed, in the user's language.
fn explained(read_only: &ReadOnly) -> String {
    read_only.explanation().translated()
}
