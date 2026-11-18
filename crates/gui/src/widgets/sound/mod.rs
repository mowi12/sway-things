// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Sound page: the output and input devices with their volume, the apps that play or record,
//! and the profiles of the sound cards, live from the sound server (pipewire-pulse or
//! PulseAudio).
//!
//! Nothing here is stored by Sway Things: the server remembers the defaults, volumes and profiles
//! itself. Device and app names come from drivers and apps; they are only ever shown as plain
//! label text.

mod volume;

use std::cell::RefCell;
use std::future::Future;
use std::time::Duration;

use futures_lite::StreamExt;
use futures_lite::future::poll_once;
use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use sway_things_core::delegate::{self, Tool};
use sway_things_core::i18n::Msg;
use sway_things_core::sound::{Card, Device, Error, LOUDEST, Meter, Snapshot, Sound, Stream};

use self::volume::{Kind, VolumeControl};
use super::Placeholder;
use crate::i18n::{msg, tr, trf};
use crate::style;

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
            let mut changes = follow.changes();
            if !refresh(&weak, &follow).await {
                return;
            }
            while changes.next().await.is_some() {
                glib::timeout_future(THROTTLE).await;
                while let Some(Some(())) = poll_once(changes.next()).await {}
                if !refresh(&weak, &follow).await {
                    return;
                }
            }
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

    /// Puts the last snapshot into the widgets.
    fn render(&self) {
        let imp = self.imp();
        let snapshot = imp.snapshot.borrow();
        let Some(snapshot) = snapshot.as_ref() else {
            return;
        };
        let loudest = if imp.above_100.get() { LOUDEST } else { 1.0 };
        imp.updating.set(true);
        Self::render_side(
            &imp.output,
            &snapshot.outputs,
            snapshot.default_output(),
            loudest,
            tr(msg!(
                "No output device. Plug in speakers or headphones; this page updates by itself."
            )),
        );
        Self::render_side(
            &imp.input,
            &snapshot.inputs,
            snapshot.default_input(),
            loudest,
            tr(msg!(
                "No input device. Plug in a microphone or headset; this page updates by itself."
            )),
        );
        imp.meter_row
            .set_visible(snapshot.default_input().is_some());
        imp.updating.set(false);
        self.render_apps(snapshot, loudest);
        self.render_cards(snapshot);
    }

    /// Fills the section of the outputs or inputs: the devices, the connectors, the volume.
    fn render_side(
        side: &Side,
        devices: &[Device],
        current: Option<&Device>,
        loudest: f64,
        empty: &str,
    ) {
        side.list.set_visible(!devices.is_empty());
        set_hint(&side.hint, devices.is_empty().then_some(empty));
        let mut names: Vec<String> = devices.iter().map(|device| device.name.clone()).collect();
        let mut labels: Vec<&str> = devices
            .iter()
            .map(|device| device.description.as_str())
            .collect();
        // With the default unknown (a monitor of an output, or a device not listed yet), the
        // drop-down says so, and every device listed can be chosen to make it the default.
        if current.is_none() && !devices.is_empty() {
            names.insert(0, String::new());
            labels.insert(0, tr(msg!("Another device")));
        }
        let chosen = current.map_or("", |device| device.name.as_str());
        fill(&side.choice, &labels, &side.names, names, chosen);
        side.choice
            .set_sensitive(devices.len() > 1 || current.is_none());
        side.volume_row.set_visible(current.is_some());
        let Some(current) = current else {
            side.port_row.set_visible(false);
            return;
        };

        let ports: Vec<_> = current.usable_ports().collect();
        side.port_row.set_visible(ports.len() > 1);
        let port_names: Vec<String> = ports.iter().map(|port| port.name.clone()).collect();
        let port_labels: Vec<&str> = ports.iter().map(|port| port.description.as_str()).collect();
        let active = current.active_port.clone().unwrap_or_default();
        fill(
            &side.ports,
            &port_labels,
            &side.port_names,
            port_names,
            &active,
        );

        if let Some(volume) = side.volume.get() {
            volume.show(current.volume.level(), current.muted, loudest);
        }
    }

    /// Shows a row per app that plays or records, keeping the rows that stay.
    fn render_apps(&self, snapshot: &Snapshot, loudest: f64) {
        let imp = self.imp();
        let streams: Vec<(bool, &Stream)> = snapshot
            .playing
            .iter()
            .map(|stream| (false, stream))
            .chain(snapshot.recording.iter().map(|stream| (true, stream)))
            .collect();
        let mut rows = imp.streams.borrow_mut();
        rows.retain(|key, row| {
            let keep = streams
                .iter()
                .any(|(recording, stream)| (*recording, stream.index) == *key);
            if !keep {
                imp.apps.remove(&row.row);
            }
            keep
        });
        // A new row goes where it sorts; the rows there stay where they are, so a slider being
        // dragged or focused keeps its place.
        for (position, (recording, stream)) in streams.iter().enumerate() {
            let key = (*recording, stream.index);
            let row = rows.entry(key).or_insert_with(|| {
                let row = self.new_stream_row(*recording, stream.index);
                imp.apps
                    .insert(&row.row, i32::try_from(position).unwrap_or(-1));
                row
            });
            update_stream_row(row, stream, *recording, snapshot, loudest);
        }
        imp.apps.set_visible(!streams.is_empty());
        set_hint(
            &imp.apps_hint,
            streams
                .is_empty()
                .then(|| tr(msg!("No app is playing or recording sound."))),
        );
    }

    /// A row for the stream with the server's `index`.
    fn new_stream_row(&self, recording: bool, index: u32) -> StreamRow {
        let icon = gtk::Image::new();
        icon.set_pixel_size(24);
        let name = gtk::Label::new(None);
        let detail = gtk::Label::new(None);
        detail.add_css_class(style::DIM);
        let kind = if recording {
            Kind::Recording
        } else {
            Kind::Playing
        };
        let volume = VolumeControl::new(tr(msg!("App volume")), kind);
        let weak = self.downgrade();
        let level = std::rc::Rc::new(move |level: f64| {
            let weak = weak.clone();
            let sent: std::pin::Pin<Box<dyn Future<Output = ()>>> = Box::pin(async move {
                let Some((sound, stream)) = weak
                    .upgrade()
                    .and_then(|page| page.stream(recording, index))
                else {
                    return;
                };
                let Some(volume) = stream.volume else {
                    return;
                };
                let volume = volume.with_level(level);
                let result = if recording {
                    sound.set_recording_volume(index, &volume).await
                } else {
                    sound.set_playing_volume(index, &volume).await
                };
                report(&weak, result, msg!("The volume wasn’t changed: {error}"));
            });
            sent
        });
        let weak = self.downgrade();
        let muted = std::rc::Rc::new(move |muted: bool| {
            let weak = weak.clone();
            let sent: std::pin::Pin<Box<dyn Future<Output = ()>>> = Box::pin(async move {
                let Some(sound) = weak
                    .upgrade()
                    .and_then(|page| page.imp().sound.borrow().clone())
                else {
                    return;
                };
                let result = if recording {
                    sound.set_recording_muted(index, muted).await
                } else {
                    sound.set_playing_muted(index, muted).await
                };
                report(&weak, result, msg!("The mute wasn’t changed: {error}"));
            });
            sent
        });
        volume.on_change(level, muted);
        let texts = gtk::Box::new(gtk::Orientation::Vertical, 2);
        for label in [&name, &detail] {
            label.set_xalign(0.0);
            label.set_ellipsize(gtk::pango::EllipsizeMode::End);
            texts.append(label);
        }
        texts.set_width_request(120);
        let line = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        line.append(&icon);
        line.append(&texts);
        line.append(volume.widget());
        StreamRow {
            row: list_row(&line),
            icon,
            name,
            detail,
            volume,
        }
    }

    /// The connection and the stream with the server's `index`, as last read.
    fn stream(&self, recording: bool, index: u32) -> Option<(Sound, Stream)> {
        let imp = self.imp();
        let sound = imp.sound.borrow().clone()?;
        let snapshot = imp.snapshot.borrow();
        let streams = if recording {
            &snapshot.as_ref()?.recording
        } else {
            &snapshot.as_ref()?.playing
        };
        let stream = streams.iter().find(|stream| stream.index == index)?;
        Some((sound, stream.clone()))
    }

    /// Shows a row with the profile choice per card that offers one.
    fn render_cards(&self, snapshot: &Snapshot) {
        let imp = self.imp();
        let cards: Vec<&Card> = snapshot.cards_with_choice().collect();
        let mut rows = imp.card_rows.borrow_mut();
        rows.retain(|index, row| {
            let keep = cards.iter().any(|card| card.index == *index);
            if !keep {
                imp.cards.remove(&row.row);
            }
            keep
        });
        imp.updating.set(true);
        for card in &cards {
            let row = rows.entry(card.index).or_insert_with(|| {
                let row = self.new_card_row(card.index);
                imp.cards.append(&row.row);
                row
            });
            row.name.set_text(&card.description);
            row.choice
                .update_property(&[gtk::accessible::Property::Label(&trf(
                    msg!("Profile of {card}"),
                    &[("card", &card.description)],
                ))]);
            let offered: Vec<_> = card.offered().collect();
            let labels: Vec<&str> = offered.iter().map(|p| p.description.as_str()).collect();
            let names = offered.iter().map(|p| p.name.clone()).collect();
            let active = card.active_profile.as_deref().unwrap_or_default();
            fill(&row.choice, &labels, &row.profiles, names, active);
        }
        imp.updating.set(false);
        imp.cards_box.set_visible(!cards.is_empty());
    }

    /// A row for the card with the server's `index`.
    fn new_card_row(&self, index: u32) -> CardRow {
        let name = gtk::Label::new(None);
        let detail = gtk::Label::new(Some(tr(msg!("Connectors and sound quality"))));
        detail.add_css_class(style::DIM);
        let choice = gtk::DropDown::from_strings(&[]);
        choice.set_valign(gtk::Align::Center);
        shorten_names(&choice);
        let weak = self.downgrade();
        choice.connect_selected_notify(move |choice| {
            if let Some(page) = weak.upgrade() {
                page.profile_chosen(index, choice.selected());
            }
        });
        CardRow {
            row: list_row(&row(&name, &detail, &[choice.upcast_ref()])),
            name,
            choice,
            profiles: RefCell::default(),
        }
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

/// Updates the row of `stream`: its icon, name, what it plays and its volume.
fn update_stream_row(
    row: &StreamRow,
    stream: &Stream,
    recording: bool,
    snapshot: &Snapshot,
    loudest: f64,
) {
    let fallback = if recording {
        "audio-input-microphone-symbolic"
    } else {
        "audio-x-generic-symbolic"
    };
    let theme = gtk::IconTheme::for_display(&row.icon.display());
    let icon = stream
        .icon
        .as_deref()
        .filter(|icon| theme.has_icon(icon))
        .unwrap_or(fallback);
    row.icon.set_icon_name(Some(icon));
    row.name.set_text(&stream.app);
    row.volume.set_names(
        &trf(msg!("Volume of {app}"), &[("app", &stream.app)]),
        &trf(msg!("Mute {app}"), &[("app", &stream.app)]),
    );
    row.detail
        .set_text(&stream_detail(stream, recording, snapshot));
    row.volume.widget().set_visible(stream.volume.is_some());
    if let Some(volume) = &stream.volume {
        row.volume.show(volume.level(), stream.muted, loudest);
    }
}

/// What the row of `stream` says under the app's name: what it plays, that it records, and the
/// device when it isn't the default.
fn stream_detail(stream: &Stream, recording: bool, snapshot: &Snapshot) -> String {
    let (device, default) = if recording {
        (
            snapshot.input_by_index(stream.device),
            snapshot.input().map(|d| d.index),
        )
    } else {
        (
            snapshot.output_by_index(stream.device),
            snapshot.output().map(|d| d.index),
        )
    };
    let elsewhere = device.filter(|device| Some(device.index) != default);
    let what = match (&stream.title, recording) {
        (Some(title), _) => title.clone(),
        (None, true) => tr(msg!("Recording")).to_owned(),
        (None, false) => tr(msg!("Playing")).to_owned(),
    };
    match elsewhere {
        Some(device) => trf(
            msg!("{what} · on {device}"),
            &[("what", &what), ("device", &device.description)],
        ),
        None => what,
    }
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

/// A section heading.
fn heading(title: &str, first: bool) -> gtk::Label {
    let label = gtk::Label::builder()
        .label(title.to_uppercase())
        .xalign(0.0)
        .margin_start(4)
        .margin_bottom(8)
        .margin_top(if first { 0 } else { 24 })
        .accessible_role(gtk::AccessibleRole::Heading)
        .build();
    label.add_css_class("st-section-heading");
    label.add_css_class(style::DIM);
    label
}

/// Makes `list` a bordered list of a section named `title`.
fn section(list: &gtk::ListBox, title: &str) {
    list.set_selection_mode(gtk::SelectionMode::None);
    list.add_css_class("st-section");
    list.update_property(&[gtk::accessible::Property::Label(title)]);
}

/// Styles `label` as the hint shown in place of an empty list.
fn hint(label: &gtk::Label) {
    label.set_xalign(0.0);
    label.set_wrap(true);
    label.add_css_class(style::DIM);
    label.add_css_class("st-section-hint");
    label.set_visible(false);
}

/// Shows `text` in `label`, or hides it.
fn set_hint(label: &gtk::Label, text: Option<&str>) {
    label.set_text(text.unwrap_or_default());
    label.set_visible(text.is_some());
}

/// `child` as a row of a section list. The row itself takes no focus; its controls do.
fn list_row(child: &impl IsA<gtk::Widget>) -> gtk::ListBoxRow {
    let item = gtk::ListBoxRow::new();
    item.set_child(Some(child));
    plain_row(&item);
    item
}

/// Makes `row` a row that takes no focus and can't be activated; its controls can.
fn plain_row(row: &gtk::ListBoxRow) {
    row.set_activatable(false);
    row.set_focusable(false);
}

/// A row with `title` above `subtitle` on the left and `controls` on the right.
fn row(title: &gtk::Label, subtitle: &gtk::Label, controls: &[&gtk::Widget]) -> gtk::Box {
    for label in [title, subtitle] {
        label.set_xalign(0.0);
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    }
    let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
    text.set_hexpand(true);
    text.append(title);
    text.append(subtitle);
    let line = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    line.append(&text);
    for control in controls {
        line.append(*control);
    }
    line
}

#[cfg(test)]
mod tests {
    use sway_things_core::sound::{Device, Server, Snapshot, Stream};

    use super::stream_detail;

    #[test]
    fn an_app_on_another_device_says_so() {
        let device = |index, name: &str, description: &str| Device {
            index,
            name: name.to_owned(),
            description: description.to_owned(),
            ..Device::default()
        };
        let snapshot = Snapshot {
            server: Server {
                default_output: Some("speakers".to_owned()),
                ..Server::default()
            },
            outputs: vec![device(1, "hdmi", "HDMI"), device(2, "speakers", "Speakers")],
            ..Snapshot::default()
        };
        let mut stream = Stream {
            device: 2,
            title: Some("Song".to_owned()),
            ..Stream::default()
        };
        assert_eq!(stream_detail(&stream, false, &snapshot), "Song");
        stream.device = 1;
        assert_eq!(stream_detail(&stream, false, &snapshot), "Song · on HDMI");
        stream.title = None;
        assert_eq!(stream_detail(&stream, true, &snapshot), "Recording");
    }
}
