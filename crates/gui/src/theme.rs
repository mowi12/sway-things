// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The colors of the app: the GTK theme's, light or dark as the desktop prefers, with sway's
//! focus color as the accent.
//!
//! The accent is the `client.focused` child border as a row shows it: Sway Things' stored value,
//! otherwise the user's config line, otherwise sway's default. A stylesheet of its own colors the
//! accented parts with it (checked switches and check buttons, slider fills, selections, focus
//! rings, links, suggested actions), because GTK's own theme compiles its accent into literal
//! colors; it also sets the theme's named accent colors for themes that use them. Fills get sway's
//! exact color with black or white on it; where the accent is drawn on the window background it
//! is shifted until it is readable there ([`sway_things_core::theme::Accent`]). The stylesheet is
//! rebuilt when the color, the preference `app.accent_source` or the light/dark style changes.
//!
//! GTK 4.20 and newer follow the desktop's light/dark preference by themselves. Older GTK
//! doesn't, so [`follow_color_scheme`] reads it from the XDG Settings portal
//! (`org.freedesktop.appearance color-scheme`) and follows its changes.

use std::cell::RefCell;

use gtk::glib;
use gtk::prelude::*;
use sway_things_core::registry::{Registry, Value};
use sway_things_core::theme::{Accent, Rgb};

use crate::contrast;
use crate::model::Model;

/// The setting whose color is the accent.
const ACCENT_KEY: &str = "windows.focused.child_border";
/// The preference that says where the accent comes from.
const SOURCE_KEY: &str = "app.accent_source";

/// The window backgrounds the accent is made readable on: those of GTK's own theme. Other themes
/// are close enough, as the contrast is checked against the darker or lighter end.
const LIGHT_BACKGROUND: Rgb = Rgb {
    r: 0.98,
    g: 0.98,
    b: 0.98,
};
const DARK_BACKGROUND: Rgb = Rgb {
    r: 0.14,
    g: 0.14,
    b: 0.14,
};

thread_local! {
    /// The stylesheet of the accent, on the default display, with the CSS it holds.
    static ACCENT: RefCell<Option<(gtk::CssProvider, String)>> = const { RefCell::new(None) };
}

/// Colors the app with the accent of `model`, and keeps it current: when the settings or the
/// light/dark style change. `widget` tells which style is in use.
pub fn follow_accent(widget: &impl IsA<gtk::Widget>, model: &Model) {
    let widget = widget.as_ref().clone();
    let update = glib::clone!(
        #[weak]
        widget,
        move |model: &Model| apply(&widget, model)
    );
    update(model);
    model.connect_changed(update);
    if let Some(settings) = gtk::Settings::default() {
        for property in [
            "gtk-application-prefer-dark-theme",
            "gtk-theme-name",
            "gtk-interface-color-scheme",
        ] {
            if settings.find_property(property).is_none() {
                continue;
            }
            settings.connect_notify_local(
                Some(property),
                glib::clone!(
                    #[weak]
                    widget,
                    #[weak]
                    model,
                    move |_, _| {
                        // The style of the widget changes once GTK has restyled it.
                        glib::idle_add_local_once(glib::clone!(
                            #[weak]
                            widget,
                            #[weak]
                            model,
                            move || apply(&widget, &model)
                        ));
                    }
                ),
            );
        }
    }
}

/// The CSS of the accent as it is applied now: empty while the theme's own accent is used.
pub fn accent_css() -> String {
    ACCENT.with_borrow(|accent| {
        accent
            .as_ref()
            .map(|(_, css)| css.clone())
            .unwrap_or_default()
    })
}

/// Builds the accent stylesheet for `model` and the style of `widget`, and loads it if it
/// changed.
fn apply(widget: &gtk::Widget, model: &Model) {
    let css = accent_of(model).map_or_else(String::new, |color| {
        // A light text color means a dark style.
        let dark = contrast::rgb(widget.color()).luminance() > 0.5;
        let background = if dark {
            DARK_BACKGROUND
        } else {
            LIGHT_BACKGROUND
        };
        stylesheet(&Accent::new(color, background))
    });
    ACCENT.with_borrow_mut(|accent| {
        if accent.as_ref().is_some_and(|(_, loaded)| *loaded == css) {
            return;
        }
        let provider = if let Some((provider, _)) = accent.take() {
            provider
        } else {
            let provider = gtk::CssProvider::new();
            if let Some(display) = gtk::gdk::Display::default() {
                gtk::style_context_add_provider_for_display(
                    &display,
                    &provider,
                    // Above the app's own style sheet.
                    gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1,
                );
            }
            provider
        };
        provider.load_from_string(&css);
        *accent = Some((provider, css));
    });
}

/// The accent color, or `None` while the preference asks for the theme's own.
fn accent_of(model: &Model) -> Option<sway_things_core::command::Color> {
    let registry = Registry::builtin();
    let source = registry
        .get(SOURCE_KEY)
        .and_then(|setting| model.view(setting, None));
    if source.is_some_and(|view| view.value == Value::Choice("theme".to_owned())) {
        return None;
    }
    match registry
        .get(ACCENT_KEY)
        .and_then(|setting| model.view(setting, None))
        .map(|view| view.value)
    {
        Some(Value::Color(color)) => Some(color),
        _ => None,
    }
}

/// The stylesheet that colors the accented parts with `accent`.
fn stylesheet(accent: &Accent) -> String {
    let fill = accent.fill;
    let on_fill = accent.on_fill;
    let ink = accent.on_background;
    format!(
        "/* Generated from sway's focus color. */
@define-color accent_bg_color {fill};
@define-color accent_fg_color {on_fill};
@define-color accent_color {ink};
@define-color theme_selected_bg_color {fill};
@define-color theme_selected_fg_color {on_fill};

switch:checked {{
  background-color: {fill};
  border-color: {fill};
  color: {on_fill};
}}

checkbutton check:checked,
checkbutton radio:checked {{
  background-color: {fill};
  background-image: none;
  border-color: {fill};
  color: {on_fill};
}}

scale > trough > highlight,
progressbar > trough > progress {{
  background-color: {fill};
  background-image: none;
  border-color: {fill};
}}

selection,
text selection,
label > selection,
entry > text > selection {{
  background-color: {fill};
  color: {on_fill};
}}

/* Selected rows keep the normal text: a light tint of the accent under it. */
row:selected,
listview > row:selected,
popover modelbutton:selected {{
  background-color: alpha({fill}, 0.18);
}}

button.suggested-action {{
  background-color: {fill};
  background-image: none;
  border-color: {fill};
  color: {on_fill};
}}

entry:focus-within,
spinbutton:focus-within {{
  border-color: {ink};
  outline-color: alpha({ink}, 0.5);
}}

*:focus-visible {{
  outline-color: alpha({ink}, 0.5);
}}

link,
label > link {{
  color: {ink};
}}
"
    )
}

/// Follows the desktop's light/dark preference from the XDG Settings portal, on GTK versions
/// that don't do that themselves (before 4.20). Without a portal nothing changes.
pub fn follow_color_scheme() {
    let Some(settings) = gtk::Settings::default() else {
        return;
    };
    if settings
        .find_property("gtk-interface-color-scheme")
        .is_some()
    {
        // GTK reads the portal itself.
        return;
    }
    glib::spawn_future_local(async move {
        // What the user's GTK settings say without a preference from the desktop.
        let initial = settings.is_gtk_application_prefer_dark_theme();
        let apply = |scheme: u32| {
            settings.set_gtk_application_prefer_dark_theme(prefers_dark(scheme).unwrap_or(initial));
        };
        if let Err(error) = portal::follow(apply).await {
            glib::g_debug!(
                "sway-things",
                "no light/dark preference from the portal: {error}"
            );
        }
    });
}

/// Whether the portal's `color-scheme` asks for dark: 1 dark, 2 light, otherwise no preference.
fn prefers_dark(scheme: u32) -> Option<bool> {
    match scheme {
        1 => Some(true),
        2 => Some(false),
        _ => None,
    }
}

/// The XDG Settings portal over GIO's D-Bus.
mod portal {
    use gtk::gio::{self, prelude::*};
    use gtk::glib;

    const DESTINATION: &str = "org.freedesktop.portal.Desktop";
    const PATH: &str = "/org/freedesktop/portal/desktop";
    const INTERFACE: &str = "org.freedesktop.portal.Settings";
    const NAMESPACE: &str = "org.freedesktop.appearance";
    const KEY: &str = "color-scheme";

    /// Calls `apply` with the `color-scheme` the portal reports, and again with each change, for
    /// as long as the app runs.
    pub(super) async fn follow(apply: impl Fn(u32)) -> Result<(), glib::Error> {
        let proxy = gio::DBusProxy::for_bus_future(
            gio::BusType::Session,
            gio::DBusProxyFlags::DO_NOT_LOAD_PROPERTIES,
            None,
            DESTINATION,
            PATH,
            INTERFACE,
        )
        .await?;
        // First the subscription, so a change in between isn't missed. The signal comes on
        // GIO's thread; the channel brings it to the main loop.
        let (sender, changes) = async_channel::unbounded();
        proxy.connect_g_signal(None, move |_, _, signal, parameters| {
            if signal != "SettingChanged" {
                return;
            }
            let Some((namespace, key, value)) = parameters.get::<(String, String, glib::Variant)>()
            else {
                return;
            };
            if namespace == NAMESPACE && key == KEY {
                if let Some(scheme) = value.get::<u32>() {
                    let _ = sender.send_blocking(scheme);
                }
            }
        });
        if let Some(scheme) = read(&proxy).await {
            apply(scheme);
        }
        while let Ok(scheme) = changes.recv().await {
            apply(scheme);
        }
        Ok(())
    }

    /// The current `color-scheme`: `ReadOne` (portal version 2), else the older `Read`, which
    /// wraps the value in one more variant. `None` if the portal has none.
    async fn read(proxy: &gio::DBusProxy) -> Option<u32> {
        let arguments = (NAMESPACE, KEY).to_variant();
        let reply = match proxy
            .call_future("ReadOne", Some(&arguments), gio::DBusCallFlags::NONE, -1)
            .await
        {
            Ok(reply) => reply,
            Err(_) => proxy
                .call_future("Read", Some(&arguments), gio::DBusCallFlags::NONE, -1)
                .await
                .ok()?,
        };
        let mut value = reply.try_child_value(0)?;
        // Unwrap `v` until the number.
        while let Some(inner) = value.as_variant() {
            value = inner;
        }
        value.get::<u32>()
    }
}

#[cfg(test)]
mod tests {
    use sway_things_core::command::Color;
    use sway_things_core::theme::{Accent, Rgb};

    use super::{prefers_dark, stylesheet};

    #[test]
    fn the_portals_color_scheme_maps_to_dark_light_or_nothing() {
        assert_eq!(prefers_dark(1), Some(true));
        assert_eq!(prefers_dark(2), Some(false));
        assert_eq!(prefers_dark(0), None);
        assert_eq!(prefers_dark(7), None);
    }

    #[test]
    fn the_stylesheet_fills_with_the_color_and_inks_with_the_readable_one() {
        let accent = Accent::new(Color::rgb(0x28, 0x55, 0x77), Rgb::WHITE);
        let css = stylesheet(&accent);
        assert!(
            css.contains("@define-color accent_bg_color #285577;"),
            "{css}"
        );
        assert!(
            css.contains("@define-color accent_fg_color #ffffff;"),
            "{css}"
        );
        assert!(
            css.contains("switch:checked {\n  background-color: #285577;"),
            "{css}"
        );
    }
}
