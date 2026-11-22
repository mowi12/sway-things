// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The settings of one item, in two levels:
//!
//! - **Basic**: a few rows in plain words, for the items most bars have: the clock's format as
//!   examples with the time now, what the battery, volume and network show, the battery's
//!   warning levels, the workspaces of every display, the window title's length; for a command
//!   of the user's, the program, its arguments, how often it runs and a test run ([`script`]);
//!   else the format with what it shows.
//! - **Advanced** (an expander): every option the catalog knows as a field (format strings with
//!   what they show, numbers checked against their range, choices with waybar's default first),
//!   and the options it doesn't know as JSON, kept as they are.
//!
//! The basic rows fill in the advanced fields, so _Save_ stores one definition. A field left
//! empty leaves the option out, so waybar's default applies. Commands are the user's data:
//! they are stored and shown as written, and waybar runs them.
//!
//! [`script`]: super::script

use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;
use serde_json::{Map, Number, Value as Json};
use sway_things_core::bar::catalog::{self, Kind, Opt};
use sway_things_core::bar::groups::{self, Drawer};
use sway_things_core::bar::is_module_name;

use super::preview::{item_title, now};
use super::{Ctx, lanes, script};
use crate::i18n::{Msg, msg, tr, trf};
use crate::style;

/// A field whose value isn't a number.
const NOT_A_NUMBER: Msg = msg!("{option}: must be a number from {min} to {max}");
/// The JSON of the other options doesn't read.
const BAD_JSON: Msg = msg!("Other options: {error}");
/// The JSON isn't an object.
const NOT_AN_OBJECT: Msg = msg!("Other options must be one JSON object, in braces");
/// The name of a new command item isn't one.
const BAD_NAME: Msg = msg!("The name can't be empty or have spaces, and no other item may have it");
/// The preview of a format.
const PREVIEW: Msg = msg!("Shows as: {text}");
/// A format of the user's own, which only the advanced field shows.
const OWN_FORMAT: Msg = msg!("Your own: {format}");

/// A field of the form and how its value is read.
pub(crate) enum Field {
    Text(gtk::Entry),
    Number(gtk::Entry, Kind),
    Choice(gtk::DropDown, Vec<Option<Json>>),
}

/// What the form of an item holds: its advanced fields, the JSON of the other options, and the
/// line that says what is wrong.
pub(crate) struct Form {
    fields: Vec<(&'static str, Field)>,
    buffer: gtk::TextBuffer,
    error: gtk::Label,
    name_entry: Option<gtk::Entry>,
}

impl Form {
    fn field(&self, key: &str) -> Option<&Field> {
        self.fields
            .iter()
            .find(|(field, _)| *field == key)
            .map(|(_, field)| field)
    }

    /// The text of the text or number field `key`.
    pub(crate) fn text(&self, key: &str) -> String {
        match self.field(key) {
            Some(Field::Text(entry) | Field::Number(entry, _)) => entry.text().to_string(),
            _ => String::new(),
        }
    }

    /// Sets the text of the text or number field `key`.
    pub(crate) fn set_text(&self, key: &str, text: &str) {
        if let Some(Field::Text(entry) | Field::Number(entry, _)) = self.field(key) {
            if entry.text() != text {
                entry.set_text(text);
            }
        }
    }

    /// The value the choice field `key` has picked; `None` for waybar's default.
    pub(crate) fn choice(&self, key: &str) -> Option<Json> {
        match self.field(key) {
            Some(Field::Choice(dropdown, values)) => usize::try_from(dropdown.selected())
                .ok()
                .and_then(|at| values.get(at))
                .cloned()
                .flatten(),
            _ => None,
        }
    }

    /// Picks `value` in the choice field `key` (`None`: waybar's default).
    pub(crate) fn set_choice(&self, key: &str, value: Option<&Json>) {
        if let Some(Field::Choice(dropdown, values)) = self.field(key) {
            if let Some(at) = values
                .iter()
                .position(|candidate| candidate.as_ref() == value)
                .and_then(|at| u32::try_from(at).ok())
            {
                dropdown.set_selected(at);
            }
        }
    }

    /// The other options as JSON, if they read.
    fn others(&self) -> Option<Map<String, Json>> {
        let buffer = &self.buffer;
        let text = buffer.text(&buffer.start_iter(), &buffer.end_iter(), false);
        if text.trim().is_empty() {
            return Some(Map::new());
        }
        match sway_things_core::bar::jsonc::parse(&text) {
            Ok(Json::Object(map)) => Some(map),
            _ => None,
        }
    }

    /// The other option `key`.
    pub(crate) fn other(&self, key: &str) -> Option<Json> {
        self.others()?.get(key).cloned()
    }

    /// Sets the other option `key`, or with `None` takes it out. Nothing happens while the JSON
    /// doesn't read: the user's text isn't replaced.
    pub(crate) fn set_other(&self, key: &str, value: Option<Json>) {
        let Some(mut others) = self.others() else {
            return;
        };
        match value {
            Some(value) => {
                others.insert(key.to_owned(), value);
            }
            None => {
                others.remove(key);
            }
        }
        self.buffer.set_text(&pretty(&others));
    }
}

/// `others` as the JSON field shows them.
fn pretty(others: &Map<String, Json>) -> String {
    if others.is_empty() {
        "{}".to_owned()
    } else {
        serde_json::to_string_pretty(&Json::Object(others.clone())).unwrap_or_default()
    }
}

/// A dialog window titled `title`, `width` by `height`, over the window of `anchor`, closed with
/// Escape.
pub(crate) fn window(anchor: &gtk::Widget, title: &str, width: i32, height: i32) -> gtk::Window {
    let window = gtk::Window::builder()
        .modal(true)
        .destroy_with_parent(true)
        .default_width(width)
        .default_height(height)
        .title(title)
        .build();
    window.set_transient_for(anchor.root().and_downcast::<gtk::Window>().as_ref());
    window.add_css_class("st-dialog");
    let escape = gtk::ShortcutController::new();
    escape.add_shortcut(gtk::Shortcut::new(
        gtk::ShortcutTrigger::parse_string("Escape"),
        Some(gtk::CallbackAction::new(|widget, _| {
            if let Some(window) = widget.downcast_ref::<gtk::Window>() {
                window.close();
            }
            glib::Propagation::Stop
        })),
    ));
    window.add_controller(escape);
    window
}

/// Opens the settings of item `name`, over the window of `anchor`.
pub fn open(ctx: &Rc<Ctx>, anchor: &gtk::Widget, name: &str) {
    build(ctx, anchor, name, None);
}

/// Opens the form of a new item that shows a command of the user's, added to lane `key`.
pub fn new_command(ctx: &Rc<Ctx>, anchor: &gtk::Widget, key: &'static str) {
    build(ctx, anchor, "custom/", Some(key));
}

/// The dialog of item `name`, or with `add_to` of a new command item for that lane.
#[expect(
    clippy::too_many_lines,
    reason = "the parts of one dialog, read best in order"
)]
fn build(ctx: &Rc<Ctx>, anchor: &gtk::Widget, name: &str, add_to: Option<&'static str>) {
    let modules = match ctx.model().bar_modules() {
        Some(Ok(modules)) => modules,
        Some(Err(error)) => {
            let window = anchor.root().and_downcast::<gtk::Window>();
            gtk::AlertDialog::builder()
                .message(error.text().translated())
                .modal(true)
                .build()
                .show(window.as_ref());
            return;
        }
        None => return,
    };
    let creating = add_to.is_some();
    let definition = if creating {
        Map::new()
    } else {
        modules.definition(name).unwrap_or_default()
    };
    let title = if creating {
        tr(msg!("Your Own Command")).to_owned()
    } else {
        item_title(&modules, name)
    };
    let window = window(anchor, &title, 580, 680);
    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.add_css_class("st-bar-dialog-content");
    let heading = gtk::Label::new(Some(&title));
    heading.add_css_class("title-2");
    heading.set_xalign(0.0);
    heading.set_accessible_role(gtk::AccessibleRole::Heading);
    content.append(&heading);
    let module = catalog::find(name);
    let about = gtk::Label::new(Some(&match module {
        Some(module) if !creating => {
            let description = tr(module.description);
            if name.contains('/') && module.name == "custom" {
                format!("{description}. {name}")
            } else {
                description.to_owned()
            }
        }
        _ => tr(msg!(
            "Shows what a program or script of yours prints. Waybar runs it; Sway Things runs \
             it only when you press Test."
        ))
        .to_owned(),
    }));
    about.set_xalign(0.0);
    about.set_wrap(true);
    about.add_css_class(style::DIM);
    content.append(&about);
    let name_entry = creating.then(|| {
        let entry = gtk::Entry::new();
        entry.set_placeholder_text(Some("updates"));
        labeled(&content, tr(msg!("Name, without spaces")), &entry);
        entry
    });
    let advanced = gtk::Box::new(gtk::Orientation::Vertical, 8);
    advanced.add_css_class("st-bar-advanced");
    let form = Rc::new(form(&advanced, &definition, name, name_entry));
    let basic = basic(&content, &form, name);
    let expander = gtk::Expander::new(Some(tr(msg!("Advanced: every option"))));
    expander.set_child(Some(&advanced));
    expander.set_expanded(!basic);
    content.append(&expander);
    content.append(&form.error);
    let scrolled = gtk::ScrolledWindow::new();
    scrolled.set_hscrollbar_policy(gtk::PolicyType::Never);
    scrolled.set_vexpand(true);
    scrolled.set_child(Some(&content));
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    buttons.add_css_class("st-bar-dialog-buttons");
    // A group's definition holds its members: it has no default to go back to.
    if !creating && modules.is_stored(name) && !groups::is_group(name) {
        let reset = gtk::Button::with_label(tr(msg!("Reset to Default")));
        reset.set_tooltip_text(Some(tr(msg!(
            "Back to the item's definition in waybar's default config, or none"
        ))));
        let name = name.to_owned();
        let weak = Rc::downgrade(ctx);
        let closing = window.downgrade();
        reset.connect_clicked(move |_| {
            if let Some(ctx) = weak.upgrade() {
                if let Some(Ok(mut modules)) = ctx.model().bar_modules() {
                    modules.reset(&name);
                    ctx.model().set_bar_modules(modules);
                }
            }
            if let Some(window) = closing.upgrade() {
                window.close();
            }
        });
        buttons.append(&reset);
    }
    let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    buttons.append(&spacer);
    let cancel = gtk::Button::with_label(tr(msg!("Cancel")));
    let closing = window.downgrade();
    cancel.connect_clicked(move |_| {
        if let Some(window) = closing.upgrade() {
            window.close();
        }
    });
    buttons.append(&cancel);
    let save = gtk::Button::with_label(tr(if creating { msg!("Add") } else { msg!("Save") }));
    save.add_css_class("suggested-action");
    buttons.append(&save);
    window.set_default_widget(Some(&save));
    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.append(&scrolled);
    root.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    root.append(&buttons);
    window.set_child(Some(&root));
    let name = name.to_owned();
    let weak = Rc::downgrade(ctx);
    let closing = window.downgrade();
    save.connect_clicked(move |_| {
        let (Some(ctx), Some(window)) = (weak.upgrade(), closing.upgrade()) else {
            return;
        };
        if store(&form, &ctx, &name, add_to) {
            window.close();
        }
    });
    window.present();
}

/// The basic rows of item `name`, which fill in the fields of `form`. Says whether it has any.
#[expect(
    clippy::too_many_lines,
    reason = "the rows of each kind of item, read best as a list"
)]
fn basic(content: &gtk::Box, form: &Rc<Form>, name: &str) -> bool {
    let kind = catalog::find(name).map_or("", |module| module.name);
    let list = crate::widgets::list::section_list(tr(msg!("Basic")));
    list.add_css_class("st-bar-basic");
    let rows: Vec<gtk::Widget> = match kind {
        "clock" => clock(form),
        "battery" => {
            let mut rows = vec![shows(
                form,
                name,
                "format",
                msg!("Shows"),
                &["{capacity}% {icon}", "{capacity}%", "{icon}"],
            )];
            rows.push(level(form, "warning", msg!("Warn below"), 30));
            rows.push(level(form, "critical", msg!("Critical below"), 15));
            rows
        }
        "pulseaudio" | "wireplumber" => vec![
            shows(
                form,
                name,
                "format",
                msg!("Shows"),
                &["{volume}% {icon}", "{volume}%", "{icon}"],
            ),
            number(
                form,
                "scroll-step",
                msg!("Scroll step (percent)"),
                1.0,
                25.0,
                1.0,
            ),
        ],
        "network" => vec![shows(
            form,
            name,
            "format-wifi",
            msg!("On Wi-Fi, shows"),
            &[
                "{essid} ({signalStrength}%) {icon}",
                "{essid}",
                "{signalStrength}%",
                "{icon}",
            ],
        )],
        "sway/language" => vec![shows(
            form,
            name,
            "format",
            msg!("Shows"),
            &["{short}", "{long}", "{flag}"],
        )],
        "sway/workspaces" => vec![
            switch(
                form,
                "all-outputs",
                msg!("Workspaces of every display"),
                false,
                false,
            ),
            switch(
                form,
                "disable-scroll",
                msg!("Switch with the scroll wheel"),
                true,
                true,
            ),
        ],
        "sway/window" => vec![
            number(
                form,
                "max-length",
                msg!("Longest title (characters, 0: any)"),
                0.0,
                1000.0,
                0.0,
            ),
            switch(form, "icon", msg!("Show the app's icon"), false, false),
        ],
        "tray" => vec![
            number(form, "icon-size", msg!("Icon size"), 8.0, 128.0, 16.0),
            number(
                form,
                "spacing",
                msg!("Space between icons"),
                0.0,
                64.0,
                10.0,
            ),
        ],
        "custom" => {
            script::basic(content, form);
            return true;
        }
        "group" => group_rows(form),
        _ => {
            let mut rows = Vec::new();
            if form.field("format").is_some() {
                rows.push(format_row(form, name));
            }
            if form.field("interval").is_some() {
                rows.push(number(
                    form,
                    "interval",
                    msg!("Update every (seconds)"),
                    1.0,
                    3600.0,
                    10.0,
                ));
            }
            rows
        }
    };
    if rows.is_empty() {
        return false;
    }
    for row in rows {
        list.append(&crate::widgets::list::list_row(&row));
    }
    content.append(&list);
    true
}

/// A row with `title` and `control` beside it.
fn line(title: &str, control: &impl IsA<gtk::Widget>) -> gtk::Widget {
    let label = gtk::Label::new(Some(title));
    label.set_xalign(0.0);
    label.set_wrap(true);
    label.set_hexpand(true);
    let control = control.as_ref();
    control.set_valign(gtk::Align::Center);
    control.update_property(&[gtk::accessible::Property::Label(title)]);
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    row.add_css_class("st-bar-basic-row");
    row.append(&label);
    row.append(control);
    row.upcast()
}

/// The clock's formats as examples with the time now, as radio buttons; seconds update every
/// second.
fn clock(form: &Rc<Form>) -> Vec<gtk::Widget> {
    const FORMATS: [&str; 6] = [
        "{:%H:%M}",
        "{:%I:%M %p}",
        "{:%H:%M:%S}",
        "{:%a %d %b  %H:%M}",
        "{:%a %d %b  %I:%M %p}",
        "{:%Y-%m-%d  %H:%M}",
    ];
    let current = form.text("format");
    // waybar's own format, when none is set.
    let current = if current.is_empty() {
        FORMATS[0].to_owned()
    } else {
        current
    };
    let title = gtk::Label::new(Some(tr(msg!("Shows"))));
    title.set_xalign(0.0);
    let choices = gtk::Box::new(gtk::Orientation::Vertical, 4);
    choices.set_accessible_role(gtk::AccessibleRole::RadioGroup);
    choices.update_property(&[gtk::accessible::Property::Label(tr(msg!("Clock format")))]);
    let mut group: Option<gtk::CheckButton> = None;
    let mut known = false;
    for format in FORMATS {
        let button = gtk::CheckButton::with_label(&catalog::preview("clock", format, now()));
        if let Some(first) = &group {
            button.set_group(Some(first));
        } else {
            group = Some(button.clone());
        }
        if format == current {
            button.set_active(true);
            known = true;
        }
        let weak = Rc::downgrade(form);
        button.connect_toggled(move |button| {
            let Some(form) = weak.upgrade() else { return };
            if !button.is_active() {
                return;
            }
            form.set_text("format", format);
            // Seconds need an update every second; waybar's default is a minute.
            if format.contains("%S") {
                form.set_text("interval", "1");
            } else if form.text("interval") == "1" {
                form.set_text("interval", "");
            }
        });
        choices.append(&button);
    }
    if !known {
        let own = gtk::CheckButton::with_label(&trf(OWN_FORMAT, &[("format", &current)]));
        own.set_group(group.as_ref());
        own.set_active(true);
        choices.append(&own);
    }
    let row = gtk::Box::new(gtk::Orientation::Vertical, 8);
    row.add_css_class("st-bar-basic-row");
    row.append(&title);
    row.append(&choices);
    vec![row.upcast()]
}

/// A drop-down of what text field `key` shows, from `formats`, each shown as it would look; a
/// format of the user's stays, as the last entry.
fn shows(
    form: &Rc<Form>,
    name: &str,
    key: &'static str,
    title: Msg,
    formats: &[&'static str],
) -> gtk::Widget {
    let current = form.text(key);
    let mut labels: Vec<String> = formats
        .iter()
        .map(|format| {
            let shown = catalog::preview(name, format, now());
            let shown = shown.trim();
            if shown.is_empty() {
                (*format).to_owned()
            } else {
                shown.to_owned()
            }
        })
        .collect();
    let mut at = formats.iter().position(|format| *format == current);
    if at.is_none() && !current.is_empty() {
        labels.push(trf(OWN_FORMAT, &[("format", &current)]));
        at = Some(formats.len());
    }
    let strings: Vec<&str> = labels.iter().map(String::as_str).collect();
    let dropdown = gtk::DropDown::from_strings(&strings);
    if let Some(at) = at.and_then(|at| u32::try_from(at).ok()) {
        dropdown.set_selected(at);
    } else {
        dropdown.set_selected(gtk::INVALID_LIST_POSITION);
    }
    let formats: Vec<&'static str> = formats.to_vec();
    let weak = Rc::downgrade(form);
    dropdown.connect_selected_notify(move |dropdown| {
        let Some(form) = weak.upgrade() else { return };
        if let Some(format) = usize::try_from(dropdown.selected())
            .ok()
            .and_then(|at| formats.get(at))
        {
            form.set_text(key, format);
        }
    });
    line(tr(title), &dropdown)
}

/// A spin button for the warning level `state` of a battery (`states` in the other options).
fn level(form: &Rc<Form>, state: &'static str, title: Msg, default: i64) -> gtk::Widget {
    let current = form
        .other("states")
        .and_then(|states| states.get(state)?.as_i64())
        .unwrap_or(default);
    let spin = gtk::SpinButton::with_range(0.0, 100.0, 5.0);
    #[expect(clippy::cast_precision_loss, reason = "a percentage")]
    spin.set_value(current as f64);
    let weak = Rc::downgrade(form);
    spin.connect_value_changed(move |spin| {
        let Some(form) = weak.upgrade() else { return };
        let mut states = form
            .other("states")
            .and_then(|states| states.as_object().cloned())
            .unwrap_or_default();
        states.insert(state.to_owned(), Json::from(spin.value_as_int()));
        form.set_other("states", Some(Json::Object(states)));
    });
    let unit = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    unit.append(&spin);
    unit.append(&gtk::Label::new(Some("%")));
    spin.update_property(&[gtk::accessible::Property::Label(tr(title))]);
    line(tr(title), &unit)
}

/// The drawer the form of a group holds, if it is one.
fn drawer_of(form: &Form) -> Option<Drawer> {
    let mut definition = Map::new();
    definition.insert("drawer".to_owned(), form.other("drawer")?);
    groups::drawer(&definition)
}

/// Makes the group of the form a drawer like `drawer`, or a plain pill.
fn set_drawer(form: &Form, drawer: Option<Drawer>) {
    let mut definition = Map::new();
    if let Some(options) = form.other("drawer") {
        definition.insert("drawer".to_owned(), options);
    }
    groups::set_drawer(&mut definition, drawer);
    form.set_other("drawer", definition.get("drawer").cloned());
}

/// The rows of a group: its items, whether it is a drawer that shows the others only on hover,
/// where and how fast it opens, and whether on a click.
fn group_rows(form: &Rc<Form>) -> Vec<gtk::Widget> {
    let members: Vec<String> = form
        .other("modules")
        .and_then(|members| members.as_array().cloned())
        .unwrap_or_default()
        .iter()
        .filter_map(Json::as_str)
        .map(|member| item_title(&sway_things_core::bar::Modules::default(), member))
        .collect();
    let items = gtk::Label::new(Some(&if members.is_empty() {
        tr(msg!("No items yet")).to_owned()
    } else {
        members.join(", ")
    }));
    items.set_xalign(1.0);
    items.set_wrap(true);
    items.add_css_class(style::DIM);
    let current = drawer_of(form);
    let drawer = gtk::Switch::new();
    drawer.set_active(current.is_some());
    let direction =
        gtk::DropDown::from_strings(&[tr(msg!("Toward the left")), tr(msg!("Toward the right"))]);
    direction.set_selected(u32::from(!current.unwrap_or_default().to_left));
    let duration = gtk::SpinButton::with_range(0.0, f64::from(groups::MAX_DURATION), 50.0);
    duration.set_value(f64::from(current.unwrap_or_default().duration));
    let click = gtk::Switch::new();
    click.set_active(current.is_some_and(|drawer| drawer.on_click));
    let dependent: Vec<gtk::Widget> = vec![
        direction.clone().upcast(),
        duration.clone().upcast(),
        click.clone().upcast(),
    ];
    let sync = {
        let form = Rc::downgrade(form);
        let (drawer, direction, duration, click) = (
            drawer.downgrade(),
            direction.downgrade(),
            duration.downgrade(),
            click.downgrade(),
        );
        move || {
            let (Some(form), Some(drawer), Some(direction), Some(duration), Some(click)) = (
                form.upgrade(),
                drawer.upgrade(),
                direction.upgrade(),
                duration.upgrade(),
                click.upgrade(),
            ) else {
                return;
            };
            for widget in [
                direction.upcast_ref::<gtk::Widget>(),
                duration.upcast_ref(),
                click.upcast_ref(),
            ] {
                widget.set_sensitive(drawer.is_active());
            }
            let wanted = drawer.is_active().then(|| Drawer {
                duration: u32::try_from(duration.value_as_int()).unwrap_or(250),
                to_left: direction.selected() == 0,
                on_click: click.is_active(),
            });
            set_drawer(&form, wanted);
        }
    };
    for widget in &dependent {
        widget.set_sensitive(current.is_some());
    }
    let on_drawer = sync.clone();
    drawer.connect_active_notify(move |_| on_drawer());
    let on_direction = sync.clone();
    direction.connect_selected_notify(move |_| on_direction());
    let on_duration = sync.clone();
    duration.connect_value_changed(move |_| on_duration());
    click.connect_active_notify(move |_| sync());
    vec![
        line(tr(msg!("Items")), &items),
        line(tr(msg!("Show the others only on hover")), &drawer),
        line(tr(msg!("The others slide out")), &direction),
        line(tr(msg!("Slide time (milliseconds)")), &duration),
        line(tr(msg!("Open on a click instead of on hover")), &click),
    ]
}

/// A spin button for the number field `key`, from `min` to `max`; it shows `default` while the
/// field is empty.
fn number(
    form: &Rc<Form>,
    key: &'static str,
    title: Msg,
    min: f64,
    max: f64,
    default: f64,
) -> gtk::Widget {
    let spin = gtk::SpinButton::with_range(min, max, 1.0);
    spin.set_value(form.text(key).parse::<f64>().unwrap_or(default));
    let weak = Rc::downgrade(form);
    spin.connect_value_changed(move |spin| {
        if let Some(form) = weak.upgrade() {
            form.set_text(key, &spin.value_as_int().to_string());
        }
    });
    line(tr(title), &spin)
}

/// A switch for the on/off field `key`, which is `default` while not set; `inverted` where the
/// option says the opposite of the row (`disable-scroll` for "Switch with the scroll wheel").
fn switch(
    form: &Rc<Form>,
    key: &'static str,
    title: Msg,
    default: bool,
    inverted: bool,
) -> gtk::Widget {
    let value = form
        .choice(key)
        .and_then(|value| value.as_bool())
        .unwrap_or(if inverted { !default } else { default });
    let switch = gtk::Switch::new();
    switch.set_active(value != inverted);
    let weak = Rc::downgrade(form);
    switch.connect_active_notify(move |switch| {
        if let Some(form) = weak.upgrade() {
            form.set_choice(key, Some(&Json::Bool(switch.is_active() != inverted)));
        }
    });
    line(tr(title), &switch)
}

/// The format of an item in a field, with what it shows below it.
fn format_row(form: &Rc<Form>, name: &str) -> gtk::Widget {
    let entry = gtk::Entry::new();
    entry.set_text(&form.text("format"));
    entry.set_placeholder_text(Some(tr(msg!("waybar's default"))));
    entry.set_hexpand(true);
    entry.update_property(&[gtk::accessible::Property::Label(tr(msg!("Format")))]);
    let weak = Rc::downgrade(form);
    entry.connect_changed(move |entry| {
        if let Some(form) = weak.upgrade() {
            form.set_text("format", &entry.text());
        }
    });
    let title = gtk::Label::new(Some(tr(msg!("Format"))));
    title.set_xalign(0.0);
    let row = gtk::Box::new(gtk::Orientation::Vertical, 6);
    row.add_css_class("st-bar-basic-row");
    row.append(&title);
    row.append(&entry);
    row.append(&preview(&entry, name));
    row.upcast()
}

/// The form of item `name` with `definition` in `content`: a field per option of the catalog,
/// and the other options as JSON.
fn form(
    content: &gtk::Box,
    definition: &Map<String, Json>,
    name: &str,
    name_entry: Option<gtk::Entry>,
) -> Form {
    let module = catalog::find(name);
    let options: &[Opt] = module.map_or(&[], |module| module.options);
    let mut others = definition.clone();
    let mut fields: Vec<(&'static str, Field)> = Vec::new();
    for option in options {
        let current = others.get(option.key).cloned();
        let (field, taken) = field_for(option, current.as_ref());
        if taken {
            others.remove(option.key);
        }
        let shown = format!("{} ({})", tr(option.title), option.key);
        labeled(content, &shown, field_widget(&field));
        if option.kind == Kind::Format {
            if let Field::Text(entry) = &field {
                content.append(&preview(entry, name));
            }
        }
        fields.push((option.key, field));
    }
    let other = gtk::Label::new(Some(tr(msg!("Other options (JSON)"))));
    other.set_xalign(0.0);
    other.add_css_class("heading");
    content.append(&other);
    let buffer = gtk::TextBuffer::new(None);
    buffer.set_text(&pretty(&others));
    let text = gtk::TextView::with_buffer(&buffer);
    text.add_css_class("monospace");
    text.set_monospace(true);
    text.set_wrap_mode(gtk::WrapMode::WordChar);
    text.set_size_request(-1, 120);
    text.update_property(&[gtk::accessible::Property::Label(tr(msg!(
        "Other options (JSON)"
    )))]);
    let frame = gtk::Frame::new(None);
    frame.set_child(Some(&text));
    content.append(&frame);
    let error = gtk::Label::new(None);
    error.set_xalign(0.0);
    error.set_wrap(true);
    error.add_css_class("error");
    error.set_visible(false);
    Form {
        fields,
        buffer,
        error,
        name_entry,
    }
}

/// Stores what `form` holds as the definition of item `name` (or of the new command item it
/// names, added to the lane `add_to`). Shows what is wrong and says `false` if it can't.
fn store(form: &Form, ctx: &Rc<Ctx>, name: &str, add_to: Option<&'static str>) -> bool {
    let fail = |message: &str| {
        form.error.set_text(message);
        form.error.set_visible(true);
        form.error
            .announce(message, gtk::AccessibleAnnouncementPriority::High);
        false
    };
    let definition = match collect(&form.fields, &form.buffer) {
        Ok(definition) => definition,
        Err(message) => return fail(&message),
    };
    let target = match &form.name_entry {
        Some(entry) => {
            let wanted = format!("custom/{}", entry.text().trim());
            if !is_module_name(&wanted) || wanted == "custom/" || lanes::taken(ctx, &wanted) {
                return fail(tr(BAD_NAME));
            }
            wanted
        }
        None => name.to_owned(),
    };
    let Some(Ok(mut modules)) = ctx.model().bar_modules() else {
        return false;
    };
    modules.set(&target, definition);
    ctx.model().set_bar_modules(modules);
    if let Some(key) = add_to {
        lanes::add_item(ctx, key, &target);
    }
    true
}

/// The field for `option` with its `current` value, and whether the field holds it (a value of
/// another type stays in the JSON).
fn field_for(option: &Opt, current: Option<&Json>) -> (Field, bool) {
    match option.kind {
        Kind::Format | Kind::Text | Kind::Command => {
            let entry = gtk::Entry::new();
            entry.set_placeholder_text(Some(tr(msg!("waybar's default"))));
            if option.kind == Kind::Command {
                entry.add_css_class("monospace");
            }
            let taken = match current {
                Some(Json::String(text)) => {
                    entry.set_text(text);
                    true
                }
                None => true,
                Some(_) => false,
            };
            (Field::Text(entry), taken)
        }
        Kind::Integer { .. } | Kind::Decimal { .. } => {
            let entry = gtk::Entry::new();
            entry.set_placeholder_text(Some(tr(msg!("waybar's default"))));
            entry.set_input_purpose(gtk::InputPurpose::Number);
            let taken = match current {
                Some(Json::Number(number)) => {
                    entry.set_text(&number.to_string());
                    true
                }
                None => true,
                Some(_) => false,
            };
            (Field::Number(entry, option.kind), taken)
        }
        Kind::Bool => {
            let values = vec![None, Some(Json::Bool(true)), Some(Json::Bool(false))];
            let labels = [
                tr(msg!("waybar's default")),
                tr(msg!("On")),
                tr(msg!("Off")),
            ];
            choice(&labels, values, current)
        }
        Kind::Choice(words) => {
            let mut labels = vec![tr(msg!("waybar's default"))];
            let mut values = vec![None];
            for word in words {
                labels.push(if word.is_empty() {
                    tr(msg!("Plain text"))
                } else {
                    word
                });
                values.push(Some(Json::String((*word).to_owned())));
            }
            choice(&labels, values, current)
        }
    }
}

/// A drop-down of `labels` for `values`, with `current` picked if it is one of them.
fn choice(labels: &[&str], values: Vec<Option<Json>>, current: Option<&Json>) -> (Field, bool) {
    let dropdown = gtk::DropDown::from_strings(labels);
    let at = values
        .iter()
        .position(|value| value.as_ref() == current)
        .and_then(|at| u32::try_from(at).ok());
    if let Some(at) = at {
        dropdown.set_selected(at);
    }
    (Field::Choice(dropdown, values), at.is_some())
}

fn field_widget(field: &Field) -> &gtk::Widget {
    match field {
        Field::Text(entry) | Field::Number(entry, _) => entry.upcast_ref(),
        Field::Choice(dropdown, _) => dropdown.upcast_ref(),
    }
}

/// `widget` under a label, with the label as its accessible name.
fn labeled(content: &gtk::Box, label: &str, widget: &impl IsA<gtk::Widget>) {
    let title = gtk::Label::new(Some(label));
    title.set_xalign(0.0);
    title.set_wrap(true);
    content.append(&title);
    widget
        .as_ref()
        .update_property(&[gtk::accessible::Property::Label(label)]);
    content.append(widget);
}

/// The preview under a format field, following what is typed.
fn preview(entry: &gtk::Entry, name: &str) -> gtk::Label {
    let label = gtk::Label::new(None);
    label.set_xalign(0.0);
    label.set_wrap(true);
    label.add_css_class(style::DIM);
    let name = name.to_owned();
    let weak = label.downgrade();
    let update = move |entry: &gtk::Entry| {
        let Some(label) = weak.upgrade() else { return };
        let text = entry.text();
        if text.is_empty() {
            label.set_text("");
            return;
        }
        let shown = catalog::preview(&name, &text, now());
        label.set_text(&trf(PREVIEW, &[("text", &shown)]));
    };
    update(entry);
    entry.connect_changed(update);
    label
}

/// The definition the form holds: the JSON of the other options with the fields that are set.
fn collect(
    fields: &[(&'static str, Field)],
    buffer: &gtk::TextBuffer,
) -> Result<Map<String, Json>, String> {
    let text = buffer.text(&buffer.start_iter(), &buffer.end_iter(), false);
    let mut definition = if text.trim().is_empty() {
        Map::new()
    } else {
        match sway_things_core::bar::jsonc::parse(&text) {
            Ok(Json::Object(map)) => map,
            Ok(_) => return Err(tr(NOT_AN_OBJECT).to_owned()),
            Err(error) => {
                return Err(trf(BAD_JSON, &[("error", &error.text().translated())]));
            }
        }
    };
    for (key, field) in fields {
        let value = match field {
            Field::Text(entry) => {
                let text = entry.text();
                (!text.is_empty()).then(|| Json::String(text.to_string()))
            }
            Field::Number(entry, kind) => number_of(entry, *kind, key)?,
            Field::Choice(dropdown, values) => usize::try_from(dropdown.selected())
                .ok()
                .and_then(|at| values.get(at))
                .and_then(Clone::clone),
        };
        if let Some(value) = value {
            definition.insert((*key).to_owned(), value);
        }
    }
    Ok(definition)
}

/// The number in `entry`, checked against `kind`; `None` while it is empty.
fn number_of(entry: &gtk::Entry, kind: Kind, key: &str) -> Result<Option<Json>, String> {
    let text = entry.text();
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    let (min, max, whole) = match kind {
        #[expect(clippy::cast_precision_loss, reason = "the ranges are far below 2^52")]
        Kind::Integer { min, max } => (min as f64, max as f64, true),
        Kind::Decimal { min, max } => (min, max, false),
        _ => (f64::MIN, f64::MAX, false),
    };
    let wrong = || {
        trf(
            NOT_A_NUMBER,
            &[("option", &key), ("min", &min), ("max", &max)],
        )
    };
    if whole {
        let value: i64 = text.parse().map_err(|_| wrong())?;
        #[expect(clippy::cast_precision_loss, reason = "the ranges are far below 2^52")]
        let in_range = (min..=max).contains(&(value as f64));
        return if in_range {
            Ok(Some(Json::from(value)))
        } else {
            Err(wrong())
        };
    }
    let value: f64 = text.parse().map_err(|_| wrong())?;
    if !(min..=max).contains(&value) {
        return Err(wrong());
    }
    Number::from_f64(value)
        .map(|number| Some(Json::Number(number)))
        .ok_or_else(wrong)
}
