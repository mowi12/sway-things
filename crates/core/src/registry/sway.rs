// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! How a setting becomes sway commands.

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

use super::Registry;
use super::read::ReadFn;
use super::setting::Scope;
use super::value::{Kind, Value, ValueError};
use crate::command::input::{Input, InputError, InputSelector};
use crate::command::output::{Output, OutputError, OutputSelector};
use crate::command::window::WindowError;
use crate::command::{Color, CommandError, OutOfRange, SwayCmd, Target};
use crate::i18n::Text;
use crate::msg;
use crate::sway_config;

/// Where a directive stands in a sway config.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Block {
    /// At the top level, e.g. `focus_follows_mouse`.
    Top,
    /// In an `input <identifier> { … }` block, or after `input <identifier>` on one line. Which
    /// identifiers count follows from the setting's [`Scope::PerDevice`] and the selected device.
    Input,
    /// In a `seat <name> { … }` block, or after `seat <name>` on one line. Sway Things writes
    /// `seat *`; `*` and the default seat, `seat0`, count.
    Seat,
    /// After `output <name>`, in a block or on one line. sway reads several settings from one
    /// line (`output DP-4 mode 2560x1440 pos 0 0`); each is a directive of its own here.
    Output,
}

/// The sway directive a setting maps to. Conflict detection looks for it in the user's config, and
/// settings with the same directive are values of one command and render together.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Directive {
    /// Where the directive stands.
    pub block: Block,
    /// The directive, e.g. `tap`, `gaps` or `client.focused`.
    pub name: &'static str,
    /// The leading arguments, where one name covers several independent values: `inner` for
    /// `gaps inner <px>`, `[all]` and `inhibit_idle` for `for_window [all] inhibit_idle <mode>`.
    pub arguments: &'static [&'static str],
    /// First arguments after the leading ones that make the line another directive: `hide_cursor
    /// <ms>` isn't `hide_cursor when-typing <on/off>`.
    pub except: &'static [&'static str],
}

impl Directive {
    /// A top-level directive.
    pub const fn top(name: &'static str) -> Self {
        Self {
            block: Block::Top,
            name,
            arguments: &[],
            except: &[],
        }
    }

    /// An option of `input` blocks, e.g. `tap`.
    pub const fn input(name: &'static str) -> Self {
        Self {
            block: Block::Input,
            name,
            arguments: &[],
            except: &[],
        }
    }

    /// An option of `seat` blocks, e.g. `xcursor_theme`.
    pub const fn seat(name: &'static str) -> Self {
        Self {
            block: Block::Seat,
            name,
            arguments: &[],
            except: &[],
        }
    }

    /// A setting of `output` commands, e.g. `scale`.
    pub const fn output(name: &'static str) -> Self {
        Self {
            block: Block::Output,
            name,
            arguments: &[],
            except: &[],
        }
    }

    /// Narrows the directive to those that start with these arguments.
    #[must_use]
    pub const fn arguments(mut self, arguments: &'static [&'static str]) -> Self {
        self.arguments = arguments;
        self
    }

    /// Leaves out the lines whose first argument after the leading ones is one of these.
    #[must_use]
    pub const fn except(mut self, except: &'static [&'static str]) -> Self {
        self.except = except;
        self
    }
}

/// The generated sway config file a setting's lines go to, in
/// `$XDG_CONFIG_HOME/sway/sway-things/`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ConfigFile {
    /// `10-input.conf`
    Input,
    /// `20-output.conf`
    Output,
    /// `30-appearance.conf`
    Appearance,
    /// `40-behavior.conf`
    Behavior,
    /// `50-bindings.conf`: the key bindings of the Keyboard Shortcuts page.
    Bindings,
    /// `70-autostart.conf`: the autostart entries of the Apps & Autostart page and the runner of
    /// the XDG autostart entries.
    Autostart,
    /// `75-bar.conf`: what starts the bar: swaybar's `bar` lines, or the hook that starts
    /// waybar with the config and stylesheet of Sway Things.
    Bar,
    /// `80-hooks.conf`: the lines that run hooks of the command line, `sway-things hook …`, and
    /// the notification daemon Sway Things starts.
    Hooks,
    /// `90-effects.conf`: SwayFX directives only, written only while SwayFX runs.
    Effects,
}

impl ConfigFile {
    /// Every generated file, in the order of their names, which is the order sway reads them in.
    pub const ALL: [Self; 9] = [
        Self::Input,
        Self::Output,
        Self::Appearance,
        Self::Behavior,
        Self::Bindings,
        Self::Autostart,
        Self::Bar,
        Self::Hooks,
        Self::Effects,
    ];

    /// The file name.
    pub const fn file_name(self) -> &'static str {
        match self {
            Self::Input => "10-input.conf",
            Self::Output => "20-output.conf",
            Self::Appearance => "30-appearance.conf",
            Self::Behavior => "40-behavior.conf",
            Self::Bindings => "50-bindings.conf",
            Self::Autostart => "70-autostart.conf",
            Self::Bar => "75-bar.conf",
            Self::Hooks => "80-hooks.conf",
            Self::Effects => "90-effects.conf",
        }
    }
}

/// Builds the commands of a directive from the values in the [`Context`].
pub type RenderFn = fn(&Context<'_>) -> Result<Vec<SwayCmd>, RenderError>;

/// How a setting maps to sway.
#[derive(Debug, Clone, Copy)]
pub struct SwayMapping {
    pub(super) directive: Directive,
    pub(super) file: ConfigFile,
    pub(super) render: RenderFn,
    pub(super) read: ReadFn,
}

impl SwayMapping {
    /// The directive, for conflict detection and for grouping settings of one command.
    pub fn directive(&self) -> Directive {
        self.directive
    }

    /// The generated file the config lines go to.
    pub fn file(&self) -> ConfigFile {
        self.file
    }

    /// Whether a directive of the user's config is this setting's directive, and if so, the
    /// arguments its value is read from: those after the leading arguments the directive names.
    /// Like sway, names, the `input`, `seat` and `output` blocks and the leading arguments match
    /// without regard to case.
    /// Which `input` identifiers, seat names and output names count is up to the caller.
    pub fn matches<'a>(&self, directive: &'a sway_config::Directive) -> Option<&'a [String]> {
        let block = match self.directive.block {
            Block::Top => directive.block.is_empty(),
            Block::Input => is_block(directive, "input"),
            Block::Seat => is_block(directive, "seat"),
            Block::Output => is_block(directive, "output"),
        };
        if !block || !directive.name.eq_ignore_ascii_case(self.directive.name) {
            return None;
        }
        let leading = self.directive.arguments;
        let rest = directive.args.get(leading.len()..)?;
        let excluded = rest.first().is_some_and(|first| {
            self.directive
                .except
                .iter()
                .any(|word| first.eq_ignore_ascii_case(word))
        });
        (!excluded
            && leading
                .iter()
                .zip(&directive.args)
                .all(|(expected, arg)| arg.eq_ignore_ascii_case(expected)))
        .then_some(rest)
    }
}

/// Whether `directive` is in an `<kind> <identifier>` block, e.g. `input type:touchpad`.
fn is_block(directive: &sway_config::Directive, kind: &str) -> bool {
    directive.block.len() == 2
        && directive
            .block
            .first()
            .is_some_and(|word| word.eq_ignore_ascii_case(kind))
}

/// The stored values a setting is rendered from. A key that isn't stored renders with its default.
pub trait Values {
    /// The stored value of `key`, if any.
    fn get(&self, key: &str) -> Option<&Value>;

    /// The current picture of the wallpaper collection of `output` (`None`: every output) as a
    /// `bg` line names it: the link in the state folder that points at it
    /// ([`Paths::wallpaper_picture`](crate::paths::Paths::wallpaper_picture)). `None` while it
    /// points at no picture, and for values that don't know where the links are; the color then
    /// fills the screen.
    fn wallpaper_picture(&self, output: Option<&str>) -> Option<PathBuf> {
        let _ = output;
        None
    }
}

/// No stored values: everything renders with its default.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Defaults;

impl Values for Defaults {
    fn get(&self, _key: &str) -> Option<&Value> {
        None
    }
}

impl Values for BTreeMap<String, Value> {
    fn get(&self, key: &str) -> Option<&Value> {
        BTreeMap::get(self, key)
    }
}

/// What a [`RenderFn`] reads: the values of its settings, the selected device and the target.
#[derive(Clone, Copy)]
pub struct Context<'a> {
    pub(super) registry: &'a Registry,
    pub(super) values: &'a dyn Values,
    pub(super) scope: Scope,
    /// The input device or output, by its identifier; already checked against the scope.
    pub(super) device: Option<&'a str>,
    pub(super) target: Target,
}

impl Context<'_> {
    /// Where the commands go. A setting can need other commands over IPC than in a config file.
    pub fn target(&self) -> Target {
        self.target
    }

    /// The input device, output or collection the setting is rendered for; `None` for all.
    pub fn instance(&self) -> Option<&str> {
        self.device
    }

    /// The current picture of the wallpaper collection of the output the setting is rendered
    /// for, as a `bg` line names it ([`Values::wallpaper_picture`]).
    pub fn wallpaper_picture(&self) -> Option<PathBuf> {
        self.values.wallpaper_picture(self.device)
    }

    /// The `input` command builder for the devices the setting is rendered for: the selected
    /// device, or every device of the setting's type.
    ///
    /// # Errors
    ///
    /// [`RenderError::NotPerDevice`] if the setting's scope isn't [`Scope::PerDevice`].
    pub fn input(&self) -> Result<Input, RenderError> {
        let Scope::PerDevice(input_type) = self.scope else {
            return Err(RenderError::NotPerDevice);
        };
        let selector = match self.device {
            Some(device) => InputSelector::device(device)?,
            None => InputSelector::Type(input_type),
        };
        Ok(Input::new(selector))
    }

    /// The `output` command builder for the outputs the setting is rendered for: the selected
    /// output, or every output (`*`).
    ///
    /// # Errors
    ///
    /// [`RenderError::NotPerDevice`] if the setting's scope isn't [`Scope::PerOutput`].
    pub fn output(&self) -> Result<Output, RenderError> {
        if self.scope != Scope::PerOutput {
            return Err(RenderError::NotPerDevice);
        }
        let selector = match self.device {
            Some(output) => OutputSelector::output(output)?,
            None => OutputSelector::All,
        };
        Ok(Output::new(selector))
    }

    /// Whether a value is stored for `key`, rather than its default used. A setting whose default
    /// means "sway decides" (an output's automatic position) renders no command then.
    pub fn is_stored(&self, key: &str) -> bool {
        self.values.get(key).is_some()
    }

    /// The checked value of `key`: the stored one, or the default.
    fn value(&self, key: &'static str) -> Result<(&Kind, Value), RenderError> {
        let setting = self
            .registry
            .get(key)
            .ok_or(RenderError::UnknownKey { key })?;
        let value = match self.values.get(key) {
            Some(value) => {
                setting
                    .check(value)
                    .map_err(|error| RenderError::InvalidValue { key, error })?;
                value.clone()
            }
            None => setting.default_value(),
        };
        Ok((setting.kind(), value))
    }

    fn wrong_type(key: &'static str, expected: &'static str, kind: &Kind) -> RenderError {
        RenderError::InvalidValue {
            key,
            error: ValueError::WrongType {
                expected,
                found: kind.type_name(),
            },
        }
    }

    /// The value of the on/off setting `key`.
    ///
    /// # Errors
    ///
    /// [`RenderError::UnknownKey`] or [`RenderError::InvalidValue`]; the same goes for the other
    /// getters.
    pub fn bool(&self, key: &'static str) -> Result<bool, RenderError> {
        match self.value(key)? {
            (_, Value::Bool(value)) => Ok(value),
            (kind, _) => Err(Self::wrong_type(key, "boolean", kind)),
        }
    }

    /// The value of the whole-number setting `key`, in the integer type the command builder
    /// takes.
    ///
    /// # Errors
    ///
    /// Like [`Context::bool`], and [`RenderError::InvalidValue`] if the value doesn't fit `T`.
    pub fn integer<T: TryFrom<i64>>(&self, key: &'static str) -> Result<T, RenderError> {
        match self.value(key)? {
            (Kind::Integer { min, max, .. }, Value::Integer(value)) => {
                T::try_from(value).map_err(|_| RenderError::InvalidValue {
                    key,
                    error: ValueError::IntegerOutOfRange {
                        min: *min,
                        max: *max,
                    },
                })
            }
            (kind, _) => Err(Self::wrong_type(key, "integer", kind)),
        }
    }

    /// The value of the decimal setting `key`.
    ///
    /// # Errors
    ///
    /// Like [`Context::bool`].
    pub fn decimal(&self, key: &'static str) -> Result<f64, RenderError> {
        match self.value(key)? {
            (_, Value::Decimal(value)) => Ok(value),
            (kind, _) => Err(Self::wrong_type(key, "decimal", kind)),
        }
    }

    /// The id of the selected choice of the setting `key`.
    ///
    /// # Errors
    ///
    /// Like [`Context::bool`].
    pub fn choice(&self, key: &'static str) -> Result<&'static str, RenderError> {
        match self.value(key)? {
            (Kind::Choice { choices, .. }, Value::Choice(id)) => choices
                .iter()
                .find(|choice| choice.id == id)
                .map(|choice| choice.id)
                .ok_or(RenderError::InvalidValue {
                    key,
                    error: ValueError::UnknownChoice { choices },
                }),
            (kind, _) => Err(Self::wrong_type(key, "choice", kind)),
        }
    }

    /// The value of the color setting `key`.
    ///
    /// # Errors
    ///
    /// Like [`Context::bool`].
    pub fn color(&self, key: &'static str) -> Result<Color, RenderError> {
        match self.value(key)? {
            (_, Value::Color(value)) => Ok(value),
            (kind, _) => Err(Self::wrong_type(key, "color", kind)),
        }
    }

    /// The value of the text setting `key`.
    ///
    /// # Errors
    ///
    /// Like [`Context::bool`].
    pub fn text(&self, key: &'static str) -> Result<String, RenderError> {
        match self.value(key)? {
            (_, Value::Text(value)) => Ok(value),
            (kind, _) => Err(Self::wrong_type(key, "text", kind)),
        }
    }

    /// The value of the list setting `key`.
    ///
    /// # Errors
    ///
    /// Like [`Context::bool`].
    pub fn list(&self, key: &'static str) -> Result<Vec<String>, RenderError> {
        match self.value(key)? {
            (_, Value::List(value)) => Ok(value),
            (kind, _) => Err(Self::wrong_type(key, "list", kind)),
        }
    }
}

impl fmt::Debug for Context<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Context")
            .field("scope", &self.scope)
            .field("device", &self.device)
            .field("target", &self.target)
            .finish_non_exhaustive()
    }
}

/// Why a setting can't be turned into sway commands.
#[derive(Debug, Clone, PartialEq)]
pub enum RenderError {
    /// The registry has no setting with this key.
    UnknownKey {
        /// The key.
        key: &'static str,
    },
    /// The setting is a preference of Sway Things itself and has no sway commands.
    NotASwaySetting {
        /// The key.
        key: &'static str,
    },
    /// A device was selected, but the setting isn't per device.
    NotPerDevice,
    /// A stored value doesn't fit its setting.
    InvalidValue {
        /// The key of the setting.
        key: &'static str,
        /// What is wrong with the value.
        error: ValueError,
    },
    /// The `input` command builder rejected a value, or the device isn't an input device.
    Input(InputError),
    /// The `output` command builder rejected a value, or the output's name.
    Output(OutputError),
    /// A window command builder rejected a value.
    Window(WindowError),
    /// A command builder rejected a number.
    OutOfRange(OutOfRange),
    /// A built command can't be written as a config line.
    Command(CommandError),
    /// More keyboard variants than layouts: each variant belongs to the layout at its position.
    TooManyVariants {
        /// The key of the variants setting.
        key: &'static str,
        /// The number of variants.
        variants: usize,
        /// The number of layouts.
        layouts: usize,
    },
}

impl From<InputError> for RenderError {
    fn from(error: InputError) -> Self {
        Self::Input(error)
    }
}

impl From<OutputError> for RenderError {
    fn from(error: OutputError) -> Self {
        Self::Output(error)
    }
}

impl From<WindowError> for RenderError {
    fn from(error: WindowError) -> Self {
        Self::Window(error)
    }
}

impl From<OutOfRange> for RenderError {
    fn from(error: OutOfRange) -> Self {
        Self::OutOfRange(error)
    }
}

impl RenderError {
    /// What went wrong, to show.
    pub fn text(&self) -> Text {
        match self {
            Self::UnknownKey { key } => Text::new(msg!("unknown setting {key}")).arg("key", key),
            Self::NotASwaySetting { key } => {
                Text::new(msg!("{key} is not a sway setting")).arg("key", key)
            }
            Self::NotPerDevice => {
                msg!("the setting can't be set for a single device or output").into()
            }
            Self::InvalidValue { key, error } => Text::new(msg!("{key}: {error}"))
                .arg("key", key)
                .text("error", error.text()),
            Self::Input(error) => error.text(),
            Self::Output(error) => error.text(),
            Self::Window(error) => error.text(),
            Self::OutOfRange(error) => error.text(),
            Self::Command(error) => error.text(),
            Self::TooManyVariants {
                key,
                variants,
                layouts,
            } => Text::new(msg!(
                "{key}: {variants} variants for {layouts} layouts; each variant belongs to the \
                 layout at the same position, so remove the extra variants or add their layouts"
            ))
            .arg("key", key)
            .arg("variants", variants)
            .arg("layouts", layouts),
        }
    }
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text().english())
    }
}

impl std::error::Error for RenderError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidValue { error, .. } => Some(error),
            Self::Input(error) => Some(error),
            Self::Output(error) => Some(error),
            Self::Window(error) => Some(error),
            Self::OutOfRange(error) => Some(error),
            Self::Command(error) => Some(error),
            Self::UnknownKey { .. }
            | Self::NotASwaySetting { .. }
            | Self::NotPerDevice
            | Self::TooManyVariants { .. } => None,
        }
    }
}
