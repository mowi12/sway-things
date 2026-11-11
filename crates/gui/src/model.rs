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

use std::time::Instant;

use gtk::glib;
use gtk::glib::closure_local;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use sway_things_core::apply::{Applier, SAVE_DELAY};
use sway_things_core::generate::Locations;
use sway_things_core::include::{IncludeStatus, IncludeWatch, MainConfig};
use sway_things_core::ipc::Compositor;
use sway_things_core::registry::{Registry, Setting, Value};
use sway_things_core::state::{Paths, State};
use sway_things_core::view::SettingView;

/// What the model does next, one at a time.
#[derive(Debug, Clone, PartialEq)]
enum Op {
    /// Changes a setting.
    Set {
        key: &'static str,
        device: Option<String>,
        value: Value,
    },
    /// Saves the settings files and writes the generated files.
    Persist,
    /// Loads the settings files that changed on disk.
    Reload,
}

/// What the model works with once it is loaded.
#[derive(Debug)]
struct Core {
    /// `None` while a task uses it.
    applier: Option<Applier>,
    /// The state as of the last change, for the rows to read.
    state: State,
    status: IncludeStatus,
    compositor: Compositor,
    locations: Locations,
    /// Whether changes are saved by themselves (not in tests, where saving would run sway).
    autosave: bool,
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
        match Self::connect().await {
            Ok((applier, watch)) => {
                let state_watch = applier.state().watch();
                model.start(applier, watch.status().clone(), true);
                model.watch_config(watch);
                match state_watch {
                    Ok(watcher) => model.watch_state(watcher),
                    Err(error) => model.emit_problem(&format!(
                        "Changes to the settings files from outside won't show: {error}"
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
        let paths = Paths::from_env().map_err(|error| error.to_string())?;
        let locations = Locations::from_env().map_err(|error| error.to_string())?;
        let state = State::load(Registry::builtin(), paths);
        let mut applier = Applier::connect(state, locations.clone())
            .await
            .map_err(|error| error.to_string())?;
        let main = match applier.client() {
            Some(client) => match client.version().await {
                Ok(version) if !version.loaded_config_file_name.is_empty() => {
                    MainConfig::Path(version.loaded_config_file_name.into())
                }
                _ => MainConfig::Default,
            },
            None => MainConfig::Default,
        };
        let watch = IncludeWatch::new(main, locations).map_err(|error| error.to_string())?;
        Ok((applier, watch))
    }

    /// A model of `applier` with the include status `status`, which never changes, and nothing
    /// saved by itself: for tests.
    pub fn with_applier(applier: Applier, status: IncludeStatus) -> Self {
        let model = Self::default();
        model.start(applier, status, false);
        model
    }

    fn start(&self, applier: Applier, status: IncludeStatus, autosave: bool) {
        let core = Core {
            state: applier.state().clone(),
            compositor: applier.compositor(),
            locations: applier.locations().clone(),
            applier: Some(applier),
            status,
            autosave,
        };
        self.imp().core.replace(Some(core));
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
        Some(SettingView::of(
            setting,
            device,
            &core.state,
            &core.status,
            core.compositor,
        ))
    }

    /// Where the generated files go.
    pub fn locations(&self) -> Option<Locations> {
        self.imp()
            .core
            .borrow()
            .as_ref()
            .map(|core| core.locations.clone())
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
            Op::Persist | Op::Reload => false,
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
                model.plan_save();
            }
        ));
    }

    async fn run(&self, op: Op) {
        let (applier, access) = {
            let mut core = self.imp().core.borrow_mut();
            let Some(core) = core.as_mut() else { return };
            let Some(applier) = core.applier.take() else {
                return;
            };
            (applier, core.status.write_access())
        };
        let mut applier = applier;
        match (op, access) {
            (Op::Set { key, device, .. }, Err(read_only)) => {
                self.emit_failed(key, device.as_deref(), read_only.explanation());
            }
            (Op::Set { key, device, value }, Ok(access)) => {
                if let Err(error) = applier.set(key, device.as_deref(), value, &access).await {
                    self.emit_failed(key, device.as_deref(), &error.to_string());
                }
            }
            (Op::Persist, Ok(access)) => match applier.persist(&[], &access).await {
                Ok(persisted) => {
                    for (file, outcome) in persisted.problems() {
                        if let Some(problem) = outcome.problem(file) {
                            self.emit_problem(&problem);
                        }
                    }
                }
                Err(error) => self.emit_problem(&error.to_string()),
            },
            (Op::Reload, Ok(access)) => {
                applier.reload(&access).await;
            }
            // Nothing can be saved or loaded without the include line; it is shown elsewhere.
            (Op::Persist | Op::Reload, Err(_)) => {}
        }
        if let Some(core) = self.imp().core.borrow_mut().as_mut() {
            core.state = applier.state().clone();
            core.applier = Some(applier);
        }
        self.emit_by_name::<()>("changed", &[]);
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
                    let status = status.clone();
                    if let Some(core) = model.imp().core.borrow_mut().as_mut() {
                        core.status = status;
                    }
                    model.emit_by_name::<()>("changed", &[]);
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
