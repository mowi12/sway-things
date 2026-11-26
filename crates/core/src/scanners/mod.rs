// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Scanners through SANE: the scanners it finds, what is missing for the others, and scanning
//! documents into files.
//!
//! [`Sane`] runs `scanimage` from SANE (sane-utils, sane-backends), always with an argument
//! array: `-f` lists the scanners ([`list`]), `-A` tells what one can do ([`options`]), and a
//! scan writes its pages into a directory of ours ([`ScanDir`]), from where [`save`] puts them
//! into the one file the user picked, as an image or a PDF ([`pdf`]). SANE's backends do the
//! looking, on USB and on the network (sane-airscan, SANE's escl backend), as cupsd does for
//! printers; Sway Things contacts no scanner itself, and uses no libsane (that would need
//! `unsafe`). Avahi (`_uscan._tcp`, `_uscans._tcp`) tells of driverless scanners on the network
//! that SANE doesn't see ([`crate::printers::Discovery::scanners`]), and [`look`] what is
//! missing on this computer: scanimage, sane-airscan, ipp-usb, the access to a USB scanner.
//!
//! Nothing here is stored by Sway Things. Names come from scanners, backends and the network:
//! they are data, shown as plain text and only ever passed to scanimage as one argument.

pub mod list;
pub mod look;
pub mod options;
pub mod pdf;
mod sane;

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub use list::{Connection, Scanner};
pub use look::{Hint, Look};
pub use options::{ModeKind, Options, SourceKind};
pub use sane::{Canceler, Format, LIST_TIMEOUT, Progress, Request, Sane, Scan, parse_formats};

pub(crate) use crate::dbus::clean;
use crate::i18n::{Msg, Text};
use crate::msg;

/// Whether one of `scanners` SANE lists is the driverless scanner Avahi `found`: sane-airscan
/// names its devices after the service (`airscan:e0:NAME`) and gives that name as the model,
/// and SANE's escl backend gives the model the scanner announces (`ty`).
pub fn sees(scanners: &[Scanner], found: &crate::printers::Found) -> bool {
    let name = found.name.to_lowercase();
    let model = found.model.to_lowercase();
    scanners.iter().any(|scanner| {
        let device = scanner.device.to_lowercase();
        let title = scanner.title().to_lowercase();
        (!name.is_empty() && (device.ends_with(&format!(":{name}")) || title == name))
            || (!model.is_empty() && (title == model || title.ends_with(&format!(" {model}"))))
    })
}

/// Why scanimage failed, in words a user can act on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Problem {
    /// Another program uses the scanner (`SANE_STATUS_DEVICE_BUSY`).
    Busy,
    /// Its cover is open (`SANE_STATUS_COVER_OPEN`).
    CoverOpen,
    /// Paper is jammed in the feeder (`SANE_STATUS_JAMMED`).
    Jammed,
    /// The feeder is empty (`SANE_STATUS_NO_DOCS`).
    NoDocuments,
    /// The user may not use it (`SANE_STATUS_ACCESS_DENIED`).
    AccessDenied,
    /// It stopped answering (`SANE_STATUS_IO_ERROR`).
    Io,
    /// It can't be opened: unplugged, off, or gone from the network.
    Gone,
    /// It doesn't take a value it was given (`SANE_STATUS_INVAL`).
    Invalid,
    /// Something else.
    Other,
}

impl Problem {
    /// The problem of scanimage ending with `status` after printing `stderr`: its exit status
    /// is SANE's status of a failed read, and its message names the others.
    pub fn of(status: Option<i32>, stderr: &str) -> Self {
        let text = stderr.to_ascii_lowercase();
        if text.contains("open of device") {
            return Self::Gone;
        }
        let by_text = [
            ("device busy", Self::Busy),
            ("cover is open", Self::CoverOpen),
            ("feeder jammed", Self::Jammed),
            ("out of documents", Self::NoDocuments),
            ("access to resource has been denied", Self::AccessDenied),
            ("error during device i/o", Self::Io),
            ("invalid argument", Self::Invalid),
        ];
        if let Some((_, problem)) = by_text.iter().find(|(words, _)| text.contains(words)) {
            return *problem;
        }
        match status {
            Some(3) => Self::Busy,
            Some(4) => Self::Invalid,
            Some(6) => Self::Jammed,
            Some(7) => Self::NoDocuments,
            Some(8) => Self::CoverOpen,
            Some(9) => Self::Io,
            Some(11) => Self::AccessDenied,
            _ => Self::Other,
        }
    }

    /// The problem in words.
    pub fn text(self) -> Msg {
        match self {
            Self::Busy => msg!("The scanner is busy: another program may be using it."),
            Self::CoverOpen => msg!("The scanner's cover is open. Close it and try again."),
            Self::Jammed => msg!("Paper is jammed in the document feeder."),
            Self::NoDocuments => msg!("There is no paper in the document feeder."),
            Self::AccessDenied => {
                msg!("You aren't allowed to use the scanner. See the hints on the page.")
            }
            Self::Io => msg!("The scanner stopped answering. Check its cable or network."),
            Self::Gone => msg!(
                "The scanner can't be reached. Make sure it is on and connected, then search \
                 again."
            ),
            Self::Invalid => msg!("The scanner doesn't take one of the settings."),
            Self::Other => msg!("The scanner reported an error."),
        }
    }
}

/// What went wrong with scanimage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// scanimage isn't installed.
    Missing,
    /// It didn't answer in time.
    Timeout,
    /// It failed: why, and what it said.
    Failed {
        /// Why, in words.
        problem: Problem,
        /// What it printed.
        details: String,
    },
    /// The scan was canceled.
    Canceled,
    /// A file couldn't be read or written.
    Io(String),
}

impl Error {
    /// The error in words.
    pub fn text(&self) -> Text {
        match self {
            Self::Missing => msg!("scanimage isn't installed.").into(),
            Self::Timeout => msg!("The scanner didn't answer in time.").into(),
            Self::Failed { problem, .. } => problem.text().into(),
            Self::Canceled => msg!("Canceled.").into(),
            Self::Io(error) => {
                Text::new(msg!("A file can't be written: {error}")).arg("error", error)
            }
        }
    }

    /// What scanimage printed, for the details of the error; empty if nothing.
    pub fn details(&self) -> &str {
        match self {
            Self::Failed { details, .. } => details,
            _ => "",
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.text().english())
    }
}

impl std::error::Error for Error {}

/// A directory of our own for the pages of a scan, until they are saved: in
/// `$XDG_CACHE_HOME/sway-things/scans`, readable only by the user, removed when dropped.
#[derive(Debug)]
pub struct ScanDir {
    path: PathBuf,
}

impl ScanDir {
    /// A new, empty directory in `parent` (made if missing). Directories older than a day that
    /// a crashed Sway Things left there are removed first.
    ///
    /// # Blocking
    ///
    /// Reads the directory of the scans.
    ///
    /// # Errors
    ///
    /// If it can't be made.
    pub fn new_in(parent: &Path) -> io::Result<Self> {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(parent)?;
        remove_stale(parent);
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |time| time.as_nanos());
        for attempt in 0..100u32 {
            let path = parent.join(format!("scan-{}-{stamp}-{attempt}", std::process::id()));
            match fs::DirBuilder::new().mode(0o700).create(&path) {
                Ok(()) => return Ok(Self { path }),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(io::ErrorKind::AlreadyExists, "no free name"))
    }

    /// A new directory in the user's cache.
    ///
    /// # Blocking
    ///
    /// Reads the directory of the scans.
    ///
    /// # Errors
    ///
    /// If it can't be made, or there is no home directory.
    pub fn new() -> io::Result<Self> {
        let cache = std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .filter(|path| path.is_absolute())
                    .map(|home| home.join(".cache"))
            })
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no home directory"))?;
        Self::new_in(&cache.join("sway-things").join("scans"))
    }

    /// Its path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// A new, empty directory inside it for one scan.
    ///
    /// # Errors
    ///
    /// If it can't be made.
    pub fn batch(&self) -> io::Result<PathBuf> {
        use std::os::unix::fs::DirBuilderExt;
        for number in 0..10_000u32 {
            let path = self.path.join(format!("batch-{number:04}"));
            match fs::DirBuilder::new().mode(0o700).create(&path) {
                Ok(()) => return Ok(path),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(io::ErrorKind::AlreadyExists, "no free name"))
    }
}

impl Drop for ScanDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// Removes the directories `scan-*` in `parent` older than a day whose process is gone.
///
/// # Blocking
///
/// Reads the directory of the scans.
fn remove_stale(parent: &Path) {
    let Ok(entries) = fs::read_dir(parent) else {
        return;
    };
    let day = std::time::Duration::from_secs(24 * 3600);
    for entry in entries.filter_map(Result::ok) {
        let name = entry.file_name().to_string_lossy().into_owned();
        // `scan-PID-…`: a directory of a Sway Things that still runs stays, however old.
        let alive = name
            .strip_prefix("scan-")
            .and_then(|rest| rest.split('-').next())
            .is_some_and(|pid| !pid.is_empty() && Path::new("/proc").join(pid).exists());
        let stale = name.starts_with("scan-")
            && !alive
            && entry
                .metadata()
                .ok()
                .and_then(|meta| meta.modified().ok())
                .and_then(|modified| modified.elapsed().ok())
                .is_some_and(|age| age > day);
        if stale {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

/// What a scan is saved as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SaveAs {
    /// A PDF of every page.
    Pdf,
    /// The one page as an image, in the format it was scanned in.
    Image,
}

/// Saves the scanned `pages` (files of scanimage's, each with the resolution it was scanned at)
/// into `destination`, the file the user picked: a PDF of them all, or the first as it is. The
/// file is written next to its place and renamed into it, so a failure never leaves half a file,
/// and replaces what the user chose to replace in the file chooser.
///
/// # Errors
///
/// [`Error::Io`] if a page can't be read or isn't fit for a PDF, or the file can't be written.
pub fn save(pages: &[(PathBuf, u32)], save_as: SaveAs, destination: &Path) -> Result<(), Error> {
    let io_error = |error: &dyn std::fmt::Display| Error::Io(error.to_string());
    let bytes = match save_as {
        SaveAs::Image => {
            let (first, _) = pages
                .first()
                .ok_or_else(|| Error::Io("no page".to_owned()))?;
            fs::read(first).map_err(|error| io_error(&error))?
        }
        SaveAs::Pdf => {
            let mut read = Vec::new();
            for (page, dpi) in pages {
                let bytes = fs::read(page).map_err(|error| io_error(&error))?;
                let page = pdf::Page::read(&bytes).map_err(|error| io_error(&error))?;
                read.push((page, *dpi));
            }
            if read.is_empty() {
                return Err(Error::Io("no page".to_owned()));
            }
            pdf::write(&read)
        }
    };
    crate::atomic::write_bytes(destination, &bytes).map_err(|error| io_error(&error))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scanners_avahi_finds_are_told_apart_from_those_sane_lists() {
        let scanner = |device: &str, vendor: &str, model: &str| Scanner {
            device: device.to_owned(),
            vendor: vendor.to_owned(),
            model: model.to_owned(),
            kind: String::new(),
        };
        let found = |name: &str, model: &str| crate::printers::Found {
            key: name.to_owned(),
            name: name.to_owned(),
            model: model.to_owned(),
            location: String::new(),
            uri: String::new(),
            host: String::new(),
            resource: String::new(),
            secure: false,
            local: false,
        };
        let listed = [
            scanner("airscan:e0:Office MFP (5E1A)", "eSCL", "Office MFP (5E1A)"),
            scanner("escl:https://192.0.2.9:443", "HP", "LaserJet MFP M28w"),
        ];
        assert!(sees(
            &listed,
            &found("Office MFP (5E1A)", "Kyocera M2040dn")
        ));
        assert!(sees(
            &listed,
            &found("HP M28w [AB12]", "HP LaserJet MFP M28w")
        ));
        assert!(!sees(&listed, &found("Hall Scanner", "Brother ADS-1700W")));
        assert!(!sees(&[], &found("Office MFP (5E1A)", "")));
    }

    #[test]
    fn problems_are_told_by_status_and_message() {
        let cases = [
            (
                Some(8),
                "scanimage: sane_read: Scanner cover is open",
                Problem::CoverOpen,
            ),
            (
                Some(6),
                "scanimage: sane_read: Document feeder jammed",
                Problem::Jammed,
            ),
            (
                Some(7),
                "scanimage: sane_start: Document feeder out of documents",
                Problem::NoDocuments,
            ),
            (Some(3), "scanimage: sane_read: Device busy", Problem::Busy),
            (Some(11), "", Problem::AccessDenied),
            (Some(9), "", Problem::Io),
            (
                Some(1),
                "scanimage: open of device airscan:e0:Gone failed: Invalid argument",
                Problem::Gone,
            ),
            (Some(1), "something", Problem::Other),
            (None, "", Problem::Other),
        ];
        for (status, stderr, problem) in cases {
            assert_eq!(Problem::of(status, stderr), problem, "{stderr}");
        }
    }

    #[test]
    fn a_scan_dir_is_private_and_removed() {
        use std::os::unix::fs::PermissionsExt;
        let parent = tempfile::tempdir().unwrap();
        let stale = parent.path().join("scan-999999999-2-3");
        fs::create_dir(&stale).unwrap();
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(3 * 24 * 3600);
        fs::File::open(&stale).unwrap().set_modified(old).unwrap();
        // As old, but of a process that runs: ours.
        let running = parent
            .path()
            .join(format!("scan-{}-2-3", std::process::id()));
        fs::create_dir(&running).unwrap();
        fs::File::open(&running).unwrap().set_modified(old).unwrap();
        let fresh = parent.path().join("scan-4-5-6");
        fs::create_dir(&fresh).unwrap();
        let dir = ScanDir::new_in(parent.path()).unwrap();
        assert!(!stale.exists() && fresh.exists() && running.exists());
        let mode = fs::metadata(dir.path()).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700);
        let batch = dir.batch().unwrap();
        assert_ne!(batch, dir.batch().unwrap());
        let path = dir.path().to_owned();
        drop(dir);
        assert!(!path.exists());
    }

    #[test]
    fn pages_are_saved_as_an_image_or_a_pdf() {
        let dir = tempfile::tempdir().unwrap();
        let page = dir.path().join("page-0001.pnm");
        let mut bitmap = b"P4 8 2\n".to_vec();
        bitmap.extend_from_slice(&[0xff, 0x00]);
        fs::write(&page, &bitmap).unwrap();
        let image = dir.path().join("Scan.pnm");
        save(&[(page.clone(), 300)], SaveAs::Image, &image).unwrap();
        assert_eq!(fs::read(&image).unwrap(), bitmap);
        let pdf = dir.path().join("Scan.pdf");
        fs::write(&pdf, "replaced").unwrap();
        save(&[(page.clone(), 300), (page, 150)], SaveAs::Pdf, &pdf).unwrap();
        assert!(fs::read(&pdf).unwrap().starts_with(b"%PDF-1.4"));
        // Nothing is left next to it.
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 3);
        let broken = dir.path().join("page-0002.png");
        fs::write(&broken, "not a page").unwrap();
        let error = save(
            &[(broken, 300)],
            SaveAs::Pdf,
            &dir.path().join("Broken.pdf"),
        );
        assert!(matches!(error, Err(Error::Io(_))));
        assert!(!dir.path().join("Broken.pdf").exists());
    }
}
