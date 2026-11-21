// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Builders for `output <name> …` commands (`man 5 sway-output`), used by the Displays and
//! Wallpaper pages.
//!
//! An output is selected by its name (the connector, `DP-4`) or by its identifier, make, model
//! and serial separated by spaces (`Dell Inc. DELL U2723QE ABC123`), which stays the same when a
//! dock or another port gives the output another name. Both are external strings and go into the
//! command as text.
//!
//! ```
//! use std::path::Path;
//! use sway_things_core::command::output::{Fill, Mode, Output, OutputSelector, Transform};
//! use sway_things_core::command::{Color, Target};
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
//!
//! let laptop = Output::new(OutputSelector::output("eDP-1")?);
//! assert_eq!(
//!     laptop.background_color(Color::rgb(0x28, 0x55, 0x77)).render(Target::Ipc)?,
//!     r#"output "eDP-1" bg #285577 solid_color"#
//! );
//! let image = laptop.background_image(
//!     Path::new("/home/me/Pictures/Wallpapers/lake.jpg"),
//!     Fill::Fill,
//!     Color::rgb(0, 0, 0),
//! )?;
//! assert_eq!(
//!     image.render(Target::Config)?,
//!     r#"output "eDP-1" bg "'/home/me/Pictures/Wallpapers/lake.jpg'" fill #000000"#
//! );
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use std::fmt;
use std::path::Path;

use super::{BoolStyle, Color, OutOfRange, SwayCmd, SwayText, TextError};
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

/// How a background image covers an output (`bg <file> <mode>`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Fill {
    /// `fill`: scaled to cover the output, cut off where the aspect ratios differ.
    Fill,
    /// `fit`: scaled to fit the output, with the fallback color around it.
    Fit,
    /// `stretch`: scaled to the size of the output, distorted where the aspect ratios differ.
    Stretch,
    /// `center`: unscaled, in the middle of the output.
    Center,
    /// `tile`: unscaled, repeated.
    Tile,
}

impl Fill {
    /// Every mode, in the order of the Wallpaper page.
    pub const ALL: [Self; 5] = [
        Self::Fill,
        Self::Fit,
        Self::Stretch,
        Self::Center,
        Self::Tile,
    ];

    /// The keyword sway takes.
    pub const fn keyword(self) -> &'static str {
        match self {
            Self::Fill => "fill",
            Self::Fit => "fit",
            Self::Stretch => "stretch",
            Self::Center => "center",
            Self::Tile => "tile",
        }
    }

    /// The mode with this [keyword](Fill::keyword), without regard to case, as sway reads it.
    pub fn from_keyword(keyword: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|mode| mode.keyword().eq_ignore_ascii_case(keyword))
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

    /// `bg <color> solid_color`: the output is filled with one color. sway only takes opaque
    /// colors, so an alpha is dropped.
    pub fn background_color(&self, color: Color) -> SwayCmd {
        self.command("bg")
            .color(color.opaque())
            .keyword("solid_color")
    }

    /// `bg <file> <mode> <fallback>`: the image at `path`, covering the output as `mode` says,
    /// with `fallback` where it doesn't. The fallback is always given: sway 1.11 and later also
    /// show it if the file can't be read, instead of rejecting the command.
    ///
    /// `path` is absolute, or starts with `~/` for the home folder of whoever runs sway, so the
    /// same settings work for every user name.
    ///
    /// sway passes the file to `wordexp(3)`, which treats it like a word of a shell command line
    /// and runs `$(…)` in it. The path is therefore quoted in single quotes for `wordexp`, an
    /// inner `'` as `"'"`, before it is quoted as text for the target; only a leading `~` stays
    /// outside the quotes, so `wordexp` replaces it.
    ///
    /// # Errors
    ///
    /// [`OutputError::NotAbsolute`] if `path` is relative otherwise (sway would look for it next
    /// to the config file), [`OutputError::NotUtf8`] if it isn't valid UTF-8,
    /// [`OutputError::DoubleSpace`] if it has two spaces in a row (sway escapes those before
    /// `wordexp`, which breaks the quoting), and [`OutputError::Text`] for a control character or
    /// a backslash.
    pub fn background_image(
        &self,
        path: &Path,
        mode: Fill,
        fallback: Color,
    ) -> Result<SwayCmd, OutputError> {
        let word = wordexp_word(path)?;
        Ok(self
            .command("bg")
            .text(SwayText::new(word)?)
            .keyword(mode.keyword())
            .color(fallback.opaque()))
    }
}

/// `path` as one word that `wordexp(3)` reads back unchanged, except for a leading `~`, which
/// it replaces with the home folder.
fn wordexp_word(path: &Path) -> Result<String, OutputError> {
    let path = path.to_str().ok_or(OutputError::NotUtf8)?;
    let (home, rest) = match path.strip_prefix("~/") {
        Some(rest) => ("~/", rest),
        None if path.starts_with('/') => ("", path),
        None => return Err(OutputError::NotAbsolute),
    };
    if path.contains("  ") {
        return Err(OutputError::DoubleSpace);
    }
    // Checked here too, so the error points into the path, not into the quoted word.
    SwayText::new(path)?;
    Ok(format!("{home}'{}'", rest.replace('\'', r#"'"'"'"#)))
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
    /// The image path is relative, and doesn't start with `~/` either.
    NotAbsolute,
    /// The image path isn't valid UTF-8.
    NotUtf8,
    /// The image path has two spaces in a row.
    DoubleSpace,
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
            Self::NotAbsolute => {
                msg!("the image needs an absolute path or one in your home folder (~/…)").into()
            }
            Self::NotUtf8 => msg!("the path of the image isn't valid UTF-8").into(),
            Self::DoubleSpace => {
                msg!("sway can't read an image path with two spaces in a row").into()
            }
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
            Self::Reserved
            | Self::InvalidMode
            | Self::InvalidPosition
            | Self::NotAbsolute
            | Self::NotUtf8
            | Self::DoubleSpace => None,
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
