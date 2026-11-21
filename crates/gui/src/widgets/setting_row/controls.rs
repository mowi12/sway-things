// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The control of a row: which widget a value type gets, the number control with its snapping
//! to the default, and how a value is shown in it.
use std::str::FromStr;

use gtk::glib;
use gtk::prelude::*;
use sway_things_core::command::Color;
use sway_things_core::registry::{Kind, Registry, Setting, Value};

use super::{Control, DIRECTIVES_KEY, SLIDER_MINIMUM_WIDTH};
use crate::i18n::tr;
use crate::model::Model;

/// The control for the value type of `setting`, or `None` if it has none.
pub(super) fn control(setting: &'static Setting) -> Option<Control> {
    Some(match setting.kind() {
        Kind::Bool { .. } => {
            let switch = gtk::Switch::new();
            switch.set_valign(gtk::Align::Center);
            Control::Toggle(switch)
        }
        Kind::Integer { min, max, .. } => {
            #[expect(
                clippy::cast_precision_loss,
                reason = "the ranges of whole-number settings are far below 2^52"
            )]
            let range = (*min as f64, *max as f64);
            number_control(setting, range, true)?
        }
        Kind::Decimal { min, max, .. } => number_control(setting, (*min, *max), false)?,
        Kind::Choice { choices, .. } => {
            let labels: Vec<&str> = choices.iter().map(|choice| tr(choice.label)).collect();
            let dropdown = gtk::DropDown::from_strings(&labels);
            dropdown.set_valign(gtk::Align::Center);
            Control::Choice {
                dropdown,
                ids: choices.iter().map(|choice| choice.id).collect(),
            }
        }
        Kind::Color { .. } => {
            let dialog = gtk::ColorDialog::new();
            dialog.set_with_alpha(true);
            let button = gtk::ColorDialogButton::new(Some(dialog));
            button.set_valign(gtk::Align::Center);
            Control::Color(button)
        }
        Kind::Text { .. } if setting.font().is_some() => {
            let label = gtk::Label::new(None);
            label.set_ellipsize(gtk::pango::EllipsizeMode::End);
            label.set_max_width_chars(28);
            let button = gtk::Button::new();
            button.set_child(Some(&label));
            button.set_valign(gtk::Align::Center);
            Control::Font(button)
        }
        Kind::Text { .. } => {
            let entry = gtk::Entry::new();
            entry.set_width_chars(20);
            if let Some(placeholder) = setting.placeholder() {
                let placeholder = tr(placeholder);
                entry.set_placeholder_text(Some(placeholder));
                // The hint fits in full where there is room; on a narrow row the entry shrinks to
                // 20 characters and cuts it off.
                let width = i32::try_from(placeholder.chars().count()).unwrap_or(i32::MAX);
                entry.set_max_width_chars(width.max(20));
            }
            entry.set_valign(gtk::Align::Center);
            Control::Text(entry)
        }
        Kind::List { .. } => Control::None,
    })
}

/// How long a number control whose changes wait ([`SettingRow::defers`]) stays still before its
/// value is sent: clicks on its buttons in a row become one change.
pub(super) const COMMIT_DELAY: std::time::Duration = std::time::Duration::from_millis(600);

/// The share of a slider's range around the default in which dragging locks to the default.
const SNAP_ZONE: f64 = 0.02;

/// Whether a slider dragged to `value` snaps to `default`: while `value` is within [`SNAP_ZONE`]
/// of the range `min` to `max` from it. Then the result is `default`; `None` leaves the slider at
/// `value`. A default outside the range never snaps.
pub(super) fn snap(value: f64, default: f64, (min, max): (f64, f64)) -> Option<f64> {
    let zone = (max - min).abs() * SNAP_ZONE;
    let in_range = (min.min(max)..=min.max(max)).contains(&default);
    (in_range && (value - default).abs() <= zone).then_some(default)
}

/// A slider over the slider range and a number field for every value the setting accepts. The
/// slider has a mark at the setting's default and snaps to it while it is dragged; keyboard steps
/// and the number field stay exact.
pub(super) fn number_control(
    setting: &Setting,
    (min, max): (f64, f64),
    whole: bool,
) -> Option<Control> {
    let slider = setting.slider()?;
    let scale = gtk::Scale::with_range(
        gtk::Orientation::Horizontal,
        slider.min,
        slider.max,
        slider.step,
    );
    #[expect(
        clippy::cast_precision_loss,
        reason = "the ranges of whole-number settings are far below 2^52"
    )]
    let default = match setting.default_value() {
        Value::Integer(number) => Some(number as f64),
        Value::Decimal(number) => Some(number),
        _ => None,
    };
    let range = (slider.min, slider.max);
    if let Some(default) = default.filter(|default| (slider.min..=slider.max).contains(default)) {
        scale.add_mark(default, gtk::PositionType::Bottom, None);
        // A pointer drag or click moves the slider with `Jump`; keys and the wheel step.
        scale.connect_change_value(move |scale, scroll, value| {
            if scroll != gtk::ScrollType::Jump {
                return glib::Propagation::Proceed;
            }
            match snap(value, default, range) {
                Some(snapped) => {
                    scale.set_value(snapped);
                    glib::Propagation::Stop
                }
                None => glib::Propagation::Proceed,
            }
        });
    }
    scale.set_draw_value(false);
    // At its minimum on a narrow row; the row lets it grow to `SLIDER_WIDTH` when there is room.
    scale.set_size_request(SLIDER_MINIMUM_WIDTH, -1);
    scale.set_hexpand(true);
    scale.set_valign(gtk::Align::Center);
    let spin = gtk::SpinButton::with_range(min, max, slider.step);
    spin.set_digits(digits(slider.step, whole));
    spin.set_width_chars(5);
    spin.set_valign(gtk::Align::Center);
    Some(Control::Number { scale, spin, whole })
}

/// The decimal places for numbers in steps of `step`.
pub(super) fn digits(step: f64, whole: bool) -> u32 {
    if whole {
        return 0;
    }
    (0..4)
        .find(|places| {
            let scaled = step * 10f64.powi(*places);
            (scaled - scaled.round()).abs() < 1e-9
        })
        .and_then(|places| u32::try_from(places).ok())
        .unwrap_or(4)
}

/// The number value of a control position.
pub(super) fn number(value: f64, whole: bool) -> Value {
    if whole {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "the control's range is that of the setting, far within i64"
        )]
        Value::Integer(value.round() as i64)
    } else {
        // The slider moves in binary fractions; the stored value is what the field shows.
        Value::Decimal((value * 1e4).round() / 1e4)
    }
}

/// Puts `value` into the control.
pub(super) fn show_value(control: &Control, setting: &Setting, value: &Value) {
    match (control, value) {
        (Control::Toggle(switch), Value::Bool(on)) => {
            switch.set_active(*on);
            switch.set_state(*on);
        }
        (Control::Number { scale, spin, .. }, Value::Integer(number)) => {
            #[expect(
                clippy::cast_precision_loss,
                reason = "the ranges of whole-number settings are far below 2^52"
            )]
            let number = *number as f64;
            spin.set_value(number);
            scale.set_value(number);
        }
        (Control::Number { scale, spin, .. }, Value::Decimal(number)) => {
            spin.set_value(*number);
            scale.set_value(*number);
        }
        (Control::Choice { dropdown, ids }, Value::Choice(id)) => {
            if let Some(index) = ids.iter().position(|known| known == id) {
                if let Ok(index) = u32::try_from(index) {
                    dropdown.set_selected(index);
                }
            }
        }
        (Control::Color(button), Value::Color(color)) => {
            if let Ok(rgba) = gtk::gdk::RGBA::parse(color.to_string()) {
                button.set_rgba(&rgba);
            }
        }
        (Control::Font(button), Value::Text(text)) => {
            if let Some(label) = button.child().and_downcast::<gtk::Label>() {
                if label.text() != *text {
                    label.set_text(text);
                    // In the font itself, at the size of the label: the size is in the text.
                    let mut font = gtk::pango::FontDescription::from_string(text);
                    font.unset_fields(gtk::pango::FontMask::SIZE);
                    let attributes = gtk::pango::AttrList::new();
                    attributes.insert(gtk::pango::AttrFontDesc::new(&font));
                    label.set_attributes(Some(&attributes));
                }
            }
        }
        (Control::Text(entry), Value::Text(text)) => {
            if entry.text() != *text {
                entry.set_text(text);
                // GTK's GPU renderers can leave a few pixels of a longer text before (seen with a
                // position after picking another display): the whole field is drawn again.
                entry.queue_draw();
                if let Some(inner) = entry.first_child() {
                    inner.queue_draw();
                }
            }
        }
        (
            Control::Pick {
                dropdown,
                labels,
                values,
            },
            Value::Text(_) | Value::Integer(_),
        ) => {
            let text = &match value {
                Value::Text(text) => text.clone(),
                other => other.to_string(),
            };
            let known = values.borrow().iter().position(|value| value == text);
            let index = known.unwrap_or_else(|| {
                // A value typed by hand: shown by its name.
                labels.append(text);
                values.borrow_mut().push(text.clone());
                values.borrow().len() - 1
            });
            if let Ok(index) = u32::try_from(index) {
                dropdown.set_selected(index);
            }
        }
        (Control::None, _) => {}
        _ => glib::g_warning!(
            "sway-things",
            "{}: value {value:?} doesn't fit its row",
            setting.key()
        ),
    }
}

/// The color of `rgba`, with an alpha only if it isn't opaque.
pub(super) fn color(rgba: &gtk::gdk::RGBA) -> Option<Color> {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the channel is clamped to 0 ..= 255 first"
    )]
    let channel = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    let (red, green, blue, alpha) = (
        channel(rgba.red()),
        channel(rgba.green()),
        channel(rgba.blue()),
        channel(rgba.alpha()),
    );
    let text = if alpha == 0xff {
        format!("#{red:02x}{green:02x}{blue:02x}")
    } else {
        format!("#{red:02x}{green:02x}{blue:02x}{alpha:02x}")
    };
    Color::from_str(&text).ok()
}

/// Whether rows show the line of the sway config, as the preference `app.show_directives` says.
pub(in crate::widgets) fn shows_directives(model: &Model) -> bool {
    Registry::builtin()
        .get(DIRECTIVES_KEY)
        .and_then(|setting| model.view(setting, None))
        .is_some_and(|view| view.value == Value::Bool(true))
}

#[cfg(test)]
mod tests {
    use super::{digits, number, snap};
    use sway_things_core::registry::Value;

    #[test]
    fn decimal_places_follow_the_step() {
        assert_eq!(digits(1.0, true), 0);
        assert_eq!(digits(0.05, false), 2);
        assert_eq!(digits(0.5, false), 1);
        assert_eq!(digits(0.02, false), 2);
        assert_eq!(digits(1.0, false), 0);
    }

    #[test]
    fn a_dragged_slider_snaps_to_the_default_within_two_percent() {
        // A range of 2 (-1 to 1): the zone is 0.04 on each side of the default.
        let range = (-1.0, 1.0);
        assert_eq!(snap(0.04, 0.0, range), Some(0.0));
        assert_eq!(snap(-0.04, 0.0, range), Some(0.0));
        assert_eq!(snap(0.02, 0.0, range), Some(0.0));
        assert_eq!(snap(0.0401, 0.0, range), None);
        assert_eq!(snap(-0.05, 0.0, range), None);
        // Negative values only.
        assert_eq!(snap(-49.5, -50.0, (-100.0, 0.0)), Some(-50.0));
        assert_eq!(snap(-47.9, -50.0, (-100.0, 0.0)), None);
        // A default at either end snaps from inside the range.
        assert_eq!(snap(1.5, 0.0, (0.0, 100.0)), Some(0.0));
        assert_eq!(snap(2.5, 0.0, (0.0, 100.0)), None);
        assert_eq!(snap(98.0, 100.0, (0.0, 100.0)), Some(100.0));
        assert_eq!(snap(97.9, 100.0, (0.0, 100.0)), None);
        // A default outside the range never snaps.
        assert_eq!(snap(10.5, 5.0, (10.0, 4000.0)), None);
    }

    #[test]
    fn control_positions_become_values() {
        assert_eq!(number(599.6, true), Value::Integer(600));
        assert_eq!(number(0.300_000_000_4, false), Value::Decimal(0.3));
    }
}
