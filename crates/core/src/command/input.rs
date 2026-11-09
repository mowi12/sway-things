// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Builders for `input <identifier> …` commands (`man 5 sway-input`), used by the Mouse & Touchpad
//! and Keyboard pages.
//!
//! ```
//! use sway_things_core::command::Target;
//! use sway_things_core::command::input::{Input, InputSelector, InputType};
//!
//! let touchpad = Input::new(InputSelector::Type(InputType::Touchpad));
//! assert_eq!(touchpad.tap(true).render(Target::Ipc)?, "input type:touchpad tap enabled");
//!
//! let mouse = Input::new(InputSelector::device("1133:16500:Logitech_G305")?);
//! assert_eq!(
//!     mouse.pointer_accel(-0.5)?.render(Target::Config)?,
//!     r#"input "1133:16500:Logitech_G305" pointer_accel -0.5"#
//! );
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use std::fmt;

use super::{BoolStyle, SwayCmd, SwayText, TextError};
use crate::version::SwayVersion;

/// A device type, for `type:<type>` selectors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InputType {
    /// `type:touchpad`
    Touchpad,
    /// `type:pointer` (mice, trackpoints, trackballs)
    Pointer,
    /// `type:keyboard`
    Keyboard,
    /// `type:touch` (touchscreens)
    Touch,
    /// `type:tablet_tool`
    TabletTool,
    /// `type:tablet_pad`
    TabletPad,
    /// `type:switch` (lid and tablet-mode switches)
    Switch,
}

impl InputType {
    const fn selector(self) -> &'static str {
        match self {
            Self::Touchpad => "type:touchpad",
            Self::Pointer => "type:pointer",
            Self::Keyboard => "type:keyboard",
            Self::Touch => "type:touch",
            Self::TabletTool => "type:tablet_tool",
            Self::TabletPad => "type:tablet_pad",
            Self::Switch => "type:switch",
        }
    }
}

/// Which devices an `input` command applies to.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum InputSelector {
    /// `*`: every device.
    All,
    /// `type:<type>`: every device of a type.
    Type(InputType),
    /// One device, by its identifier from `GET_INPUTS`. Create it with [`InputSelector::device`].
    Device(SwayText),
}

impl InputSelector {
    /// One device, by its identifier from `GET_INPUTS`.
    ///
    /// sway builds identifiers as `<vendor>:<product>:<name>`, with decimal IDs and every space or
    /// non-printable byte of the name replaced by `_`.
    ///
    /// # Errors
    ///
    /// [`InputError::NotADeviceIdentifier`] if `identifier` doesn't have that form, which also keeps
    /// it apart from `*` and `type:…`; [`InputError::Text`] if it contains a backslash.
    pub fn device(identifier: &str) -> Result<Self, InputError> {
        let mut parts = identifier.splitn(3, ':');
        let (Some(vendor), Some(product), Some(name)) = (parts.next(), parts.next(), parts.next())
        else {
            return Err(InputError::NotADeviceIdentifier);
        };
        let is_id = |part: &str| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit());
        if !is_id(vendor) || !is_id(product) || !name.bytes().all(|byte| byte.is_ascii_graphic()) {
            return Err(InputError::NotADeviceIdentifier);
        }
        Ok(Self::Device(SwayText::new(identifier)?))
    }
}

/// `accel_profile`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AccelProfile {
    /// `adaptive`: the pointer speeds up with faster movements.
    Adaptive,
    /// `flat`: constant speed.
    Flat,
}

/// `scroll_method`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScrollMethod {
    /// `none`
    None,
    /// `two_finger`
    TwoFinger,
    /// `edge`
    Edge,
    /// `on_button_down`: scroll while the [`ScrollButton`] is held.
    OnButtonDown,
}

/// `scroll_button`: the button for [`ScrollMethod::OnButtonDown`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScrollButton {
    /// `disable`
    Disable,
    /// `button1`
    Left,
    /// `button2`
    Middle,
    /// `button3`
    Right,
    /// `button8` (back)
    Side,
    /// `button9` (forward)
    Extra,
}

/// `tap_button_map` and `clickfinger_button_map`: which button one, two and three fingers press.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ButtonMap {
    /// `lrm`: left, right, middle.
    Lrm,
    /// `lmr`: left, middle, right.
    Lmr,
}

/// `drag_lock`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DragLock {
    /// `disabled`
    Disabled,
    /// `enabled`: lifting the finger briefly during a drag doesn't end it.
    Enabled,
    /// `enabled_sticky`: the drag only ends with another tap. Needs sway 1.11, built against a
    /// libinput that supports it; otherwise sway reads the value as `disabled` without an error.
    EnabledSticky,
}

/// `click_method`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClickMethod {
    /// `none`
    None,
    /// `button_areas`: the bottom of the touchpad is split into button areas.
    ButtonAreas,
    /// `clickfinger`: the number of fingers decides the button.
    Clickfinger,
}

/// `events`: whether the device sends events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SendEvents {
    /// `enabled`
    Enabled,
    /// `disabled`: the device is off.
    Disabled,
    /// `disabled_on_external_mouse`: off while an external mouse is connected.
    DisabledOnExternalMouse,
}

/// `xkb_switch_layout`: which layout to switch to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum XkbSwitch {
    /// `next`, wrapping around.
    Next,
    /// `prev`, wrapping around.
    Prev,
    /// The layout at this index of `xkb_layout`, counting from zero.
    Index(u8),
}

/// Builds `input` commands for one [`InputSelector`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Input {
    selector: InputSelector,
}

impl Input {
    /// Builds commands for the devices `selector` matches.
    pub fn new(selector: InputSelector) -> Self {
        Self { selector }
    }

    fn command(&self, option: &'static str) -> SwayCmd {
        let cmd = SwayCmd::new("input");
        let cmd = match &self.selector {
            InputSelector::All => cmd.keyword("*"),
            InputSelector::Type(input_type) => cmd.keyword(input_type.selector()),
            InputSelector::Device(identifier) => cmd.text(identifier.clone()),
        };
        cmd.keyword(option)
    }

    fn toggle(&self, option: &'static str, enabled: bool) -> SwayCmd {
        self.command(option)
            .bool(enabled, BoolStyle::EnabledDisabled)
    }

    /// `pointer_accel`: pointer speed from -1.0 (slowest) to 1.0 (fastest).
    ///
    /// # Errors
    ///
    /// [`InputError::OutOfRange`] outside -1.0 … 1.0 or for NaN.
    pub fn pointer_accel(&self, speed: f64) -> Result<SwayCmd, InputError> {
        check_range("pointer_accel", speed, -1.0, 1.0)?;
        Ok(self.command("pointer_accel").float(speed))
    }

    /// `accel_profile`
    pub fn accel_profile(&self, profile: AccelProfile) -> SwayCmd {
        self.command("accel_profile").keyword(match profile {
            AccelProfile::Adaptive => "adaptive",
            AccelProfile::Flat => "flat",
        })
    }

    /// `natural_scroll`: inverted scrolling.
    pub fn natural_scroll(&self, enabled: bool) -> SwayCmd {
        self.toggle("natural_scroll", enabled)
    }

    /// `scroll_factor`: scroll speed multiplier.
    ///
    /// # Errors
    ///
    /// [`InputError::OutOfRange`] for negative values, NaN, or values sway can't store as a 32-bit
    /// float.
    pub fn scroll_factor(&self, factor: f64) -> Result<SwayCmd, InputError> {
        check_range("scroll_factor", factor, 0.0, f64::from(f32::MAX))?;
        Ok(self.command("scroll_factor").float(factor))
    }

    /// `left_handed`: swaps the left and right buttons.
    pub fn left_handed(&self, enabled: bool) -> SwayCmd {
        self.toggle("left_handed", enabled)
    }

    /// `middle_emulation`: pressing left and right together clicks the middle button.
    pub fn middle_emulation(&self, enabled: bool) -> SwayCmd {
        self.toggle("middle_emulation", enabled)
    }

    /// `scroll_method`
    pub fn scroll_method(&self, method: ScrollMethod) -> SwayCmd {
        self.command("scroll_method").keyword(match method {
            ScrollMethod::None => "none",
            ScrollMethod::TwoFinger => "two_finger",
            ScrollMethod::Edge => "edge",
            ScrollMethod::OnButtonDown => "on_button_down",
        })
    }

    /// `scroll_button`
    pub fn scroll_button(&self, button: ScrollButton) -> SwayCmd {
        self.command("scroll_button").keyword(match button {
            ScrollButton::Disable => "disable",
            ScrollButton::Left => "button1",
            ScrollButton::Middle => "button2",
            ScrollButton::Right => "button3",
            ScrollButton::Side => "button8",
            ScrollButton::Extra => "button9",
        })
    }

    /// `tap`: tap to click.
    pub fn tap(&self, enabled: bool) -> SwayCmd {
        self.toggle("tap", enabled)
    }

    /// `tap_button_map`
    pub fn tap_button_map(&self, map: ButtonMap) -> SwayCmd {
        self.command("tap_button_map").keyword(button_map(map))
    }

    /// `drag`: tap and drag.
    pub fn drag(&self, enabled: bool) -> SwayCmd {
        self.toggle("drag", enabled)
    }

    /// `drag_lock`. [`DragLock::EnabledSticky`] raises [`SwayCmd::min_sway`] to 1.11.
    pub fn drag_lock(&self, lock: DragLock) -> SwayCmd {
        let cmd = self.command("drag_lock");
        match lock {
            DragLock::Disabled => cmd.keyword("disabled"),
            DragLock::Enabled => cmd.keyword("enabled"),
            DragLock::EnabledSticky => cmd
                .keyword("enabled_sticky")
                .requires_sway(SwayVersion::new(1, 11, 0)),
        }
    }

    /// `dwt`: disable while typing.
    pub fn dwt(&self, enabled: bool) -> SwayCmd {
        self.toggle("dwt", enabled)
    }

    /// `dwtp`: disable while using the trackpoint.
    pub fn dwtp(&self, enabled: bool) -> SwayCmd {
        self.toggle("dwtp", enabled)
    }

    /// `click_method`
    pub fn click_method(&self, method: ClickMethod) -> SwayCmd {
        self.command("click_method").keyword(match method {
            ClickMethod::None => "none",
            ClickMethod::ButtonAreas => "button_areas",
            ClickMethod::Clickfinger => "clickfinger",
        })
    }

    /// `clickfinger_button_map`
    pub fn clickfinger_button_map(&self, map: ButtonMap) -> SwayCmd {
        self.command("clickfinger_button_map")
            .keyword(button_map(map))
    }

    /// `events`
    pub fn events(&self, events: SendEvents) -> SwayCmd {
        self.command("events").keyword(match events {
            SendEvents::Enabled => "enabled",
            SendEvents::Disabled => "disabled",
            SendEvents::DisabledOnExternalMouse => "disabled_on_external_mouse",
        })
    }

    /// `xkb_layout`, e.g. `["us", "de"]`.
    ///
    /// # Errors
    ///
    /// [`InputError::EmptyList`] for no layouts, [`InputError::EmptyListElement`] or
    /// [`InputError::CommaInListElement`] for an invalid layout, [`InputError::Text`] for control
    /// characters or backslashes.
    pub fn xkb_layout<S: AsRef<str>>(&self, layouts: &[S]) -> Result<SwayCmd, InputError> {
        self.xkb_list("xkb_layout", layouts, false)
    }

    /// `xkb_variant`, one per layout, e.g. `["", "nodeadkeys"]`. An empty variant keeps the
    /// layout's default.
    ///
    /// # Errors
    ///
    /// [`InputError::EmptyList`] if the list renders empty (no variants, or only one empty one),
    /// [`InputError::CommaInListElement`], or [`InputError::Text`].
    pub fn xkb_variant<S: AsRef<str>>(&self, variants: &[S]) -> Result<SwayCmd, InputError> {
        self.xkb_list("xkb_variant", variants, true)
    }

    /// `xkb_options`, e.g. `["grp:alt_shift_toggle", "caps:escape"]`.
    ///
    /// # Errors
    ///
    /// Like [`Input::xkb_layout`].
    pub fn xkb_options<S: AsRef<str>>(&self, options: &[S]) -> Result<SwayCmd, InputError> {
        self.xkb_list("xkb_options", options, false)
    }

    /// `xkb_model`, e.g. `pc105`.
    ///
    /// # Errors
    ///
    /// [`InputError::Text`] if `model` is empty or contains a control character or backslash.
    pub fn xkb_model(&self, model: &str) -> Result<SwayCmd, InputError> {
        Ok(self.command("xkb_model").text(SwayText::new(model)?))
    }

    /// `xkb_numlock`: Num Lock state at startup. sway only accepts it in config files.
    pub fn xkb_numlock(&self, enabled: bool) -> SwayCmd {
        self.toggle("xkb_numlock", enabled).config_only()
    }

    /// `xkb_capslock`: Caps Lock state at startup. sway only accepts it in config files.
    pub fn xkb_capslock(&self, enabled: bool) -> SwayCmd {
        self.toggle("xkb_capslock", enabled).config_only()
    }

    /// `repeat_delay`: milliseconds a key is held before it repeats.
    pub fn repeat_delay(&self, milliseconds: u16) -> SwayCmd {
        self.command("repeat_delay").int(i64::from(milliseconds))
    }

    /// `repeat_rate`: repeats per second; 0 turns repeating off.
    pub fn repeat_rate(&self, per_second: u16) -> SwayCmd {
        self.command("repeat_rate").int(i64::from(per_second))
    }

    /// `xkb_switch_layout`: switches the active layout now. sway defers it while reading a config
    /// file, so it only works over IPC.
    pub fn xkb_switch_layout(&self, switch: XkbSwitch) -> SwayCmd {
        let cmd = self.command("xkb_switch_layout");
        match switch {
            XkbSwitch::Next => cmd.keyword("next"),
            XkbSwitch::Prev => cmd.keyword("prev"),
            XkbSwitch::Index(index) => cmd.int(i64::from(index)),
        }
        .runtime_only()
    }

    fn xkb_list<S: AsRef<str>>(
        &self,
        option: &'static str,
        items: &[S],
        empty_elements: bool,
    ) -> Result<SwayCmd, InputError> {
        for (index, item) in items.iter().enumerate() {
            let item = item.as_ref();
            if item.contains(',') {
                return Err(InputError::CommaInListElement { option, index });
            }
            if item.is_empty() && !empty_elements {
                return Err(InputError::EmptyListElement { option, index });
            }
        }
        let joined = items
            .iter()
            .map(AsRef::as_ref)
            .collect::<Vec<_>>()
            .join(",");
        let text = SwayText::new(joined).map_err(|error| match error {
            TextError::Empty => InputError::EmptyList { option },
            error => InputError::Text(error),
        })?;
        Ok(self.command(option).text(text))
    }
}

const fn button_map(map: ButtonMap) -> &'static str {
    match map {
        ButtonMap::Lrm => "lrm",
        ButtonMap::Lmr => "lmr",
    }
}

fn check_range(option: &'static str, value: f64, min: f64, max: f64) -> Result<(), InputError> {
    if (min..=max).contains(&value) {
        Ok(())
    } else {
        Err(InputError::OutOfRange { option, min, max })
    }
}

/// Why an `input` command can't be built.
#[derive(Debug, Clone, PartialEq)]
pub enum InputError {
    /// A number is outside the range sway accepts (or NaN).
    OutOfRange {
        /// The option, e.g. `pointer_accel`.
        option: &'static str,
        /// The smallest accepted value.
        min: f64,
        /// The largest accepted value.
        max: f64,
    },
    /// The string isn't a device identifier of the form `<vendor>:<product>:<name>`.
    NotADeviceIdentifier,
    /// An XKB list is empty.
    EmptyList {
        /// The option, e.g. `xkb_layout`.
        option: &'static str,
    },
    /// An element of an XKB list is empty.
    EmptyListElement {
        /// The option.
        option: &'static str,
        /// Index of the element.
        index: usize,
    },
    /// An element of an XKB list contains the separator `,`.
    CommaInListElement {
        /// The option.
        option: &'static str,
        /// Index of the element.
        index: usize,
    },
    /// A string can't be used as text.
    Text(TextError),
}

impl From<TextError> for InputError {
    fn from(error: TextError) -> Self {
        Self::Text(error)
    }
}

impl fmt::Display for InputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutOfRange { option, min, max } => {
                write!(f, "{option} must be between {min} and {max}")
            }
            Self::NotADeviceIdentifier => {
                f.write_str("expected a device identifier like 1133:16500:Logitech_G305")
            }
            Self::EmptyList { option } => write!(f, "{option} needs at least one value"),
            Self::EmptyListElement { option, index } => {
                write!(f, "value {index} of {option} is empty")
            }
            Self::CommaInListElement { option, index } => {
                write!(f, "value {index} of {option} contains a comma")
            }
            Self::Text(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for InputError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Text(error) => Some(error),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests;
