// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Builders for the Windows page (`man 5 sway`): colors, borders, gaps, title bars and per-app
//! opacity. The SwayFX effects are in [`effects`](super::effects).
//!
//! These directives are global, so they are plain functions; `workspace <name> gaps` takes the
//! workspace as a [`Workspace`] builder.
//!
//! ```
//! use sway_things_core::command::Target;
//! use sway_things_core::command::window::{self, BorderStyle, Gaps, GapsScope};
//!
//! let border = window::default_border(BorderStyle::Pixel(2));
//! assert_eq!(border.render(Target::Config)?, "default_border pixel 2");
//! // New workspaces get the default; open workspaces only change with the runtime form.
//! assert_eq!(window::gaps(Gaps::Inner, 8)?.render(Target::Config)?, "gaps inner 8");
//! assert_eq!(
//!     window::gaps_runtime(Gaps::Inner, GapsScope::All, 8)?.render(Target::Ipc)?,
//!     "gaps inner all set 8"
//! );
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use std::fmt;

use super::{Color, OutOfRange, SwayCmd, SwayText, TextError};

/// A window class for `client.<class>` colors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClientClass {
    /// `client.focused`: the focused window.
    Focused,
    /// `client.focused_inactive`: the most recently focused window of a container that isn't
    /// focused.
    FocusedInactive,
    /// `client.unfocused`
    Unfocused,
    /// `client.urgent`: a window with an urgency hint (Xwayland only).
    Urgent,
}

impl ClientClass {
    const fn directive(self) -> &'static str {
        match self {
            Self::Focused => "client.focused",
            Self::FocusedInactive => "client.focused_inactive",
            Self::Unfocused => "client.unfocused",
            Self::Urgent => "client.urgent",
        }
    }
}

/// The five colors of a window class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WindowColors {
    /// The border around the title bar.
    pub border: Color,
    /// The background of the title bar.
    pub background: Color,
    /// The text of the title bar.
    pub text: Color,
    /// The border edge where the next window opens.
    pub indicator: Color,
    /// The border around the window. The `client.focused` child border is also the app accent.
    pub child_border: Color,
}

/// The title bar colors of `client.focused_tab_title`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TitleColors {
    /// The border around the title bar.
    pub border: Color,
    /// The background of the title bar.
    pub background: Color,
    /// The text of the title bar.
    pub text: Color,
}

/// `client.<class> <border> <background> <text> <indicator> <child_border>`.
///
/// sway also knows `client.background` and `client.placeholder`, but ignores them (they exist for
/// i3 compatibility), so there are no builders for them.
pub fn client(class: ClientClass, colors: &WindowColors) -> SwayCmd {
    SwayCmd::new(class.directive())
        .color(colors.border)
        .color(colors.background)
        .color(colors.text)
        .color(colors.indicator)
        .color(colors.child_border)
}

/// `client.focused_tab_title <border> <background> <text>`: the title of a tabbed or stacked
/// container whose child has focus. sway ignores an indicator and child border for it.
pub fn client_focused_tab_title(colors: &TitleColors) -> SwayCmd {
    SwayCmd::new("client.focused_tab_title")
        .color(colors.border)
        .color(colors.background)
        .color(colors.text)
}

/// A border style, with the border width in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BorderStyle {
    /// `none`
    None,
    /// `normal <width>`: border and title bar.
    Normal(u16),
    /// `pixel <width>`: border without title bar.
    Pixel(u16),
}

fn border(directive: &'static str, style: BorderStyle) -> SwayCmd {
    let cmd = SwayCmd::new(directive);
    match style {
        BorderStyle::None => cmd.keyword("none"),
        BorderStyle::Normal(width) => cmd.keyword("normal").int(i64::from(width)),
        BorderStyle::Pixel(width) => cmd.keyword("pixel").int(i64::from(width)),
    }
}

/// `default_border`: the border of new tiled windows.
pub fn default_border(style: BorderStyle) -> SwayCmd {
    border("default_border", style)
}

/// `default_floating_border`: the border of windows that open floating.
pub fn default_floating_border(style: BorderStyle) -> SwayCmd {
    border("default_floating_border", style)
}

/// Which window borders `hide_edge_borders` hides at the screen edges.
///
/// [`HideEdgeBorders::Smart`] and [`HideEdgeBorders::SmartNoGaps`] don't hide edge borders: sway
/// reads them as `none` plus [`smart_borders`] `on` or `no_gaps`, so they change the smart borders
/// setting too.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HideEdgeBorders {
    /// `none`
    None,
    /// `vertical`
    Vertical,
    /// `horizontal`
    Horizontal,
    /// `both`
    Both,
    /// `smart`: like `none` with `smart_borders on`.
    Smart,
    /// `smart_no_gaps`: like `none` with `smart_borders no_gaps`.
    SmartNoGaps,
}

/// `hide_edge_borders [--i3] <hide>`. `hide_lone_tab` adds `--i3`, which also hides the title bar
/// of a tabbed or stacked container with one child.
pub fn hide_edge_borders(hide: HideEdgeBorders, hide_lone_tab: bool) -> SwayCmd {
    let cmd = SwayCmd::new("hide_edge_borders");
    let cmd = if hide_lone_tab {
        cmd.keyword("--i3")
    } else {
        cmd
    };
    cmd.keyword(match hide {
        HideEdgeBorders::None => "none",
        HideEdgeBorders::Vertical => "vertical",
        HideEdgeBorders::Horizontal => "horizontal",
        HideEdgeBorders::Both => "both",
        HideEdgeBorders::Smart => "smart",
        HideEdgeBorders::SmartNoGaps => "smart_no_gaps",
    })
}

/// `smart_borders`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SmartBorders {
    /// `on`: borders only if the workspace has more than one visible window.
    On,
    /// `no_gaps`: like `on`, and only if the gaps are zero.
    NoGaps,
    /// `off`
    Off,
}

/// `smart_borders`
pub fn smart_borders(smart: SmartBorders) -> SwayCmd {
    SwayCmd::new("smart_borders").keyword(match smart {
        SmartBorders::On => "on",
        SmartBorders::NoGaps => "no_gaps",
        SmartBorders::Off => "off",
    })
}

/// Which gaps a `gaps` command changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Gaps {
    /// `inner`: around each window. Can't be negative.
    Inner,
    /// `outer`: around the workspace, on every side. Negative values reduce the inner gaps at the
    /// edges.
    Outer,
    /// `horizontal`: outer gaps left and right.
    Horizontal,
    /// `vertical`: outer gaps top and bottom.
    Vertical,
    /// `top` outer gap.
    Top,
    /// `right` outer gap.
    Right,
    /// `bottom` outer gap.
    Bottom,
    /// `left` outer gap.
    Left,
}

impl Gaps {
    const fn keyword(self) -> &'static str {
        match self {
            Self::Inner => "inner",
            Self::Outer => "outer",
            Self::Horizontal => "horizontal",
            Self::Vertical => "vertical",
            Self::Top => "top",
            Self::Right => "right",
            Self::Bottom => "bottom",
            Self::Left => "left",
        }
    }

    fn check(self, px: i16) -> Result<(), WindowError> {
        if self == Self::Inner {
            OutOfRange::check("gaps inner", f64::from(px), 0.0, f64::from(i16::MAX))?;
        }
        Ok(())
    }
}

/// `gaps <which> <px>`: the default gaps of workspaces created from now on. Open workspaces keep
/// their gaps; change them with [`gaps_runtime`].
///
/// # Errors
///
/// [`WindowError::OutOfRange`] for negative inner gaps.
pub fn gaps(which: Gaps, px: i16) -> Result<SwayCmd, WindowError> {
    which.check(px)?;
    Ok(SwayCmd::new("gaps")
        .keyword(which.keyword())
        .int(i64::from(px)))
}

/// Which open workspaces [`gaps_runtime`] changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GapsScope {
    /// `all` open workspaces, including those with gaps of their own from `workspace <name> gaps`.
    All,
    /// The `current` workspace.
    Current,
}

/// `gaps <which> all|current set <px>`: the gaps of open workspaces. It doesn't change the default
/// for new workspaces (see [`gaps`]) and only works over IPC.
///
/// # Errors
///
/// [`WindowError::OutOfRange`] for negative inner gaps.
pub fn gaps_runtime(which: Gaps, scope: GapsScope, px: i16) -> Result<SwayCmd, WindowError> {
    which.check(px)?;
    Ok(SwayCmd::new("gaps")
        .keyword(which.keyword())
        .keyword(match scope {
            GapsScope::All => "all",
            GapsScope::Current => "current",
        })
        .keyword("set")
        .int(i64::from(px))
        .runtime_only())
}

/// `smart_gaps`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SmartGaps {
    /// `on`: gaps only if the workspace has more than one window.
    On,
    /// `off`
    Off,
    /// `inverse_outer`: outer gaps only if the workspace has exactly one window.
    InverseOuter,
}

/// `smart_gaps`
pub fn smart_gaps(smart: SmartGaps) -> SwayCmd {
    SwayCmd::new("smart_gaps").keyword(match smart {
        SmartGaps::On => "on",
        SmartGaps::Off => "off",
        SmartGaps::InverseOuter => "inverse_outer",
    })
}

/// Builds `workspace <name> …` settings for one workspace.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Workspace {
    name: SwayText,
}

impl Workspace {
    /// A workspace by name, e.g. `1` or `3: mail`.
    ///
    /// # Errors
    ///
    /// [`WindowError::ReservedWorkspaceName`] for `output` and `gaps` (in any case): sway looks for
    /// these keywords after unquoting, so the command would break. [`WindowError::Text`] for empty
    /// names, control characters and backslashes.
    pub fn new(name: &str) -> Result<Self, WindowError> {
        if name.eq_ignore_ascii_case("output") || name.eq_ignore_ascii_case("gaps") {
            return Err(WindowError::ReservedWorkspaceName);
        }
        Ok(Self {
            name: SwayText::new(name)?,
        })
    }

    /// `workspace <name> gaps <which> <px>`: the gaps the workspace gets when it is created. An
    /// open workspace keeps its gaps.
    ///
    /// # Errors
    ///
    /// [`WindowError::OutOfRange`] for negative inner gaps.
    pub fn gaps(&self, which: Gaps, px: i16) -> Result<SwayCmd, WindowError> {
        which.check(px)?;
        Ok(SwayCmd::new("workspace")
            .text(self.name.clone())
            .keyword("gaps")
            .keyword(which.keyword())
            .int(i64::from(px)))
    }
}

/// `font [pango:]<description>`: the title bar font as a Pango font description, e.g.
/// `monospace 10`. `markup` adds the `pango:` prefix, which enables Pango markup in titles.
///
/// # Errors
///
/// [`WindowError::PangoPrefix`] if `description` starts with `pango:` itself (use `markup`);
/// [`WindowError::Text`] for empty descriptions, control characters and backslashes.
pub fn font(description: &str, markup: bool) -> Result<SwayCmd, WindowError> {
    if description.starts_with("pango:") {
        return Err(WindowError::PangoPrefix);
    }
    let text = SwayText::new(description)?;
    let text = if markup {
        SwayText::new(format!("pango:{text}"))?
    } else {
        text
    };
    Ok(SwayCmd::new("font").text(text))
}

/// `title_align`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TitleAlign {
    /// `left`
    Left,
    /// `center`
    Center,
    /// `right`
    Right,
}

/// `title_align`
pub fn title_align(align: TitleAlign) -> SwayCmd {
    SwayCmd::new("title_align").keyword(match align {
        TitleAlign::Left => "left",
        TitleAlign::Center => "center",
        TitleAlign::Right => "right",
    })
}

/// `titlebar_padding <horizontal> <vertical>` in pixels.
///
/// sway rejects padding smaller than the current `titlebar_border_thickness`, and a thickness
/// larger than the current vertical padding, so the order of the two commands matters.
pub fn titlebar_padding(horizontal: u16, vertical: u16) -> SwayCmd {
    SwayCmd::new("titlebar_padding")
        .int(i64::from(horizontal))
        .int(i64::from(vertical))
}

/// `titlebar_border_thickness <px>`. See [`titlebar_padding`] for the order of the two commands.
pub fn titlebar_border_thickness(px: u16) -> SwayCmd {
    SwayCmd::new("titlebar_border_thickness").int(i64::from(px))
}

/// `opacity <value>`, from 0 (transparent) to 1 (opaque).
///
/// On its own it changes the focused window and only works over IPC. For per-app opacity, wrap
/// it: [`SwayCmd::for_window`] for windows that open later, [`SwayCmd::with_criteria`] for open
/// windows.
///
/// # Errors
///
/// [`WindowError::OutOfRange`] outside 0 … 1 or for NaN.
pub fn opacity(value: f64) -> Result<SwayCmd, WindowError> {
    OutOfRange::check("opacity", value, 0.0, 1.0)?;
    Ok(SwayCmd::new("opacity").float(value).runtime_only())
}

/// Why a window command can't be built.
#[derive(Debug, Clone, PartialEq)]
pub enum WindowError {
    /// A number is outside the range sway accepts.
    OutOfRange(OutOfRange),
    /// The workspace name is `output` or `gaps`, which sway reads as keywords.
    ReservedWorkspaceName,
    /// The font description starts with `pango:`.
    PangoPrefix,
    /// A string can't be used as text.
    Text(TextError),
}

impl From<OutOfRange> for WindowError {
    fn from(error: OutOfRange) -> Self {
        Self::OutOfRange(error)
    }
}

impl From<TextError> for WindowError {
    fn from(error: TextError) -> Self {
        Self::Text(error)
    }
}

impl fmt::Display for WindowError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutOfRange(error) => error.fmt(f),
            Self::ReservedWorkspaceName => {
                f.write_str("a workspace can't be called output or gaps in this command")
            }
            Self::PangoPrefix => f.write_str("the font description must not start with pango:"),
            Self::Text(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for WindowError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::OutOfRange(error) => Some(error),
            Self::Text(error) => Some(error),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests;
