// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Sound page: the output and input devices with their volume, the apps that play or record,
//! and the profiles of the sound cards, live from the sound server (pipewire-pulse or
//! PulseAudio).
//!
//! Nothing here is stored by Sway Things: the server remembers the defaults, volumes and profiles
//! itself. Device and app names come from drivers and apps; they are only ever shown as plain
//! label text.

mod rows;
mod volume;

use std::cell::RefCell;
use std::future::Future;
use std::time::Duration;

use futures_lite::StreamExt;
use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use sway_things_core::delegate::{self, Tool};
use sway_things_core::i18n::Msg;
use sway_things_core::sound::{Device, Error, Meter, Snapshot, Sound};

use self::volume::{Kind, VolumeControl};
use super::Placeholder;
use crate::i18n::{msg, tr, trf};
use crate::style;
use crate::widgets::list::{heading, hint, list_row, plain_row, row, section};
use crate::widgets::live::follow_changes;

/// How long to collect a burst of changes before reading the snapshot again.
const THROTTLE: Duration = Duration::from_millis(100);
/// How often the page looks for a sound server while none runs.
const RETRY: Duration = Duration::from_secs(3);
/// The volume tools "Advanced…" starts, the first installed one.
const ADVANCED: [Tool; 2] = [delegate::PWVUCONTROL, delegate::PAVUCONTROL];

/// Where the page finds the sound server.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Server {
    /// The session's (`PULSE_SERVER`, else the user's runtime directory).
    #[default]
    Session,
    /// The server at this address, for tests.
    Address(String),
    /// None: the page shows that no server runs.
    Off,
}

/// The level meter while it runs: the input's name and the connection it was started on, the
/// meter, and the task showing its levels.
type Metering = ((String, u64), Meter, glib::JoinHandle<()>);

/// A row of an app that plays or records.
struct StreamRow {
    row: gtk::ListBoxRow,
    icon: gtk::Image,
    name: gtk::Label,
    detail: gtk::Label,
    volume: VolumeControl,
}

/// A row of a sound card with its profiles.
struct CardRow {
    row: gtk::ListBoxRow,
    name: gtk::Label,
    choice: gtk::DropDown,
    /// The profile names in the order of the drop-down.
    profiles: RefCell<Vec<String>>,
}

/// The device choice, connector choice and volume of the outputs or the inputs.
#[derive(Default)]
struct Side {
    list: gtk::ListBox,
    hint: gtk::Label,
    choice: gtk::DropDown,
    /// The device names in the order of the drop-down.
    names: RefCell<Vec<String>>,
    port_row: gtk::ListBoxRow,
    ports: gtk::DropDown,
    /// The port names in the order of the drop-down.
    port_names: RefCell<Vec<String>>,
    volume_row: gtk::ListBoxRow,
    volume: std::cell::OnceCell<VolumeControl>,
}

mod imp {
    use std::cell::{Cell, OnceCell, RefCell};
    use std::collections::HashMap;

    use gtk::glib;
    use gtk::prelude::*;
    use gtk::subclass::prelude::*;
    use sway_things_core::sound::{Snapshot, Sound};

    use super::{CardRow, Side, StreamRow};

    #[derive(Default)]
    pub struct SoundPage {
        /// The only child: holds the placeholder and the controls.
        pub content: gtk::Box,
        /// Shown instead of the controls while no sound server runs.
        pub missing: gtk::Box,
        pub controls: gtk::Box,
        pub(super) output: Side,
        pub(super) input: Side,
        pub meter_row: gtk::ListBoxRow,
        pub meter: gtk::LevelBar,
        /// The input measured and the connection it was measured on, the meter, and the task
        /// showing its levels.
        pub metering: RefCell<Option<super::Metering>>,
        /// The window the page is in, and our handler of its `suspended`, while mapped.
        pub window: RefCell<Option<(gtk::Window, glib::SignalHandlerId)>>,
        pub apps: gtk::ListBox,
        pub apps_hint: gtk::Label,
        pub(super) streams: RefCell<HashMap<(bool, u32), StreamRow>>,
        pub cards_box: gtk::Box,
        pub cards: gtk::ListBox,
        pub(super) card_rows: RefCell<HashMap<u32, CardRow>>,
        pub error: gtk::Label,
        pub sound: RefCell<Option<Sound>>,
        pub snapshot: RefCell<Option<Snapshot>>,
        /// Whether volume sliders go above 100 %.
        pub above_100: Cell<bool>,
        /// Set while the drop-downs are filled, so that isn't taken for a choice.
        pub updating: Cell<bool>,
        pub advanced: RefCell<Option<gtk::Button>>,
        pub tasks: RefCell<Vec<glib::JoinHandle<()>>>,
        pub built: OnceCell<()>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SoundPage {
        const NAME: &'static str = "StSoundPage";
        type Type = super::SoundPage;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_css_name("st-sound");
            klass.set_layout_manager_type::<gtk::BinLayout>();
        }
    }

    impl ObjectImpl for SoundPage {
        fn constructed(&self) {
            self.parent_constructed();
            self.content.set_parent(&*self.obj());
            self.obj().build();
        }

        fn dispose(&self) {
            for task in self.tasks.take() {
                task.abort();
            }
            if let Some((_, _, task)) = self.metering.take() {
                task.abort();
            }
            if let Some((window, handler)) = self.window.take() {
                window.disconnect(handler);
            }
            self.content.unparent();
        }
    }

    impl WidgetImpl for SoundPage {
        fn map(&self) {
            self.parent_map();
            // A window on a hidden workspace stays mapped, but is suspended.
            if let Some(window) = self.obj().root().and_downcast::<gtk::Window>() {
                let page = self.obj().downgrade();
                let handler = window.connect_suspended_notify(move |_| {
                    if let Some(page) = page.upgrade() {
                        page.update_meter();
                    }
                });
                if let Some((old, handler)) = self.window.replace(Some((window, handler))) {
                    old.disconnect(handler);
                }
            }
            self.obj().update_meter();
        }

        fn unmap(&self) {
            self.parent_unmap();
            if let Some((window, handler)) = self.window.take() {
                window.disconnect(handler);
            }
            self.obj().update_meter();
        }
    }
}

glib::wrapper! {
    /// The sound devices, their volume and the apps, live from the sound server.
    pub struct SoundPage(ObjectSubclass<imp::SoundPage>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl SoundPage {
    /// The page on `server`, which connects right away and follows the server.
    pub fn new(server: &Server) -> Self {
        let page: Self = glib::Object::new();
        page.follow(server);
        page
    }

    /// Lets the volume sliders go up to 150 % (`true`) or 100 %.
    pub fn set_above_100(&self, above: bool) {
        if self.imp().above_100.replace(above) != above {
            self.render();
        }
    }

    /// The "Advanced…" button for the page header: it starts pwvucontrol or pavucontrol. `None`
    /// if neither is installed.
    pub fn advanced_button(&self) -> Option<gtk::Button> {
        let imp = self.imp();
        let tool = ADVANCED.into_iter().find(|tool| tool.find().is_some())?;
        if imp.advanced.borrow().is_none() {
            let button = gtk::Button::with_label(tr(msg!("Advanced…")));
            button.set_tooltip_text(Some(tr(msg!(
                "Open a volume control for balance, latency and more"
            ))));
            let weak = self.downgrade();
            button.connect_clicked(move |_| {
                if let Some(page) = weak.upgrade() {
                    page.open_advanced(tool);
                }
            });
            imp.advanced.replace(Some(button));
        }
        imp.advanced.borrow().clone()
    }

    /// The output drop-down, for tests.
    pub fn output_choice(&self) -> &gtk::DropDown {
        &self.imp().output.choice
    }

    /// The input drop-down, for tests.
    pub fn input_choice(&self) -> &gtk::DropDown {
        &self.imp().input.choice
    }

    /// The server's name of the output the page shows, for tests.
    pub fn output_name(&self) -> Option<String> {
        self.device(true).map(|(_, device)| device.name)
    }

    /// The output's volume slider and mute button, for tests.
    pub fn output_volume(&self) -> Option<(gtk::Scale, gtk::ToggleButton)> {
        let volume = self.imp().output.volume.get()?;
        Some((volume.scale().clone(), volume.mute_button().clone()))
    }

    /// The level meter of the input, for tests.
    pub fn meter(&self) -> &gtk::LevelBar {
        &self.imp().meter
    }

    /// The name and detail of each app row, in order; for tests.
    pub fn app_rows(&self) -> Vec<(String, String)> {
        let imp = self.imp();
        let streams = imp.streams.borrow();
        let mut rows = Vec::new();
        let mut child = imp.apps.first_child();
        while let Some(widget) = child {
            if let Some(row) = streams
                .values()
                .find(|row| row.row.upcast_ref::<gtk::Widget>() == &widget)
            {
                rows.push((row.name.text().to_string(), row.detail.text().to_string()));
            }
            child = widget.next_sibling();
        }
        rows
    }

    /// The volume slider of the app row named `name`, for tests.
    pub fn app_volume(&self, name: &str) -> Option<gtk::Scale> {
        self.imp()
            .streams
            .borrow()
            .values()
            .find(|row| row.name.text() == name)
            .map(|row| row.volume.scale().clone())
    }

    /// Whether the page shows that it has no sound server (or can't read it) instead of the
    /// controls; for tests.
    pub fn missing_shown(&self) -> bool {
        self.imp().missing.is_visible()
    }

    /// The text of the error under the controls; empty without one. For tests.
    pub fn error_text(&self) -> String {
        let error = &self.imp().error;
        if error.is_visible() {
            error.text().to_string()
        } else {
            String::new()
        }
    }

    /// Lays out the widgets.
    fn build(&self) {
        let imp = self.imp();
        imp.built.get_or_init(|| ());
        imp.content.set_orientation(gtk::Orientation::Vertical);
        imp.content.append(&imp.missing);
        imp.content.append(&imp.controls);
        imp.missing.set_visible(false);
        imp.controls.set_visible(false);
        imp.controls.set_orientation(gtk::Orientation::Vertical);

        self.build_side(true);
        self.build_side(false);
        imp.meter.set_min_value(0.0);
        imp.meter.set_max_value(1.0);
        imp.meter.set_hexpand(true);
        imp.meter.set_valign(gtk::Align::Center);
        imp.meter
            .update_property(&[gtk::accessible::Property::Label(tr(msg!("Input level")))]);
        let meter_hint = gtk::Label::new(Some(tr(msg!("Speak to test the microphone"))));
        meter_hint.add_css_class(style::DIM);
        imp.meter_row.set_child(Some(&row(
            &gtk::Label::new(Some(tr(msg!("Input level")))),
            &meter_hint,
            &[imp.meter.upcast_ref()],
        )));
        plain_row(&imp.meter_row);
        imp.input.list.append(&imp.meter_row);

        imp.controls.append(&heading(tr(msg!("Apps")), false));
        section(&imp.apps, tr(msg!("Apps")));
        imp.controls.append(&imp.apps);
        hint(&imp.apps_hint);
        imp.controls.append(&imp.apps_hint);

        imp.cards_box.set_orientation(gtk::Orientation::Vertical);
        imp.cards_box
            .append(&heading(tr(msg!("Sound cards")), false));
        section(&imp.cards, tr(msg!("Sound cards")));
        imp.cards_box.append(&imp.cards);
        imp.cards_box.set_visible(false);
        imp.controls.append(&imp.cards_box);

        imp.error.set_xalign(0.0);
        imp.error.set_wrap(true);
        imp.error.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        imp.error.set_margin_top(8);
        imp.error.add_css_class("error");
        imp.error.set_visible(false);
        imp.controls.append(&imp.error);
    }

    /// The section of the outputs (`output`) or the inputs.
    fn build_side(&self, output: bool) {
        let imp = self.imp();
        let side = if output { &imp.output } else { &imp.input };
        let (title, device, device_hint, connector, volume_name, kind) = if output {
            (
                tr(msg!("Output")),
                tr(msg!("Output device")),
                tr(msg!("Where sound plays")),
                tr(msg!("Output connector")),
                tr(msg!("Output volume")),
                Kind::Playing,
            )
        } else {
            (
                tr(msg!("Input")),
                tr(msg!("Input device")),
                tr(msg!("What apps record from, like a microphone")),
                tr(msg!("Input connector")),
                tr(msg!("Input volume")),
                Kind::Recording,
            )
        };
        imp.controls.append(&heading(title, output));
        section(&side.list, title);
        imp.controls.append(&side.list);
        hint(&side.hint);
        imp.controls.append(&side.hint);

        side.choice.set_valign(gtk::Align::Center);
        shorten_names(&side.choice);
        side.choice
            .update_property(&[gtk::accessible::Property::Label(device)]);
        let weak = self.downgrade();
        side.choice.connect_selected_notify(move |_| {
            if let Some(page) = weak.upgrade() {
                page.device_chosen(output);
            }
        });
        let subtitle = gtk::Label::new(Some(device_hint));
        subtitle.add_css_class(style::DIM);
        side.list.append(&list_row(&row(
            &gtk::Label::new(Some(device)),
            &subtitle,
            &[side.choice.upcast_ref()],
        )));

        side.ports.set_valign(gtk::Align::Center);
        shorten_names(&side.ports);
        side.ports
            .update_property(&[gtk::accessible::Property::Label(connector)]);
        let weak = self.downgrade();
        side.ports.connect_selected_notify(move |_| {
            if let Some(page) = weak.upgrade() {
                page.port_chosen(output);
            }
        });
        let port_hint = gtk::Label::new(Some(tr(msg!("Like speakers or headphones"))));
        port_hint.add_css_class(style::DIM);
        side.port_row.set_child(Some(&row(
            &gtk::Label::new(Some(tr(msg!("Connector")))),
            &port_hint,
            &[side.ports.upcast_ref()],
        )));
        plain_row(&side.port_row);
        side.port_row.set_visible(false);
        side.list.append(&side.port_row);

        let control = VolumeControl::new(volume_name, kind);
        let mute_name = if output {
            tr(msg!("Mute output"))
        } else {
            tr(msg!("Mute input"))
        };
        control.set_names(volume_name, mute_name);
        self.connect_device_volume(&control, output);
        let line = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        let label = gtk::Label::new(Some(tr(msg!("Volume"))));
        label.set_xalign(0.0);
        line.append(&label);
        line.append(control.widget());
        side.volume_row.set_child(Some(&line));
        plain_row(&side.volume_row);
        side.list.append(&side.volume_row);
        let _ = side.volume.set(control);
    }

    /// Sends the volume and mute of the output (`output`) or input shown, from `control`.
    fn connect_device_volume(&self, control: &VolumeControl, output: bool) {
        let weak = self.downgrade();
        let level = std::rc::Rc::new(move |level: f64| {
            let weak = weak.clone();
            let sent: std::pin::Pin<Box<dyn Future<Output = ()>>> = Box::pin(async move {
                let Some((sound, device)) = weak.upgrade().and_then(|page| page.device(output))
                else {
                    return;
                };
                let volume = device.volume.with_level(level);
                let result = if output {
                    sound.set_output_volume(device.index, &volume).await
                } else {
                    sound.set_input_volume(device.index, &volume).await
                };
                report(&weak, result, msg!("The volume wasn’t changed: {error}"));
            });
            sent
        });
        let weak = self.downgrade();
        let muted = std::rc::Rc::new(move |muted: bool| {
            let weak = weak.clone();
            let sent: std::pin::Pin<Box<dyn Future<Output = ()>>> = Box::pin(async move {
                let Some((sound, device)) = weak.upgrade().and_then(|page| page.device(output))
                else {
                    return;
                };
                let result = if output {
                    sound.set_output_muted(device.index, muted).await
                } else {
                    sound.set_input_muted(device.index, muted).await
                };
                report(&weak, result, msg!("The mute wasn’t changed: {error}"));
            });
            sent
        });
        control.on_change(level, muted);
    }

    /// The connection and the default output (`output`) or input, as last read.
    fn device(&self, output: bool) -> Option<(Sound, Device)> {
        let imp = self.imp();
        let sound = imp.sound.borrow().clone()?;
        let snapshot = imp.snapshot.borrow();
        let snapshot = snapshot.as_ref()?;
        let device = if output {
            snapshot.default_output()
        } else {
            snapshot.default_input()
        };
        Some((sound, device?.clone()))
    }

    /// Connects to the server, shows what it reports and follows its changes until the page
    /// goes. While no server runs, it looks again every few seconds.
    fn follow(&self, server: &Server) {
        let sound = match server {
            Server::Session => Sound::connect(),
            Server::Address(address) => Sound::connect_to(Some(address)),
            Server::Off => Err(Error::Unavailable),
        };
        let sound = match sound {
            Ok(sound) => sound,
            Err(error) => {
                self.show(Err(error));
                return;
            }
        };
        self.imp().sound.replace(Some(sound.clone()));
        let weak = self.downgrade();
        let follow = sound.clone();
        let task = glib::spawn_future_local(async move {
            let changes = follow.changes();
            if !refresh(&weak, &follow).await {
                return;
            }
            follow_changes(changes, THROTTLE, || refresh(&weak, &follow)).await;
        });
        self.imp().tasks.borrow_mut().push(task);
        let weak = self.downgrade();
        let task = glib::spawn_future_local(async move {
            loop {
                glib::timeout_future(RETRY).await;
                let Some(page) = weak.upgrade() else {
                    return;
                };
                let missing = page.imp().snapshot.borrow().is_none();
                drop(page);
                if missing && !refresh(&weak, &sound).await {
                    return;
                }
            }
        });
        self.imp().tasks.borrow_mut().push(task);
    }

    /// Shows `snapshot`, or that no server runs.
    fn show(&self, snapshot: Result<Snapshot, Error>) {
        let imp = self.imp();
        match snapshot {
            Ok(snapshot) => {
                imp.snapshot.replace(Some(snapshot));
                imp.missing.set_visible(false);
                imp.controls.set_visible(true);
                self.render();
            }
            Err(error) => {
                imp.snapshot.replace(None);
                self.show_missing(&error);
            }
        }
        self.update_meter();
    }

    /// Shows a placeholder for `error` instead of the controls.
    fn show_missing(&self, error: &Error) {
        let imp = self.imp();
        let placeholder = match error {
            Error::Unavailable => Placeholder::new(
                "audio-volume-muted-symbolic",
                tr(msg!("No sound server")),
                tr(msg!(
                    "No PulseAudio-compatible sound server is running. Start PipeWire’s with \
                     `systemctl --user start pipewire-pulse`; this page updates by itself."
                )),
            ),
            Error::Failed(_) => Placeholder::new(
                "dialog-error-symbolic",
                tr(msg!("Sound can’t be read")),
                &error.text().translated(),
            ),
        };
        while let Some(child) = imp.missing.first_child() {
            imp.missing.remove(&child);
        }
        placeholder.set_hexpand(true);
        imp.missing.append(&placeholder);
        imp.controls.set_visible(false);
        imp.missing.set_visible(true);
    }

    /// Shows `message` under the controls, or hides it when empty.
    fn show_error(&self, message: &str) {
        let error = &self.imp().error;
        error.set_text(message);
        error.set_visible(!message.is_empty());
    }

    /// Switches the default output (`output`) or input to the one chosen.
    fn device_chosen(&self, output: bool) {
        let imp = self.imp();
        if imp.updating.get() {
            return;
        }
        let side = if output { &imp.output } else { &imp.input };
        // The entry for a default not listed stands for no device.
        let Some(name) = chosen(&side.choice, &side.names.borrow()).filter(|name| !name.is_empty())
        else {
            return;
        };
        let Some(sound) = imp.sound.borrow().clone() else {
            return;
        };
        self.run(
            async move {
                if output {
                    sound.set_default_output(&name).await
                } else {
                    sound.set_default_input(&name).await
                }
            },
            msg!("The device wasn’t switched: {error}"),
        );
    }

    /// Switches the default output's (`output`) or input's connector to the one chosen.
    fn port_chosen(&self, output: bool) {
        let imp = self.imp();
        if imp.updating.get() {
            return;
        }
        let side = if output { &imp.output } else { &imp.input };
        let Some(port) = chosen(&side.ports, &side.port_names.borrow()) else {
            return;
        };
        let Some((sound, device)) = self.device(output) else {
            return;
        };
        self.run(
            async move {
                if output {
                    sound.set_output_port(device.index, &port).await
                } else {
                    sound.set_input_port(device.index, &port).await
                }
            },
            msg!("The connector wasn’t switched: {error}"),
        );
    }

    /// Switches the card with the server's `index` to the profile at `position`.
    fn profile_chosen(&self, index: u32, position: u32) {
        let imp = self.imp();
        if imp.updating.get() {
            return;
        }
        let profile = imp.card_rows.borrow().get(&index).and_then(|row| {
            usize::try_from(position)
                .ok()
                .and_then(|position| row.profiles.borrow().get(position).cloned())
        });
        let (Some(profile), Some(sound)) = (profile, imp.sound.borrow().clone()) else {
            return;
        };
        self.run(
            async move { sound.set_card_profile(index, &profile).await },
            msg!("The profile wasn’t switched: {error}"),
        );
    }

    /// Starts `tool`.
    fn open_advanced(&self, tool: Tool) {
        match tool.launch() {
            Ok(mut child) => {
                // Waiting reaps it once it ends, so it doesn't stay behind as a zombie.
                glib::spawn_future_local(async move {
                    let _ = child.status().await;
                });
            }
            Err(error) => self.show_error(&trf(
                msg!("{tool} can’t be started: {error}"),
                &[("tool", &tool.program), ("error", &error)],
            )),
        }
    }

    /// Runs `action`, showing its error with `failure`.
    fn run(&self, action: impl Future<Output = Result<(), Error>> + 'static, failure: Msg) {
        let weak = self.downgrade();
        glib::spawn_future_local(async move {
            let result = action.await;
            report(&weak, result, failure);
        });
    }

    /// Starts, moves or stops the level meter: it records the default input's level while the
    /// page is on screen (mapped, in a window that isn't suspended on a hidden workspace), and
    /// starts again on a new connection to the server.
    fn update_meter(&self) {
        let imp = self.imp();
        let visible = self.is_mapped()
            && self
                .root()
                .and_downcast::<gtk::Window>()
                .is_none_or(|window| !window.is_suspended());
        let wanted = visible
            .then(|| self.device(false))
            .flatten()
            .map(|(sound, device)| {
                let generation = sound.generation();
                (sound, (device.name, generation))
            });
        let running = imp
            .metering
            .borrow()
            .as_ref()
            .map(|(key, _, _)| key.clone());
        if running.as_ref() == wanted.as_ref().map(|(_, key)| key) {
            return;
        }
        if let Some((_, _, task)) = imp.metering.take() {
            task.abort();
        }
        imp.meter.set_value(0.0);
        let Some((sound, key)) = wanted else {
            return;
        };
        let Ok(meter) = Meter::start(&sound, &key.0) else {
            return;
        };
        let mut levels = meter.levels();
        let bar = imp.meter.downgrade();
        let task = glib::spawn_future_local(async move {
            while let Some(level) = levels.next().await {
                let Some(bar) = bar.upgrade() else {
                    return;
                };
                // The square root shows quiet speech, which peaks far below full scale.
                bar.set_value(level.sqrt());
            }
        });
        imp.metering.replace(Some((key, meter, task)));
    }
}

/// Reads the snapshot of `sound` and shows it on the page; `false` once the page is gone.
async fn refresh(page: &glib::WeakRef<SoundPage>, sound: &Sound) -> bool {
    let snapshot = sound.snapshot().await;
    let Some(page) = page.upgrade() else {
        return false;
    };
    page.show(snapshot);
    true
}

/// Shows the error of `result` on the page with `failure`, or clears an earlier one.
fn report(page: &glib::WeakRef<SoundPage>, result: Result<(), Error>, failure: Msg) {
    let Some(page) = page.upgrade() else {
        return;
    };
    match result {
        Ok(()) => page.show_error(""),
        Err(error) => {
            page.show_error(&trf(failure, &[("error", &error.text().translated())]));
            // A refused choice gives way to what the server has.
            page.render();
        }
    }
}

/// Fills `choice` with `labels`, keeps `names` in their order and selects `current`.
fn fill(
    choice: &gtk::DropDown,
    labels: &[&str],
    store: &RefCell<Vec<String>>,
    names: Vec<String>,
    current: &str,
) {
    let changed = *store.borrow() != names
        || choice
            .model()
            .is_none_or(|model| model.n_items() != u32::try_from(labels.len()).unwrap_or(0));
    if changed {
        // Replacing the items of the same list keeps an open list open.
        match choice.model().and_downcast::<gtk::StringList>() {
            Some(list) => list.splice(0, list.n_items(), labels),
            None => choice.set_model(Some(&gtk::StringList::new(labels))),
        }
    }
    let index = names.iter().position(|name| name == current);
    choice.set_selected(
        index
            .and_then(|i| u32::try_from(i).ok())
            .unwrap_or(u32::MAX),
    );
    store.replace(names);
}

/// The name behind the choice of `choice` in `names`.
fn chosen(choice: &gtk::DropDown, names: &[String]) -> Option<String> {
    usize::try_from(choice.selected())
        .ok()
        .and_then(|index| names.get(index).cloned())
}

/// Makes `choice` as wide as its names need, up to 40 characters, and shorten longer ones (a
/// card's `… HDMI / DisplayPort 3 Output`) with an ellipsis; it shrinks to 12 characters when the
/// page is narrow. The open list shows the names whole, wrapped only beyond 50 characters.
fn shorten_names(choice: &gtk::DropDown) {
    let factory = |shown: bool| {
        let factory = gtk::SignalListItemFactory::new();
        factory.connect_setup(move |_, item| {
            let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
                return;
            };
            let label = gtk::Label::new(None);
            label.set_xalign(0.0);
            if shown {
                label.set_ellipsize(gtk::pango::EllipsizeMode::End);
                label.set_width_chars(12);
                label.set_max_width_chars(40);
            } else {
                // A wrapped label asks only for its longest word; this keeps the lines long.
                label.set_wrap(true);
                label.set_width_chars(30);
                label.set_max_width_chars(50);
            }
            item.set_child(Some(&label));
        });
        factory.connect_bind(|_, item| {
            let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
                return;
            };
            let text = item
                .item()
                .and_downcast::<gtk::StringObject>()
                .map(|object| object.string());
            if let Some(label) = item.child().and_downcast::<gtk::Label>() {
                label.set_text(text.as_deref().unwrap_or_default());
                label.set_tooltip_text(text.as_deref());
            }
        });
        factory
    };
    choice.set_factory(Some(&factory(true)));
    choice.set_list_factory(Some(&factory(false)));
}
