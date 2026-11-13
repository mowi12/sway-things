// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! "Show generated config": a window of its own with a tab per generated file, showing what Sway
//! Things writes to it for the current settings, with its path, and a copy button in the corner of
//! the text, as on a code snippet. Nothing is
//! written to show it, so it works while Sway Things is read-only too.
//!
//! A file that was changed outside Sway Things gets a banner: its changes stay until the user
//! overwrites it, after a confirmation. _Show file on disk_ shows what is there instead. A file
//! Sway Things didn't make is never written, so it only offers that view.
//!
//! The window follows every change of the settings, and reads the files again whenever it gets
//! the focus, after the user may have edited one elsewhere.

use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use sway_things_core::generate::{GeneratedFile, OnDisk, Ownership};
use sway_things_core::registry::ConfigFile;

use super::setting_row::tilde;
use crate::model::Model;
use crate::style;

/// The tooltip of the button that copies a file's contents.
const COPY: &str = "Copy contents";

/// The icon of that button, and the one it shows for a moment once it copied.
const COPY_ICON: &str = "edit-copy-symbolic";
const COPIED_ICON: &str = "object-select-symbolic";

/// The widgets of one file's tab.
#[derive(Debug)]
pub(crate) struct FileView {
    file: ConfigFile,
    path: gtk::Label,
    copy: gtk::Button,
    note: gtk::Label,
    banner: gtk::Revealer,
    banner_text: gtk::Label,
    on_disk: gtk::ToggleButton,
    overwrite: gtk::Button,
    text: gtk::TextView,
}

mod imp {
    use std::cell::{OnceCell, RefCell};

    use gtk::glib;
    use gtk::subclass::prelude::*;

    use crate::model::Model;

    #[derive(Debug, Default)]
    pub struct GeneratedWindow {
        pub(super) model: OnceCell<Model>,
        pub(super) stack: gtk::Stack,
        pub(super) files: RefCell<Vec<super::FileView>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for GeneratedWindow {
        const NAME: &'static str = "StGeneratedWindow";
        type Type = super::GeneratedWindow;
        type ParentType = gtk::Window;
    }

    impl ObjectImpl for GeneratedWindow {}
    impl WidgetImpl for GeneratedWindow {}
    impl WindowImpl for GeneratedWindow {}
}

glib::wrapper! {
    /// The window of "Show generated config".
    pub struct GeneratedWindow(ObjectSubclass<imp::GeneratedWindow>)
        @extends gtk::Window, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Native,
            gtk::Root, gtk::ShortcutManager;
}

impl GeneratedWindow {
    /// The window for the settings of `model`, on the first file.
    pub fn new(model: &Model) -> Self {
        let window: Self = glib::Object::new();
        window.build(model);
        window
    }

    fn build(&self, model: &Model) {
        let imp = self.imp();
        let _ = imp.model.set(model.clone());
        self.set_title(Some("Generated config"));
        self.set_default_size(760, 560);

        let switcher = gtk::StackSwitcher::new();
        switcher.set_stack(Some(&imp.stack));
        switcher.set_halign(gtk::Align::Center);
        switcher.add_css_class("st-generated-tabs");
        imp.stack.set_vexpand(true);
        imp.stack
            .set_transition_type(gtk::StackTransitionType::None);

        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.add_css_class("view");
        content.append(&switcher);
        content.append(&imp.stack);
        self.set_child(Some(&content));

        let files = model.generated().unwrap_or_default();
        let views: Vec<FileView> = files
            .iter()
            .map(|(generated, _)| self.add_file(generated.file()))
            .collect();
        imp.files.replace(views);
        self.refresh();

        model.connect_changed(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| window.refresh()
        ));
        // The user may have edited a file elsewhere meanwhile.
        self.connect_is_active_notify(|window| {
            if window.is_active() {
                window.refresh();
            }
        });
        let escape = gtk::Shortcut::new(
            gtk::ShortcutTrigger::parse_string("Escape"),
            Some(gtk::CallbackAction::new(|widget, _| {
                if let Some(window) = widget.downcast_ref::<gtk::Window>() {
                    window.close();
                }
                glib::Propagation::Stop
            })),
        );
        let shortcuts = gtk::ShortcutController::new();
        shortcuts.add_shortcut(escape);
        self.add_controller(shortcuts);
    }

    /// Adds the tab of `file` to the stack.
    fn add_file(&self, file: ConfigFile) -> FileView {
        let view = FileView {
            file,
            path: gtk::Label::new(None),
            copy: gtk::Button::from_icon_name(COPY_ICON),
            note: gtk::Label::new(None),
            banner: gtk::Revealer::new(),
            banner_text: gtk::Label::new(None),
            on_disk: gtk::ToggleButton::with_label("Show file on disk"),
            overwrite: gtk::Button::with_label("Overwrite\u{2026}"),
            text: gtk::TextView::new(),
        };
        view.path.set_xalign(0.0);
        view.path.set_hexpand(true);
        view.path.set_wrap(true);
        view.path.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        view.path.set_selectable(true);
        view.path.set_focusable(false);
        view.path.add_css_class("monospace");
        view.copy.set_halign(gtk::Align::End);
        view.copy.set_valign(gtk::Align::Start);
        view.copy.set_tooltip_text(Some(COPY));
        view.copy.add_css_class("st-generated-copy");
        view.copy
            .update_property(&[gtk::accessible::Property::Label(&format!(
                "Copy the contents of {}",
                file.file_name()
            ))]);

        view.note.set_xalign(0.0);
        view.note.set_wrap(true);
        view.note.add_css_class(style::DIM);

        view.banner_text.set_xalign(0.0);
        view.banner_text.set_hexpand(true);
        view.banner_text.set_wrap(true);
        view.banner_text
            .set_wrap_mode(gtk::pango::WrapMode::WordChar);
        let icon = gtk::Image::from_icon_name("dialog-warning-symbolic");
        icon.set_valign(gtk::Align::Center);
        icon.set_accessible_role(gtk::AccessibleRole::Presentation);
        view.on_disk.set_valign(gtk::Align::Center);
        view.overwrite.set_valign(gtk::Align::Center);
        let banner = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        banner.add_css_class("st-banner");
        banner.add_css_class("st-generated-banner");
        banner.append(&icon);
        banner.append(&view.banner_text);
        banner.append(&view.on_disk);
        banner.append(&view.overwrite);
        view.banner.set_child(Some(&banner));

        view.text.set_editable(false);
        view.text.set_monospace(true);
        // Room for the copy button at the end of the first lines.
        view.text.set_right_margin(48);
        view.text.set_wrap_mode(gtk::WrapMode::None);
        view.text.add_css_class("st-generated-text");
        view.text
            .update_property(&[gtk::accessible::Property::Label(file.file_name())]);
        let scroller = gtk::ScrolledWindow::new();
        scroller.set_child(Some(&view.text));
        scroller.set_vexpand(true);
        // The copy button floats in the corner of the contents, as on a code snippet.
        let code = gtk::Overlay::new();
        code.add_css_class("st-generated-code");
        code.set_overflow(gtk::Overflow::Hidden);
        code.set_child(Some(&scroller));
        code.add_overlay(&view.copy);

        let page = gtk::Box::new(gtk::Orientation::Vertical, 8);
        page.add_css_class("st-generated-page");
        page.append(&view.path);
        page.append(&view.note);
        page.append(&view.banner);
        page.append(&code);
        self.imp()
            .stack
            .add_titled(&page, Some(file.file_name()), file.file_name());

        view.copy.connect_clicked(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |button| window.copy(file, button)
        ));
        view.on_disk.connect_toggled(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| window.refresh()
        ));
        view.overwrite.connect_clicked(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| window.confirm_overwrite(file)
        ));
        view
    }

    /// Shows the tab of `file`.
    pub fn show_file(&self, file: ConfigFile) {
        self.imp().stack.set_visible_child_name(file.file_name());
    }

    /// The file whose tab is shown.
    pub fn shown_file(&self) -> Option<ConfigFile> {
        let name = self.imp().stack.visible_child_name()?;
        ConfigFile::ALL
            .into_iter()
            .find(|file| file.file_name() == name.as_str())
    }

    /// The text shown for `file`, its banner if it is shown, and whether it can be overwritten.
    pub fn file_state(&self, file: ConfigFile) -> Option<(String, Option<String>, bool)> {
        let files = self.imp().files.borrow();
        let view = files.iter().find(|view| view.file == file)?;
        let buffer = view.text.buffer();
        let text = buffer
            .text(&buffer.start_iter(), &buffer.end_iter(), false)
            .to_string();
        let banner = view
            .banner
            .reveals_child()
            .then(|| view.banner_text.text().to_string());
        Some((
            text,
            banner,
            view.overwrite.is_sensitive() && view.overwrite.is_visible(),
        ))
    }

    /// The _Show file on disk_ button of `file`.
    pub fn on_disk_button(&self, file: ConfigFile) -> Option<gtk::ToggleButton> {
        let files = self.imp().files.borrow();
        files
            .iter()
            .find(|view| view.file == file)
            .map(|view| view.on_disk.clone())
    }

    /// Reads the generated files and the files on disk again and shows them.
    pub fn refresh(&self) {
        let imp = self.imp();
        let Some(model) = imp.model.get() else { return };
        let Some(files) = model.generated() else {
            return;
        };
        let writable = model.status().is_some_and(|status| status.is_active());
        for view in imp.files.borrow().iter() {
            if let Some((generated, on_disk)) = files
                .iter()
                .find(|(generated, _)| generated.file() == view.file)
            {
                view.show(generated, on_disk, writable);
            }
        }
    }

    /// Copies the text shown for `file`, and says so on `button` for a moment.
    fn copy(&self, file: ConfigFile, button: &gtk::Button) {
        let Some((text, _, _)) = self.file_state(file) else {
            return;
        };
        self.clipboard().set_text(&text);
        button.set_icon_name(COPIED_ICON);
        button.set_tooltip_text(Some("Copied"));
        glib::timeout_add_seconds_local_once(
            2,
            glib::clone!(
                #[weak]
                button,
                move || {
                    button.set_icon_name(COPY_ICON);
                    button.set_tooltip_text(Some(COPY));
                }
            ),
        );
    }

    /// Asks whether to overwrite `file`, and has the model write it on yes.
    fn confirm_overwrite(&self, file: ConfigFile) {
        let Some(model) = self.imp().model.get() else {
            return;
        };
        let dialog = gtk::AlertDialog::builder()
            .message(format!("Overwrite {}?", file.file_name()))
            .detail(
                "The changes made to it outside Sway Things are lost: it gets what Sway Things \
                 generates from your settings.",
            )
            .buttons(["Cancel", "Overwrite"])
            .cancel_button(0)
            .default_button(0)
            .modal(true)
            .build();
        dialog.choose(
            Some(self),
            gtk::gio::Cancellable::NONE,
            glib::clone!(
                #[weak]
                model,
                move |answer| {
                    if answer == Ok(1) {
                        model.overwrite(file);
                    }
                }
            ),
        );
    }
}

impl FileView {
    /// Shows `generated`, or with _Show file on disk_ the file as it is (`on_disk`), with the
    /// banner if it was changed outside. `writable`: whether Sway Things may write files now.
    fn show(&self, generated: &GeneratedFile, on_disk: &OnDisk, writable: bool) {
        self.path.set_text(&tilde(&on_disk.path));
        let (banner, can_overwrite) = match &on_disk.ownership {
            Ok(Ownership::Edited) => (
                Some(
                    "Changed outside Sway Things: it isn\u{2019}t updated until you overwrite it."
                        .to_owned(),
                ),
                true,
            ),
            Ok(Ownership::Foreign) => (
                Some(
                    "Not made by Sway Things, so it is never written. Move it away to let Sway \
                     Things create it."
                        .to_owned(),
                ),
                false,
            ),
            Ok(Ownership::Absent | Ownership::Current | Ownership::Ours) => (None, false),
            Err(error) => (Some(format!("It can\u{2019}t be read: {error}")), false),
        };
        let note = match &on_disk.ownership {
            Ok(Ownership::Absent) if generated.is_empty() => {
                "Not created: no setting of yours goes to this file yet."
            }
            Ok(Ownership::Absent) => "Not written yet.",
            _ => "",
        };
        self.note.set_text(note);
        self.note.set_visible(!note.is_empty());
        let on_disk_shown = banner.is_some() && on_disk.content.is_some();
        self.on_disk.set_visible(on_disk_shown);
        if !on_disk_shown {
            self.on_disk.set_active(false);
        }
        self.overwrite.set_visible(can_overwrite);
        self.overwrite.set_sensitive(writable);
        self.overwrite.set_tooltip_text(
            (!writable).then_some("Add the include line to your sway config first"),
        );
        self.banner_text
            .set_text(banner.as_deref().unwrap_or_default());
        self.banner.set_reveal_child(banner.is_some());
        let text = match &on_disk.content {
            Some(content) if self.on_disk.is_active() => content.clone(),
            _ => generated.text(),
        };
        let buffer = self.text.buffer();
        let shown = buffer.text(&buffer.start_iter(), &buffer.end_iter(), false);
        // Unchanged text keeps the scroll position and the selection.
        if shown.as_str() != text {
            buffer.set_text(&text);
        }
    }
}
