// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Language & Formats section: the user's language and formats, picked from the installed
//! UTF-8 locales by their names, with a preview of the formats; _System default…_ in the
//! heading sets the system's through systemd-localed.
//!
//! The language and formats are stored settings that go into Sway Things' environment.d file and
//! apply after the next login; the notice at the top says when the session doesn't read that
//! file ([`crate::widgets::environment::note`]). Locale names come from `locale` and iso-codes
//! and are only ever shown as plain label text.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use gtk::glib;
use gtk::prelude::*;
use sway_things_core::region::localed::{self, Localed, SystemLocale};
use sway_things_core::region::locales::{self, Locale, Moment, Preview};
use sway_things_core::region::{Bus, Error};
use sway_things_core::registry::region::{FORMATS, LANGUAGE};
use sway_things_core::registry::{Registry, Value};
use sway_things_core::scanners::look::Family;

use super::Help;
use crate::i18n::{msg, tr, trf};
use crate::model::Model;
use crate::style;
use crate::widgets::SettingRow;
use crate::widgets::list::{heading, list_row, section};
use crate::widgets::notice::{self, Tone};

/// The widgets and state of the section. Cheap to clone; every clone shares them.
#[derive(Clone)]
pub struct LanguageSection {
    inner: Rc<Inner>,
}

struct Inner {
    /// Opens the dialog for the system's language and formats.
    system_button: gtk::Button,
    /// The rows of the language and the formats, without a model none.
    language: Option<SettingRow>,
    formats: Option<SettingRow>,
    /// The preview of the formats in effect: a label per line.
    preview: Vec<gtk::Label>,
    help: Help,
    bus: Bus,
    /// The installed locales: their names as Sway Things writes them and as people read them.
    installed: RefCell<Vec<(String, String)>>,
    /// The system's locale, once localed answered.
    system: RefCell<Option<SystemLocale>>,
    /// The locale the preview shows, so it is only read again when it changes.
    previewed: RefCell<Option<String>>,
    /// Set while a change waits for localed (and polkit).
    busy: Cell<bool>,
    tasks: RefCell<Vec<glib::JoinHandle<()>>>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        for task in self.tasks.take() {
            task.abort();
        }
    }
}

/// The names of the preview's lines, in order.
const PREVIEW_LINES: [sway_things_core::registry::Msg; 6] = [
    msg!("Date"),
    msg!("Time"),
    msg!("Number"),
    msg!("Money"),
    msg!("Paper"),
    msg!("Units"),
];

impl LanguageSection {
    /// The section, appended to `content`, with the rows of the settings from `model` and
    /// localed on `bus`.
    pub fn new(content: &gtk::Box, model: Option<&Model>, bus: &Bus) -> Self {
        let title = tr(msg!("Language & Formats"));
        let line = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        line.set_margin_top(24);
        line.set_margin_bottom(8);
        let label = heading(title, false);
        label.set_margin_top(0);
        label.set_margin_bottom(0);
        label.set_hexpand(true);
        label.set_valign(gtk::Align::Center);
        line.append(&label);
        let system_button = gtk::Button::with_label(tr(msg!("System Default…")));
        system_button.add_css_class("flat");
        system_button.set_valign(gtk::Align::Center);
        system_button.set_tooltip_text(Some(tr(msg!(
            "The language and formats of every user, the login screen and the console"
        ))));
        system_button.set_sensitive(false);
        line.append(&system_button);
        content.append(&line);

        if let Some(model) = model {
            content.append(&crate::widgets::environment::note(model));
        }
        let list = gtk::ListBox::new();
        section(&list, title);
        let row = |key| {
            let model = model?;
            let setting = Registry::builtin().get(key)?;
            SettingRow::with_choices(model, setting, None, &[])
        };
        let language = row(LANGUAGE);
        let formats = row(FORMATS);
        for row in [&language, &formats].into_iter().flatten() {
            list.append(&list_row(row));
            // Below the row: which line of a later environment.d file sets one of its variables
            // again, as below the rows of the Environment page.
            for widget in model
                .map(|model| crate::widgets::environment::below(model, row))
                .unwrap_or_default()
            {
                list.append(&list_row(&widget));
            }
        }
        let (preview_box, preview) = preview_row();
        list.append(&list_row(&preview_box));
        content.append(&list);

        content.append(&add_notice(Family::read()));
        let help = Help::new();
        content.append(&help.root);

        let inner = Inner {
            system_button,
            language,
            formats,
            preview,
            help,
            bus: bus.clone(),
            installed: RefCell::default(),
            system: RefCell::default(),
            previewed: RefCell::default(),
            busy: Cell::new(false),
            tasks: RefCell::default(),
        };
        let section = Self {
            inner: Rc::new(inner),
        };
        section.build(model);
        section
    }

    fn build(&self, model: Option<&Model>) {
        let inner = &self.inner;
        let weak = Rc::downgrade(&self.inner);
        inner.system_button.connect_clicked(move |button| {
            if let Some(inner) = weak.upgrade() {
                Self { inner }.pick_system(button);
            }
        });
        if let Some(model) = model {
            let weak = Rc::downgrade(&self.inner);
            model.connect_changed(move |_| {
                if let Some(inner) = weak.upgrade() {
                    Self { inner }.render();
                }
            });
        }
        self.load();
        self.render();
    }

    /// Reads the installed locales and the system's, then shows them.
    fn load(&self) {
        let weak = Rc::downgrade(&self.inner);
        let task = glib::spawn_future_local(async move {
            let names = locales::installed().await;
            let installed = names
                .into_iter()
                .map(|name| {
                    let label = Locale::parse(&name).map_or_else(
                        || name.clone(),
                        |locale| {
                            trf(
                                // TRANSLATORS: An installed locale in the list: its name and how
                                // it is written, such as "German (Germany) · de_DE.UTF-8".
                                msg!("{name} · {locale}"),
                                &[("name", &locale.display_name()), ("locale", &name)],
                            )
                        },
                    );
                    (name, label)
                })
                .collect::<Vec<_>>();
            let mut installed = installed;
            installed.sort_by_cached_key(|(_, label)| label.to_lowercase());
            let Some(inner) = weak.upgrade() else {
                return;
            };
            inner.installed.replace(installed);
            let section = LanguageSection { inner };
            section.render();
            section.read_system().await;
        });
        self.inner.tasks.borrow_mut().push(task);
    }

    /// Reads the system's locale from localed; without localed, _System Default…_ says why it
    /// can't be used.
    async fn read_system(&self) {
        let bus = self.inner.bus.clone();
        let weak = Rc::downgrade(&self.inner);
        let read = async {
            let localed = Localed::connect(&bus).await?;
            localed.locale().await
        };
        let result = read.await;
        let Some(inner) = weak.upgrade() else {
            return;
        };
        match result {
            Ok(system) => {
                inner.system.replace(Some(system));
                inner.system_button.set_sensitive(!inner.busy.get());
                inner.system_button.set_tooltip_text(Some(tr(msg!(
                    "The language and formats of every user, the login screen and the console"
                ))));
            }
            Err(error) => {
                inner.system.replace(None);
                inner.system_button.set_sensitive(false);
                let reason = match &error {
                    Error::Unavailable(_) => localed::gone().translated(),
                    other => other.text().translated(),
                };
                inner.system_button.set_tooltip_text(Some(&reason));
            }
        }
        LanguageSection { inner }.render();
    }

    /// Puts the choices, the defaults and the preview into the rows.
    fn render(&self) {
        let inner = &self.inner;
        let installed = inner.installed.borrow().clone();
        let system = inner.system.borrow().clone();
        let name_of = |locale: &str| {
            installed
                .iter()
                .find(|(name, _)| name == locale)
                .map(|(_, label)| label.clone())
                .or_else(|| Locale::parse(locale).map(|locale| locale.display_name()))
                .unwrap_or_else(|| locale.to_owned())
        };
        let text_of = |row: &Option<SettingRow>| match row.as_ref().and_then(SettingRow::value) {
            Some(Value::Text(text)) => text,
            _ => String::new(),
        };
        let language = text_of(&inner.language);
        let formats = text_of(&inner.formats);
        let system_language = system
            .as_ref()
            .map(|system| system.language.clone())
            .filter(|language| !language.is_empty());

        if let Some(row) = &inner.language {
            let default = match &system_language {
                Some(system) => trf(
                    msg!("System default ({language})"),
                    &[("language", &name_of(system))],
                ),
                None => tr(msg!("System default")).to_owned(),
            };
            row.set_choices(&choices(default, &installed));
        }
        if let Some(row) = &inner.formats {
            let follows = if language.is_empty() {
                system_language.clone()
            } else {
                Some(language.clone())
            };
            let default = match &follows {
                Some(locale) => trf(
                    msg!("Same as language ({language})"),
                    &[("language", &name_of(locale))],
                ),
                None => tr(msg!("Same as language")).to_owned(),
            };
            row.set_choices(&choices(default, &installed));
        }

        // The formats in effect after the next login: the ones picked, else the language's, else
        // the system's, else the session's.
        let effective = [formats, language]
            .into_iter()
            .find(|locale| !locale.is_empty())
            .or_else(|| system.as_ref().and_then(|system| system.formats.clone()))
            .or(system_language)
            .or_else(|| std::env::var("LC_TIME").ok().filter(|v| !v.is_empty()))
            .or_else(|| std::env::var("LANG").ok().filter(|v| !v.is_empty()))
            .map(|locale| Locale::parse(&locale).map_or(locale, |parsed| parsed.name()));
        self.show_preview(effective);
        inner
            .system_button
            .set_sensitive(!inner.busy.get() && inner.system.borrow().is_some());
    }

    /// Shows the preview of `locale`'s formats, read again only when it changed.
    fn show_preview(&self, locale: Option<String>) {
        if *self.inner.previewed.borrow() == locale {
            return;
        }
        self.inner.previewed.replace(locale.clone());
        let weak = Rc::downgrade(&self.inner);
        let task = glib::spawn_future_local(async move {
            let keywords = match &locale {
                Some(locale) => locales::keywords(locale).await,
                None => None,
            };
            let Some(inner) = weak.upgrade() else {
                return;
            };
            if *inner.previewed.borrow() != locale {
                return;
            }
            fill_preview(&inner.preview, keywords.as_ref());
        });
        self.inner.tasks.borrow_mut().push(task);
    }

    /// Opens the dialog for the system's language and formats; what is picked is set through
    /// localed.
    fn pick_system(&self, button: &gtk::Button) {
        let Some(system) = self.inner.system.borrow().clone() else {
            return;
        };
        let installed = self.inner.installed.borrow().clone();
        let parent = button.root().and_downcast::<gtk::Window>();
        let weak = Rc::downgrade(&self.inner);
        open_system_dialog(parent.as_ref(), &installed, &system, move |variables| {
            if let Some(inner) = weak.upgrade() {
                Self { inner }.set_system(variables);
            }
        });
    }

    /// Asks localed to set `variables` and shows what came of it.
    fn set_system(&self, variables: Vec<String>) {
        let inner = &self.inner;
        inner.busy.set(true);
        inner.help.show(None, None);
        self.render();
        let bus = inner.bus.clone();
        let weak: Weak<Inner> = Rc::downgrade(&self.inner);
        let task = glib::spawn_future_local(async move {
            let result = match Localed::connect(&bus).await {
                Ok(localed) => localed.set_locale(&variables).await,
                Err(error) => Err(error),
            };
            let Some(inner) = weak.upgrade() else {
                return;
            };
            inner.busy.set(false);
            if let Err(error) = &result {
                let command = matches!(error, Error::Denied(_))
                    .then(|| SystemLocale::change(&variables).command());
                inner
                    .help
                    .show(Some(&error.text().translated()), command.as_deref());
            }
            LanguageSection { inner }.read_system().await;
        });
        self.inner.tasks.borrow_mut().push(task);
    }

    /// The row of the language, for tests.
    pub fn language_row(&self) -> Option<&SettingRow> {
        self.inner.language.as_ref()
    }

    /// The row of the formats, for tests.
    pub fn formats_row(&self) -> Option<&SettingRow> {
        self.inner.formats.as_ref()
    }

    /// The rows of the stored settings.
    pub fn rows(&self) -> Vec<SettingRow> {
        [&self.inner.language, &self.inner.formats]
            .into_iter()
            .flatten()
            .cloned()
            .collect()
    }

    /// The button that opens the dialog for the system's locale, for tests.
    pub fn system_button(&self) -> &gtk::Button {
        &self.inner.system_button
    }

    /// The lines of the preview, for tests.
    pub fn preview_texts(&self) -> Vec<String> {
        self.inner
            .preview
            .iter()
            .map(|label| label.text().to_string())
            .collect()
    }

    /// The help shown after a failed change, for tests.
    pub fn help(&self) -> &Help {
        &self.inner.help
    }

    /// Sets the system's locale to `language` and `formats` as the dialog does, for tests.
    pub fn set_system_locale(&self, language: &str, formats: Option<&str>) {
        if let Some(variables) = SystemLocale::wanted(language, formats) {
            self.set_system(variables);
        }
    }
}

/// The choices of a row: `default` for the empty value, then the installed locales.
fn choices(default: String, installed: &[(String, String)]) -> Vec<(String, String)> {
    std::iter::once((String::new(), default))
        .chain(installed.iter().cloned())
        .collect()
}

/// The notice below the list that says how to add a missing language on `family`: the command,
/// where Sway Things knows it, on a line of its own with _Copy_.
fn add_notice(family: Family) -> gtk::Box {
    let command = locales::add_command(family);
    let text = notice::text(if command.is_some() {
        tr(msg!(
            "Only installed languages are listed. To add more, run this in a terminal:"
        ))
    } else {
        tr(msg!(
            "Only installed languages are listed. Your distribution's documentation says how to \
             add more."
        ))
    });
    let content = gtk::Box::new(gtk::Orientation::Vertical, 6);
    content.append(&text);
    let copy = command.map(|command| {
        let line = gtk::Label::new(Some(command));
        line.set_xalign(0.0);
        line.set_wrap(true);
        line.set_wrap_mode(gtk::pango::WrapMode::Char);
        line.set_selectable(true);
        line.add_css_class("monospace");
        line.add_css_class(style::DIM);
        content.append(&line);
        let copy = gtk::Button::with_label(tr(msg!("Copy")));
        copy.update_property(&[gtk::accessible::Property::Label(tr(msg!(
            "Copy the command"
        )))]);
        copy.connect_clicked(move |button| {
            button.clipboard().set_text(command);
            button.set_label(tr(msg!("Copied")));
        });
        copy
    });
    let added = notice::notice(Tone::Info, &content, copy.as_ref());
    added.set_margin_top(12);
    added
}

/// The row of the preview: a title and a line per format, and the labels of the values.
fn preview_row() -> (gtk::Box, Vec<gtk::Label>) {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 6);
    root.set_margin_top(10);
    root.set_margin_bottom(10);
    root.set_margin_start(12);
    root.set_margin_end(12);
    let title = gtk::Label::new(Some(tr(msg!("Preview"))));
    title.set_xalign(0.0);
    title.add_css_class("st-row-title");
    root.append(&title);
    let grid = gtk::Grid::new();
    grid.set_column_spacing(16);
    grid.set_row_spacing(4);
    let mut values = Vec::new();
    for (index, line) in PREVIEW_LINES.iter().enumerate() {
        let row = i32::try_from(index).unwrap_or_default();
        let name = gtk::Label::new(Some(tr(*line)));
        name.set_xalign(0.0);
        name.add_css_class(style::DIM);
        let value = gtk::Label::new(None);
        value.set_xalign(0.0);
        value.set_selectable(true);
        value.set_wrap(true);
        grid.attach(&name, 0, row, 1, 1);
        grid.attach(&value, 1, row, 1, 1);
        values.push(value);
    }
    root.append(&grid);
    (root, values)
}

/// Fills the preview's labels from `keywords` (`locale -k`), or says it isn't known.
fn fill_preview(labels: &[gtk::Label], keywords: Option<&HashMap<String, String>>) {
    let Some(keywords) = keywords else {
        for label in labels {
            label.set_text("—");
        }
        return;
    };
    let now = glib::DateTime::now_local().ok();
    let part = |get: fn(&glib::DateTime) -> i32, default: u8| {
        now.as_ref()
            .and_then(|now| u8::try_from(get(now)).ok())
            .unwrap_or(default)
    };
    let moment = Moment {
        year: now.as_ref().map_or(2026, glib::DateTime::year),
        month: part(glib::DateTime::month, 1),
        day: part(glib::DateTime::day_of_month, 1),
        weekday: part(|now| now.day_of_week() % 7, 0),
        hour: part(glib::DateTime::hour, 12),
        minute: part(glib::DateTime::minute, 0),
        second: part(glib::DateTime::second, 0),
    };
    let preview = Preview::new(keywords, &moment);
    let texts = [
        preview.date,
        preview.time,
        preview.number,
        preview.money,
        preview.paper.translated(),
        preview.units.translated(),
    ];
    for (label, text) in labels.iter().zip(texts) {
        label.set_text(&text);
    }
}

/// A drop-down of `choices` (value and label) with search, at `selected`.
fn locale_dropdown(choices: &[(String, String)], selected: &str, name: &str) -> gtk::DropDown {
    let labels: Vec<&str> = choices.iter().map(|(_, label)| label.as_str()).collect();
    let dropdown = gtk::DropDown::new(
        Some(gtk::StringList::new(&labels)),
        Some(gtk::PropertyExpression::new(
            gtk::StringObject::static_type(),
            gtk::Expression::NONE,
            "string",
        )),
    );
    dropdown.set_enable_search(true);
    dropdown.set_search_match_mode(gtk::StringFilterMatchMode::Substring);
    dropdown.update_property(&[gtk::accessible::Property::Label(name)]);
    let index = choices
        .iter()
        .position(|(value, _)| value == selected)
        .and_then(|index| u32::try_from(index).ok())
        .unwrap_or(0);
    dropdown.set_selected(index);
    dropdown
}

/// Opens the dialog for the system's language and formats, at `system`; `picked` gets the
/// variables to set.
fn open_system_dialog(
    parent: Option<&gtk::Window>,
    installed: &[(String, String)],
    system: &SystemLocale,
    picked: impl FnOnce(Vec<String>) + 'static,
) {
    let (window, content) = super::dialog(parent, tr(msg!("System Language & Formats")), false);
    window.set_default_width(480);
    let note = gtk::Label::new(Some(tr(msg!(
        "For every user, the login screen and the console, in /etc/locale.conf. Your own \
         choice on this page wins over it. Changing it needs an administrator's password."
    ))));
    note.set_wrap(true);
    note.set_xalign(0.0);
    note.set_max_width_chars(50);
    content.append(&note);

    let mut languages = installed.to_vec();
    if !system.language.is_empty() && !languages.iter().any(|(name, _)| *name == system.language) {
        languages.insert(0, (system.language.clone(), system.language.clone()));
    }
    let language_name = tr(msg!("Language"));
    let language = locale_dropdown(&languages, &system.language, language_name);
    let mut formats_choices = vec![(String::new(), tr(msg!("Same as language")).to_owned())];
    formats_choices.extend(installed.iter().cloned());
    let formats_name = tr(msg!("Formats"));
    let formats = locale_dropdown(
        &formats_choices,
        system.formats.as_deref().unwrap_or_default(),
        formats_name,
    );
    let grid = gtk::Grid::new();
    grid.set_column_spacing(12);
    grid.set_row_spacing(8);
    for (row, (name, dropdown)) in [(language_name, &language), (formats_name, &formats)]
        .into_iter()
        .enumerate()
    {
        let label = gtk::Label::new(Some(name));
        label.set_xalign(0.0);
        dropdown.set_hexpand(true);
        let row = i32::try_from(row).unwrap_or_default();
        grid.attach(&label, 0, row, 1, 1);
        grid.attach(dropdown, 1, row, 1, 1);
    }
    content.append(&grid);

    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    buttons.set_homogeneous(true);
    buttons.set_margin_top(8);
    let cancel = super::cancel_button(&window);
    let set = gtk::Button::with_label(tr(msg!("Set")));
    set.add_css_class("suggested-action");
    set.set_sensitive(!languages.is_empty());
    buttons.append(&cancel);
    buttons.append(&set);
    content.append(&buttons);
    window.set_default_widget(Some(&set));

    let picked = RefCell::new(Some(picked));
    let value = |choices: &[(String, String)], dropdown: &gtk::DropDown| {
        usize::try_from(dropdown.selected())
            .ok()
            .and_then(|index| choices.get(index))
            .map(|(value, _)| value.clone())
            .unwrap_or_default()
    };
    set.connect_clicked(glib::clone!(
        #[weak]
        window,
        #[weak]
        language,
        #[weak]
        formats,
        move |_| {
            let language = value(&languages, &language);
            let formats = value(&formats_choices, &formats);
            let formats = (!formats.is_empty()).then_some(formats.as_str());
            if let Some(variables) = SystemLocale::wanted(&language, formats) {
                if let Some(done) = picked.borrow_mut().take() {
                    done(variables);
                }
            }
            window.close();
        }
    ));
    window.present();
    language.grab_focus();
}
