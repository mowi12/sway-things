// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The main window: the sidebar and the content pane.

use std::time::Duration;

use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use sway_things_core::apply::{Pending, Until};
use sway_things_core::registry::{ConfigFile, Link, Page, Registry};

use crate::i18n::tr;
use crate::model::Model;
use crate::widgets::{GeneratedWindow, PageView, Panes, Placeholder, Sidebar, Welcome};

/// The width of the sidebar, in pixels.
const SIDEBAR_WIDTH: i32 = 240;

/// The width below which the window shows one pane at a time, in pixels: the sidebar and the
/// widest of the pages side by side, with the default font.
pub const NARROW_WIDTH: i32 = 960;

/// The name of the welcome screen in the content pane.
const WELCOME: &str = "welcome";

mod imp {
    use std::cell::{Cell, OnceCell, RefCell};

    use gtk::glib;
    use gtk::subclass::prelude::*;
    use sway_things_core::registry::Page;

    use crate::model::Model;
    use crate::widgets::{Panes, Sidebar, Welcome};

    #[derive(Debug, Default)]
    pub struct Window {
        pub sidebar: Sidebar,
        pub panes: OnceCell<Panes>,
        /// The bar above the page with the button back to the sidebar, while narrow.
        pub back_bar: gtk::Box,
        pub back: gtk::Button,
        pub pages: gtk::Stack,
        pub model: OnceCell<Model>,
        pub banner: gtk::Revealer,
        pub banner_text: gtk::Label,
        /// The welcome screen while the include line is missing.
        pub welcome: Welcome,
        /// Whether the user chose to look around instead of the welcome screen.
        pub welcome_dismissed: Cell<bool>,
        /// The banner with the include status: read-only, or a line that isn't last.
        pub status: gtk::Revealer,
        pub status_text: gtk::Label,
        pub status_button: gtk::Button,
        /// Whether the user dismissed the note that the line isn't last.
        pub note_dismissed: Cell<bool>,
        /// The page open last, to go back to from the welcome screen.
        pub last_page: Cell<Option<Page>>,
        /// The banner of the safety net: keep or revert the risky change that waits.
        pub safety: gtk::Revealer,
        pub safety_text: gtk::Label,
        pub keep: gtk::Button,
        pub revert: gtk::Button,
        /// Updates the seconds left while the banner is shown.
        pub safety_tick: RefCell<Option<glib::SourceId>>,
        /// Set when the window was asked to close while a change waited: it closes once the
        /// change is reverted.
        pub closing: Cell<bool>,
        /// The banner for changes that wait for a reload of sway.
        pub reload: gtk::Revealer,
        pub reload_text: gtk::Label,
        pub reload_button: gtk::Button,
        /// The window of "Show generated config", while it is open.
        pub generated: RefCell<Option<crate::widgets::GeneratedWindow>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Window {
        const NAME: &'static str = "StWindow";
        type Type = super::Window;
        type ParentType = gtk::ApplicationWindow;
    }

    impl ObjectImpl for Window {}

    impl WidgetImpl for Window {}
    impl WindowImpl for Window {}
    impl ApplicationWindowImpl for Window {}
}

glib::wrapper! {
    /// The main window of Sway Things.
    pub struct Window(ObjectSubclass<imp::Window>)
        @extends gtk::ApplicationWindow, gtk::Window, gtk::Widget,
        @implements gtk::gio::ActionGroup, gtk::gio::ActionMap, gtk::Accessible, gtk::Buildable,
            gtk::ConstraintTarget, gtk::Native, gtk::Root, gtk::ShortcutManager;
}

impl Window {
    /// The main window of `app` for the settings of `model`, showing the first page.
    pub fn new(app: &gtk::Application, model: &Model) -> Self {
        let window: Self = glib::Object::builder().property("application", app).build();
        window.build(model);
        window
    }

    /// A main window for the settings of `model` that belongs to no application, e.g. in tests.
    pub fn with_model(model: &Model) -> Self {
        let window: Self = glib::Object::new();
        window.build(model);
        window
    }

    fn build(&self, model: &Model) {
        self.set_title(Some("Sway Things"));
        self.set_default_size(980, 700);

        let imp = self.imp();
        imp.sidebar.set_size_request(SIDEBAR_WIDTH, -1);
        imp.sidebar.connect_page_activated(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_, page| window.show_page(page)
        ));
        imp.sidebar.connect_link_activated(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_, link| window.open_link(&link)
        ));
        // Typing anywhere goes to the search field.
        imp.sidebar
            .search_entry()
            .set_key_capture_widget(Some(self));
        // While narrow, typing on the page shows the sidebar with the results.
        imp.sidebar.search_entry().connect_changed(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |entry| window.follow_typing(entry)
        ));

        let _ = imp.model.set(model.clone());
        imp.pages.set_hexpand(true);
        imp.pages.set_vexpand(true);

        // Problems that aren't one setting's: a generated file that couldn't be written, …
        imp.banner_text.set_xalign(0.0);
        imp.banner_text.set_hexpand(true);
        imp.banner_text.set_wrap(true);
        imp.banner_text
            .set_wrap_mode(gtk::pango::WrapMode::WordChar);
        let dismiss = gtk::Button::with_label("Dismiss");
        dismiss.connect_clicked(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| window.imp().banner.set_reveal_child(false)
        ));
        let banner = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        banner.add_css_class("st-banner");
        banner.append(&imp.banner_text);
        banner.append(&dismiss);
        imp.banner.set_child(Some(&banner));
        model.connect_problem(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_, message| window.show_problem(message)
        ));

        // The include status: why everything is read-only, or that some of the user's lines win.
        imp.status_text.set_xalign(0.0);
        imp.status_text.set_hexpand(true);
        imp.status_text.set_wrap(true);
        imp.status_text
            .set_wrap_mode(gtk::pango::WrapMode::WordChar);
        imp.status_button.set_valign(gtk::Align::Center);
        imp.status_button.connect_clicked(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| window.status_action()
        ));
        let status = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        status.add_css_class("st-banner");
        status.append(&imp.status_text);
        status.append(&imp.status_button);
        imp.status.set_child(Some(&status));

        self.build_safety_net(model);
        self.build_reload(model);
        crate::theme::follow_accent(self, model);

        imp.welcome.connect_look_around(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| window.look_around()
        ));
        imp.pages.add_named(&imp.welcome, Some(WELCOME));
        model.connect_changed(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| window.follow_status()
        ));

        self.build_panes();

        self.add_shortcuts();
        // The window of the generated files goes with the main window, which is unrealized when
        // it is destroyed.
        self.connect_unrealize(|window| {
            if let Some(generated) = window.imp().generated.take() {
                generated.destroy();
            }
        });

        if let Some(first) = Page::ALL.first() {
            self.open_page(*first);
        }
        self.follow_status();
    }

    /// The sidebar and the content pane: the page, with the banners and, while narrow, the back
    /// button above it.
    fn build_panes(&self) {
        let imp = self.imp();
        self.build_back();
        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.add_css_class("view");
        content.set_hexpand(true);
        content.append(&imp.back_bar);
        content.append(&imp.safety);
        content.append(&imp.status);
        content.append(&imp.reload);
        content.append(&imp.banner);
        content.append(&imp.pages);

        let panes = Panes::new(&imp.sidebar, &content, NARROW_WIDTH);
        panes.connect_narrow_changed(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_, narrow| window.imp().back_bar.set_visible(narrow)
        ));
        self.set_child(Some(&panes));
        let _ = imp.panes.set(panes);
        // While narrow, the pane the keyboard focus moves to is shown: the search field on
        // Ctrl+F, the sidebar on Esc, the page on the welcome screen or a deep link.
        self.connect_focus_widget_notify(Self::follow_focus);
    }

    /// The shortcuts of the window: Esc and Ctrl+F.
    fn add_shortcuts(&self) {
        // Esc in the content pane goes back to the sidebar, in the sidebar it ends the search.
        let escape = gtk::Shortcut::new(
            gtk::ShortcutTrigger::parse_string("Escape"),
            Some(gtk::CallbackAction::new(|widget, _| {
                widget
                    .downcast_ref::<Self>()
                    .is_some_and(|window| {
                        window.back_to_sidebar() || window.sidebar().stop_search()
                    })
                    .into()
            })),
        );
        let search = gtk::Shortcut::new(
            gtk::ShortcutTrigger::parse_string("<Control>f"),
            Some(gtk::CallbackAction::new(|widget, _| {
                widget
                    .downcast_ref::<Self>()
                    .is_some_and(|window| {
                        window.show_sidebar();
                        window.sidebar().focus_search()
                    })
                    .into()
            })),
        );
        let shortcuts = gtk::ShortcutController::new();
        shortcuts.add_shortcut(escape);
        shortcuts.add_shortcut(search);
        self.add_controller(shortcuts);
    }

    /// Opens `page` and marks it in the sidebar. Opening a page from the welcome screen counts as
    /// looking around. While narrow, the page replaces the sidebar and gets the keyboard focus.
    pub fn show_page(&self, page: Page) {
        if self.is_welcome_shown() {
            self.imp().welcome_dismissed.set(true);
        }
        self.open_page(page);
        self.follow_status();
        self.show_content();
    }

    /// The bar above the page with the button back to the sidebar, shown while narrow.
    fn build_back(&self) {
        let imp = self.imp();
        imp.back.set_icon_name("go-previous-symbolic");
        imp.back.add_css_class("flat");
        imp.back.set_tooltip_text(Some("Back to the pages"));
        imp.back
            .update_property(&[gtk::accessible::Property::Label("Back to the pages")]);
        imp.back.connect_clicked(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| {
                window.back_to_sidebar();
            }
        ));
        imp.back_bar.add_css_class("st-back-bar");
        imp.back_bar.append(&imp.back);
        imp.back_bar.set_visible(false);
    }

    /// While narrow, shows the content pane instead of the sidebar, and moves the keyboard focus
    /// into it: to the first control of the page, or the back button.
    fn show_content(&self) {
        let imp = self.imp();
        let Some(panes) = imp.panes.get() else { return };
        if panes.is_content_shown() {
            return;
        }
        panes.show_content();
        let focus = GtkWindowExt::focus(self);
        if focus.is_some_and(|focus| focus.is_ancestor(&imp.pages)) {
            return;
        }
        let focused = imp
            .pages
            .visible_child()
            .is_some_and(|page| page.child_focus(gtk::DirectionType::TabForward));
        if !focused {
            imp.back.grab_focus();
        }
    }

    /// While narrow, shows the sidebar instead of the content pane.
    fn show_sidebar(&self) {
        if let Some(panes) = self.imp().panes.get() {
            panes.show_sidebar();
        }
    }

    /// While narrow, shows the sidebar when text is typed into the search field from the page, and
    /// gives the field the focus, which is still on the page.
    fn follow_typing(&self, entry: &gtk::SearchEntry) {
        let Some(panes) = self.imp().panes.get() else {
            return;
        };
        if !panes.is_narrow() || !panes.is_content_shown() || entry.text().is_empty() {
            return;
        }
        panes.show_sidebar();
        if !GtkWindowExt::focus(self).is_some_and(|focus| focus.is_ancestor(entry)) {
            entry.grab_focus();
            entry.set_position(-1);
        }
    }

    /// While narrow, shows the pane the keyboard focus moved to.
    fn follow_focus(&self) {
        let imp = self.imp();
        let (Some(panes), Some(focus)) = (imp.panes.get(), GtkWindowExt::focus(self)) else {
            return;
        };
        if !panes.is_narrow() {
            return;
        }
        if focus.is_ancestor(&imp.sidebar) {
            panes.show_sidebar();
        } else if focus.is_ancestor(panes) {
            panes.show_content();
        }
    }

    /// Opens what `link` points to: its page, and with a setting, the setting's row, scrolled into
    /// view, focused and tinted for a moment. A setting without a row shown, like one of an input
    /// type without devices, only opens the page.
    pub fn open_link(&self, link: &Link) {
        let page = link.page();
        self.show_page(page);
        let Some(setting) = link.setting() else {
            return;
        };
        let view = self
            .imp()
            .pages
            .child_by_name(page.id())
            .and_downcast::<PageView>();
        if let Some((view, row)) =
            view.and_then(|view| Some((view.clone(), view.row(setting.key())?)))
        {
            view.point_to(&row);
        }
    }

    /// Opens `page` and marks it in the sidebar.
    fn open_page(&self, page: Page) {
        let imp = self.imp();
        if imp.pages.child_by_name(page.id()).is_none() {
            imp.pages.add_named(&self.build_page(page), Some(page.id()));
        }
        imp.pages.set_visible_child_name(page.id());
        imp.sidebar.select(page);
        imp.last_page.set(Some(page));
    }

    /// Whether the welcome screen fills the content pane.
    pub fn is_welcome_shown(&self) -> bool {
        self.imp().pages.visible_child_name().as_deref() == Some(WELCOME)
    }

    /// The welcome screen.
    pub fn welcome(&self) -> &Welcome {
        &self.imp().welcome
    }

    /// The text of the banner with the include status, if it is shown.
    pub fn status_banner(&self) -> Option<String> {
        let imp = self.imp();
        imp.status
            .reveals_child()
            .then(|| imp.status_text.text().to_string())
    }

    /// The button of the banner with the include status.
    pub fn status_button(&self) -> &gtk::Button {
        &self.imp().status_button
    }

    /// Leaves the welcome screen for the page that was open, with the banner above it.
    pub fn look_around(&self) {
        let imp = self.imp();
        imp.welcome_dismissed.set(true);
        let page = imp.last_page.get().or_else(|| Page::ALL.first().copied());
        if let Some(page) = page {
            self.show_page(page);
        }
    }

    /// What the button of the status banner does: shows the instructions again while read-only,
    /// and dismisses the note that the line isn't last otherwise.
    fn status_action(&self) {
        let imp = self.imp();
        let read_only = self.read_only_status().is_some();
        if read_only {
            imp.welcome_dismissed.set(false);
        } else {
            imp.note_dismissed.set(true);
        }
        self.follow_status();
    }

    /// The include status, if Sway Things is read-only.
    fn read_only_status(&self) -> Option<sway_things_core::include::IncludeStatus> {
        self.imp()
            .model
            .get()
            .and_then(Model::status)
            .filter(|status| !status.is_active())
    }

    /// Shows the welcome screen or the banner as the include status asks: the welcome screen
    /// while read-only (unless the user chose to look around), a banner with a way back to it,
    /// and once the line is there, a note if some of the user's lines come after it.
    fn follow_status(&self) {
        let imp = self.imp();
        let Some(status) = imp.model.get().and_then(Model::status) else {
            return;
        };
        if status.is_active() {
            if self.is_welcome_shown() {
                // The line is there: on to the page that was open.
                imp.welcome_dismissed.set(true);
                if let Some(page) = imp.last_page.get().or_else(|| Page::ALL.first().copied()) {
                    imp.pages.set_visible_child_name(page.id());
                    imp.sidebar.select(page);
                }
            }
            let note = !status.is_last() && !imp.note_dismissed.get();
            if note {
                imp.status_text.set_text(&status.explanation());
                imp.status_button.set_label("Dismiss");
            }
            imp.status.set_reveal_child(note);
            return;
        }
        imp.welcome.update(&status);
        if imp.welcome_dismissed.get() {
            imp.status_text
                .set_text("Not enabled yet: add one line to your sway config to change settings.");
            imp.status_button.set_label("Show instructions");
            imp.status.set_reveal_child(true);
        } else {
            imp.status.set_reveal_child(false);
            if !self.is_welcome_shown() {
                imp.pages.set_visible_child_name(WELCOME);
                // The keyboard starts on the welcome screen, which also keeps the sidebar from
                // marking the row it would focus.
                GtkWindowExt::set_focus(self, Some(&imp.welcome.first_button()));
                imp.sidebar.unselect();
            }
        }
    }

    /// The banner of the safety net, and the close of the window while a change waits.
    fn build_safety_net(&self, model: &Model) {
        let imp = self.imp();
        imp.safety_text.set_xalign(0.0);
        imp.safety_text.set_hexpand(true);
        imp.safety_text.set_wrap(true);
        imp.safety_text
            .set_wrap_mode(gtk::pango::WrapMode::WordChar);
        imp.revert.set_label("Revert");
        imp.keep.set_label("Keep");
        for button in [&imp.revert, &imp.keep] {
            button.set_valign(gtk::Align::Center);
        }
        imp.revert.connect_clicked(glib::clone!(
            #[weak]
            model,
            move |_| model.revert()
        ));
        imp.keep.connect_clicked(glib::clone!(
            #[weak]
            model,
            move |_| model.keep()
        ));
        let safety = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        safety.add_css_class("st-banner");
        safety.append(&imp.safety_text);
        safety.append(&imp.revert);
        safety.append(&imp.keep);
        imp.safety.set_child(Some(&safety));
        model.connect_changed(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| window.follow_safety_net()
        ));
        // A change that waits is reverted before the window closes.
        self.connect_close_request(|window| {
            let Some(model) = window.model() else {
                return glib::Propagation::Proceed;
            };
            if model.pending_change().is_none() {
                return glib::Propagation::Proceed;
            }
            window.imp().closing.set(true);
            model.revert();
            glib::Propagation::Stop
        });
    }

    /// Shows the banner of the safety net while a risky change waits, and closes the window if
    /// that was asked for once the change is reverted.
    fn follow_safety_net(&self) {
        let imp = self.imp();
        let Some(model) = imp.model.get() else { return };
        let Some(pending) = model.pending_change() else {
            imp.safety.set_reveal_child(false);
            if let Some(tick) = imp.safety_tick.take() {
                tick.remove();
            }
            if imp.closing.replace(false) {
                // After the model has finished the revert that sent this.
                glib::idle_add_local_once(glib::clone!(
                    #[weak(rename_to = window)]
                    self,
                    move || window.close()
                ));
            }
            return;
        };
        let text = safety_text(model, &pending);
        imp.safety_text.set_text(&text);
        if imp.safety.reveals_child() {
            return;
        }
        imp.safety.set_reveal_child(true);
        // Revert has the focus, so a stray key press undoes the change instead of keeping it.
        GtkWindowExt::set_focus(self, Some(&imp.revert));
        imp.safety
            .announce(&text, gtk::AccessibleAnnouncementPriority::High);
        let tick = glib::timeout_add_local(
            Duration::from_millis(250),
            glib::clone!(
                #[weak(rename_to = window)]
                self,
                #[upgrade_or]
                glib::ControlFlow::Break,
                move || {
                    let imp = window.imp();
                    let model = imp.model.get();
                    if let Some((model, pending)) =
                        model.and_then(|model| Some((model, model.pending_change()?)))
                    {
                        imp.safety_text.set_text(&safety_text(model, &pending));
                        glib::ControlFlow::Continue
                    } else {
                        imp.safety_tick.take();
                        glib::ControlFlow::Break
                    }
                }
            ),
        );
        if let Some(old) = imp.safety_tick.replace(Some(tick)) {
            old.remove();
        }
    }

    /// The banner for changes that wait for a reload of sway, with its button that asks first.
    fn build_reload(&self, model: &Model) {
        let imp = self.imp();
        imp.reload_text.set_xalign(0.0);
        imp.reload_text.set_hexpand(true);
        imp.reload_text.set_wrap(true);
        imp.reload_text
            .set_wrap_mode(gtk::pango::WrapMode::WordChar);
        imp.reload_button.set_label("Reload sway\u{2026}");
        imp.reload_button.set_valign(gtk::Align::Center);
        imp.reload_button.connect_clicked(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| window.confirm_reload()
        ));
        let reload = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        reload.add_css_class("st-banner");
        let icon = gtk::Image::from_icon_name("view-refresh-symbolic");
        icon.set_valign(gtk::Align::Center);
        icon.set_accessible_role(gtk::AccessibleRole::Presentation);
        reload.append(&icon);
        reload.append(&imp.reload_text);
        reload.append(&imp.reload_button);
        imp.reload.set_child(Some(&reload));
        model.connect_changed(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| window.follow_reload()
        ));
    }

    /// Shows the reload banner while changes wait for a reload of sway. Its button waits while a
    /// risky change waits, since a reload would put the stored value back.
    fn follow_reload(&self) {
        let imp = self.imp();
        let Some(model) = imp.model.get() else { return };
        let count = model
            .waiting()
            .map_or(0, |waiting| waiting.until(Until::Reload).count());
        let active = model.status().is_some_and(|status| status.is_active());
        if count > 0 && active {
            imp.reload_text.set_text(&reload_text(count));
        }
        imp.reload.set_reveal_child(count > 0 && active);
        imp.reload_button
            .set_sensitive(model.pending_change().is_none());
    }

    /// Asks whether to reload sway, and reloads it on yes.
    fn confirm_reload(&self) {
        let Some(model) = self.imp().model.get() else {
            return;
        };
        let relogin: Vec<String> = model
            .waiting()
            .map(|waiting| {
                waiting
                    .until(Until::Relogin)
                    .filter_map(|change| Registry::builtin().get(change.key))
                    .map(|setting| tr(setting.title()).to_owned())
                    .collect()
            })
            .unwrap_or_default();
        let mut detail = "Sway reads its config again. Open windows stay; the bars start again \
                          and exec_always lines run again."
            .to_owned();
        if !relogin.is_empty() {
            let lines = if relogin.len() == 1 {
                "its line"
            } else {
                "their lines"
            };
            detail = format!(
                "{detail}\n\n{} changed, which sway only reads at login: until you log in \
                 again, sway reports {lines} as an error after the reload.",
                relogin.join(", ")
            );
        }
        let dialog = gtk::AlertDialog::builder()
            .message("Reload sway?")
            .detail(detail)
            .buttons(["Cancel", "Reload"])
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
                        model.reload_sway();
                    }
                }
            ),
        );
    }

    /// Opens the window of "Show generated config", or brings it to the front, at the tab of
    /// `file` if given.
    pub fn show_generated(&self, file: Option<ConfigFile>) {
        let imp = self.imp();
        let Some(model) = imp.model.get() else { return };
        let existing = imp.generated.borrow().clone();
        let generated = existing.unwrap_or_else(|| {
            let generated = GeneratedWindow::new(model);
            if let Some(app) = self.application() {
                generated.set_application(Some(&app));
            }
            generated.connect_close_request(glib::clone!(
                #[weak(rename_to = window)]
                self,
                #[upgrade_or]
                glib::Propagation::Proceed,
                move |_| {
                    window.imp().generated.take();
                    glib::Propagation::Proceed
                }
            ));
            imp.generated.replace(Some(generated.clone()));
            generated
        });
        if let Some(file) = file {
            generated.show_file(file);
        }
        generated.present();
    }

    /// The window of "Show generated config", if it is open.
    pub fn generated(&self) -> Option<GeneratedWindow> {
        self.imp().generated.borrow().clone()
    }

    /// The text of the reload banner, if it is shown.
    pub fn reload_banner(&self) -> Option<String> {
        let imp = self.imp();
        imp.reload
            .reveals_child()
            .then(|| imp.reload_text.text().to_string())
    }

    /// The button of the reload banner.
    pub fn reload_button(&self) -> &gtk::Button {
        &self.imp().reload_button
    }

    /// The text of the safety-net banner, if it is shown.
    pub fn safety_banner(&self) -> Option<String> {
        let imp = self.imp();
        imp.safety
            .reveals_child()
            .then(|| imp.safety_text.text().to_string())
    }

    /// The Keep button of the safety-net banner.
    pub fn keep_button(&self) -> &gtk::Button {
        &self.imp().keep
    }

    /// The Revert button of the safety-net banner.
    pub fn revert_button(&self) -> &gtk::Button {
        &self.imp().revert
    }

    fn build_page(&self, page: Page) -> PageView {
        let Some(model) = self.imp().model.get() else {
            return PageView::unbuilt(page);
        };
        if let Some(error) = model.error() {
            let placeholder =
                Placeholder::new("dialog-error-symbolic", "Settings can't be loaded", &error);
            return PageView::new(tr(page.title()), tr(page.description()), &placeholder);
        }
        PageView::for_settings(model, page).unwrap_or_else(|| PageView::unbuilt(page))
    }

    /// Shows `message` in the banner above the page.
    pub fn show_problem(&self, message: &str) {
        let imp = self.imp();
        imp.banner_text.set_text(message);
        imp.banner.set_reveal_child(true);
    }

    /// The text of the banner above the page, if it is shown.
    pub fn problem(&self) -> Option<String> {
        let imp = self.imp();
        imp.banner
            .reveals_child()
            .then(|| imp.banner_text.text().to_string())
    }

    /// The settings of the window.
    pub fn model(&self) -> Option<&Model> {
        self.imp().model.get()
    }

    /// The open page.
    pub fn page(&self) -> Option<Page> {
        self.imp()
            .pages
            .visible_child_name()
            .and_then(|id| Page::from_id(&id))
    }

    /// The sidebar.
    pub fn sidebar(&self) -> &Sidebar {
        &self.imp().sidebar
    }

    /// The open page's widget in the content pane.
    pub fn content(&self) -> Option<gtk::Widget> {
        self.imp().pages.visible_child()
    }

    /// The sidebar and the content pane, side by side or one at a time.
    pub fn panes(&self) -> Option<&Panes> {
        self.imp().panes.get()
    }

    /// The button back to the sidebar, shown above the page while narrow.
    pub fn back_button(&self) -> &gtk::Button {
        &self.imp().back
    }

    /// What Esc and the back button do: move the focus from the content pane back to the open
    /// page in the sidebar, and while narrow, show the sidebar instead of the page. Returns
    /// whether it went back: whether the focus was in the content pane, or while narrow, whether
    /// the page was shown.
    pub fn back_to_sidebar(&self) -> bool {
        let imp = self.imp();
        if let Some(panes) = imp.panes.get().filter(|panes| panes.is_narrow()) {
            if !panes.is_content_shown() {
                return false;
            }
            panes.show_sidebar();
            imp.sidebar.focus_selected();
            return true;
        }
        let in_content = gtk::prelude::GtkWindowExt::focus(self)
            .is_some_and(|focus| focus.is_ancestor(&imp.pages));
        in_content && imp.sidebar.focus_selected()
    }
}

/// "1 change applies after sway reloads its config", or "N changes apply …".
fn reload_text(count: usize) -> String {
    if count == 1 {
        "1 change applies after sway reloads its config".to_owned()
    } else {
        format!("{count} changes apply after sway reloads its config")
    }
}

/// "Keep the change to Section › Title? Reverting in N s", with the seconds rounded up. The
/// section says which type of device a title like "Enabled" is about; a change for one device
/// names it too ("Mouse › MX Master 3 › Enabled").
fn safety_text(model: &Model, pending: &Pending) -> String {
    let device = pending.device().map(|identifier| {
        model
            .devices()
            .and_then(|devices| {
                devices
                    .get(identifier)
                    .map(|device| device.name().to_owned())
            })
            .unwrap_or_else(|| identifier.to_owned())
    });
    let name = Registry::builtin().get(pending.key()).map_or_else(
        || pending.key().to_owned(),
        |setting| {
            let (section, title) = (tr(setting.section()), tr(setting.title()));
            let mut parts = vec![section];
            parts.extend(device.as_deref());
            if section != title {
                parts.push(title);
            }
            parts.join(" › ")
        },
    );
    let left = pending.countdown().remaining();
    let seconds = left.as_secs() + u64::from(left.subsec_nanos() > 0);
    format!("Keep the change to {name}? Reverting in {seconds} s")
}
