// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Key combinations the way sway reads them in `bindsym` and `bindcode` (`cmd_bindsym_or_bindcode`
//! in `sway/commands/bind.c`, `get_modifier_mask_by_name` in `sway/input/keyboard.c` and
//! `get_mouse_bindsym` / `get_mouse_bindcode` in `sway/input/cursor.c`, unchanged from sway 1.10
//! to 1.12).
//!
//! sway splits the combination at `+` (empty parts are dropped), takes every part that names a
//! modifier into a bit mask, and every other part as a key. A keysym is looked up without regard
//! to case, and where two keysyms differ only in case the lowercase one wins, so `E` and `e` are
//! the same key. The keys are sorted, so their order doesn't matter either. Two combinations
//! are the same binding when the modifiers and the set of keys are equal.

use std::fmt;

use crate::i18n::Text;
use crate::msg;

/// The modifiers of a key combination: sway's bit mask, the values of wlroots'
/// `WLR_MODIFIER_*`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Modifiers(u32);

/// sway's names of the modifiers, in the order of its table. The first name of a mask is the one
/// sway writes.
const NAMES: &[(&str, u32)] = &[
    ("Shift", 1),
    ("Lock", 2),
    ("Control", 4),
    ("Ctrl", 4),
    ("Mod1", 8),
    ("Alt", 8),
    ("Mod2", 16),
    ("Mod3", 32),
    ("Mod4", 64),
    ("Super", 64),
    ("Mod5", 128),
];

impl Modifiers {
    /// No modifier.
    pub const NONE: Self = Self(0);
    /// `Shift`.
    pub const SHIFT: Self = Self(1);
    /// `Lock`, the Caps Lock state.
    pub const LOCK: Self = Self(2);
    /// `Control` or `Ctrl`.
    pub const CTRL: Self = Self(4);
    /// `Mod1` or `Alt`.
    pub const ALT: Self = Self(8);
    /// `Mod2`, usually Num Lock.
    pub const MOD2: Self = Self(16);
    /// `Mod3`.
    pub const MOD3: Self = Self(32);
    /// `Mod4` or `Super`, the logo key.
    pub const SUPER: Self = Self(64);
    /// `Mod5`, usually the third level (`AltGr`).
    pub const MOD5: Self = Self(128);

    /// The modifier `name` stands for, without regard to case as in sway, or `None` if it isn't
    /// one.
    pub fn from_name(name: &str) -> Option<Self> {
        NAMES
            .iter()
            .find(|(known, _)| known.eq_ignore_ascii_case(name))
            .map(|&(_, mask)| Self(mask))
    }

    /// The bit mask.
    pub const fn bits(self) -> u32 {
        self.0
    }

    /// Whether no modifier is set.
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Whether every modifier of `other` is set.
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Both sets together.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Without the modifiers of `other`.
    #[must_use]
    pub const fn without(self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }

    /// The names sway reads, one per modifier, in the order people read them: Super, Ctrl,
    /// Alt, Shift, then the rest (`Mod4`, `Ctrl`, `Mod1`, `Shift`, …).
    pub fn config_names(self) -> Vec<&'static str> {
        READING_ORDER
            .iter()
            .filter(|(modifier, _, _)| self.contains(*modifier))
            .map(|(_, name, _)| *name)
            .collect()
    }

    /// The names to show, in the order people read them: `Super`, `Ctrl`, `Alt`, `Shift`, ….
    pub fn labels(self) -> Vec<Text> {
        READING_ORDER
            .iter()
            .filter(|(modifier, _, _)| self.contains(*modifier))
            .map(|(_, _, label)| label.clone())
            .collect()
    }
}

/// The modifiers in the order they are read, each with the name written in a config and the
/// label shown.
static READING_ORDER: std::sync::LazyLock<[(Modifiers, &str, Text); 8]> =
    std::sync::LazyLock::new(|| {
        [
            (Modifiers::SUPER, "Mod4", msg!("Super").into()),
            (Modifiers::CTRL, "Ctrl", msg!("Ctrl").into()),
            (Modifiers::ALT, "Mod1", msg!("Alt").into()),
            (Modifiers::SHIFT, "Shift", msg!("Shift").into()),
            (Modifiers::MOD5, "Mod5", Text::data("Mod5")),
            (Modifiers::MOD3, "Mod3", Text::data("Mod3")),
            (Modifiers::LOCK, "Lock", msg!("Caps Lock").into()),
            (Modifiers::MOD2, "Mod2", msg!("Num Lock").into()),
        ]
    });

/// A key of a combination, as sway compares it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Key {
    /// A keysym of `bindsym`, by its name in lowercase with aliases resolved: `e` for `E`,
    /// `prior` for `Page_Up`. sway looks names up without regard to case.
    Sym(String),
    /// A keycode of `bindcode`, as XKB numbers them (the evdev code plus 8).
    Code(u32),
    /// A mouse button, by its event code (`BTN_LEFT` is 272); the scroll directions of
    /// `button4` to `button7` have codes above every event code, as in sway.
    Button(u32),
    /// A mouse button by an event name this table doesn't know (`BTN_TRIGGER_HAPPY1`).
    ButtonName(String),
}

/// Whether a combination is for the keyboard or the mouse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Device {
    /// Keys of the keyboard.
    Keyboard,
    /// Buttons of the mouse (or a key with `--whole-window`, `--border` or
    /// `--exclude-titlebar`).
    Mouse,
}

/// A key combination as sway reads it: the modifiers, the keys (sorted) and the layout group it
/// is limited to.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Combo {
    /// The modifiers.
    pub modifiers: Modifiers,
    /// The keys, sorted. Empty for a combination of modifiers only, which sway accepts.
    pub keys: Vec<Key>,
    /// The layout group (0 for `Group1`), or `None` for every group.
    pub group: Option<u8>,
    /// Keyboard or mouse.
    pub device: Device,
}

/// Why a key combination can't be read. sway rejects the line with the same reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyError {
    /// More than one `Group` part, or `Mode_switch` with one.
    TwoGroups,
    /// A `Group` part other than `Group1` to `Group4`.
    InvalidGroup(String),
    /// `button0` or `button10`: sway has names for buttons 1 to 9 only.
    InvalidButton(String),
    /// Not a key name: empty, or with characters no keysym has.
    InvalidKey(String),
}

impl KeyError {
    /// What is wrong, to show.
    pub fn text(&self) -> Text {
        match self {
            Self::TwoGroups => msg!("only one layout group can be given").into(),
            Self::InvalidGroup(part) => {
                Text::new(msg!("invalid layout group {group}")).arg("group", part)
            }
            Self::InvalidButton(part) => {
                Text::new(msg!("{button}: only buttons 1 to 9 have names")).arg("button", part)
            }
            Self::InvalidKey(part) => Text::new(msg!("invalid key {key}")).arg("key", part),
        }
    }
}

impl fmt::Display for KeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text().english())
    }
}

impl std::error::Error for KeyError {}

/// Keysym names that stand for the same keysym, in lowercase: the second one is used.
const ALIASES: &[(&str, &str)] = &[
    ("page_up", "prior"),
    ("page_down", "next"),
    ("kp_page_up", "kp_prior"),
    ("kp_page_down", "kp_next"),
    ("script_switch", "mode_switch"),
    ("iso_group_shift", "mode_switch"),
    ("kana_switch", "mode_switch"),
    ("arabic_switch", "mode_switch"),
    ("greek_switch", "mode_switch"),
    ("hebrew_switch", "mode_switch"),
    ("henkan", "henkan_mode"),
    ("quoteleft", "grave"),
    ("quoteright", "apostrophe"),
];

/// The names of the keysyms of printable ASCII by their value, `0x20` to `0x7e`, without the
/// capital letters, which xkbcommon's case-insensitive lookup never gives for a name.
const ASCII: [&str; 95] = [
    "space",
    "exclam",
    "quotedbl",
    "numbersign",
    "dollar",
    "percent",
    "ampersand",
    "apostrophe",
    "parenleft",
    "parenright",
    "asterisk",
    "plus",
    "comma",
    "minus",
    "period",
    "slash",
    "0",
    "1",
    "2",
    "3",
    "4",
    "5",
    "6",
    "7",
    "8",
    "9",
    "colon",
    "semicolon",
    "less",
    "equal",
    "greater",
    "question",
    "at",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "bracketleft",
    "backslash",
    "bracketright",
    "asciicircum",
    "underscore",
    "grave",
    "a",
    "b",
    "c",
    "d",
    "e",
    "f",
    "g",
    "h",
    "i",
    "j",
    "k",
    "l",
    "m",
    "n",
    "o",
    "p",
    "q",
    "r",
    "s",
    "t",
    "u",
    "v",
    "w",
    "x",
    "y",
    "z",
    "braceleft",
    "bar",
    "braceright",
    "asciitilde",
];

/// The keysym value of a numeric name: `0x60` (hexadecimal) or `U0060` (a Unicode code point,
/// which is the keysym itself for printable Latin-1 and `0x1000000` above it otherwise), as
/// xkbcommon reads them.
fn numeric(name: &str) -> Option<u32> {
    let (hex, unicode) = match name.strip_prefix("0x").or_else(|| name.strip_prefix("0X")) {
        Some(hex) => (hex, false),
        None => (
            name.strip_prefix('U').or_else(|| name.strip_prefix('u'))?,
            true,
        ),
    };
    if hex.is_empty() || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let value = u32::from_str_radix(hex, 16).ok()?;
    if !unicode {
        return Some(value);
    }
    Some(match value {
        0x20..=0x7e | 0xa0..=0xff => value,
        _ => value.checked_add(0x0100_0000)?,
    })
}

/// The event codes of the mouse buttons sway's `button1` to `button9` stand for; 4 to 7 scroll.
const X11_BUTTONS: [u32; 9] = [
    0x110,
    0x112,
    0x111,
    SCROLL_UP,
    SCROLL_DOWN,
    SCROLL_LEFT,
    SCROLL_RIGHT,
    0x113,
    0x114,
];

/// sway's codes for the scroll directions, right after `KEY_MAX`.
const SCROLL_UP: u32 = 0x300;
const SCROLL_DOWN: u32 = 0x301;
const SCROLL_LEFT: u32 = 0x302;
const SCROLL_RIGHT: u32 = 0x303;

/// The event names of common mouse buttons and their codes.
const BUTTON_NAMES: &[(&str, u32)] = &[
    ("BTN_LEFT", 0x110),
    ("BTN_RIGHT", 0x111),
    ("BTN_MIDDLE", 0x112),
    ("BTN_SIDE", 0x113),
    ("BTN_EXTRA", 0x114),
    ("BTN_FORWARD", 0x115),
    ("BTN_BACK", 0x116),
    ("BTN_TASK", 0x117),
];

/// Whether the event code `code` is a button, as `libevdev_event_code_get_name` names them
/// `BTN_…`.
fn is_button_code(code: u32) -> bool {
    matches!(code, 0x100..=0x151 | 0x220..=0x223 | 0x2c0..=0x2e7)
}

/// C's `strtol(text, NULL, 10)` for what sway reads as a keycode: leading blanks, a sign, then
/// digits; 0 if there are none.
fn strtol(text: &str) -> i64 {
    let text = text.trim_start();
    let (negative, digits) = match text.as_bytes().first() {
        Some(b'-') => (true, &text[1..]),
        Some(b'+') => (false, &text[1..]),
        _ => (false, text),
    };
    let mut value: i64 = 0;
    for byte in digits.bytes().take_while(u8::is_ascii_digit) {
        value = value
            .saturating_mul(10)
            .saturating_add(i64::from(byte - b'0'));
    }
    if negative { -value } else { value }
}

/// The canonical name of a keysym: lowercase, with aliases resolved; a numeric name (`0x60`,
/// `U0060`) as the name of its printable ASCII keysym, or as `0x…` in lowercase.
pub fn keysym_key(name: &str) -> String {
    if let Some(value) = numeric(name) {
        let ascii = value
            .checked_sub(0x20)
            .and_then(|index| ASCII.get(usize::try_from(index).ok()?))
            .filter(|name| !name.is_empty());
        return ascii.map_or_else(|| format!("0x{value:x}"), |name| (*name).to_owned());
    }
    let lower = name.to_ascii_lowercase();
    ALIASES
        .iter()
        .find(|(alias, _)| *alias == lower)
        .map_or(lower, |(_, canonical)| (*canonical).to_owned())
}

/// Whether `part` can be the name of a keysym: ASCII letters, digits and `_`, as every name in
/// xkbcommon's table, or a number (`0x1008ff13`, `U20AC`).
fn is_keysym_name(part: &str) -> bool {
    !part.is_empty()
        && part
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

/// Reads the key combination `text` of a `bindsym` (`code` false) or `bindcode` (`code` true).
/// `mouse` is true for a binding with `--whole-window`, `--border` or `--exclude-titlebar`,
/// which sway takes as a mouse binding.
///
/// # Errors
///
/// A [`KeyError`] for what sway rejects too: two layout groups, a group outside 1 to 4, a
/// button name sway doesn't know. A keysym name with characters no keysym has is refused here;
/// whether xkbcommon knows a name is up to sway (`sway --validate` says so).
pub fn parse(text: &str, code: bool, mouse: bool) -> Result<Combo, KeyError> {
    let mut combo = Combo {
        modifiers: Modifiers::NONE,
        keys: Vec::new(),
        group: None,
        device: if mouse {
            Device::Mouse
        } else {
            Device::Keyboard
        },
    };
    for part in text.split('+').filter(|part| !part.is_empty()) {
        if let Some(number) = part.strip_prefix("Group") {
            if combo.group.is_some() {
                return Err(KeyError::TwoGroups);
            }
            match number.parse::<u8>() {
                Ok(group @ 1..=4) => combo.group = Some(group - 1),
                _ => return Err(KeyError::InvalidGroup(part.to_owned())),
            }
            continue;
        }
        if part == "Mode_switch" {
            if combo.group.is_some() {
                return Err(KeyError::TwoGroups);
            }
            combo.group = Some(1);
        }
        if let Some(modifier) = Modifiers::from_name(part) {
            combo.modifiers = combo.modifiers.union(modifier);
            continue;
        }
        let first = combo.keys.is_empty();
        let key = if code {
            code_key(part, first, &mut combo.device)
        } else {
            sym_key(part, first, &mut combo.device)?
        };
        combo.keys.push(key);
    }
    combo.keys.sort();
    Ok(combo)
}

/// A key of `bindcode`: on the first key, a number that is a button's event code makes it a
/// mouse binding.
fn code_key(part: &str, first: bool, device: &mut Device) -> Key {
    let number = strtol(part);
    let code = u32::try_from(number).unwrap_or(0);
    if *device == Device::Mouse || first && is_button_code(code) && number > 0 {
        *device = Device::Mouse;
        return Key::Button(code);
    }
    Key::Code(code)
}

/// A key of `bindsym`: on the first key, a button name (`button3`, `BTN_LEFT`) makes it a mouse
/// binding.
fn sym_key(part: &str, first: bool, device: &mut Device) -> Result<Key, KeyError> {
    if first || *device == Device::Mouse {
        if let Some(button) = mouse_button(part)? {
            *device = Device::Mouse;
            return Ok(button);
        }
    }
    if !is_keysym_name(part) {
        return Err(KeyError::InvalidKey(part.to_owned()));
    }
    Ok(Key::Sym(keysym_key(part)))
}

/// `get_mouse_bindsym`: `button1` to `button9` (without regard to case) or an event name
/// `BTN_…`; `None` if `part` is neither.
fn mouse_button(part: &str) -> Result<Option<Key>, KeyError> {
    if part.len() >= 6 && part[..6].eq_ignore_ascii_case("button") {
        let digit = part.as_bytes().get(6).copied().unwrap_or(0);
        return match digit {
            b'1'..=b'9' if part.len() == 7 => {
                let index = usize::from(digit - b'1');
                Ok(X11_BUTTONS.get(index).map(|&code| Key::Button(code)))
            }
            _ => Err(KeyError::InvalidButton(part.to_owned())),
        };
    }
    if part.starts_with("BTN_") {
        return Ok(Some(
            BUTTON_NAMES
                .iter()
                .find(|(name, _)| *name == part)
                .map_or_else(
                    || Key::ButtonName(part.to_owned()),
                    |&(_, code)| Key::Button(code),
                ),
        ));
    }
    Ok(None)
}

/// Whether a binding of `combo` leaves the keys it is on usable for typing: it has Super, Ctrl,
/// Alt or `Mod3`, or its keys are ones nobody types with (function keys, media keys, Print,
/// Pause, …). A letter or Enter bound alone, or with only Shift or `AltGr` (`Mod5`), which type
/// other characters, would no longer reach any app.
pub fn leaves_typing(combo: &Combo) -> bool {
    let typing = Modifiers::SHIFT
        .union(Modifiers::LOCK)
        .union(Modifiers::MOD2)
        .union(Modifiers::MOD5);
    if !combo.modifiers.without(typing).is_empty() || combo.device == Device::Mouse {
        return true;
    }
    !combo.keys.is_empty()
        && combo.keys.iter().all(|key| match key {
            Key::Sym(name) => {
                name.starts_with("xf86")
                    || name.strip_prefix('f').is_some_and(|number| {
                        !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit())
                    })
                    || matches!(
                        name.as_str(),
                        "print"
                            | "pause"
                            | "break"
                            | "scroll_lock"
                            | "sys_req"
                            | "menu"
                            | "cancel"
                            | "help"
                    )
            }
            _ => false,
        })
}

/// The variable of the main modifier in the combinations Sway Things stores; it stands for
/// the modifier chosen on the Keyboard Shortcuts page.
pub const MOD: &str = "$mod";

/// Whether `text` can be written as the key combination of a generated line: parts of ASCII
/// letters, digits and `_` (or [`MOD`]) joined by `+`, so it needs no quotes and holds no `$`
/// sway would replace.
pub fn is_plain(text: &str) -> bool {
    !text.is_empty()
        && text
            .split('+')
            .all(|part| part == MOD || is_keysym_name(part))
}

/// `text` with [`MOD`] replaced by the name of `modifier` (`Mod4`).
pub fn with_mod(text: &str, modifier: Modifiers) -> String {
    let name = modifier.config_names().first().copied().unwrap_or("Mod4");
    text.split('+')
        .map(|part| if part == MOD { name } else { part })
        .collect::<Vec<_>>()
        .join("+")
}

/// The names of the keys of `text` to show, as people read them: `E` for `e`, `Enter` for
/// `Return`, `Volume Up` for `XF86AudioRaiseVolume`. Modifiers and layout groups are left out;
/// `code` reads the keys as keycodes, which show as numbers unless `keysym_of` names them (the
/// keymap of the app's keyboard).
pub fn key_labels(text: &str, code: bool, keysym_of: &dyn Fn(u32) -> Option<String>) -> Vec<Text> {
    text.split('+')
        .filter(|part| !part.is_empty())
        .filter(|part| Modifiers::from_name(part).is_none() && !part.starts_with("Group"))
        .map(|part| {
            if code {
                let number = strtol(part);
                if let Some(button) = u32::try_from(number).ok().filter(|&c| is_button_code(c)) {
                    return button_label(button);
                }
                match u32::try_from(number).ok().and_then(keysym_of) {
                    Some(name) => key_label(&name),
                    None => Text::new(msg!("key {code}")).arg("code", number),
                }
            } else {
                key_label(part)
            }
        })
        .collect()
}

/// The label of a mouse button by its event code.
fn button_label(code: u32) -> Text {
    match code {
        0x110 => msg!("Left Button").into(),
        0x111 => msg!("Right Button").into(),
        0x112 => msg!("Middle Button").into(),
        0x113 => msg!("Back Button").into(),
        0x114 => msg!("Forward Button").into(),
        SCROLL_UP => msg!("Scroll Up").into(),
        SCROLL_DOWN => msg!("Scroll Down").into(),
        SCROLL_LEFT => msg!("Scroll Left").into(),
        SCROLL_RIGHT => msg!("Scroll Right").into(),
        other => Text::new(msg!("Button {code}")).arg("code", other),
    }
}

/// The label of the key `name` (a keysym or button name).
pub fn key_label(name: &str) -> Text {
    let lower = name.to_ascii_lowercase();
    if let Some(Key::Button(code)) = mouse_button(name).ok().flatten() {
        return button_label(code);
    }
    if let Some(text) = named_key(&keysym_key(&lower)) {
        return text;
    }
    // A letter as it is printed on the key.
    if name.chars().count() == 1 {
        return Text::data(name.to_uppercase());
    }
    let shown = name
        .strip_prefix("XF86")
        .or_else(|| name.strip_prefix("KP_"))
        .unwrap_or(name)
        .replace('_', " ");
    if name.starts_with("KP_") {
        return Text::new(msg!("Keypad {key}")).arg("key", shown);
    }
    Text::data(shown)
}

/// The label of a key with a name of its own, by its canonical keysym name.
fn named_key(canonical: &str) -> Option<Text> {
    let label = match canonical {
        "return" => msg!("Enter"),
        "space" => msg!("Space"),
        "escape" => msg!("Esc"),
        "backspace" => msg!("Backspace"),
        "delete" => msg!("Delete"),
        "insert" => msg!("Insert"),
        "tab" => msg!("Tab"),
        "home" => msg!("Home"),
        "end" => msg!("End"),
        "prior" => msg!("Page Up"),
        "next" => msg!("Page Down"),
        "left" => msg!("Left"),
        "right" => msg!("Right"),
        "up" => msg!("Up"),
        "down" => msg!("Down"),
        "print" => msg!("Print"),
        "xf86audioraisevolume" => msg!("Volume Up"),
        "xf86audiolowervolume" => msg!("Volume Down"),
        "xf86audiomute" => msg!("Mute"),
        "xf86audiomicmute" => msg!("Microphone Mute"),
        "xf86monbrightnessup" => msg!("Brightness Up"),
        "xf86monbrightnessdown" => msg!("Brightness Down"),
        "xf86audioplay" => msg!("Play"),
        "xf86audiopause" => msg!("Pause"),
        "xf86audiostop" => msg!("Stop"),
        "xf86audionext" => msg!("Next Track"),
        "xf86audioprev" => msg!("Previous Track"),
        _ => return symbol(canonical).map(Text::data),
    };
    Some(label.into())
}

/// The character a punctuation keysym types.
fn symbol(canonical: &str) -> Option<&'static str> {
    Some(match canonical {
        "minus" => "-",
        "equal" => "=",
        "plus" => "+",
        "comma" => ",",
        "period" => ".",
        "slash" => "/",
        "backslash" => "\\",
        "semicolon" => ";",
        "apostrophe" => "'",
        "grave" => "`",
        "bracketleft" => "[",
        "bracketright" => "]",
        "exclam" => "!",
        "asterisk" => "*",
        _ => return None,
    })
}
