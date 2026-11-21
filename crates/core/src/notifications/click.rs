// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Clicking a notification shows its app: the window of the app that sent it, found in sway's
//! tree by the desktop entry and the app name the notification carries, gets the focus, and the
//! pointer moves to its middle. The app itself opens what the notification is about (the chat,
//! the mail) through the notification's default action, which the daemon invokes as before.
//!
//! The names come from whoever sent the notification, so they are only compared, never put into
//! a command: the window is focused by its container id.

use std::path::Path;

use swayipc_types::{Node, NodeType};

use crate::command::{Criteria, SwayCmd, seat};

/// The workspace sway keeps the scratchpad in.
const SCRATCHPAD: &str = "__i3_scratch";

/// A window in sway's tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Window {
    /// Its container id.
    pub id: i64,
    /// `app_id` of a Wayland window.
    pub app_id: Option<String>,
    /// `class` and `instance` of an Xwayland window.
    pub class: Vec<String>,
    /// The workspace it is on (`__i3_scratch` for the scratchpad), if any.
    pub workspace: Option<String>,
    /// Its middle, in the layout.
    pub middle: (i64, i64),
}

impl Window {
    /// Whether it is hidden in the scratchpad, or has no place on screen.
    pub fn is_hidden(&self) -> bool {
        self.workspace
            .as_deref()
            .is_none_or(|workspace| workspace == SCRATCHPAD)
    }
}

/// Every window of `tree`, tiled or floating, with its workspace.
pub fn windows(tree: &Node) -> Vec<Window> {
    let mut found = Vec::new();
    collect(tree, None, &mut found);
    found
}

fn collect(node: &Node, workspace: Option<&str>, found: &mut Vec<Window>) {
    let workspace = if node.node_type == NodeType::Workspace {
        node.name.as_deref()
    } else {
        workspace
    };
    let is_window = matches!(node.node_type, NodeType::Con | NodeType::FloatingCon)
        && (node.app_id.is_some() || node.window_properties.is_some());
    if is_window {
        let class = node
            .window_properties
            .iter()
            .flat_map(|properties| [properties.class.clone(), properties.instance.clone()])
            .flatten()
            .collect();
        found.push(Window {
            id: node.id,
            app_id: node.app_id.clone(),
            class,
            workspace: workspace.map(ToOwned::to_owned),
            middle: (
                i64::from(node.rect.x) + i64::from(node.rect.width) / 2,
                i64::from(node.rect.y) + i64::from(node.rect.height) / 2,
            ),
        });
    }
    for child in node.nodes.iter().chain(&node.floating_nodes) {
        collect(child, workspace, found);
    }
}

/// The forms of a name to compare: as it is in lowercase, without `.desktop`, the last part of
/// a reverse-DNS name (`org.keepassxc.KeePassXC` → `keepassxc`), and without `-desktop`
/// (`signal-desktop` → `signal`).
fn forms(name: &str) -> Vec<String> {
    let name = name.trim().to_lowercase();
    if name.is_empty() {
        return Vec::new();
    }
    let mut forms = vec![name.clone()];
    let base = name.strip_suffix(".desktop").unwrap_or(&name).to_owned();
    forms.push(base.clone());
    let last = base.rsplit('.').next().unwrap_or(&base).to_owned();
    forms.push(last.clone());
    for form in [base, last] {
        if let Some(short) = form.strip_suffix("-desktop") {
            forms.push(short.to_owned());
        }
    }
    forms.retain(|form| !form.is_empty());
    forms.dedup();
    forms
}

/// The workspaces to look on first: the focused one, then the visible ones.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Workspaces {
    /// The focused workspace.
    pub focused: Option<String>,
    /// The workspaces visible on some display.
    pub visible: Vec<String>,
}

impl Workspaces {
    /// How near a window on `workspace` is to the user: 0 on the focused workspace, 1 on a
    /// visible one, 2 elsewhere, 3 hidden in the scratchpad. sway keeps no history of focus,
    /// so this stands in for "the window used last".
    fn rank(&self, window: &Window) -> u8 {
        match window.workspace.as_deref() {
            _ if window.is_hidden() => 3,
            Some(workspace) if self.focused.as_deref() == Some(workspace) => 0,
            Some(workspace) if self.visible.iter().any(|visible| visible == workspace) => 1,
            _ => 2,
        }
    }
}

/// The window of the app with the desktop entry `entry` and the name `app` (either may be
/// empty): one whose `app_id` or class equals a form of either, compared without case, else one
/// where one contains the other (four letters at least; never window titles). Among several,
/// the nearest by `workspaces` ([`Workspaces::rank`]).
pub fn find<'a>(
    windows: &'a [Window],
    entry: &str,
    app: &str,
    workspaces: &Workspaces,
) -> Option<&'a Window> {
    let wanted: Vec<String> = forms(entry).into_iter().chain(forms(app)).collect();
    if wanted.is_empty() {
        return None;
    }
    let names = |window: &Window| -> Vec<String> {
        window
            .app_id
            .iter()
            .chain(&window.class)
            .flat_map(|name| forms(name))
            .collect()
    };
    let exact = windows
        .iter()
        .filter(|window| names(window).iter().any(|name| wanted.contains(name)))
        .min_by_key(|window| workspaces.rank(window));
    exact.or_else(|| {
        windows
            .iter()
            .filter(|window| {
                names(window).iter().any(|name| {
                    wanted.iter().any(|want| {
                        want.chars().count() >= 4
                            && name.chars().count() >= 4
                            && (name.contains(want.as_str()) || want.contains(name.as_str()))
                    })
                })
            })
            .min_by_key(|window| workspaces.rank(window))
    })
}

/// The command that focuses `window` by its id.
///
/// # Errors
///
/// None in practice: the command is built from a number.
pub fn focus(window: &Window) -> Result<SwayCmd, crate::command::CommandError> {
    SwayCmd::with_criteria(Criteria::con_id(window.id), SwayCmd::new("focus"))
}

/// The command that moves the pointer to the middle of `window` as the tree shows it after the
/// focus moved there; `None` for a window hidden in the scratchpad.
pub fn warp(window: &Window) -> Option<SwayCmd> {
    (!window.is_hidden()).then(|| seat::cursor_set(window.middle.0, window.middle.1))
}

/// Whether the sway listening on `socket` is the one of this session: its socket is in
/// `runtime_dir` (this session's `XDG_RUNTIME_DIR`), as is the Wayland display `display` the
/// notification daemon shows on, if it names one.
pub fn same_session(socket: &Path, runtime_dir: &Path, display: Option<&str>) -> bool {
    let canonical = |path: &Path| std::fs::canonicalize(path).ok();
    let Some(runtime) = canonical(runtime_dir) else {
        return false;
    };
    let in_runtime = socket
        .parent()
        .and_then(canonical)
        .is_some_and(|dir| dir == runtime);
    let display_there = display.is_none_or(|display| {
        let display = Path::new(display);
        if display.is_absolute() {
            display.starts_with(&runtime)
        } else {
            runtime.join(display).exists()
        }
    });
    in_runtime && display_there
}

#[cfg(test)]
mod tests {
    use super::{Window, Workspaces, find, forms, same_session, warp};

    fn wayland(id: i64, app_id: &str) -> Window {
        Window {
            id,
            app_id: Some(app_id.to_owned()),
            class: Vec::new(),
            workspace: Some("1".to_owned()),
            middle: (0, 0),
        }
    }

    fn xwayland(id: i64, class: &str, instance: &str) -> Window {
        Window {
            id,
            app_id: None,
            class: vec![class.to_owned(), instance.to_owned()],
            workspace: Some("1".to_owned()),
            middle: (0, 0),
        }
    }

    fn on(mut window: Window, workspace: &str) -> Window {
        window.workspace = Some(workspace.to_owned());
        window
    }

    fn sample() -> Vec<Window> {
        vec![
            wayland(1, "foot"),
            xwayland(2, "Signal", "signal"),
            wayland(3, "Zulip"),
            wayland(4, "org.keepassxc.KeePassXC"),
            wayland(5, "chrome-agimnkijcaahngcdmfeangaknmldooml-Default"),
            wayland(6, "firefox"),
        ]
    }

    #[test]
    fn names_are_compared_in_their_short_forms() {
        assert_eq!(
            forms("org.keepassxc.KeePassXC.desktop"),
            [
                "org.keepassxc.keepassxc.desktop",
                "org.keepassxc.keepassxc",
                "keepassxc"
            ]
        );
        assert_eq!(forms("signal-desktop"), ["signal-desktop", "signal"]);
        assert_eq!(forms("  "), Vec::<String>::new());
    }

    #[test]
    fn the_window_of_the_app_is_found() {
        let windows = sample();
        let id = |entry: &str, app: &str| {
            find(&windows, entry, app, &Workspaces::default()).map(|window| window.id)
        };
        // Signal runs under Xwayland with the class "Signal"; its desktop entry is
        // signal-desktop and its app name "Signal".
        assert_eq!(id("signal-desktop", "Signal"), Some(2));
        assert_eq!(id("", "signal"), Some(2));
        // Zulip's desktop entry is reverse-DNS, its window app_id plain.
        assert_eq!(id("org.zulip.Zulip", "Zulip"), Some(3));
        // KeePassXC: the window has the reverse-DNS app_id, the notification the plain name.
        assert_eq!(id("", "KeePassXC"), Some(4));
        assert_eq!(id("org.keepassxc.KeePassXC", ""), Some(4));
        // A Chrome app: desktop entry and app_id are the same.
        assert_eq!(
            id(
                "chrome-agimnkijcaahngcdmfeangaknmldooml-Default",
                "WhatsApp Web"
            ),
            Some(5)
        );
        // Firefox sends "Firefox" with the desktop entry "firefox-esr" on Debian.
        assert_eq!(id("firefox-esr", "Firefox"), Some(6));
    }

    #[test]
    fn nothing_is_found_for_an_app_without_a_window() {
        let windows = sample();
        let none = Workspaces::default();
        assert_eq!(find(&windows, "thunderbird", "Thunderbird", &none), None);
        assert_eq!(find(&windows, "", "", &none), None);
        // Three letters are too few to guess from.
        assert_eq!(find(&windows, "", "fox", &none), None);
    }

    #[test]
    fn the_nearest_window_of_the_app_wins_and_an_exact_name_before_a_guess() {
        let windows = vec![
            on(wayland(1, "firefox"), "3"),
            on(wayland(2, "firefox"), "2"),
            on(wayland(3, "firefox"), "__i3_scratch"),
            on(wayland(4, "firefox-developer-edition"), "1"),
        ];
        let at = |focused: &str, visible: &[&str]| Workspaces {
            focused: Some(focused.to_owned()),
            visible: visible.iter().map(|name| (*name).to_owned()).collect(),
        };
        let id = |workspaces: &Workspaces| {
            find(&windows, "firefox", "Firefox", workspaces).map(|window| window.id)
        };
        assert_eq!(id(&at("2", &["2"])), Some(2));
        assert_eq!(id(&at("1", &["1", "3"])), Some(1));
        // An exact name elsewhere beats a guess on the focused workspace.
        assert_eq!(id(&at("1", &["1"])), Some(1));
        // A window in the scratchpad is shown, but the pointer isn't moved to it.
        assert!(warp(&windows[2]).is_none());
        assert!(warp(&windows[0]).is_some());
    }

    #[test]
    fn only_a_sway_of_this_session_is_used() {
        let runtime = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        std::fs::write(runtime.path().join("wayland-1"), "").unwrap();
        let socket = runtime.path().join("sway-ipc.1000.1.sock");
        assert!(same_session(&socket, runtime.path(), Some("wayland-1")));
        assert!(same_session(&socket, runtime.path(), None));
        assert!(!same_session(&socket, runtime.path(), Some("wayland-9")));
        assert!(!same_session(
            &other.path().join("sway-ipc.1000.1.sock"),
            runtime.path(),
            None
        ));
    }
}
