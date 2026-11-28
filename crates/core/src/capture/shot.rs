// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A screenshot: what it captures, taken with grim, and where it goes.
//!
//! - **A region:** slurp lets the user drag a rectangle, or click one of the visible windows,
//!   which it gets as boxes on its standard input. Escape or another button cancels, which is no
//!   error ([`CaptureError::Canceled`]).
//! - **The focused window:** its rectangle from `GET_TREE` (with its borders, without the title
//!   bar).
//! - **The focused display:** by its name from `GET_OUTPUTS`.
//! - **All displays:** grim without arguments.
//!
//! Coordinates are sway's layout coordinates, which grim takes as they are, also across displays
//! of different scales: it takes the picture at the largest scale it covers. grim writes the
//! picture to its standard output; it is saved as a new file (numbered if one of its name exists)
//! and copied with `wl-copy --type <mime>`.

use std::ffi::OsStr;
use std::path::PathBuf;

use super::tools::{self, GRIM, SLURP, WL_COPY};
use super::{CaptureError, Settings, name};
use crate::ipc::Client;
use crate::ipc::types::{Node, NodeType, Output, Rect};

/// What a screenshot captures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Area {
    /// A region the user selects, or a window clicked.
    Region,
    /// The focused window.
    Window,
    /// The display with the focus.
    Display,
    /// Every display.
    All,
}

impl Area {
    /// Every area, in the order of the page.
    pub const ALL: [Self; 4] = [Self::Region, Self::Window, Self::Display, Self::All];

    /// The name the command line takes.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Region => "region",
            Self::Window => "window",
            Self::Display => "display",
            Self::All => "all",
        }
    }
}

/// What grim is told to capture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// A rectangle of the layout, as `x,y wxh`.
    Geometry(String),
    /// A display, by its name.
    Output(String),
    /// Every display.
    All,
}

impl Target {
    /// grim's arguments for it.
    pub fn grim_args(&self) -> Vec<String> {
        match self {
            Self::Geometry(geometry) => vec!["-g".to_owned(), geometry.clone()],
            Self::Output(name) => vec!["-o".to_owned(), name.clone()],
            Self::All => Vec::new(),
        }
    }
}

/// `rect` as grim and slurp write a rectangle: `x,y wxh`.
pub fn geometry(rect: &Rect) -> String {
    format!("{},{} {}x{}", rect.x, rect.y, rect.width, rect.height)
}

/// Whether `node` is a window (a view), not a container of others or a workspace.
fn is_window(node: &Node) -> bool {
    matches!(node.node_type, NodeType::Con | NodeType::FloatingCon)
        && node.nodes.is_empty()
        && node.floating_nodes.is_empty()
        && (node.pid.is_some() || node.app_id.is_some() || node.window.is_some())
}

/// Every node of `tree`, depth first.
fn walk(tree: &Node) -> Vec<&Node> {
    let mut nodes = vec![tree];
    let mut index = 0;
    while let Some(node) = nodes.get(index).copied() {
        nodes.extend(node.nodes.iter().chain(&node.floating_nodes));
        index += 1;
    }
    nodes
}

/// The rectangle of the focused window in `tree`; `None` if the focus is on no window (an empty
/// workspace).
pub fn focused_window(tree: &Node) -> Option<Rect> {
    walk(tree)
        .into_iter()
        .find(|node| node.focused && is_window(node))
        .map(|node| node.rect)
        .filter(|rect| rect.width > 0 && rect.height > 0)
}

/// The rectangles of the windows on screen in `tree`, for slurp to offer: those sway reports as
/// visible.
pub fn visible_windows(tree: &Node) -> Vec<Rect> {
    walk(tree)
        .into_iter()
        .filter(|node| is_window(node) && node.visible == Some(true))
        .map(|node| node.rect)
        .filter(|rect| rect.width > 0 && rect.height > 0)
        .collect()
}

/// The name of the display with the focus among `outputs`.
pub fn focused_output(outputs: &[Output]) -> Option<String> {
    outputs
        .iter()
        .find(|output| output.focused && output.active)
        .map(|output| output.name.clone())
}

/// Lets the user select a region with slurp, offering `boxes` to click.
///
/// # Blocking
///
/// Waits for the program to end.
///
/// # Errors
///
/// [`CaptureError::Canceled`] when the user canceled, else the errors of [`tools::run`].
pub fn select(boxes: &[Rect]) -> Result<String, CaptureError> {
    let input = boxes.iter().fold(String::new(), |mut input, rect| {
        input.push_str(&geometry(rect));
        input.push('\n');
        input
    });
    // slurp reads boxes from a standard input that isn't a terminal; an empty one offers none.
    match tools::run(SLURP, &[], Some(input.as_bytes())) {
        Ok(output) => {
            let text = String::from_utf8_lossy(&output).trim().to_owned();
            if text.is_empty() {
                Err(CaptureError::Canceled)
            } else {
                Ok(text)
            }
        }
        Err(CaptureError::Failed { message, .. }) if message.contains("cancel") => {
            Err(CaptureError::Canceled)
        }
        Err(error) => Err(error),
    }
}

/// What to capture for `area`: asks sway for the focused window or display, and slurp for a
/// region (with the windows on screen to click, if sway can be asked).
///
/// # Errors
///
/// [`CaptureError::Canceled`], [`CaptureError::NoWindow`], [`CaptureError::NoDisplay`],
/// [`CaptureError::Sway`] if sway can't be asked, and those of [`select`].
pub async fn target(area: Area, client: Option<&mut Client>) -> Result<Target, CaptureError> {
    let sway = |error: crate::ipc::IpcError| CaptureError::Sway(error.text());
    match area {
        Area::All => Ok(Target::All),
        Area::Region => {
            let boxes = match client {
                Some(client) => client
                    .tree()
                    .await
                    .map(|tree| visible_windows(&tree))
                    .unwrap_or_default(),
                None => Vec::new(),
            };
            // slurp waits for the user: on the blocking pool.
            blocking::unblock(move || select(&boxes))
                .await
                .map(Target::Geometry)
        }
        Area::Window => {
            let client = client.ok_or(CaptureError::NoWindow)?;
            let tree = client.tree().await.map_err(sway)?;
            focused_window(&tree)
                .map(|rect| Target::Geometry(geometry(&rect)))
                .ok_or(CaptureError::NoWindow)
        }
        Area::Display => {
            let client = client.ok_or(CaptureError::NoDisplay)?;
            let outputs = client.outputs().await.map_err(sway)?;
            focused_output(&outputs)
                .map(Target::Output)
                .ok_or(CaptureError::NoDisplay)
        }
    }
}

/// The picture formats grim writes, by the extension of the file name: `png`, `jpg`/`jpeg` and
/// `ppm`; the first is the default.
pub const EXTENSIONS: [&str; 4] = ["png", "jpg", "jpeg", "ppm"];

/// grim's file type and the media type for the extension `ext`.
pub fn format(ext: &str) -> (&'static str, &'static str) {
    match ext {
        "jpg" | "jpeg" => ("jpeg", "image/jpeg"),
        "ppm" => ("ppm", "image/x-portable-pixmap"),
        _ => ("png", "image/png"),
    }
}

/// A screenshot that was taken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shot {
    /// The picture.
    pub bytes: Vec<u8>,
    /// Its media type (`image/png`).
    pub mime: &'static str,
    /// Its file, if it was saved.
    pub path: Option<PathBuf>,
    /// Whether it was copied to the clipboard.
    pub copied: bool,
    /// Why the other half of _Copy and save_ didn't work, when only one did: saved but not
    /// copied, or copied but not saved.
    pub failed: Option<CaptureError>,
}

/// Takes the picture of `target` with grim, in the format of `ext`.
///
/// # Blocking
///
/// Waits for the program to end.
///
/// # Errors
///
/// Those of [`tools::run`].
pub fn grim(target: &Target, ext: &str) -> Result<Vec<u8>, CaptureError> {
    let (kind, _) = format(ext);
    let mut args: Vec<String> = vec!["-t".to_owned(), kind.to_owned()];
    args.extend(target.grim_args());
    // To standard output.
    args.push("-".to_owned());
    let args: Vec<&OsStr> = args.iter().map(OsStr::new).collect();
    tools::run(GRIM, &args, None)
}

/// Puts `bytes` on the clipboard with wl-copy, of the media type `mime` or, without one, of the
/// type wl-copy finds. wl-copy forks and stays in the background to hand them out until
/// something else is copied, so no pipe to its output is kept, which it would hold open.
///
/// # Blocking
///
/// Waits for wl-copy to take the data.
///
/// # Errors
///
/// Those of [`tools::run`].
pub fn wl_copy(bytes: &[u8], mime: Option<&str>) -> Result<(), CaptureError> {
    use std::io::Write as _;
    use std::process::{Command, Stdio};
    let program = WL_COPY;
    let path = program.require()?;
    let failed = |message: String| CaptureError::Failed {
        program: program.name.to_owned(),
        message,
    };
    let mut child = Command::new(path)
        .args(mime.map(|mime| ["--type", mime]).iter().flatten())
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| failed(error.to_string()))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(bytes)
            .map_err(|error| failed(error.to_string()))?;
    }
    let status = child.wait().map_err(|error| failed(error.to_string()))?;
    if status.success() {
        Ok(())
    } else {
        Err(failed(status.to_string()))
    }
}

/// Takes a screenshot of `target` and saves and copies it as `settings` say. The name is made
/// before the picture is taken, so a pattern that can't be used costs no picture. Saving and
/// copying are tried each on their own: when only one of them works, the shot is returned with
/// [`Shot::failed`] saying why the other didn't, so the picture is never lost.
///
/// # Blocking
///
/// Waits for wl-copy to take the data.
///
/// # Errors
///
/// [`CaptureError::Time`] without the local time, [`CaptureError::FileName`] for a pattern that
/// makes no name, the errors of [`grim`], and those of saving ([`CaptureError::Write`]) or
/// copying ([`wl_copy`]) when nothing of what was asked worked.
pub fn take(target: &Target, settings: &Settings) -> Result<Shot, CaptureError> {
    let now = gio::glib::DateTime::now_local().map_err(|_| CaptureError::Time)?;
    let file_name = name::file_name(&settings.file_name, &now).ok_or(CaptureError::FileName)?;
    let (file_name, ext) = name::with_extension(&file_name, &EXTENSIONS);
    let (_, mime) = format(ext);
    if !settings.destination.saves() {
        // Nothing else could keep the picture.
        WL_COPY.require()?;
    }
    let bytes = grim(target, ext)?;
    let saved = settings.destination.saves().then(|| {
        name::write_new(&settings.folder, &file_name, &bytes).map_err(|error| CaptureError::Write {
            path: settings.folder.join(&file_name),
            error: error.to_string(),
        })
    });
    let copy = settings
        .destination
        .copies()
        .then(|| wl_copy(&bytes, Some(mime)));
    let (path, save_error) = match saved {
        Some(Ok(path)) => (Some(path), None),
        Some(Err(error)) => (None, Some(error)),
        None => (None, None),
    };
    let (copied, copy_error) = match copy {
        Some(Ok(())) => (true, None),
        Some(Err(error)) => (false, Some(error)),
        None => (false, None),
    };
    if path.is_none() && !copied {
        // Nothing worked: the first error says why.
        return Err(save_error.or(copy_error).unwrap_or(CaptureError::FileName));
    }
    Ok(Shot {
        bytes,
        mime,
        path,
        copied,
        failed: save_error.or(copy_error),
    })
}
