// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Settings of the Window Behavior page (`man 5 sway`).

use super::read;
use super::{Apply, Choice, ConfigFile, Context, Directive, Page, RenderError, Setting};
use crate::command::SwayCmd;
use crate::command::behavior::{self, FocusFollowsMouse, WorkspaceLayout};

const FOCUS_FOLLOWS_MOUSE: &str = "behavior.focus_follows_mouse";
const FOCUS_FOLLOWS_MOUSE_CHOICES: &[Choice] = &[
    Choice::new("yes", "When the pointer enters a window"),
    Choice::new("no", "Only on click"),
    Choice::new("always", "Always the window under the pointer"),
];

const WORKSPACE_LAYOUT: &str = "behavior.workspace_layout";
const WORKSPACE_LAYOUT_CHOICES: &[Choice] = &[
    Choice::new("default", "Tiled"),
    Choice::new("stacking", "Stacked"),
    Choice::new("tabbed", "Tabbed"),
];

fn focus_follows_mouse(ctx: &Context<'_>) -> Result<Vec<SwayCmd>, RenderError> {
    let mode = match ctx.choice(FOCUS_FOLLOWS_MOUSE)? {
        "no" => FocusFollowsMouse::No,
        "always" => FocusFollowsMouse::Always,
        _ => FocusFollowsMouse::Yes,
    };
    Ok(vec![behavior::focus_follows_mouse(mode)])
}

fn workspace_layout(ctx: &Context<'_>) -> Result<Vec<SwayCmd>, RenderError> {
    let layout = match ctx.choice(WORKSPACE_LAYOUT)? {
        "stacking" => WorkspaceLayout::Stacking,
        "tabbed" => WorkspaceLayout::Tabbed,
        _ => WorkspaceLayout::Default,
    };
    Ok(vec![behavior::workspace_layout(layout)])
}

pub(super) fn settings() -> Vec<Setting> {
    vec![
        Setting::choice(
            FOCUS_FOLLOWS_MOUSE,
            Page::WindowBehavior,
            "yes",
            FOCUS_FOLLOWS_MOUSE_CHOICES,
        )
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
            WORKSPACE_LAYOUT,
            Page::WindowBehavior,
            "default",
            WORKSPACE_LAYOUT_CHOICES,
        )
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
    ]
}
