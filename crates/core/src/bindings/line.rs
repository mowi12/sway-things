// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! One `bindsym`, `bindcode`, `unbindsym` or `unbindcode`, as sway reads it, and how it is
//! written back.

use std::fmt;

use super::command::{BindCommand, CommandError};
use super::keys::{self, Combo, Device, Key, KeyError, Modifiers};
use crate::i18n::Text;
use crate::msg;
use crate::sway_config::line::strip_quotes;

/// The options of a binding (`--locked`, `--to-code`, …).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "each is one option of sway's, set or not"
)]
pub struct Flags {
    /// `--release`: runs when the key is released.
    pub release: bool,
    /// `--locked`: also runs while the screen is locked.
    pub locked: bool,
    /// `--inhibited`: also runs while a window inhibits shortcuts.
    pub inhibited: bool,
    /// `--to-code`: the keysyms are translated to the keycodes of the first layout, so the
    /// binding works on the same keys in every layout.
    pub to_code: bool,
    /// `--no-repeat`: doesn't run again while the key is held.
    pub no_repeat: bool,
    /// `--no-warn`: no warning when it replaces a binding of the same keys.
    pub no_warn: bool,
    /// `--whole-window`: a mouse binding over the whole window.
    pub whole_window: bool,
    /// `--border`: a mouse binding over the border.
    pub border: bool,
    /// `--exclude-titlebar`: a mouse binding that leaves out the title bar.
    pub exclude_titlebar: bool,
}

impl Flags {
    /// The options, as written in a config, in a fixed order.
    pub fn words(self) -> Vec<&'static str> {
        [
            (self.whole_window, "--whole-window"),
            (self.border, "--border"),
            (self.exclude_titlebar, "--exclude-titlebar"),
            (self.release, "--release"),
            (self.locked, "--locked"),
            (self.inhibited, "--inhibited"),
            (self.to_code, "--to-code"),
            (self.no_repeat, "--no-repeat"),
            (self.no_warn, "--no-warn"),
        ]
        .into_iter()
        .filter_map(|(set, word)| set.then_some(word))
        .collect()
    }

    /// Whether this is a mouse binding by its options.
    fn mouse(self) -> bool {
        self.whole_window || self.border || self.exclude_titlebar
    }
}

/// A binding line: what to bind or unbind, and for a binding the command.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Line {
    /// `unbindsym` or `unbindcode`.
    pub unbind: bool,
    /// `bindcode` or `unbindcode`: the keys are keycodes.
    pub code: bool,
    /// The options.
    pub flags: Flags,
    /// `--input-device=<identifier>`: only for this device; `None` for every device.
    pub input: Option<String>,
    /// The keys as written, with variables replaced (`Mod4+Shift+e`).
    pub keys: String,
    /// The keys as sway compares them.
    pub combo: Combo,
    /// What runs; `None` for an unbind.
    pub command: Option<String>,
}

/// Why a binding line can't be read, or can't be written by Sway Things.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LineError {
    /// It isn't `bindsym`, `bindcode`, `unbindsym` or `unbindcode`.
    NotABinding,
    /// The keys are missing, or the command of a binding.
    Missing,
    /// The keys can't be read.
    Keys(KeyError),
    /// The keys or the device can't be written without quotes.
    NotPlain,
    /// The command can't be written back exactly.
    Command(CommandError),
}

/// What is wrong with a binding line, without the details: for
/// [`ValueError`](crate::registry::ValueError), which is `Copy`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LineErrorKind {
    /// See [`LineError::NotABinding`].
    NotABinding,
    /// See [`LineError::Missing`].
    Missing,
    /// See [`LineError::Keys`].
    Keys,
    /// See [`LineError::NotPlain`].
    NotPlain,
    /// See [`LineError::Command`].
    Command,
}

impl LineErrorKind {
    /// What is wrong, to show.
    pub fn text(self) -> Text {
        match self {
            Self::NotABinding => LineError::NotABinding.text(),
            Self::Missing => LineError::Missing.text(),
            Self::Keys => msg!("the keys can't be read").into(),
            Self::NotPlain => LineError::NotPlain.text(),
            Self::Command => msg!(
                "the command can't be empty, end with a brace or contain a backslash or line break"
            )
            .into(),
        }
    }
}

impl LineError {
    /// What is wrong, without the details.
    pub fn kind(&self) -> LineErrorKind {
        match self {
            Self::NotABinding => LineErrorKind::NotABinding,
            Self::Missing => LineErrorKind::Missing,
            Self::Keys(_) => LineErrorKind::Keys,
            Self::NotPlain => LineErrorKind::NotPlain,
            Self::Command(_) => LineErrorKind::Command,
        }
    }

    /// What is wrong, to show.
    pub fn text(&self) -> Text {
        match self {
            Self::NotABinding => {
                msg!("must start with bindsym, bindcode, unbindsym or unbindcode").into()
            }
            Self::Missing => msg!("the keys or the command are missing").into(),
            Self::Keys(error) => error.text(),
            Self::NotPlain => {
                msg!("keys and devices can only have letters, digits, _ and the + between keys")
                    .into()
            }
            Self::Command(error) => error.text(),
        }
    }
}

impl fmt::Display for LineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text().english())
    }
}

impl std::error::Error for LineError {}

impl From<KeyError> for LineError {
    fn from(error: KeyError) -> Self {
        Self::Keys(error)
    }
}

impl From<CommandError> for LineError {
    fn from(error: CommandError) -> Self {
        Self::Command(error)
    }
}

impl Line {
    /// Reads the binding command `name` (any case) with the arguments `args` as sway's handler
    /// gets them: options, keys, and the words of the command.
    ///
    /// # Errors
    ///
    /// [`LineError::NotABinding`] for another command; [`LineError::Missing`] and
    /// [`LineError::Keys`] for a line sway rejects too.
    pub fn read(name: &str, args: &[String]) -> Result<Self, LineError> {
        let (unbind, code) = match name.to_ascii_lowercase().as_str() {
            "bindsym" => (false, false),
            "bindcode" => (false, true),
            "unbindsym" => (true, false),
            "unbindcode" => (true, true),
            _ => return Err(LineError::NotABinding),
        };
        let mut flags = Flags::default();
        let mut input = None;
        let start = keys_index(args);
        for word in &args[..start] {
            match word.as_str() {
                "--release" => flags.release = true,
                "--locked" => flags.locked = true,
                "--inhibited" => flags.inhibited = true,
                "--whole-window" => flags.whole_window = true,
                "--border" => flags.border = true,
                // sway ignores it for bindcode.
                "--to-code" => flags.to_code = !code,
                "--exclude-titlebar" => flags.exclude_titlebar = true,
                "--no-warn" => flags.no_warn = true,
                "--no-repeat" => flags.no_repeat = true,
                _ => {
                    if let Some(device) = word.strip_prefix("--input-device=") {
                        let device = strip_quotes(device.as_bytes());
                        input = Some(String::from_utf8_lossy(&device).into_owned());
                    }
                }
            }
        }
        let Some((keys, command)) = args[start..].split_first() else {
            return Err(LineError::Missing);
        };
        if !unbind && command.is_empty() {
            return Err(LineError::Missing);
        }
        let combo = keys::parse(keys, code, flags.mouse())?;
        Ok(Self {
            unbind,
            code,
            flags,
            input: input.filter(|device| device != "*"),
            keys: keys.clone(),
            combo,
            command: (!unbind).then(|| command.join(" ")),
        })
    }

    /// The command name: `bindsym`, `bindcode`, `unbindsym` or `unbindcode`.
    pub fn verb(&self) -> &'static str {
        match (self.unbind, self.code) {
            (false, false) => "bindsym",
            (false, true) => "bindcode",
            (true, false) => "unbindsym",
            (true, true) => "unbindcode",
        }
    }

    /// What makes this binding the same as another one in sway's eyes (`binding_key_compare`),
    /// in the mode `mode`. `keymap` translates the keysyms of a `--to-code` binding into
    /// keycodes, as sway does with the first layout; without it they stay keysyms.
    pub fn identity(&self, mode: &str, keymap: Option<&dyn Keymap>) -> Identity {
        let mut combo = self.combo.clone();
        let mut code = self.code;
        if self.flags.to_code && !self.code && combo.device == Device::Keyboard {
            if let Some(keymap) = keymap {
                let codes: Option<Vec<Key>> = combo
                    .keys
                    .iter()
                    .map(|key| match key {
                        Key::Sym(name) => keymap.keycode(name).map(Key::Code),
                        other => Some(other.clone()),
                    })
                    .collect();
                if let Some(mut codes) = codes {
                    codes.sort();
                    combo.keys = codes;
                    code = true;
                }
            }
        }
        let kind = match combo.device {
            Device::Mouse => Kind::Mouse,
            Device::Keyboard if code => Kind::Code,
            Device::Keyboard => Kind::Sym,
        };
        // sway's BINDING_BORDER, CONTENTS and TITLEBAR: a mouse binding is on the title bar
        // unless it leaves it out.
        let mut region = 0u8;
        if self.flags.whole_window {
            region |= 0b111;
        }
        if self.flags.border {
            region |= 0b001;
        }
        if self.flags.exclude_titlebar {
            region &= !0b100;
        } else if kind == Kind::Mouse {
            region |= 0b100;
        }
        Identity {
            mode: mode.to_owned(),
            input: self.input.clone().unwrap_or_else(|| "*".to_owned()),
            kind,
            release: self.flags.release,
            locked: self.flags.locked,
            inhibited: self.flags.inhibited,
            region,
            group: combo.group,
            modifiers: combo.modifiers,
            keys: combo.keys,
        }
    }

    /// The line that undoes this binding in the same place: an unbind with the options that
    /// make it the same binding (not `--no-repeat` or `--no-warn`).
    #[must_use]
    pub fn unbinding(&self) -> Self {
        Self {
            unbind: true,
            flags: Flags {
                no_repeat: false,
                no_warn: false,
                ..self.flags
            },
            command: None,
            ..self.clone()
        }
    }

    /// The words of the options, the device and the keys, as written in a config line or an
    /// IPC message, with [`keys::MOD`] standing for `modifier`.
    ///
    /// # Errors
    ///
    /// [`LineError::NotPlain`] if the keys or the device would need quotes.
    fn head(&self, modifier: Modifiers, no_warn: bool) -> Result<Vec<String>, LineError> {
        if !keys::is_plain(&self.keys) {
            return Err(LineError::NotPlain);
        }
        let mut words = vec![self.verb().to_owned()];
        let flags = Flags {
            no_warn: (self.flags.no_warn || no_warn) && !self.unbind,
            ..self.flags
        };
        words.extend(flags.words().into_iter().map(str::to_owned));
        if let Some(device) = &self.input {
            if !is_device(device) {
                return Err(LineError::NotPlain);
            }
            words.push(format!("--input-device={device}"));
        }
        words.push(keys::with_mod(&self.keys, modifier));
        Ok(words)
    }

    /// The line for a generated config file. `no_warn` adds `--no-warn` to a binding.
    ///
    /// # Errors
    ///
    /// [`LineError::NotPlain`] for keys or a device that need quotes, and
    /// [`LineError::Command`] for a command that can't be written back exactly.
    pub fn config(&self, modifier: Modifiers, no_warn: bool) -> Result<String, LineError> {
        let mut words = self.head(modifier, no_warn)?;
        if let Some(command) = &self.command {
            words.push(BindCommand::new(command)?.config_text());
        }
        Ok(words.join(" "))
    }

    /// The IPC message that makes the same change in a running sway. `no_warn` adds
    /// `--no-warn` to a binding.
    ///
    /// # Errors
    ///
    /// Like [`Line::config`].
    pub fn ipc(&self, modifier: Modifiers, no_warn: bool) -> Result<String, LineError> {
        let mut words = self.head(modifier, no_warn)?;
        if let Some(command) = &self.command {
            words.push(BindCommand::new(command)?.ipc_text());
        }
        Ok(words.join(" "))
    }

    /// The labels of the keys, modifiers first, as people read them (`Super`, `Shift`, `E`).
    /// [`keys::MOD`] stands for `modifier`; `keymap` names the keys of a `bindcode`.
    pub fn labels(&self, modifier: Modifiers, keymap: Option<&dyn Keymap>) -> Vec<Text> {
        let keys = keys::with_mod(&self.keys, modifier);
        let modifiers = keys
            .split('+')
            .filter_map(Modifiers::from_name)
            .fold(Modifiers::NONE, Modifiers::union);
        let keysym_of = |code: u32| keymap.and_then(|keymap| keymap.keysym(code));
        let mut labels = modifiers.labels();
        labels.extend(keys::key_labels(&keys, self.code, &keysym_of));
        labels
    }
}

/// The options sway reads before the keys of a binding line.
const OPTIONS: &[&str] = &[
    "--release",
    "--locked",
    "--inhibited",
    "--whole-window",
    "--border",
    "--to-code",
    "--exclude-titlebar",
    "--no-warn",
    "--no-repeat",
];

/// The index of the keys in the arguments `args` of a binding line: sway reads options until a
/// word isn't one.
pub(crate) fn keys_index(args: &[String]) -> usize {
    args.iter()
        .take_while(|word| OPTIONS.contains(&word.as_str()) || word.starts_with("--input-device="))
        .count()
}

/// Whether `device` can follow `--input-device=` without quotes.
fn is_device(device: &str) -> bool {
    !device.is_empty()
        && device
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b':' | b'.' | b'-'))
}

/// The keymap of the keyboard: which keycodes give which keysyms, for `bindcode` and
/// `--to-code`. The app asks GTK; without one, keysyms and keycodes aren't compared.
pub trait Keymap {
    /// The keycode (XKB numbering) whose first level gives the keysym `name` (a canonical
    /// name, see [`keys::keysym_key`]), if exactly one key gives it.
    fn keycode(&self, name: &str) -> Option<u32>;

    /// The name of the keysym of the first level of keycode `code`.
    fn keysym(&self, code: u32) -> Option<String>;
}

/// The kind of keys a binding is on, as sway keeps them in separate lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Kind {
    /// Keysyms (`bindsym`).
    Sym,
    /// Keycodes (`bindcode`, or `bindsym --to-code` translated).
    Code,
    /// Mouse buttons.
    Mouse,
}

/// What makes two bindings the same binding for sway: a later one replaces an earlier one with
/// the same identity, and an unbind removes it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Identity {
    /// The binding mode, `default` outside `mode` blocks.
    pub mode: String,
    /// The input device, `*` for every one.
    pub input: String,
    /// The kind of keys.
    pub kind: Kind,
    /// `--release`.
    pub release: bool,
    /// `--locked`.
    pub locked: bool,
    /// `--inhibited`.
    pub inhibited: bool,
    /// The parts of the window a mouse binding is on.
    pub region: u8,
    /// The layout group.
    pub group: Option<u8>,
    /// The modifiers.
    pub modifiers: Modifiers,
    /// The keys, sorted.
    pub keys: Vec<Key>,
}

impl Identity {
    /// Whether `self` and `other` are on the same keys in the same mode but differ only in
    /// `--locked`: while the screen isn't locked, sway runs the one without it.
    pub fn differs_in_lock_only(&self, other: &Self) -> bool {
        self.locked != other.locked
            && Self {
                locked: other.locked,
                ..self.clone()
            } == *other
    }
}
