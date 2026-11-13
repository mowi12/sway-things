// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Settings of the Window Behavior page (`man 5 sway`). Defaults are sway's own
//! (`config_defaults` in `sway/config.c`).

use std::num::NonZeroU16;

use super::read::{self, ReadError};
use super::{
    Apply, Choice, ConfigFile, Context, Directive, Page, RenderError, Setting, Value, When,
};
use crate::command::Criteria;
use crate::command::SwayCmd;
use crate::command::behavior::{
    self, FloatingModifier, FocusFollowsMouse, FocusOnWindowActivation, FocusWrapping, InhibitIdle,
    Modifier, MouseWarping, Orientation, PopupDuringFullscreen, SizeLimit, WorkspaceLayout,
    Xwayland,
};

const FOCUS_FOLLOWS_MOUSE: &str = "behavior.focus_follows_mouse";
const FOCUS_FOLLOWS_MOUSE_CHOICES: &[Choice] = &[
    Choice::new("yes", "When the pointer enters a window"),
    Choice::new("no", "Only on click"),
    Choice::new("always", "Always the window under the pointer"),
];

const MOUSE_WARPING: &str = "behavior.mouse_warping";
const MOUSE_WARPING_CHOICES: &[Choice] = &[
    Choice::new("output", "Onto the screen that gets the focus"),
    Choice::new("container", "Into the window that gets the focus"),
    Choice::new("none", "Never"),
];

const FOCUS_WRAPPING: &str = "behavior.focus_wrapping";
const FOCUS_WRAPPING_CHOICES: &[Choice] = &[
    Choice::new("yes", "Wraps to the first window"),
    Choice::new("no", "Stays at the last window"),
    Choice::new("force", "Wraps inside every container"),
    Choice::new("workspace", "Wraps, never to another screen"),
];

const FOCUS_ON_ACTIVATION: &str = "behavior.focus_on_window_activation";
const FOCUS_ON_ACTIVATION_CHOICES: &[Choice] = &[
    Choice::new("urgent", "Mark it urgent"),
    Choice::new("smart", "Focus it if visible, otherwise mark it urgent"),
    Choice::new("focus", "Focus it"),
    Choice::new("none", "Ignore the request"),
];

const WORKSPACE_LAYOUT: &str = "behavior.workspace_layout";
const WORKSPACE_LAYOUT_CHOICES: &[Choice] = &[
    Choice::new("default", "Tiled"),
    Choice::new("stacking", "Stacked"),
    Choice::new("tabbed", "Tabbed"),
];

const DEFAULT_ORIENTATION: &str = "behavior.default_orientation";
const DEFAULT_ORIENTATION_CHOICES: &[Choice] = &[
    Choice::new("auto", "By the shape of the screen"),
    Choice::new("horizontal", "Side by side"),
    Choice::new("vertical", "One above the other"),
];

const BACK_AND_FORTH: &str = "behavior.workspace_auto_back_and_forth";

const MODIFIER_KEY: &str = "behavior.floating_modifier.key";
const MODIFIER_KEY_CHOICES: &[Choice] = &[
    Choice::new("none", "Off"),
    Choice::new("Mod4", "Super"),
    Choice::new("Mod1", "Alt"),
    Choice::new("Ctrl", "Ctrl"),
    Choice::new("Shift", "Shift"),
];
/// The modifier choices other than `none`.
const MODIFIER_KEYS: &[&str] = &["Mod4", "Mod1", "Ctrl", "Shift"];

const MODIFIER_BUTTONS: &str = "behavior.floating_modifier.buttons";
const MODIFIER_BUTTON_CHOICES: &[Choice] = &[
    Choice::new("normal", "Left button moves, right button resizes"),
    Choice::new("inverse", "Left button resizes, right button moves"),
];

const TILING_DRAG: &str = "behavior.tiling_drag";
const TILING_DRAG_THRESHOLD: &str = "behavior.tiling_drag_threshold";

const MIN_SIZE: Size = Size {
    keys: [
        "behavior.floating_minimum_size.mode",
        "behavior.floating_minimum_size.width",
        "behavior.floating_minimum_size.height",
    ],
    directive: "floating_minimum_size",
    group: (
        "behavior.floating_minimum_size",
        "Minimum size of floating windows",
    ),
    choices: &[
        Choice::new("automatic", "Automatic (75 × 50)"),
        Choice::new("none", "No limit"),
        Choice::new("custom", "Custom"),
    ],
    defaults: [75, 50],
    titles: [
        "Smallest floating window",
        "Smallest width",
        "Smallest height",
    ],
};
const MAX_SIZE: Size = Size {
    keys: [
        "behavior.floating_maximum_size.mode",
        "behavior.floating_maximum_size.width",
        "behavior.floating_maximum_size.height",
    ],
    directive: "floating_maximum_size",
    group: (
        "behavior.floating_maximum_size",
        "Maximum size of floating windows",
    ),
    choices: &[
        Choice::new("automatic", "Automatic (all screens together)"),
        Choice::new("none", "No limit"),
        Choice::new("custom", "Custom"),
    ],
    defaults: [1920, 1080],
    titles: ["Largest floating window", "Largest width", "Largest height"],
};

const POPUP_DURING_FULLSCREEN: &str = "behavior.popup_during_fullscreen";
const POPUP_CHOICES: &[Choice] = &[
    Choice::new("smart", "Show them"),
    Choice::new("ignore", "Don't show them"),
    Choice::new("leave_fullscreen", "Leave fullscreen and show them"),
];

const INHIBIT_IDLE_FULLSCREEN: &str = "behavior.inhibit_idle_fullscreen";

const XWAYLAND: &str = "behavior.xwayland";
const XWAYLAND_CHOICES: &[Choice] = &[
    Choice::new("enable", "When the first X11 app starts"),
    Choice::new("force", "Right away"),
    Choice::new("disable", "Never: X11 apps don't run"),
];

/// The settings of `floating_minimum_size` or `floating_maximum_size`: whether there is a limit,
/// and its width and height.
struct Size {
    keys: [&'static str; 3],
    directive: &'static str,
    group: (&'static str, &'static str),
    choices: &'static [Choice],
    defaults: [i64; 2],
    titles: [&'static str; 3],
}

fn focus_follows_mouse(ctx: &Context<'_>) -> Result<Vec<SwayCmd>, RenderError> {
    let mode = match ctx.choice(FOCUS_FOLLOWS_MOUSE)? {
        "no" => FocusFollowsMouse::No,
        "always" => FocusFollowsMouse::Always,
        _ => FocusFollowsMouse::Yes,
    };
    Ok(vec![behavior::focus_follows_mouse(mode)])
}

fn mouse_warping(ctx: &Context<'_>) -> Result<Vec<SwayCmd>, RenderError> {
    let mode = match ctx.choice(MOUSE_WARPING)? {
        "container" => MouseWarping::Container,
        "none" => MouseWarping::None,
        _ => MouseWarping::Output,
    };
    Ok(vec![behavior::mouse_warping(mode)])
}

fn focus_wrapping(ctx: &Context<'_>) -> Result<Vec<SwayCmd>, RenderError> {
    let mode = match ctx.choice(FOCUS_WRAPPING)? {
        "no" => FocusWrapping::No,
        "force" => FocusWrapping::Force,
        "workspace" => FocusWrapping::Workspace,
        _ => FocusWrapping::Yes,
    };
    Ok(vec![behavior::focus_wrapping(mode)])
}

/// A `focus_wrapping` line: `force` and `workspace` as `strcmp` compares them, otherwise sway's
/// `parse_boolean`.
fn read_focus_wrapping(args: &[String]) -> Result<Value, ReadError> {
    let id = match read::one(args)? {
        word @ ("force" | "workspace") => word,
        word if read::boolean(word, true) => "yes",
        _ => "no",
    };
    Ok(Value::Choice(id.to_owned()))
}

fn focus_on_activation(ctx: &Context<'_>) -> Result<Vec<SwayCmd>, RenderError> {
    let mode = match ctx.choice(FOCUS_ON_ACTIVATION)? {
        "smart" => FocusOnWindowActivation::Smart,
        "focus" => FocusOnWindowActivation::Focus,
        "none" => FocusOnWindowActivation::None,
        _ => FocusOnWindowActivation::Urgent,
    };
    Ok(vec![behavior::focus_on_window_activation(mode)])
}

fn workspace_layout(ctx: &Context<'_>) -> Result<Vec<SwayCmd>, RenderError> {
    let layout = match ctx.choice(WORKSPACE_LAYOUT)? {
        "stacking" => WorkspaceLayout::Stacking,
        "tabbed" => WorkspaceLayout::Tabbed,
        _ => WorkspaceLayout::Default,
    };
    Ok(vec![behavior::workspace_layout(layout)])
}

fn default_orientation(ctx: &Context<'_>) -> Result<Vec<SwayCmd>, RenderError> {
    let orientation = match ctx.choice(DEFAULT_ORIENTATION)? {
        "horizontal" => Orientation::Horizontal,
        "vertical" => Orientation::Vertical,
        _ => Orientation::Auto,
    };
    Ok(vec![behavior::default_orientation(orientation)])
}

fn floating_modifier(ctx: &Context<'_>) -> Result<Vec<SwayCmd>, RenderError> {
    let key = match ctx.choice(MODIFIER_KEY)? {
        "Mod4" => Modifier::Super,
        "Mod1" => Modifier::Alt,
        "Ctrl" => Modifier::Ctrl,
        "Shift" => Modifier::Shift,
        _ => return Ok(vec![behavior::floating_modifier(FloatingModifier::None)]),
    };
    let modifier = match ctx.choice(MODIFIER_BUTTONS)? {
        "inverse" => FloatingModifier::Inverse(key),
        _ => FloatingModifier::Normal(key),
    };
    Ok(vec![behavior::floating_modifier(modifier)])
}

/// A `floating_modifier` line as sway reads it: `none`, or a modifier by any of its names
/// (`get_modifier_mask_by_name`, compared as `strcasecmp`) with `normal` or `inverse`. `none`
/// leaves the buttons as they were, here sway's default.
fn read_floating_modifier(args: &[String]) -> Result<(&'static str, &'static str), ReadError> {
    let name = read::first(args)?;
    if name.eq_ignore_ascii_case("none") {
        return Ok(("none", "normal"));
    }
    let is = |names: &[&str]| names.iter().any(|known| name.eq_ignore_ascii_case(known));
    let key = if is(&["Mod4", "Super"]) {
        "Mod4"
    } else if is(&["Mod1", "Alt"]) {
        "Mod1"
    } else if is(&["Ctrl", "Control"]) {
        "Ctrl"
    } else if is(&["Shift"]) {
        "Shift"
    } else if is(&["Lock", "Mod2", "Mod3", "Mod5"]) {
        return Err(ReadError::Unsupported(format!(
            "{name} isn't offered as the drag modifier"
        )));
    } else {
        return Err(ReadError::Rejected(format!("not a modifier: {name}")));
    };
    let buttons = match args.get(1) {
        None => "normal",
        Some(word) if word.eq_ignore_ascii_case("normal") => "normal",
        Some(word) if word.eq_ignore_ascii_case("inverse") => "inverse",
        Some(word) => {
            return Err(ReadError::Rejected(format!(
                "expected normal or inverse, got {word}"
            )));
        }
    };
    Ok((key, buttons))
}

/// One dimension of a `floating_*_size` line, as sway's `strtol` with its end pointer reads it.
fn read_limit(text: &str) -> Result<SizeLimit, ReadError> {
    match read::strtol(text) {
        Some((-1, "")) => Ok(SizeLimit::NoLimit),
        Some((0, "")) => Ok(SizeLimit::Automatic),
        Some((px, "")) => u16::try_from(px)
            .ok()
            .and_then(NonZeroU16::new)
            .map(SizeLimit::Pixels)
            .ok_or_else(|| ReadError::Unsupported(format!("not a size in pixels: {px}"))),
        Some(_) => Err(ReadError::Rejected(format!("not a whole number: {text}"))),
        None => Err(ReadError::Unsupported(format!("too large: {text}"))),
    }
}

/// A `floating_*_size <width> x <height>` line: the mode and, for `custom`, the size. Only a
/// limit of the same kind in both directions fits the settings.
fn read_size(args: &[String]) -> Result<(&'static str, Option<[i64; 2]>), ReadError> {
    let [width, x, height] = args else {
        return Err(ReadError::Rejected(format!(
            "expected <width> x <height>, got {} arguments",
            args.len()
        )));
    };
    if x != "x" {
        return Err(ReadError::Rejected(format!("expected x, got {x}")));
    }
    match (read_limit(width)?, read_limit(height)?) {
        (SizeLimit::Automatic, SizeLimit::Automatic) => Ok(("automatic", None)),
        (SizeLimit::NoLimit, SizeLimit::NoLimit) => Ok(("none", None)),
        (SizeLimit::Pixels(width), SizeLimit::Pixels(height)) => Ok((
            "custom",
            Some([i64::from(width.get()), i64::from(height.get())]),
        )),
        _ => Err(ReadError::Unsupported(
            "width and height have different kinds of limit".to_owned(),
        )),
    }
}

/// The settings of one `floating_*_size` directive. The render and read functions are per
/// directive and position, so they take both as const parameters: 0 for the minimum, 1 for the
/// maximum.
fn size_settings<const S: usize>() -> Vec<Setting> {
    let size = [&MIN_SIZE, &MAX_SIZE][S];
    let render: super::RenderFn = |ctx| {
        let size = [&MIN_SIZE, &MAX_SIZE][S];
        let [mode, width, height] = size.keys;
        let (width, height) = match ctx.choice(mode)? {
            "none" => (SizeLimit::NoLimit, SizeLimit::NoLimit),
            "custom" => {
                let px = |key| {
                    ctx.integer::<u16>(key).map(|px| {
                        NonZeroU16::new(px).map_or(SizeLimit::Automatic, SizeLimit::Pixels)
                    })
                };
                (px(width)?, px(height)?)
            }
            _ => (SizeLimit::Automatic, SizeLimit::Automatic),
        };
        let command = if S == 0 {
            behavior::floating_minimum_size(width, height)
        } else {
            behavior::floating_maximum_size(width, height)
        };
        Ok(vec![command])
    };
    let reads: [super::ReadFn; 3] = [
        |args| Ok(Value::Choice(read_size(args)?.0.to_owned())),
        |args| {
            let (_, px) = read_size(args)?;
            Ok(Value::Integer(
                px.map_or([&MIN_SIZE, &MAX_SIZE][S].defaults[0], |px| px[0]),
            ))
        },
        |args| {
            let (_, px) = read_size(args)?;
            Ok(Value::Integer(
                px.map_or([&MIN_SIZE, &MAX_SIZE][S].defaults[1], |px| px[1]),
            ))
        },
    ];
    let directive = Directive::top(size.directive);
    let keywords: &'static [&'static str] = &["floating", "window size", "limit"];
    let [mode, width, height] = size.keys;
    vec![
        Setting::choice(mode, Page::WindowBehavior, "automatic", size.choices)
            .section("Floating windows")
            .group(size.group.0, size.group.1)
            .title(size.titles[0])
            .keywords(keywords)
            .sway(directive, ConfigFile::Behavior, render, reads[0])
            .build(),
        Setting::integer(
            width,
            Page::WindowBehavior,
            size.defaults[0],
            1,
            i64::from(u16::MAX),
        )
        .section("Floating windows")
        .group(size.group.0, size.group.1)
        .title(size.titles[1])
        .subtitle("In pixels")
        .slider(10.0, 4000.0, 10.0)
        .keywords(keywords)
        .depends_on(mode, When::OneOf(&["custom"]))
        .sway(directive, ConfigFile::Behavior, render, reads[1])
        .build(),
        Setting::integer(
            height,
            Page::WindowBehavior,
            size.defaults[1],
            1,
            i64::from(u16::MAX),
        )
        .section("Floating windows")
        .group(size.group.0, size.group.1)
        .title(size.titles[2])
        .subtitle("In pixels")
        .slider(10.0, 4000.0, 10.0)
        .keywords(keywords)
        .depends_on(mode, When::OneOf(&["custom"]))
        .sway(directive, ConfigFile::Behavior, render, reads[2])
        .build(),
    ]
}

fn popup_during_fullscreen(ctx: &Context<'_>) -> Result<Vec<SwayCmd>, RenderError> {
    let mode = match ctx.choice(POPUP_DURING_FULLSCREEN)? {
        "ignore" => PopupDuringFullscreen::Ignore,
        "leave_fullscreen" => PopupDuringFullscreen::LeaveFullscreen,
        _ => PopupDuringFullscreen::Smart,
    };
    Ok(vec![behavior::popup_during_fullscreen(mode)])
}

/// On, a rule for every new window; off, no line at all, so the user's own `inhibit_idle` rules
/// stay in force.
fn inhibit_idle_fullscreen(ctx: &Context<'_>) -> Result<Vec<SwayCmd>, RenderError> {
    if !ctx.bool(INHIBIT_IDLE_FULLSCREEN)? {
        return Ok(Vec::new());
    }
    Ok(vec![
        SwayCmd::for_window(
            Criteria::all(),
            behavior::inhibit_idle(InhibitIdle::Fullscreen),
        )
        .map_err(RenderError::Command)?,
    ])
}

/// The rest of a `for_window [all] inhibit_idle` line. `none` sets no inhibitor, like no rule.
fn read_inhibit_idle(args: &[String]) -> Result<Value, ReadError> {
    match read::one(args)? {
        "fullscreen" => Ok(Value::Bool(true)),
        "none" => Ok(Value::Bool(false)),
        mode @ ("focus" | "open" | "visible") => Err(ReadError::Unsupported(format!(
            "inhibit_idle {mode} for every window"
        ))),
        mode => Err(ReadError::Rejected(format!(
            "expected focus, fullscreen, open, visible or none, got {mode}"
        ))),
    }
}

fn xwayland(ctx: &Context<'_>) -> Result<Vec<SwayCmd>, RenderError> {
    let mode = match ctx.choice(XWAYLAND)? {
        "force" => Xwayland::Force,
        "disable" => Xwayland::Disable,
        _ => Xwayland::Enable,
    };
    Ok(vec![behavior::xwayland(mode)])
}

#[expect(
    clippy::too_many_lines,
    reason = "one builder per setting, read best as a list"
)]
pub(super) fn settings() -> Vec<Setting> {
    let mut settings = vec![
        Setting::choice(
            FOCUS_FOLLOWS_MOUSE,
            Page::WindowBehavior,
            "yes",
            FOCUS_FOLLOWS_MOUSE_CHOICES,
        )
        .section("Focus")
        .title("Focus follows mouse")
        .keywords(&["pointer", "hover", "sloppy focus"])
        .sway(
            Directive::top("focus_follows_mouse"),
            ConfigFile::Behavior,
            focus_follows_mouse,
            |args| read::choice(read::one(args)?, &["yes", "no", "always"], true),
        )
        .build(),
        Setting::choice(
            MOUSE_WARPING,
            Page::WindowBehavior,
            "output",
            MOUSE_WARPING_CHOICES,
        )
        .section("Focus")
        .title("Move the pointer with the focus")
        .subtitle("When the keyboard moves the focus")
        .keywords(&["mouse warping", "cursor", "jump"])
        .sway(
            Directive::top("mouse_warping"),
            ConfigFile::Behavior,
            mouse_warping,
            |args| read::choice(read::one(args)?, &["output", "container", "none"], false),
        )
        .build(),
        Setting::choice(
            FOCUS_WRAPPING,
            Page::WindowBehavior,
            "yes",
            FOCUS_WRAPPING_CHOICES,
        )
        .section("Focus")
        .title("Focus past the last window")
        .subtitle("When the keyboard moves the focus beyond the edge")
        .keywords(&["focus wrapping", "cycle", "edge"])
        .sway(
            Directive::top("focus_wrapping"),
            ConfigFile::Behavior,
            focus_wrapping,
            read_focus_wrapping,
        )
        .build(),
        Setting::choice(
            FOCUS_ON_ACTIVATION,
            Page::WindowBehavior,
            "urgent",
            FOCUS_ON_ACTIVATION_CHOICES,
        )
        .section("Focus")
        .title("When an app asks for focus")
        .subtitle("E.g. a browser opening a link from another app")
        .keywords(&[
            "focus on window activation",
            "urgent",
            "steal focus",
            "attention",
        ])
        .sway(
            Directive::top("focus_on_window_activation"),
            ConfigFile::Behavior,
            focus_on_activation,
            |args| {
                read::choice(
                    read::one(args)?,
                    &["smart", "urgent", "focus", "none"],
                    true,
                )
            },
        )
        .build(),
        Setting::choice(
            WORKSPACE_LAYOUT,
            Page::WindowBehavior,
            "default",
            WORKSPACE_LAYOUT_CHOICES,
        )
        .section("Workspaces")
        .title("Layout of new workspaces")
        .keywords(&["tabs", "stack", "tiling"])
        .apply(Apply::Reload)
        .sway(
            Directive::top("workspace_layout"),
            ConfigFile::Behavior,
            workspace_layout,
            |args| read::choice(read::one(args)?, &["default", "stacking", "tabbed"], false),
        )
        .build(),
        Setting::choice(
            DEFAULT_ORIENTATION,
            Page::WindowBehavior,
            "auto",
            DEFAULT_ORIENTATION_CHOICES,
        )
        .section("Workspaces")
        .title("Split of new workspaces")
        .subtitle("How the windows of a new workspace are arranged")
        .keywords(&["default orientation", "horizontal", "vertical", "tiling"])
        .apply(Apply::Reload)
        .sway(
            Directive::top("default_orientation"),
            ConfigFile::Behavior,
            default_orientation,
            |args| read::choice(read::one(args)?, &["horizontal", "vertical", "auto"], false),
        )
        .build(),
        Setting::toggle(BACK_AND_FORTH, Page::WindowBehavior, false)
            .section("Workspaces")
            .title("Back and forth")
            .subtitle("Switching to the current workspace again goes back to the previous one")
            .keywords(&[
                "workspace_auto_back_and_forth",
                "previous workspace",
                "toggle",
            ])
            .sway(
                Directive::top("workspace_auto_back_and_forth"),
                ConfigFile::Behavior,
                |ctx| {
                    Ok(vec![behavior::workspace_auto_back_and_forth(
                        ctx.bool(BACK_AND_FORTH)?,
                    )])
                },
                |args| Ok(Value::Bool(read::boolean(read::one(args)?, false))),
            )
            .build(),
        Setting::choice(
            MODIFIER_KEY,
            Page::WindowBehavior,
            "none",
            MODIFIER_KEY_CHOICES,
        )
        .section("Mouse")
        .group("behavior.floating_modifier", "Drag modifier")
        .title("Drag modifier")
        .subtitle("Hold this key to move and resize windows from anywhere inside them")
        .keywords(&["floating modifier", "mod key", "move", "resize"])
        .sway(
            Directive::top("floating_modifier"),
            ConfigFile::Behavior,
            floating_modifier,
            |args| Ok(Value::Choice(read_floating_modifier(args)?.0.to_owned())),
        )
        .build(),
        Setting::choice(
            MODIFIER_BUTTONS,
            Page::WindowBehavior,
            "normal",
            MODIFIER_BUTTON_CHOICES,
        )
        .section("Mouse")
        .group("behavior.floating_modifier", "Drag modifier")
        .title("Mouse buttons with the drag modifier")
        .keywords(&["floating modifier", "inverse", "move", "resize"])
        .depends_on(MODIFIER_KEY, When::OneOf(MODIFIER_KEYS))
        .sway(
            Directive::top("floating_modifier"),
            ConfigFile::Behavior,
            floating_modifier,
            |args| Ok(Value::Choice(read_floating_modifier(args)?.1.to_owned())),
        )
        .build(),
        Setting::toggle(TILING_DRAG, Page::WindowBehavior, true)
            .section("Mouse")
            .title("Drag tiled windows")
            .subtitle("By the title bar, or anywhere with the drag modifier")
            .keywords(&["tiling drag", "move", "rearrange"])
            .sway(
                Directive::top("tiling_drag"),
                ConfigFile::Behavior,
                |ctx| Ok(vec![behavior::tiling_drag(ctx.bool(TILING_DRAG)?)]),
                |args| Ok(Value::Bool(read::boolean(read::one(args)?, true))),
            )
            .build(),
        Setting::integer(
            TILING_DRAG_THRESHOLD,
            Page::WindowBehavior,
            9,
            0,
            i64::from(u16::MAX),
        )
        .section("Mouse")
        .title("Drag threshold")
        .subtitle("How far a title bar must be dragged before the window moves, in pixels")
        .slider(0.0, 50.0, 1.0)
        .keywords(&["tiling drag threshold", "distance"])
        .depends_on(TILING_DRAG, When::On)
        .sway(
            Directive::top("tiling_drag_threshold"),
            ConfigFile::Behavior,
            |ctx| {
                Ok(vec![behavior::tiling_drag_threshold(
                    ctx.integer(TILING_DRAG_THRESHOLD)?,
                )])
            },
            |args| match read::whole(read::one(args)?)? {
                px if px >= 0 => Ok(Value::Integer(px)),
                px => Err(ReadError::Rejected(format!("negative threshold: {px}"))),
            },
        )
        .build(),
    ];
    settings.extend(size_settings::<0>());
    settings.extend(size_settings::<1>());
    settings.extend([
        Setting::choice(
            POPUP_DURING_FULLSCREEN,
            Page::WindowBehavior,
            "smart",
            POPUP_CHOICES,
        )
        .section("Fullscreen")
        .title("Dialogs of a fullscreen window")
        .keywords(&["popup during fullscreen", "popups", "dialog"])
        .sway(
            Directive::top("popup_during_fullscreen"),
            ConfigFile::Behavior,
            popup_during_fullscreen,
            |args| {
                read::choice(
                    read::one(args)?,
                    &["smart", "ignore", "leave_fullscreen"],
                    false,
                )
            },
        )
        .build(),
        Setting::toggle(INHIBIT_IDLE_FULLSCREEN, Page::WindowBehavior, false)
            .section("Fullscreen")
            .title("Stay awake while a window is fullscreen")
            .subtitle("Keeps the screen from locking or turning off, e.g. during a video")
            .keywords(&["inhibit idle", "screensaver", "lock", "video", "sleep"])
            .apply(Apply::Reload)
            .sway(
                Directive::top("for_window").arguments(&["[all]", "inhibit_idle"]),
                ConfigFile::Behavior,
                inhibit_idle_fullscreen,
                read_inhibit_idle,
            )
            .build(),
        Setting::choice(XWAYLAND, Page::WindowBehavior, "enable", XWAYLAND_CHOICES)
            .section("X11 apps")
            .title("Start Xwayland")
            .subtitle("Runs apps made for X11")
            .keywords(&["xwayland", "x11", "xorg", "legacy"])
            .apply(Apply::Relogin)
            .sway(
                Directive::top("xwayland"),
                ConfigFile::Behavior,
                xwayland,
                |args| {
                    let id = match read::one(args)? {
                        "force" => "force",
                        word if read::boolean(word, true) => "enable",
                        _ => "disable",
                    };
                    Ok(Value::Choice(id.to_owned()))
                },
            )
            .build(),
    ]);
    settings
}
