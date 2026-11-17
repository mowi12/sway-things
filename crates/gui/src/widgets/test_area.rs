// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The test area of the Mouse & Touchpad page: a pad that says which click it got (so tapping,
//! the button maps and left-handed mode can be tried), and a strip of lines to scroll (so scroll
//! speed and natural scrolling can be felt).

use crate::i18n::{msg, tr, trf};
use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;

/// The lines in the strip to scroll.
const LINES: usize = 40;

mod imp {
    use gtk::glib;
    use gtk::prelude::*;
    use gtk::subclass::prelude::*;

    #[derive(Debug, Default)]
    pub struct TestArea {
        pub(super) pad: gtk::Box,
        pub(super) last: gtk::Label,
        pub(super) strip: gtk::ScrolledWindow,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for TestArea {
        const NAME: &'static str = "StTestArea";
        type Type = super::TestArea;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_css_name("st-test-area");
            klass.set_layout_manager_type::<gtk::BoxLayout>();
        }
    }

    impl ObjectImpl for TestArea {
        fn dispose(&self) {
            while let Some(child) = self.obj().first_child() {
                child.unparent();
            }
        }
    }

    impl WidgetImpl for TestArea {}
}

glib::wrapper! {
    /// A pad to click and a strip to scroll, to try the pointer settings.
    pub struct TestArea(ObjectSubclass<imp::TestArea>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Default for TestArea {
    fn default() -> Self {
        Self::new()
    }
}

impl TestArea {
    /// A test area that has seen no click yet.
    pub fn new() -> Self {
        let area: Self = glib::Object::new();
        let imp = area.imp();

        let hint = gtk::Label::new(Some(tr(msg!("Click, double-click or tap here"))));
        hint.set_wrap(true);
        hint.set_justify(gtk::Justification::Center);
        imp.last.set_text(tr(msg!("No click yet")));
        imp.last.set_wrap(true);
        imp.last.add_css_class(crate::style::DIM);
        imp.pad.set_orientation(gtk::Orientation::Vertical);
        imp.pad.set_spacing(8);
        imp.pad.set_valign(gtk::Align::Fill);
        imp.pad.set_hexpand(true);
        imp.pad.add_css_class("st-test-pad");
        let centered = gtk::Box::new(gtk::Orientation::Vertical, 8);
        centered.set_vexpand(true);
        centered.set_valign(gtk::Align::Center);
        centered.append(&hint);
        centered.append(&imp.last);
        imp.pad.append(&centered);
        imp.pad
            .update_property(&[gtk::accessible::Property::Label(tr(msg!(
                "Click test area"
            )))]);
        let clicks = gtk::GestureClick::new();
        // Every button, not just the first.
        clicks.set_button(0);
        clicks.connect_pressed(glib::clone!(
            #[weak]
            area,
            move |gesture, presses, _, _| {
                area.show_click(gesture.current_button(), presses);
            }
        ));
        imp.pad.add_controller(clicks);

        let lines = gtk::Box::new(gtk::Orientation::Vertical, 6);
        lines.add_css_class("st-test-lines");
        for number in 1..=LINES {
            let line = gtk::Label::new(Some(&trf(msg!("Line {number}"), &[("number", &number)])));
            line.set_xalign(0.0);
            lines.append(&line);
        }
        imp.strip
            .set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        imp.strip.set_child(Some(&lines));
        imp.strip.set_size_request(160, 160);
        imp.strip.add_css_class("st-test-strip");
        imp.strip
            .update_property(&[gtk::accessible::Property::Label(tr(msg!(
                "Scroll test area"
            )))]);

        imp.pad.set_parent(&area);
        imp.strip.set_parent(&area);
        area
    }

    /// What the pad says about the last click.
    pub fn last_click(&self) -> String {
        self.imp().last.text().to_string()
    }

    /// The strip of lines to scroll.
    pub fn strip(&self) -> &gtk::ScrolledWindow {
        &self.imp().strip
    }

    /// Shows `presses` presses of `button` as the last click.
    pub fn show_click(&self, button: u32, presses: i32) {
        let text = trf(
            msg!("Last: {click}"),
            &[("click", &click_name(button, presses))],
        );
        let last = &self.imp().last;
        last.set_text(&text);
        last.announce(&text, gtk::AccessibleAnnouncementPriority::Medium);
    }
}

/// "Left click", "Double click", "Right double-click", …: the click of `presses` presses of
/// `button` (1 left, 2 middle, 3 right) in a row.
fn click_name(button: u32, presses: i32) -> String {
    let text = match (button, presses) {
        (1, ..=1) => msg!("Left click"),
        (1, 2) => msg!("Double-click"),
        (1, _) => msg!("Triple-click"),
        (2, ..=1) => msg!("Middle click"),
        (2, 2) => msg!("Middle double-click"),
        (2, _) => msg!("Middle triple-click"),
        (3, ..=1) => msg!("Right click"),
        (3, 2) => msg!("Right double-click"),
        (3, _) => msg!("Right triple-click"),
        (_, ..=1) => msg!("Button {number} click"),
        (_, 2) => msg!("Button {number} double-click"),
        (_, _) => msg!("Button {number} triple-click"),
    };
    trf(text, &[("number", &button)])
}

#[cfg(test)]
mod tests {
    use super::click_name;

    #[test]
    fn clicks_are_named_by_button_and_presses() {
        assert_eq!(click_name(1, 1), "Left click");
        assert_eq!(click_name(1, 2), "Double-click");
        assert_eq!(click_name(1, 3), "Triple-click");
        assert_eq!(click_name(3, 1), "Right click");
        assert_eq!(click_name(2, 2), "Middle double-click");
        assert_eq!(click_name(8, 1), "Button 8 click");
    }
}
