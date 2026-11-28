// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The "Brightness" section of Brightness & Night Light: live, nothing is stored.
//!
//! - A slider for the screen's backlight (the one the firmware drives first, as systemd and
//!   GNOME prefer it), set through logind; it goes down to 1 %, never to black.
//! - A slider per external monitor that answers over DDC/CI, through ddcutil: found in the
//!   background each time the section shows, and set once the slider stood still for a moment.
//!   Without ddcutil or the access to `/dev/i2c-*`, a notice says how to get it, while an external
//!   display is connected.
//! - The keyboard light, as buttons for its few steps (a slider where it has more).
//!
//! The sliders follow changes made elsewhere (the brightness keys, a bar) every second while the
//! section is on screen; one being moved keeps its place.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use gtk::glib;
use gtk::prelude::*;
use sway_things_core::brightness::ddc::{self, Access, Level, Monitor};
use sway_things_core::brightness::{self, Device, Kind, Logind};
use sway_things_core::packages::Manager;

use super::list::{self, list_row, section_list};
use super::notice::{self, Tone};
use crate::i18n::{Msg, msg, tr, trf};
use crate::style;

/// How long after the last move of a slider what is read doesn't move it back.
const HOLD: Duration = Duration::from_millis(1500);

/// How long a monitor's slider stands still before its value is sent: DDC/CI is slow.
const DDC_SETTLE: Duration = Duration::from_millis(500);

/// The hint while ddcutil looks for monitors.
const LOOKING: Msg = msg!("Looking for external monitors…");
/// The percent beside a slider.
const PERCENT: Msg = msg!("{percent} %");

/// The section's content: the list of what can be dimmed and the notes below it.
pub fn panel() -> gtk::Widget {
    let panel = gtk::Box::new(gtk::Orientation::Vertical, 8);
    let list = section_list(tr(msg!("Brightness")));
    let hint = gtk::Label::new(None);
    list::hint(&hint);
    let notes = gtk::Box::new(gtk::Orientation::Vertical, 8);
    panel.append(&list);
    panel.append(&hint);
    panel.append(&notes);
    let ui = Rc::new(Ui {
        list,
        hint,
        notes,
        logind: RefCell::new(None),
        backlights: RefCell::new(Vec::new()),
        monitors: RefCell::new(Vec::new()),
        detecting: Cell::new(false),
        reading: Cell::new(false),
    });
    ui.build_backlights();
    ui.follow();
    // The closures hold the section's state weakly; the widget keeps it until it goes.
    panel.connect_destroy(move |_| {
        let _ = &ui;
    });
    panel.upcast()
}

/// What the section shows and keeps.
struct Ui {
    list: gtk::ListBox,
    hint: gtk::Label,
    notes: gtk::Box,
    /// The connection to logind, made at the first change.
    logind: RefCell<Option<Logind>>,
    /// The rows of the backlight and the keyboard light.
    backlights: RefCell<Vec<Rc<BacklightRow>>>,
    /// The rows of the external monitors.
    monitors: RefCell<Vec<gtk::ListBoxRow>>,
    /// Whether ddcutil is looking for monitors now.
    detecting: Cell<bool>,
    /// Whether the backlights are being read now.
    reading: Cell<bool>,
}

impl Ui {
    /// A row for the screen's backlight and one for the keyboard light, if there are, once
    /// they are read off the main context.
    fn build_backlights(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let devices = gtk::gio::spawn_blocking(brightness::devices)
                .await
                .unwrap_or_default();
            if let Some(ui) = weak.upgrade() {
                ui.show_backlights(&devices);
            }
        });
    }

    /// Adds the rows for `devices`.
    fn show_backlights(self: &Rc<Self>, devices: &[Device]) {
        let screen = devices.iter().find(|device| device.kind.is_screen());
        let keyboard = devices.iter().find(|device| device.kind == Kind::Keyboard);
        for device in screen.into_iter().chain(keyboard) {
            let row = BacklightRow::new(self, device);
            self.list.append(&row.item);
            self.backlights.borrow_mut().push(row);
        }
        self.show_hint();
    }

    /// The hint in place of an empty list, or while monitors are looked for.
    fn show_hint(&self) {
        let empty = self.backlights.borrow().is_empty() && self.monitors.borrow().is_empty();
        let text = if self.detecting.get() {
            Some(tr(LOOKING))
        } else if empty {
            Some(tr(msg!(
                "Nothing here can be dimmed: there is no backlight, and no external monitor \
                 answers over DDC/CI."
            )))
        } else {
            None
        };
        list::set_hint(&self.hint, text);
        self.list.set_visible(!empty);
    }

    /// Reads the backlights every second while the section is on screen, and looks for the
    /// monitors each time it shows.
    fn follow(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.list.connect_map(move |_| {
            if let Some(ui) = weak.upgrade() {
                ui.detect();
            }
        });
        let weak = Rc::downgrade(self);
        glib::timeout_add_seconds_local(1, move || {
            let Some(ui) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            if ui.list.is_mapped() && !ui.reading.replace(true) {
                let names: Vec<String> = ui
                    .backlights
                    .borrow()
                    .iter()
                    .map(|row| row.device.borrow().name.clone())
                    .collect();
                let weak = Rc::downgrade(&ui);
                glib::spawn_future_local(async move {
                    let devices = gtk::gio::spawn_blocking(move || {
                        names
                            .iter()
                            .map(|name| brightness::device(name))
                            .collect::<Vec<_>>()
                    })
                    .await
                    .unwrap_or_default();
                    let Some(ui) = weak.upgrade() else {
                        return;
                    };
                    ui.reading.set(false);
                    // Collect the rows first: `show` replaces a row's device.
                    let rows: Vec<Rc<BacklightRow>> = ui.backlights.borrow().clone();
                    for (row, device) in rows.iter().zip(devices) {
                        if let Some(device) = device {
                            row.show(&device);
                        }
                    }
                });
            }
            glib::ControlFlow::Continue
        });
    }

    /// The logind connection, made the first time.
    async fn logind(&self) -> Result<Logind, brightness::BrightnessError> {
        if let Some(logind) = self.logind.borrow().as_ref() {
            return Ok(logind.clone());
        }
        let logind = Logind::connect().await?;
        self.logind.replace(Some(logind.clone()));
        Ok(logind)
    }

    /// Looks for the external monitors in the background and shows them, unless it looks
    /// already; without ddcutil or the access to the buses, says how to get them.
    fn detect(self: &Rc<Self>) {
        if self.detecting.replace(true) {
            return;
        }
        while let Some(child) = self.notes.first_child() {
            self.notes.remove(&child);
        }
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let checked = gtk::gio::spawn_blocking(|| {
                let external = external_connected();
                let access = ddc::find().map(|_| ddc::access());
                (external, access)
            })
            .await;
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let (external, access) = checked.unwrap_or((false, None));
            match access {
                Some(Access::Granted) => {}
                None => {
                    ui.detecting.set(false);
                    if external {
                        ui.notes.append(&missing_ddcutil());
                    }
                    return;
                }
                Some(access) => {
                    ui.detecting.set(false);
                    if external {
                        ui.notes.append(&access_notice(access));
                    }
                    return;
                }
            }
            ui.show_hint();
            drop(ui);
            let found = gtk::gio::spawn_blocking(|| {
                ddc::detect().map(|monitors| {
                    monitors
                        .into_iter()
                        .map(|monitor| {
                            let level = ddc::get(monitor.bus);
                            (monitor, level)
                        })
                        .collect::<Vec<_>>()
                })
            })
            .await;
            let Some(ui) = weak.upgrade() else {
                return;
            };
            ui.detecting.set(false);
            for row in ui.monitors.borrow_mut().drain(..) {
                list::remove_row(&row);
            }
            match found {
                Ok(Ok(monitors)) => {
                    for (monitor, level) in monitors {
                        let row = monitor_row(&monitor, &level);
                        ui.list.append(&row);
                        ui.monitors.borrow_mut().push(row);
                    }
                }
                Ok(Err(error)) => {
                    let text = notice::text(&error.text().translated());
                    ui.notes.append(&notice::notice(Tone::Warning, &text, None));
                }
                Err(_) => {}
            }
            ui.show_hint();
        });
    }
}

/// Whether an external display is connected: a connector of the graphics card that isn't a
/// built-in panel says `connected` in sysfs.
fn external_connected() -> bool {
    let Ok(entries) = std::fs::read_dir("/sys/class/drm") else {
        return false;
    };
    entries.filter_map(Result::ok).any(|entry| {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some((card, connector)) = name.split_once('-') else {
            return false;
        };
        card.starts_with("card")
            && !["eDP", "LVDS", "DSI"]
                .iter()
                .any(|internal| connector.starts_with(internal))
            && std::fs::read_to_string(entry.path().join("status"))
                .is_ok_and(|status| status.trim() == "connected")
    })
}

/// A notice with `text`, a command in monospace and a button that copies it.
pub fn command_notice(tone: Tone, text: &str, command: &str) -> gtk::Box {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 6);
    content.append(&notice::text(text));
    let line = gtk::Label::new(Some(command));
    line.set_xalign(0.0);
    line.set_selectable(true);
    line.set_wrap(true);
    line.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    line.add_css_class("monospace");
    content.append(&line);
    let copy = gtk::Button::with_label(tr(msg!("Copy")));
    copy.update_property(&[gtk::accessible::Property::Label(tr(msg!(
        "Copy the command"
    )))]);
    let command = command.to_owned();
    copy.connect_clicked(move |button| button.clipboard().set_text(&command));
    notice::notice(tone, &content, Some(&copy))
}

/// The command that installs `packages` here, or a plain list of them.
pub fn install_line(packages: &[&str]) -> String {
    Manager::detect().map_or_else(
        || packages.join(" "),
        |manager| manager.install_command(packages),
    )
}

/// The notice that ddcutil is needed for external monitors.
fn missing_ddcutil() -> gtk::Box {
    command_notice(
        Tone::Info,
        tr(msg!(
            "To set the brightness of external monitors, install ddcutil; most monitors take it \
             over their cable (DDC/CI)."
        )),
        &install_line(&[ddc::PROGRAM]),
    )
}

/// The notice about the access to the I²C buses that `access` lacks.
fn access_notice(access: Access) -> gtk::Box {
    match access {
        Access::NoBuses => command_notice(
            Tone::Info,
            tr(msg!(
                "External monitors are set over their cable (DDC/CI), which needs the kernel \
                 module i2c-dev. Load it, and have it loaded at boot:"
            )),
            "sudo modprobe i2c-dev && echo i2c-dev | sudo tee /etc/modules-load.d/i2c-dev.conf",
        ),
        Access::Denied | Access::Granted => command_notice(
            Tone::Info,
            tr(msg!(
                "External monitors are set over their cable (DDC/CI), which needs access to \
                 /dev/i2c-*. Add yourself to the i2c group, then log out and in again:"
            )),
            "sudo usermod -aG i2c $USER",
        ),
    }
}

/// A row with `title` and the slider `scale` with its percent.
fn slider_row(title: &str, subtitle: &str, scale: &gtk::Scale, percent: &gtk::Label) -> gtk::Box {
    let title = gtk::Label::new(Some(title));
    let subtitle = gtk::Label::new(Some(subtitle));
    subtitle.add_css_class(style::DIM);
    subtitle.add_css_class("caption");
    subtitle.set_visible(!subtitle.text().is_empty());
    let icon = gtk::Image::from_icon_name("display-brightness-symbolic");
    icon.set_accessible_role(gtk::AccessibleRole::Presentation);
    let line = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    line.append(&icon);
    line.append(scale);
    line.append(percent);
    list::row(&title, &subtitle, &[line.upcast_ref()])
}

/// A slider from `min` to 100 percent and the label of its percent.
fn percent_slider(name: &str, min: f64) -> (gtk::Scale, gtk::Label) {
    let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, min, 100.0, 1.0);
    scale.set_draw_value(false);
    scale.set_hexpand(true);
    scale.set_width_request(160);
    scale.set_valign(gtk::Align::Center);
    scale.set_increments(5.0, 10.0);
    scale.update_property(&[gtk::accessible::Property::Label(name)]);
    let percent = gtk::Label::new(None);
    percent.set_width_chars(5);
    percent.set_xalign(1.0);
    percent.add_css_class("numeric");
    (scale, percent)
}

/// Shows `value` percent in `label`.
fn show_percent(label: &gtk::Label, value: f64) {
    #[expect(clippy::cast_possible_truncation, reason = "a percent from 0 to 100")]
    let shown = value.round() as i64;
    label.set_text(&trf(PERCENT, &[("percent", &shown)]));
}

/// The row of a backlight or the keyboard light: a slider, or buttons for a few steps.
struct BacklightRow {
    item: gtk::ListBoxRow,
    device: RefCell<Device>,
    control: Control,
    subtitle: gtk::Label,
    /// Set while the control is updated from what is read, so the change isn't sent back.
    updating: Cell<bool>,
    /// Until when what is read doesn't move the control.
    held_until: Cell<Option<Instant>>,
    /// The newest raw value not sent yet, while a request is on its way.
    wanted: Cell<Option<u32>>,
    busy: Cell<bool>,
}

/// The control of a backlight row.
enum Control {
    /// A slider in percent and its label.
    Slider(gtk::Scale, gtk::Label),
    /// A button per raw step, for a keyboard light with few steps.
    Steps(Vec<gtk::ToggleButton>),
}

impl BacklightRow {
    fn new(ui: &Rc<Ui>, device: &Device) -> Rc<Self> {
        let screen = device.kind.is_screen();
        let title = if screen {
            tr(msg!("Built-in display"))
        } else {
            tr(msg!("Keyboard backlight"))
        };
        let subtitle = gtk::Label::new(None);
        subtitle.add_css_class(style::DIM);
        subtitle.set_visible(false);
        let (control, widget) = if !screen && device.max <= 3 {
            let steps: Vec<gtk::ToggleButton> = (0..=device.max)
                .map(|step| {
                    let label = if step == 0 {
                        tr(msg!("Off")).to_owned()
                    } else {
                        step.to_string()
                    };
                    gtk::ToggleButton::with_label(&label)
                })
                .collect();
            let linked = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            linked.add_css_class("linked");
            linked.set_valign(gtk::Align::Center);
            for (index, button) in steps.iter().enumerate() {
                if let Some(first) = steps.first().filter(|_| index > 0) {
                    button.set_group(Some(first));
                }
                linked.append(button);
            }
            (Control::Steps(steps), linked.upcast::<gtk::Widget>())
        } else {
            let min = if screen {
                f64::from(brightness::MIN_BACKLIGHT_PERCENT)
            } else {
                0.0
            };
            let (scale, percent) = percent_slider(title, min);
            let icon = gtk::Image::from_icon_name(if screen {
                "display-brightness-symbolic"
            } else {
                "keyboard-brightness-symbolic"
            });
            icon.set_accessible_role(gtk::AccessibleRole::Presentation);
            let line = gtk::Box::new(gtk::Orientation::Horizontal, 6);
            line.append(&icon);
            line.append(&scale);
            line.append(&percent);
            (
                Control::Slider(scale, percent),
                line.upcast::<gtk::Widget>(),
            )
        };
        let title_label = gtk::Label::new(Some(title));
        let content = list::row(&title_label, &subtitle, &[&widget]);
        let row = Rc::new(Self {
            item: list_row(&content),
            device: RefCell::new(device.clone()),
            control,
            subtitle,
            updating: Cell::new(false),
            held_until: Cell::new(None),
            wanted: Cell::new(None),
            busy: Cell::new(false),
        });
        row.show(device);
        row.connect(ui);
        row
    }

    /// Shows the brightness of `device`, unless the control was just moved.
    fn show(&self, device: &Device) {
        self.device.replace(device.clone());
        let held = self.busy.get()
            || self
                .held_until
                .get()
                .is_some_and(|until| Instant::now() < until);
        if held {
            return;
        }
        self.updating.set(true);
        match &self.control {
            Control::Slider(scale, percent) => {
                let value = f64::from(device.percent());
                scale.set_value(value);
                show_percent(percent, scale.value());
            }
            Control::Steps(steps) => {
                let index = usize::try_from(device.brightness).unwrap_or(0);
                if let Some(button) = steps.get(index) {
                    button.set_active(true);
                }
            }
        }
        self.updating.set(false);
    }

    fn connect(self: &Rc<Self>, ui: &Rc<Ui>) {
        match &self.control {
            Control::Slider(scale, percent) => {
                let (row, ui) = (Rc::downgrade(self), Rc::downgrade(ui));
                let percent = percent.downgrade();
                scale.connect_value_changed(move |scale| {
                    let (Some(row), Some(ui)) = (row.upgrade(), ui.upgrade()) else {
                        return;
                    };
                    if let Some(percent) = percent.upgrade() {
                        show_percent(&percent, scale.value());
                    }
                    if row.updating.get() {
                        return;
                    }
                    #[expect(
                        clippy::cast_possible_truncation,
                        clippy::cast_sign_loss,
                        reason = "the slider goes from 0 to 100"
                    )]
                    let value = scale.value().round().max(0.0) as u32;
                    let raw = row.device.borrow().raw_for(value);
                    row.send(&ui, raw);
                });
            }
            Control::Steps(steps) => {
                for (step, button) in steps.iter().enumerate() {
                    let (row, ui) = (Rc::downgrade(self), Rc::downgrade(ui));
                    let raw = u32::try_from(step).unwrap_or(0);
                    button.connect_toggled(move |button| {
                        let (Some(row), Some(ui)) = (row.upgrade(), ui.upgrade()) else {
                            return;
                        };
                        if button.is_active() && !row.updating.get() {
                            row.send(&ui, raw);
                        }
                    });
                }
            }
        }
    }

    /// Sends `raw` through logind, or keeps it for after the request on its way.
    fn send(self: &Rc<Self>, ui: &Rc<Ui>, raw: u32) {
        self.held_until.set(Some(Instant::now() + HOLD));
        if self.busy.get() {
            self.wanted.set(Some(raw));
            return;
        }
        self.busy.set(true);
        let (row, ui) = (Rc::clone(self), Rc::clone(ui));
        glib::spawn_future_local(async move {
            let mut next = Some(raw);
            while let Some(raw) = next {
                let device = row.device.borrow().clone();
                let result = match ui.logind().await {
                    Ok(logind) => logind.set(&device, raw).await,
                    Err(error) => Err(error),
                };
                match result {
                    Ok(()) => row.subtitle.set_visible(false),
                    Err(error) => {
                        row.subtitle.set_text(&error.text().translated());
                        row.subtitle.set_visible(true);
                    }
                }
                next = row.wanted.take();
            }
            row.busy.set(false);
            row.held_until.set(Some(Instant::now() + HOLD));
        });
    }
}

/// The row of an external monitor, with its brightness `level` or why it can't be read.
fn monitor_row(monitor: &Monitor, level: &Result<Level, ddc::DdcError>) -> gtk::ListBoxRow {
    let (scale, percent) = percent_slider(&monitor.name, 0.0);
    let subtitle = match level {
        Ok(_) => monitor.connector.clone().unwrap_or_default(),
        Err(error) => error.text().translated(),
    };
    let content = slider_row(&monitor.name, &subtitle, &scale, &percent);
    let item = list_row(&content);
    let Ok(level) = *level else {
        scale.set_sensitive(false);
        return item;
    };
    scale.set_value(f64::from(level.percent()));
    show_percent(&percent, scale.value());
    let bus = monitor.bus;
    let pending: Rc<RefCell<Option<glib::SourceId>>> = Rc::default();
    let busy = Rc::new(Cell::new(false));
    let wanted: Rc<Cell<Option<u32>>> = Rc::default();
    let percent = percent.downgrade();
    scale.connect_value_changed(move |scale| {
        if let Some(percent) = percent.upgrade() {
            show_percent(&percent, scale.value());
        }
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "the slider goes from 0 to 100"
        )]
        let value = brightness::raw_of(scale.value().round().max(0.0) as u32, level.max);
        if let Some(source) = pending.borrow_mut().take() {
            source.remove();
        }
        let (done, busy, wanted) = (Rc::clone(&pending), Rc::clone(&busy), Rc::clone(&wanted));
        let source = glib::timeout_add_local_once(DDC_SETTLE, move || {
            done.borrow_mut().take();
            if busy.get() {
                wanted.set(Some(value));
                return;
            }
            busy.set(true);
            glib::spawn_future_local(async move {
                let mut next = Some(value);
                while let Some(value) = next {
                    let _ = gtk::gio::spawn_blocking(move || ddc::set(bus, value)).await;
                    next = wanted.take();
                }
                busy.set(false);
            });
        });
        pending.replace(Some(source));
    });
    item
}
