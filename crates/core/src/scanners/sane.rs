// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Running scanimage: listing the scanners, reading what one can do, and scanning, with its
//! progress and a way to cancel. scanimage is started with an argument array, never through a
//! shell; device names and values are separate arguments, or after `=` of a long option, so none
//! can be taken for an option of its own.

use std::cell::Cell;
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::rc::Rc;
use std::time::Duration;

use async_io::Timer;
use async_process::{Child, Command};
use futures_lite::{AsyncReadExt, future};

use super::list::{self, Scanner};
use super::options::{self, Options};
use super::{Error, Problem};

/// How long listing the scanners may take: sane-airscan waits a few seconds for answers from the
/// network, and some backends probe every USB device.
pub const LIST_TIMEOUT: Duration = Duration::from_secs(45);
/// How long reading a scanner's options may take: it is opened, which wakes it up.
pub const OPTIONS_TIMEOUT: Duration = Duration::from_secs(45);
/// How long scanimage has to end after it is asked to cancel, before it is killed.
const CANCEL_GRACE: Duration = Duration::from_secs(5);
/// How much of scanimage's error output is kept for the details of an error.
const MAX_STDERR: usize = 16 * 1024;
/// How much a listing or the options may print before the rest is ignored.
const MAX_OUTPUT: usize = 1024 * 1024;

/// The file formats scanimage writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Format {
    /// PNG.
    #[default]
    Png,
    /// JPEG.
    Jpeg,
    /// TIFF.
    Tiff,
    /// PNM, which every scanimage writes; for the pages of a PDF in black and white.
    Pnm,
}

impl Format {
    /// Its name for `--format`, and its file extension.
    pub fn id(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpeg",
            Self::Tiff => "tiff",
            Self::Pnm => "pnm",
        }
    }

    /// The extension of its files.
    pub fn extension(self) -> &'static str {
        match self {
            Self::Jpeg => "jpg",
            other => other.id(),
        }
    }
}

/// scanimage, found in `PATH` or at a path of its own (tests), with the environment it runs in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sane {
    program: PathBuf,
    env: Vec<(OsString, OsString)>,
}

impl Sane {
    /// scanimage in `PATH`, if it is installed.
    pub fn find() -> Option<Self> {
        crate::delegate::find_program("scanimage", std::env::var_os("PATH").as_deref())
            .map(Self::at)
    }

    /// The scanimage at `program`.
    pub fn at(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            env: Vec::new(),
        }
    }

    /// The same, run with the environment variable `name` set to `value` (`SANE_CONFIG_DIR`).
    #[must_use]
    pub fn with_env(mut self, name: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        self.env.push((name.into(), value.into()));
        self
    }

    /// The program.
    pub fn program(&self) -> &Path {
        &self.program
    }

    /// scanimage with `args`, its output piped.
    fn command(&self, args: &[OsString]) -> Command {
        let mut command = Command::new(&self.program);
        command
            .args(args)
            .envs(self.env.iter().map(|(name, value)| (name, value)))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        command
    }

    /// Runs scanimage with `args` for at most `timeout`: what it printed, and its exit status.
    async fn output(
        &self,
        args: &[OsString],
        timeout: Duration,
    ) -> Result<(String, String, Option<i32>), Error> {
        let mut child = self
            .command(args)
            .spawn()
            .map_err(|error| spawn_error(&error))?;
        let (Some(mut stdout), Some(mut stderr)) = (child.stdout.take(), child.stderr.take())
        else {
            return Err(Error::Io("scanimage has no output".to_owned()));
        };
        let run = async {
            let mut out = Vec::new();
            let mut err = Vec::new();
            let read_out = async {
                let _ = (&mut stdout)
                    .take(MAX_OUTPUT as u64)
                    .read_to_end(&mut out)
                    .await;
            };
            let read_err = async {
                let _ = (&mut stderr)
                    .take(MAX_STDERR as u64)
                    .read_to_end(&mut err)
                    .await;
            };
            future::zip(read_out, read_err).await;
            let status = child.status().await.ok().and_then(|status| status.code());
            (out, err, status)
        };
        let timed_out = async {
            Timer::after(timeout).await;
            None
        };
        // Dropping the child on a timeout kills it.
        match future::or(async { Some(run.await) }, timed_out).await {
            Some((out, err, status)) => Ok((
                String::from_utf8_lossy(&out).into_owned(),
                String::from_utf8_lossy(&err).into_owned(),
                status,
            )),
            None => Err(Error::Timeout),
        }
    }

    /// The scanners SANE finds, cameras left out. SANE's backends look on USB and, with
    /// sane-airscan or SANE's escl backend, on the network meanwhile.
    ///
    /// # Errors
    ///
    /// [`Error::Missing`] if scanimage can't be started, [`Error::Timeout`] after
    /// [`LIST_TIMEOUT`], and [`Error::Failed`] if it fails.
    pub async fn list(&self) -> Result<Vec<Scanner>, Error> {
        let args = [OsString::from("-f"), OsString::from(list::format())];
        let (out, err, status) = self.output(&args, LIST_TIMEOUT).await?;
        if status != Some(0) {
            return Err(failure(status, &err));
        }
        Ok(list::parse_formatted(&out)
            .into_iter()
            .filter(|scanner| !scanner.is_camera())
            .collect())
    }

    /// What the scanner `device` can do.
    ///
    /// # Errors
    ///
    /// As [`list`](Self::list), after [`OPTIONS_TIMEOUT`]; [`Problem::Gone`] if it can't be
    /// opened.
    pub async fn options(&self, device: &str) -> Result<Options, Error> {
        let args = [
            OsString::from(format!("--device-name={device}")),
            OsString::from("--all-options"),
        ];
        let (out, err, status) = self.output(&args, OPTIONS_TIMEOUT).await?;
        if status != Some(0) {
            return Err(failure(status, &err));
        }
        Ok(options::parse(&out))
    }

    /// The formats this scanimage writes, from what `--help` says of `--format`; PNM always.
    pub async fn formats(&self) -> Vec<Format> {
        let help = self
            .output(&[OsString::from("--help")], Duration::from_secs(10))
            .await
            .map(|(out, _, _)| out)
            .unwrap_or_default();
        parse_formats(&help)
    }

    /// Starts scanning with `request` into `directory`, a page a file.
    ///
    /// # Errors
    ///
    /// [`Error::Missing`] if scanimage can't be started.
    pub fn scan(&self, request: &Request, directory: &Path) -> Result<Scan, Error> {
        let pattern = batch_pattern(directory, request.format.extension());
        let mut child = self
            .command(&request.args(&pattern))
            .stdout(Stdio::null())
            .spawn()
            .map_err(|error| spawn_error(&error))?;
        let stderr = child.stderr.take();
        let pid = i32::try_from(child.id()).ok();
        Ok(Scan {
            child,
            stderr,
            directory: directory.to_owned(),
            extension: request.format.extension(),
            cancel: Canceler {
                pid: Rc::new(Cell::new(pid)),
                canceled: Rc::new(Cell::new(false)),
            },
        })
    }
}

/// The pattern of `--batch` for the pages in `directory`: scanimage formats it with the page
/// number like `printf`, so a `%` of the directory is doubled.
fn batch_pattern(directory: &Path, extension: &str) -> PathBuf {
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    let mut bytes = Vec::new();
    for byte in directory.as_os_str().as_bytes() {
        bytes.push(*byte);
        if *byte == b'%' {
            bytes.push(b'%');
        }
    }
    PathBuf::from(OsString::from_vec(bytes)).join(format!("page-%04d.{extension}"))
}

/// The formats `--help` lists after `--format=`, PNM always.
pub fn parse_formats(help: &str) -> Vec<Format> {
    let mut formats = vec![Format::Pnm];
    let Some(line) = help.lines().find(|line| line.contains("--format=")) else {
        return formats;
    };
    let listed = line
        .split("--format=")
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .unwrap_or_default();
    for name in listed.split('|') {
        let format = match name {
            "png" => Format::Png,
            "jpeg" => Format::Jpeg,
            "tiff" => Format::Tiff,
            _ => continue,
        };
        formats.push(format);
    }
    formats
}

/// The error of starting scanimage.
fn spawn_error(error: &io::Error) -> Error {
    if error.kind() == io::ErrorKind::NotFound {
        Error::Missing
    } else {
        Error::Io(error.to_string())
    }
}

/// The error of scanimage ending with `status` after printing `stderr`.
pub(super) fn failure(status: Option<i32>, stderr: &str) -> Error {
    Error::Failed {
        problem: Problem::of(status, stderr),
        details: stderr.trim().to_owned(),
    }
}

/// What to scan, and how.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Request {
    /// The scanner's device name.
    pub device: String,
    /// Its source, as it names it; `None` for the one set.
    pub source: Option<String>,
    /// Its mode, as it names it; `None` for the one set.
    pub mode: Option<String>,
    /// The resolution in dpi; `None` for the one set.
    pub resolution: Option<u32>,
    /// The width and height to scan in mm from the top left corner; `None` for all of it.
    pub area: Option<(f64, f64)>,
    /// The format of the files.
    pub format: Format,
    /// Whether to scan until the document feeder is empty, else one page.
    pub all_pages: bool,
}

impl Request {
    /// scanimage's arguments to scan into files named after `pattern` (`page-%04d.png`).
    pub fn args(&self, pattern: &Path) -> Vec<OsString> {
        let mut args = vec![
            OsString::from(format!("--device-name={}", self.device)),
            OsString::from(format!("--format={}", self.format.id())),
            OsString::from("--progress"),
        ];
        let mut batch = OsString::from("--batch=");
        batch.push(pattern);
        args.push(batch);
        if !self.all_pages {
            args.push(OsString::from("--batch-count=1"));
        }
        // The options of the device come after `--device-name`, which tells scanimage what they
        // are; their values follow as arguments of their own.
        if let Some(source) = &self.source {
            args.push(OsString::from("--source"));
            args.push(OsString::from(source));
        }
        if let Some(mode) = &self.mode {
            args.push(OsString::from("--mode"));
            args.push(OsString::from(mode));
        }
        if let Some(resolution) = self.resolution {
            args.push(OsString::from("--resolution"));
            args.push(OsString::from(resolution.to_string()));
        }
        if let Some((width, height)) = self.area {
            for (name, value) in [("-l", 0.0), ("-t", 0.0), ("-x", width), ("-y", height)] {
                args.push(OsString::from(name));
                args.push(OsString::from(format!("{value:.1}")));
            }
        }
        args
    }
}

/// What a running scan reports.
#[derive(Debug, Clone, PartialEq)]
pub enum Progress {
    /// A page starts: its number, from 1.
    Page(u32),
    /// How much of the page is scanned, from 0 to 100.
    Percent(f64),
}

/// Cancels a running scan: asks scanimage to stop (`SIGTERM`, which makes it cancel the scan
/// with the scanner), and kills it if it hasn't ended a few seconds later.
#[derive(Debug, Clone)]
pub struct Canceler {
    /// The process, while it runs and isn't waited for: its id can't be reused before.
    pid: Rc<Cell<Option<i32>>>,
    canceled: Rc<Cell<bool>>,
}

impl Canceler {
    /// Asks the scan to stop.
    pub fn cancel(&self) {
        self.canceled.set(true);
        self.signal(rustix::process::Signal::TERM);
    }

    /// Whether it was asked to stop.
    pub fn is_canceled(&self) -> bool {
        self.canceled.get()
    }

    fn signal(&self, signal: rustix::process::Signal) {
        if let Some(pid) = self.pid.get().and_then(rustix::process::Pid::from_raw) {
            let _ = rustix::process::kill_process(pid, signal);
        }
    }
}

/// A scan that runs.
#[derive(Debug)]
pub struct Scan {
    child: Child,
    stderr: Option<async_process::ChildStderr>,
    directory: PathBuf,
    extension: &'static str,
    cancel: Canceler,
}

impl Scan {
    /// What cancels it.
    pub fn canceler(&self) -> Canceler {
        self.cancel.clone()
    }

    /// Waits until it ends, telling `progress` how it goes: the files of the pages scanned, in
    /// order.
    ///
    /// # Errors
    ///
    /// [`Error::Canceled`] if it was canceled, [`Error::Failed`] if scanimage fails before a
    /// page is scanned. A document feeder that runs empty after the first page ends a scan of
    /// all pages without an error.
    pub async fn finish(
        mut self,
        mut progress: impl FnMut(Progress),
    ) -> Result<Vec<PathBuf>, Error> {
        let mut kept = Vec::new();
        let stderr = self.stderr.take();
        let reading = async {
            let Some(mut stderr) = stderr else {
                return;
            };
            let mut buffer = [0u8; 4096];
            let mut pending = String::new();
            loop {
                let read = match stderr.read(&mut buffer).await {
                    Ok(0) | Err(_) => break,
                    Ok(read) => read,
                };
                let chunk = String::from_utf8_lossy(buffer.get(..read).unwrap_or_default());
                pending.push_str(&chunk);
                // Progress ends in a carriage return, the other lines in a newline.
                while let Some(end) = pending.find(['\r', '\n']) {
                    let line: String = pending.drain(..=end).collect();
                    let line = line.trim();
                    if let Some(event) = progress_of(line) {
                        progress(event);
                    } else if !line.is_empty() && kept.len() < MAX_STDERR {
                        kept.extend_from_slice(line.as_bytes());
                        kept.push(b'\n');
                    }
                }
                if pending.len() > MAX_STDERR {
                    pending.clear();
                }
            }
        };
        // A cancel that doesn't end it in time kills it, which ends its output too: the
        // watchdog runs beside the reading, so a scanimage stuck in its cancel can't hold it.
        let cancel = self.cancel.clone();
        let watchdog = async {
            loop {
                Timer::after(Duration::from_millis(200)).await;
                if cancel.is_canceled() {
                    Timer::after(CANCEL_GRACE).await;
                    cancel.signal(rustix::process::Signal::KILL);
                    break;
                }
            }
            std::future::pending::<()>().await;
        };
        future::or(reading, watchdog).await;
        let cancel = self.cancel.clone();
        let status = future::or(
            async {
                self.child
                    .status()
                    .await
                    .ok()
                    .and_then(|status| status.code())
            },
            async {
                // Its output can end before it does: the same escalation, once more.
                loop {
                    Timer::after(Duration::from_millis(200)).await;
                    if cancel.is_canceled() {
                        Timer::after(CANCEL_GRACE).await;
                        cancel.signal(rustix::process::Signal::KILL);
                        break;
                    }
                }
                std::future::pending::<Option<i32>>().await
            },
        )
        .await;
        self.cancel.pid.set(None);
        let pages = pages_in(&self.directory, self.extension);
        if self.cancel.is_canceled() {
            return Err(Error::Canceled);
        }
        let stderr = String::from_utf8_lossy(&kept).into_owned();
        // A feeder that runs empty ends a batch; that is how a scan of all pages ends.
        let empty = Problem::of(status, &stderr) == Problem::NoDocuments;
        if status == Some(0) || (empty && !pages.is_empty()) {
            if pages.is_empty() {
                return Err(failure(status, &stderr));
            }
            return Ok(pages);
        }
        Err(failure(status, &stderr))
    }
}

/// The files `page-NNNN.EXTENSION` in `directory`, in order.
fn pages_in(directory: &Path, extension: &str) -> Vec<PathBuf> {
    let mut pages: Vec<PathBuf> = std::fs::read_dir(directory)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| {
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| {
                            name.starts_with("page-") && name.ends_with(&format!(".{extension}"))
                        })
                })
                .collect()
        })
        .unwrap_or_default();
    pages.sort();
    pages
}

/// What a line of scanimage's error output says of the progress: `Progress: 45.3%`, `Scanning
/// page 2`.
fn progress_of(line: &str) -> Option<Progress> {
    if let Some(percent) = line.strip_prefix("Progress:") {
        let percent: f64 = percent.trim().trim_end_matches('%').parse().ok()?;
        return percent
            .is_finite()
            .then(|| Progress::Percent(percent.clamp(0.0, 100.0)));
    }
    let page = line.strip_prefix("Scanning page ")?;
    page.trim().parse().ok().map(Progress::Page)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_arguments_keep_names_whole() {
        let request = Request {
            device: "airscan:e0:Office -x (50%)".to_owned(),
            source: Some("ADF Duplex".to_owned()),
            mode: Some("Color".to_owned()),
            resolution: Some(300),
            area: Some((210.0, 297.0)),
            format: Format::Jpeg,
            all_pages: true,
        };
        let pattern = batch_pattern(Path::new("/home/a%b/scans"), "jpg");
        assert_eq!(pattern, Path::new("/home/a%%b/scans/page-%04d.jpg"));
        let args: Vec<String> = request
            .args(&pattern)
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            args,
            [
                "--device-name=airscan:e0:Office -x (50%)",
                "--format=jpeg",
                "--progress",
                "--batch=/home/a%%b/scans/page-%04d.jpg",
                "--source",
                "ADF Duplex",
                "--mode",
                "Color",
                "--resolution",
                "300",
                "-l",
                "0.0",
                "-t",
                "0.0",
                "-x",
                "210.0",
                "-y",
                "297.0",
            ]
        );
        let one = Request {
            all_pages: false,
            ..Request::default()
        };
        assert!(
            one.args(&pattern)
                .iter()
                .any(|arg| arg == "--batch-count=1")
        );
    }

    #[test]
    fn progress_and_formats_are_read() {
        assert_eq!(
            progress_of("Progress: 45.3%"),
            Some(Progress::Percent(45.3))
        );
        assert_eq!(
            progress_of("Progress: 120%"),
            Some(Progress::Percent(100.0))
        );
        assert_eq!(progress_of("Scanning page 3"), Some(Progress::Page(3)));
        assert_eq!(progress_of("Scanned page 3. (scanner status = 5)"), None);
        assert_eq!(progress_of("Progress: NaN%"), None);
        let help = "    --format=pnm|tiff|png|jpeg|pdf  file format of output file\n";
        assert_eq!(
            parse_formats(help),
            [Format::Pnm, Format::Tiff, Format::Png, Format::Jpeg]
        );
        assert_eq!(parse_formats(""), [Format::Pnm]);
    }
}
