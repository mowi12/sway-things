// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Builders for `output <name> …` commands (`man 5 sway-output`), used by the Displays page.
//!
//! An output is selected by its name (the connector, `DP-4`) or by its identifier, make, model
//! and serial separated by spaces (`Dell Inc. DELL U2723QE ABC123`), which stays the same when a
//! dock or another port gives the output another name. Both are external strings and go into the
//! command as text.
//!
//! ```
//! use sway_things_core::command::Target;
//! use sway_things_core::command::output::{Mode, Output, OutputSelector, Transform};
//!
//! let dell = Output::new(OutputSelector::output("Dell Inc. DELL U2723QE ABC123")?);
//! assert_eq!(
//!     dell.mode(Mode::parse("3840x2160@59.997Hz")?).render(Target::Config)?,
//!     r#"output "Dell Inc. DELL U2723QE ABC123" mode 3840x2160@59.997Hz"#
//! );
//! assert_eq!(dell.position(1920, 0).render(Target::Ipc)?, r#"output "Dell Inc. DELL U2723QE ABC123" pos 1920 0"#);
//! assert_eq!(
//!     Output::new(OutputSelector::All).transform(Transform::R90).render(Target::Ipc)?,
//!     "output * transform 90"
//! );
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use std::fmt;

use super::{BoolStyle, OutOfRange, SwayCmd, SwayText, TextError};
use crate::i18n::Text;
use crate::msg;

/// Which outputs an `output` command applies to.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum OutputSelector {
    /// `*`: every output.
    All,
    /// One output, by its name or its identifier. Create it with [`OutputSelector::output`].
    Output(SwayText),
}

impl OutputSelector {
    /// One output, by its name (`DP-4`) or its identifier (`<make> <model> <serial>`), as
    /// `GET_OUTPUTS` reports them.
    ///
    /// # Errors
    ///
    /// [`OutputError::Reserved`] for `*`, `-` and `--`, which sway reads as every output or the
    /// focused one; [`OutputError::Text`] for empty text, a control character or a backslash.
    pub fn output(name: &str) -> Result<Self, OutputError> {
        if matches!(name, "*" | "-" | "--") {
            return Err(OutputError::Reserved);
        }
        Ok(Self::Output(SwayText::new(name)?))
    }
}

/// A video mode: the size in pixels and, if given, the refresh rate. Without one sway picks the
/// highest refresh rate of that size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Mode {
    width: u32,
    height: u32,
    /// In millihertz, as `GET_OUTPUTS` reports it.
    refresh: Option<u32>,
}

impl Mode {
    /// The mode `width` × `height` at `refresh` millihertz, as `GET_OUTPUTS` reports it; `None` or
    /// 0 for any refresh rate.
    ///
    /// # Errors
    ///
    /// [`OutputError::InvalidMode`] for a width or height of 0 or above sway's limit of
    /// 2,147,483,647 pixels.
    pub fn new(width: u32, height: u32, refresh: Option<u32>) -> Result<Self, OutputError> {
        let fits = |pixels: u32| pixels > 0 && i32::try_from(pixels).is_ok();
        if !fits(width) || !fits(height) {
            return Err(OutputError::InvalidMode);
        }
        Ok(Self {
            width,
            height,
            refresh: refresh.filter(|&refresh| refresh > 0),
        })
    }

    /// Reads a mode as [`Mode`]'s `Display` writes it: `2560x1440` or `2560x1440@59.951Hz`. The
    /// refresh rate has at most three decimals, since sway counts it in millihertz.
    ///
    /// # Errors
    ///
    /// [`OutputError::InvalidMode`] for anything else.
    pub fn parse(text: &str) -> Result<Self, OutputError> {
        let (size, refresh) = match text.split_once('@') {
            Some((size, rate)) => {
                let rate = rate.strip_suffix("Hz").ok_or(OutputError::InvalidMode)?;
                (
                    size,
                    Some(parse_millihertz(rate).ok_or(OutputError::InvalidMode)?),
                )
            }
            None => (text, None),
        };
        let (width, height) = size.split_once('x').ok_or(OutputError::InvalidMode)?;
        let number = |digits: &str| {
            (!digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit()))
                .then(|| digits.parse::<u32>().ok())
                .flatten()
                .ok_or(OutputError::InvalidMode)
        };
        if refresh == Some(0) {
            return Err(OutputError::InvalidMode);
        }
        Self::new(number(width)?, number(height)?, refresh)
    }

    /// The width in pixels.
    pub fn width(self) -> u32 {
        self.width
    }

    /// The height in pixels.
    pub fn height(self) -> u32 {
        self.height
    }

    /// The refresh rate in millihertz, if the mode names one.
    pub fn refresh(self) -> Option<u32> {
        self.refresh
    }
}

/// Reads a refresh rate in hertz with at most three decimals (`59.951`) as millihertz.
fn parse_millihertz(text: &str) -> Option<u32> {
    let (whole, fraction) = text.split_once('.').unwrap_or((text, ""));
    let digits = |part: &str| part.bytes().all(|byte| byte.is_ascii_digit());
    if whole.is_empty() || !digits(whole) || !digits(fraction) || fraction.len() > 3 {
        return None;
    }
    if text.ends_with('.') {
        return None;
    }
    let whole: u32 = whole.parse().ok()?;
    let fraction: u32 = format!("{fraction:0<3}").parse().ok()?;
    whole.checked_mul(1000)?.checked_add(fraction)
}

impl fmt::Display for Mode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}x{}", self.width, self.height)?;
        if let Some(refresh) = self.refresh {
            write!(f, "@{}", refresh / 1000)?;
            let fraction = refresh % 1000;
            if fraction > 0 {
                let decimals = format!("{fraction:03}");
                write!(f, ".{}", decimals.trim_end_matches('0'))?;
            }
            f.write_str("Hz")?;
        }
        Ok(())
    }
}

/// `transform`: how the output is rotated (clockwise) and flipped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Transform {
    /// `normal`
    Normal,
    /// `90`
    R90,
    /// `180`
    R180,
    /// `270`
    R270,
    /// `flipped`
    Flipped,
    /// `flipped-90`
    Flipped90,
    /// `flipped-180`
    Flipped180,
    /// `flipped-270`
    Flipped270,
}

impl Transform {
    /// Every transform, in the order of `man 5 sway-output`.
    pub const ALL: [Self; 8] = [
        Self::Normal,
        Self::R90,
        Self::R180,
        Self::R270,
        Self::Flipped,
        Self::Flipped90,
        Self::Flipped180,
        Self::Flipped270,
    ];

    /// The name in the command and in `GET_OUTPUTS`.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::R90 => "90",
            Self::R180 => "180",
            Self::R270 => "270",
            Self::Flipped => "flipped",
            Self::Flipped90 => "flipped-90",
            Self::Flipped180 => "flipped-180",
            Self::Flipped270 => "flipped-270",
        }
    }

    /// The transform with this name, as the command takes it (also `0` for `normal`) and
    /// `GET_OUTPUTS` reports it. sway compares exactly.
    pub fn from_name(name: &str) -> Option<Self> {
        if name == "0" {
            return Some(Self::Normal);
        }
        Self::ALL
            .into_iter()
            .find(|transform| transform.name() == name)
    }

    /// Whether width and height swap: a quarter turn, flipped or not.
    pub const fn is_sideways(self) -> bool {
        matches!(
            self,
            Self::R90 | Self::R270 | Self::Flipped90 | Self::Flipped270
        )
    }
}

/// The smallest scale the builder accepts. sway takes any scale above 0, but below this nothing
/// on the output can be read.
pub const MIN_SCALE: f64 = 0.25;
/// The largest scale the builder accepts.
pub const MAX_SCALE: f64 = 10.0;

/// Builds `output` commands for one [`OutputSelector`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Output {
    selector: OutputSelector,
}

impl Output {
    /// Builds commands for the outputs `selector` matches.
    pub fn new(selector: OutputSelector) -> Self {
        Self { selector }
    }

    /// `output <selector>`
    fn selected(&self) -> SwayCmd {
        let cmd = SwayCmd::new("output");
        match &self.selector {
            OutputSelector::All => cmd.keyword("*"),
            OutputSelector::Output(name) => cmd.text(name.clone()),
        }
    }

    fn command(&self, subcommand: &'static str) -> SwayCmd {
        self.selected().keyword(subcommand)
    }

    /// `mode <width>x<height>[@<rate>Hz]`. sway takes the mode with the nearest refresh rate of
    /// that size.
    pub fn mode(&self, mode: Mode) -> SwayCmd {
        self.command("mode").word(mode.to_string())
    }

    /// `pos <x> <y>`: the top left corner in the layout, in logical pixels (the sizes of the
    /// outputs divided by their scale).
    pub fn position(&self, x: i32, y: i32) -> SwayCmd {
        self.command("pos").int(i64::from(x)).int(i64::from(y))
    }

    /// `scale <factor>`. sway may round a fractional scale to what the protocol can describe.
    ///
    /// # Errors
    ///
    /// [`OutputError::OutOfRange`] outside [`MIN_SCALE`] … [`MAX_SCALE`] or for NaN.
    pub fn scale(&self, factor: f64) -> Result<SwayCmd, OutputError> {
        OutOfRange::check("scale", factor, MIN_SCALE, MAX_SCALE)?;
        Ok(self.command("scale").float(factor))
    }

    /// `transform <transform>`
    pub fn transform(&self, transform: Transform) -> SwayCmd {
        self.command("transform").keyword(transform.name())
    }

    /// `enable` or `disable`. A disabled output loses its workspaces and windows to the others.
    pub fn enable(&self, enabled: bool) -> SwayCmd {
        self.selected().bool(enabled, BoolStyle::EnableDisable)
    }
}

/// Why an `output` command can't be built.
#[derive(Debug, Clone, PartialEq)]
pub enum OutputError {
    /// `*`, `-` or `--` stand for every output or the focused one, not for one by its name.
    Reserved,
    /// The text isn't a mode like `2560x1440` or `2560x1440@59.951Hz`.
    InvalidMode,
    /// The text isn't a position like `1920,0`.
    InvalidPosition,
    /// A number is outside the range sway accepts (or NaN).
    OutOfRange(OutOfRange),
    /// A string can't be used as text.
    Text(TextError),
}

impl From<OutOfRange> for OutputError {
    fn from(error: OutOfRange) -> Self {
        Self::OutOfRange(error)
    }
}

impl From<TextError> for OutputError {
    fn from(error: TextError) -> Self {
        Self::Text(error)
    }
}

impl OutputError {
    /// What is wrong, to show.
    pub fn text(&self) -> Text {
        match self {
            Self::Reserved => msg!("*, - and -- don't name an output").into(),
            Self::InvalidMode => {
                msg!("expected a mode like 2560x1440 or 2560x1440@59.951Hz").into()
            }
            Self::InvalidPosition => msg!("expected a position like 1920,0").into(),
            Self::OutOfRange(error) => error.text(),
            Self::Text(error) => error.text(),
        }
    }
}

impl fmt::Display for OutputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text().english())
    }
}

impl std::error::Error for OutputError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::OutOfRange(error) => Some(error),
            Self::Text(error) => Some(error),
            Self::Reserved | Self::InvalidMode | Self::InvalidPosition => None,
        }
    }
}

/// Reads a position as the Displays settings store it: `<x>,<y>` in logical pixels, each a whole
/// number that fits sway's `int`.
///
/// # Errors
///
/// [`OutputError::InvalidPosition`] for anything else.
pub fn parse_position(text: &str) -> Result<(i32, i32), OutputError> {
    let (x, y) = text.split_once(',').ok_or(OutputError::InvalidPosition)?;
    let number = |text: &str| {
        let digits = text.strip_prefix('-').unwrap_or(text);
        if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(OutputError::InvalidPosition);
        }
        text.parse::<i32>()
            .map_err(|_| OutputError::InvalidPosition)
    };
    Ok((number(x)?, number(y)?))
}

#[cfg(test)]
mod tests;
