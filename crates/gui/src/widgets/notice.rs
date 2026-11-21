// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A notice on a page: something the user should know about the page as a whole, like a captive
//! portal, kanshi setting up the displays, or a program of the user's config that sets the
//! wallpaper. Every page draws it the same way: a framed box with an icon, the text and, where
//! there is something to do about it, a button.

use gtk::prelude::*;

/// What a notice is about, which picks its icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    /// Worth knowing; nothing is wrong.
    Info,
    /// Something keeps the page from working as it should.
    Warning,
}

impl Tone {
    /// The name of the icon of the tone.
    fn icon_name(self) -> &'static str {
        match self {
            Self::Info => "dialog-information-symbolic",
            Self::Warning => "dialog-warning-symbolic",
        }
    }
}

/// A notice of `tone` that shows `content` (usually a [`text`]) and `button`, if any.
pub fn notice(
    tone: Tone,
    content: &impl IsA<gtk::Widget>,
    button: Option<&gtk::Button>,
) -> gtk::Box {
    let notice = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    fill(&notice, tone, content, button);
    notice
}

/// Makes `notice` a notice of `tone`: its children are the icon, `content` and `button`, if any,
/// in this order.
pub fn fill(
    notice: &gtk::Box,
    tone: Tone,
    content: &impl IsA<gtk::Widget>,
    button: Option<&gtk::Button>,
) {
    notice.set_orientation(gtk::Orientation::Horizontal);
    notice.set_spacing(12);
    notice.add_css_class("st-notice");
    let icon = gtk::Image::from_icon_name(tone.icon_name());
    icon.set_valign(gtk::Align::Center);
    icon.set_accessible_role(gtk::AccessibleRole::Presentation);
    notice.append(&icon);
    content.set_hexpand(true);
    content.set_valign(gtk::Align::Center);
    notice.append(content);
    if let Some(button) = button {
        button.set_valign(gtk::Align::Center);
        notice.append(button);
    }
}

/// Gives `notice`, made by [`notice`] or [`fill`], the icon of `tone`.
pub fn set_tone(notice: &gtk::Box, tone: Tone) {
    if let Some(icon) = notice
        .first_child()
        .and_then(|child| child.downcast::<gtk::Image>().ok())
    {
        icon.set_icon_name(Some(tone.icon_name()));
    }
}

/// The text of a notice: wrapped, also within long words such as paths.
pub fn text(text: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.set_xalign(0.0);
    label.set_wrap(true);
    label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    label
}
