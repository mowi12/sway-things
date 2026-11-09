// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Builders for the Window Behavior page (`man 5 sway`): focus, layouts, floating windows,
//! dragging, popups, Xwayland and idle inhibition. Like the window builders, these directives are
//! global, so they are plain functions.
//!
//! ```
//! use sway_things_core::command::Target;
//! use sway_things_core::command::behavior::{self, FloatingModifier, Modifier};
//!
//! let modifier = behavior::floating_modifier(FloatingModifier::Normal(Modifier::Super));
//! assert_eq!(modifier.render(Target::Ipc)?, "floating_modifier Mod4 normal");
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use std::num::NonZeroU16;

use super::{BoolStyle, SwayCmd};

/// `focus_follows_mouse`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FocusFollowsMouse {
    /// `yes`: moving the pointer over a window focuses it.
    Yes,
    /// `no`
    No,
    /// `always`: the window under the pointer always has focus, also after switching workspaces.
    Always,
}

/// `focus_follows_mouse`
pub fn focus_follows_mouse(mode: FocusFollowsMouse) -> SwayCmd {
    SwayCmd::new("focus_follows_mouse").keyword(match mode {
        FocusFollowsMouse::Yes => "yes",
        FocusFollowsMouse::No => "no",
        FocusFollowsMouse::Always => "always",
    })
}

/// `mouse_warping`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseWarping {
    /// `output`: the pointer moves to an output when focus moves there.
    Output,
    /// `container`: the pointer moves to the middle of the focused window.
    Container,
    /// `none`
    None,
}

/// `mouse_warping`
pub fn mouse_warping(mode: MouseWarping) -> SwayCmd {
    SwayCmd::new("mouse_warping").keyword(match mode {
        MouseWarping::Output => "output",
        MouseWarping::Container => "container",
        MouseWarping::None => "none",
    })
}

/// `focus_wrapping`: what happens when focus moves past the edge of a container.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FocusWrapping {
    /// `yes`: wrap to the opposite edge if there is no other container in that direction.
    Yes,
    /// `no`: keep focus.
    No,
    /// `force`: always wrap to the opposite edge.
    Force,
    /// `workspace`: like `yes`, and also wrap at workspace boundaries.
    Workspace,
}

/// `focus_wrapping`
pub fn focus_wrapping(mode: FocusWrapping) -> SwayCmd {
    SwayCmd::new("focus_wrapping").keyword(match mode {
        FocusWrapping::Yes => "yes",
        FocusWrapping::No => "no",
        FocusWrapping::Force => "force",
        FocusWrapping::Workspace => "workspace",
    })
}

/// `focus_on_window_activation`: what happens when an app asks to activate its window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FocusOnWindowActivation {
    /// `smart`: focus it if it is visible, otherwise mark it urgent.
    Smart,
    /// `urgent`: mark it urgent.
    Urgent,
    /// `focus`: focus it.
    Focus,
    /// `none`
    None,
}

/// `focus_on_window_activation`
pub fn focus_on_window_activation(mode: FocusOnWindowActivation) -> SwayCmd {
    SwayCmd::new("focus_on_window_activation").keyword(match mode {
        FocusOnWindowActivation::Smart => "smart",
        FocusOnWindowActivation::Urgent => "urgent",
        FocusOnWindowActivation::Focus => "focus",
        FocusOnWindowActivation::None => "none",
    })
}

/// `workspace_layout`: the layout of new containers on an empty workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WorkspaceLayout {
    /// `default`: tiled.
    Default,
    /// `stacking`
    Stacking,
    /// `tabbed`
    Tabbed,
}

/// `workspace_layout`. sway only accepts it in config files.
pub fn workspace_layout(layout: WorkspaceLayout) -> SwayCmd {
    SwayCmd::new("workspace_layout")
        .keyword(match layout {
            WorkspaceLayout::Default => "default",
            WorkspaceLayout::Stacking => "stacking",
            WorkspaceLayout::Tabbed => "tabbed",
        })
        .config_only()
}

/// `default_orientation`: how new tiled containers split.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Orientation {
    /// `horizontal`: side by side.
    Horizontal,
    /// `vertical`: stacked.
    Vertical,
    /// `auto`: by the shape of the output.
    Auto,
}

/// `default_orientation`. sway only accepts it in config files.
pub fn default_orientation(orientation: Orientation) -> SwayCmd {
    SwayCmd::new("default_orientation")
        .keyword(match orientation {
            Orientation::Horizontal => "horizontal",
            Orientation::Vertical => "vertical",
            Orientation::Auto => "auto",
        })
        .config_only()
}

/// A modifier key, as sway names them (`Mod4` is the Super or logo key, `Mod1` is Alt).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Modifier {
    /// `Shift`
    Shift,
    /// `Lock` (Caps Lock)
    Lock,
    /// `Ctrl`
    Ctrl,
    /// `Mod1` (Alt)
    Alt,
    /// `Mod2` (usually Num Lock)
    Mod2,
    /// `Mod3`
    Mod3,
    /// `Mod4` (Super, the logo key)
    Super,
    /// `Mod5` (usually the right Alt key of international layouts)
    Mod5,
}

impl Modifier {
    const fn name(self) -> &'static str {
        match self {
            Self::Shift => "Shift",
            Self::Lock => "Lock",
            Self::Ctrl => "Ctrl",
            Self::Alt => "Mod1",
            Self::Mod2 => "Mod2",
            Self::Mod3 => "Mod3",
            Self::Super => "Mod4",
            Self::Mod5 => "Mod5",
        }
    }
}

/// `floating_modifier`: the key that, held down, lets the mouse move and resize windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FloatingModifier {
    /// `none`: turns it off.
    None,
    /// `<modifier> normal`: left button moves, right button resizes.
    Normal(Modifier),
    /// `<modifier> inverse`: left button resizes, right button moves.
    Inverse(Modifier),
}

/// `floating_modifier`
pub fn floating_modifier(modifier: FloatingModifier) -> SwayCmd {
    let cmd = SwayCmd::new("floating_modifier");
    match modifier {
        FloatingModifier::None => cmd.keyword("none"),
        FloatingModifier::Normal(key) => cmd.keyword(key.name()).keyword("normal"),
        FloatingModifier::Inverse(key) => cmd.keyword(key.name()).keyword("inverse"),
    }
}

/// One dimension of `floating_minimum_size` or `floating_maximum_size`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SizeLimit {
    /// `-1`: no limit.
    NoLimit,
    /// `0`: sway's default (75 × 50 for the minimum, the size of all outputs for the maximum).
    Automatic,
    /// A size in pixels.
    Pixels(NonZeroU16),
}

impl SizeLimit {
    fn value(self) -> i64 {
        match self {
            Self::NoLimit => -1,
            Self::Automatic => 0,
            Self::Pixels(px) => i64::from(px.get()),
        }
    }
}

fn size(directive: &'static str, width: SizeLimit, height: SizeLimit) -> SwayCmd {
    SwayCmd::new(directive)
        .int(width.value())
        .keyword("x")
        .int(height.value())
}

/// `floating_minimum_size <width> x <height>`
pub fn floating_minimum_size(width: SizeLimit, height: SizeLimit) -> SwayCmd {
    size("floating_minimum_size", width, height)
}

/// `floating_maximum_size <width> x <height>`
pub fn floating_maximum_size(width: SizeLimit, height: SizeLimit) -> SwayCmd {
    size("floating_maximum_size", width, height)
}

/// `tiling_drag`: whether tiled windows can be dragged with the mouse.
pub fn tiling_drag(enabled: bool) -> SwayCmd {
    SwayCmd::new("tiling_drag").bool(enabled, BoolStyle::EnableDisable)
}

/// `tiling_drag_threshold`: how far, in pixels (multiplied by the output scale), a title bar must
/// be dragged before the window moves.
pub fn tiling_drag_threshold(px: u16) -> SwayCmd {
    SwayCmd::new("tiling_drag_threshold").int(i64::from(px))
}

/// `popup_during_fullscreen`: what happens when a fullscreen window opens a dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PopupDuringFullscreen {
    /// `smart`: show the dialog.
    Smart,
    /// `ignore`: don't show it.
    Ignore,
    /// `leave_fullscreen`: leave fullscreen and show it.
    LeaveFullscreen,
}

/// `popup_during_fullscreen`
pub fn popup_during_fullscreen(mode: PopupDuringFullscreen) -> SwayCmd {
    SwayCmd::new("popup_during_fullscreen").keyword(match mode {
        PopupDuringFullscreen::Smart => "smart",
        PopupDuringFullscreen::Ignore => "ignore",
        PopupDuringFullscreen::LeaveFullscreen => "leave_fullscreen",
    })
}

/// `workspace_auto_back_and_forth`: switching to the current workspace again goes back to the
/// previous one.
pub fn workspace_auto_back_and_forth(enabled: bool) -> SwayCmd {
    SwayCmd::new("workspace_auto_back_and_forth").bool(enabled, BoolStyle::YesNo)
}

/// `xwayland`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Xwayland {
    /// `enable`: start Xwayland when the first X11 app connects.
    Enable,
    /// `disable`: no X11 apps.
    Disable,
    /// `force`: start Xwayland right away.
    Force,
}

/// `xwayland`. sway can't change it at runtime, so it only works in config files.
pub fn xwayland(mode: Xwayland) -> SwayCmd {
    SwayCmd::new("xwayland")
        .keyword(match mode {
            Xwayland::Enable => "enable",
            Xwayland::Disable => "disable",
            Xwayland::Force => "force",
        })
        .config_only()
}

/// `inhibit_idle`: when a window keeps the session from going idle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InhibitIdle {
    /// `focus`: while it is focused.
    Focus,
    /// `fullscreen`: while it is fullscreen and visible.
    Fullscreen,
    /// `open`: until it is closed.
    Open,
    /// `visible`: while it is visible.
    Visible,
    /// `none`: remove the inhibitor.
    None,
}

/// `inhibit_idle <mode>`. On its own it changes the focused window and only works over IPC; wrap it
/// in [`SwayCmd::for_window`] for windows that open later or [`SwayCmd::with_criteria`] for open
/// windows, e.g. `for_window [app_id="mpv"] inhibit_idle fullscreen`.
pub fn inhibit_idle(mode: InhibitIdle) -> SwayCmd {
    SwayCmd::new("inhibit_idle")
        .keyword(match mode {
            InhibitIdle::Focus => "focus",
            InhibitIdle::Fullscreen => "fullscreen",
            InhibitIdle::Open => "open",
            InhibitIdle::Visible => "visible",
            InhibitIdle::None => "none",
        })
        .runtime_only()
}

#[cfg(test)]
mod tests;
