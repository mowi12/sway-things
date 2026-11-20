// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The CUPS client: IPP requests to the local CUPS, what they answer, and the fallback to
//! cups-pk-helper for changes CUPS refuses.
//!
//! Requests name the user (`requesting-user-name`). When CUPS asks for authentication over its
//! domain socket and offers `PeerCred`, the request is sent again with `Authorization: PeerCred
//! USER`: CUPS checks the socket's peer credentials, so no password is involved, and lets members
//! of its administrator group (`SystemGroup`) make changes and owners cancel their jobs.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicI32, Ordering};
use std::time::Duration;

use super::admin::{self, Bus};
use super::http::{self, Endpoint, Loopback};
use super::ipp::{Attribute, Message, Value, tag};
use super::{
    Error, Job, JobState, Level, Marker, NewPrinter, Printer, PrinterState, Reason, Snapshot,
    clean, parse_colors, percent_decode, queue_name, test_page,
};
use crate::i18n::{Text, tr};
use crate::msg;

/// How long a read may take.
const READ_TIMEOUT: Duration = Duration::from_secs(10);
/// How long adding a printer may take: CUPS asks the printer for what it can do first.
const ADD_TIMEOUT: Duration = Duration::from_secs(120);

/// CUPS's default domain socket, and where older systems have it.
const SOCKETS: [&str; 2] = ["/run/cups/cups.sock", "/var/run/cups/cups.sock"];
/// The port CUPS listens on.
const IPP_PORT: u16 = 631;

/// IPP operations.
mod op {
    pub const PRINT_JOB: u16 = 0x0002;
    pub const CANCEL_JOB: u16 = 0x0008;
    pub const GET_JOBS: u16 = 0x000A;
    pub const PAUSE_PRINTER: u16 = 0x0010;
    pub const RESUME_PRINTER: u16 = 0x0011;
    pub const CUPS_GET_DEFAULT: u16 = 0x4001;
    pub const CUPS_GET_PRINTERS: u16 = 0x4002;
    pub const CUPS_ADD_MODIFY_PRINTER: u16 = 0x4003;
    pub const CUPS_DELETE_PRINTER: u16 = 0x4004;
    pub const CUPS_ACCEPT_JOBS: u16 = 0x4008;
    pub const CUPS_SET_DEFAULT: u16 = 0x400A;
}

/// IPP status codes the client tells apart.
mod status {
    pub const FORBIDDEN: u16 = 0x0401;
    pub const NOT_AUTHENTICATED: u16 = 0x0402;
    pub const NOT_AUTHORIZED: u16 = 0x0403;
    pub const NOT_FOUND: u16 = 0x0406;
    pub const DOCUMENT_FORMAT_NOT_SUPPORTED: u16 = 0x040A;
}

/// The attributes of a printer the snapshot reads.
const PRINTER_ATTRIBUTES: [&str; 20] = [
    "printer-name",
    "printer-info",
    "printer-location",
    "printer-make-and-model",
    "device-uri",
    "printer-state",
    "printer-state-reasons",
    "printer-state-message",
    "printer-is-accepting-jobs",
    "printer-is-shared",
    "printer-is-temporary",
    "printer-type",
    "marker-names",
    "marker-types",
    "marker-colors",
    "marker-levels",
    "marker-low-levels",
    "marker-high-levels",
    "printer-uri-supported",
    "printer-state-change-time",
];

/// The attributes of a job the snapshot reads.
const JOB_ATTRIBUTES: [&str; 6] = [
    "job-id",
    "job-name",
    "job-originating-user-name",
    "job-state",
    "job-printer-uri",
    "time-at-creation",
];

/// `printer-type` bit of a class.
const CUPS_PRINTER_CLASS: i32 = 0x0001;

/// Why a request failed, before it becomes an [`Error`].
enum CallError {
    /// CUPS can't be reached.
    Unreachable(std::io::Error),
    /// CUPS wants an administrator, or the owner.
    NotAuthorized,
    /// What was asked for isn't there.
    NotFound,
    /// CUPS refused or failed, with its status and message.
    Failed(u16, String),
}

/// A connection to the local CUPS. Cheap to clone; each request opens its own connection.
#[derive(Debug, Clone)]
pub struct Cups {
    endpoint: Endpoint,
    user: String,
    admin: Bus,
    request_id: Arc<AtomicI32>,
}

impl Cups {
    /// The CUPS of this computer, found as CUPS's own tools find it: `CUPS_SERVER`, else the
    /// `ServerName` of `~/.cups/client.conf` or `/etc/cups/client.conf`, else its domain socket,
    /// else `localhost:631`. Changes CUPS refuses go to cups-pk-helper on the system bus.
    ///
    /// # Errors
    ///
    /// [`Error::Remote`] if CUPS is set up to use a print server on another computer.
    pub fn local() -> Result<Self, Error> {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let server = std::env::var("CUPS_SERVER")
            .ok()
            .filter(|server| !server.trim().is_empty())
            .or_else(|| {
                let user = home.as_ref().map(|home| home.join(".cups/client.conf"));
                user.iter()
                    .map(PathBuf::as_path)
                    .chain([Path::new("/etc/cups/client.conf")])
                    .find_map(|file| server_name(&std::fs::read_to_string(file).ok()?))
            });
        let endpoint = endpoint(server.as_deref(), Path::exists)?;
        Ok(Self::at(endpoint))
    }

    /// The CUPS at `endpoint`, with cups-pk-helper on the system bus.
    pub fn at(endpoint: Endpoint) -> Self {
        let user = header_safe(&gio::glib::user_name().to_string_lossy());
        Self {
            endpoint,
            user,
            admin: Bus::System,
            request_id: Arc::new(AtomicI32::new(1)),
        }
    }

    /// The same CUPS with cups-pk-helper on `bus`.
    #[must_use]
    pub fn with_admin_bus(mut self, bus: Bus) -> Self {
        self.admin = bus;
        self
    }

    /// Where CUPS listens.
    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    /// CUPS's version, from its answers' `Server` header (`2.4`); `None` if CUPS can't be
    /// reached or doesn't say.
    pub async fn version(&self) -> Option<String> {
        let request = self.request(op::CUPS_GET_DEFAULT);
        let response = http::post(&self.endpoint, "/", &request.encode(), None, READ_TIMEOUT)
            .await
            .ok()?;
        response
            .header("server")?
            .split_whitespace()
            .find_map(|part| part.strip_prefix("CUPS/"))
            .map(clean)
    }

    /// Reads the printers, their jobs and the default printer.
    ///
    /// # Errors
    ///
    /// [`Error::Unavailable`] if CUPS can't be reached, else what it answered.
    pub async fn snapshot(&self) -> Result<Snapshot, Error> {
        let mut request = self.request(op::CUPS_GET_PRINTERS);
        request.add(
            tag::OPERATION,
            "requested-attributes",
            keywords(&PRINTER_ATTRIBUTES),
        );
        let mut printers: Vec<Printer> = match self.call("/", request, READ_TIMEOUT).await {
            Ok(response) => response
                .groups(tag::PRINTER)
                .filter_map(|group| parse_printer(&group.attributes))
                .collect(),
            Err(CallError::NotFound) => Vec::new(),
            Err(error) => return Err(self.error(error)),
        };
        printers.sort_by(|a, b| {
            a.title()
                .to_lowercase()
                .cmp(&b.title().to_lowercase())
                .then_with(|| a.name.cmp(&b.name))
        });
        let default = match self
            .call("/", self.request(op::CUPS_GET_DEFAULT), READ_TIMEOUT)
            .await
        {
            Ok(response) => response
                .get(tag::PRINTER, "printer-name")
                .and_then(Attribute::str)
                .map(ToOwned::to_owned),
            Err(CallError::NotFound) => None,
            Err(error) => return Err(self.error(error)),
        };
        let mut request = self.target_request(op::GET_JOBS, "ipp://localhost/");
        request.add(tag::OPERATION, "which-jobs", keywords(&["not-completed"]));
        request.add(
            tag::OPERATION,
            "requested-attributes",
            keywords(&JOB_ATTRIBUTES),
        );
        let mut jobs: Vec<Job> = match self.call("/", request, READ_TIMEOUT).await {
            Ok(response) => response
                .groups(tag::JOB)
                .filter_map(|group| parse_job(&group.attributes))
                .collect(),
            Err(CallError::NotFound) => Vec::new(),
            Err(error) => return Err(self.error(error)),
        };
        jobs.sort_by_key(|job| (job.created, job.id));
        // The user's own default isn't read here: see `own_default`.
        Ok(Snapshot {
            printers,
            jobs,
            default,
            user_default: None,
        })
    }

    /// Pauses the printer `name`: its jobs wait until it is resumed.
    ///
    /// # Errors
    ///
    /// As any change: see [`Error`].
    pub async fn pause(&self, name: &str) -> Result<(), Error> {
        let request = self.printer_request(op::PAUSE_PRINTER, name);
        self.change(request, ADMIN, READ_TIMEOUT, || async {
            self.admin.call("PrinterSetEnabled", &(name, false)).await
        })
        .await
    }

    /// Resumes the printer `name`, and lets it take new jobs again.
    ///
    /// # Errors
    ///
    /// As any change: see [`Error`].
    pub async fn resume(&self, name: &str) -> Result<(), Error> {
        let request = self.printer_request(op::RESUME_PRINTER, name);
        self.change(request, ADMIN, READ_TIMEOUT, || async {
            self.admin.call("PrinterSetEnabled", &(name, true)).await
        })
        .await?;
        let request = self.printer_request(op::CUPS_ACCEPT_JOBS, name);
        self.change(request, ADMIN, READ_TIMEOUT, || async {
            self.admin
                .call("PrinterSetAcceptJobs", &(name, true, ""))
                .await
        })
        .await
    }

    /// Makes the printer `name` CUPS's default printer, for every user.
    ///
    /// # Errors
    ///
    /// As any change: see [`Error`].
    pub async fn set_default(&self, name: &str) -> Result<(), Error> {
        let request = self.printer_request(op::CUPS_SET_DEFAULT, name);
        self.change(request, ADMIN, READ_TIMEOUT, || async {
            self.admin.call("PrinterSetDefault", &(name,)).await
        })
        .await
    }

    /// Removes the printer `name`, with its jobs.
    ///
    /// # Errors
    ///
    /// As any change: see [`Error`].
    pub async fn remove(&self, name: &str) -> Result<(), Error> {
        let request = self.printer_request(op::CUPS_DELETE_PRINTER, name);
        self.change(request, ADMIN, READ_TIMEOUT, || async {
            self.admin.call("PrinterDelete", &(name,)).await
        })
        .await
    }

    /// Adds `printer` as a driverless printer (IPP Everywhere): CUPS asks the printer what it
    /// can do and makes the driver from that, which can take a while. The new printer takes jobs
    /// at once.
    ///
    /// Returns the queue's name: `printer.name`, or a free one like it if a queue took that
    /// name meanwhile.
    ///
    /// # Errors
    ///
    /// As any change: see [`Error`]. CUPS's message says why the printer couldn't be added (it is
    /// offline, or isn't driverless).
    pub async fn add(&self, printer: &NewPrinter) -> Result<String, Error> {
        // CUPS-Add-Modify-Printer changes a queue of that name if there is one: a name taken
        // since the caller chose it (lpadmin, cups-browsed) gets a free one instead, so neither
        // the add nor the cleanup below ever touches a queue that was there before.
        let names: Vec<String> = self
            .snapshot()
            .await?
            .printers
            .into_iter()
            .map(|printer| printer.name)
            .collect();
        let name = if names.iter().any(|n| n.eq_ignore_ascii_case(&printer.name)) {
            let taken: Vec<&str> = names.iter().map(String::as_str).collect();
            queue_name(&printer.name, &taken)
        } else {
            printer.name.clone()
        };
        let mut request = self.printer_request(op::CUPS_ADD_MODIFY_PRINTER, &name);
        request.add(
            tag::PRINTER,
            "device-uri",
            vec![Value::Uri(printer.uri.clone())],
        );
        request.add(
            tag::PRINTER,
            "ppd-name",
            vec![Value::Name("everywhere".to_owned())],
        );
        request.add(
            tag::PRINTER,
            "printer-info",
            vec![Value::Text(printer.info.clone())],
        );
        request.add(
            tag::PRINTER,
            "printer-location",
            vec![Value::Text(printer.location.clone())],
        );
        request.add(
            tag::PRINTER,
            "printer-is-accepting-jobs",
            vec![Value::Boolean(true)],
        );
        // Idle: the printer is enabled.
        request.add(tag::PRINTER, "printer-state", vec![Value::Enum(3)]);
        let added = self
            .change(request, ADMIN, ADD_TIMEOUT, || async {
                self.admin
                    .call(
                        "PrinterAdd",
                        &(
                            name.as_str(),
                            printer.uri.as_str(),
                            "everywhere",
                            printer.info.as_str(),
                            printer.location.as_str(),
                        ),
                    )
                    .await?;
                self.admin
                    .call("PrinterSetEnabled", &(name.as_str(), true))
                    .await?;
                self.admin
                    .call("PrinterSetAcceptJobs", &(name.as_str(), true, ""))
                    .await
            })
            .await;
        // CUPS 2.4.10 (Debian 13) keeps the queue of a printer it couldn't add, which would print
        // nowhere; 2.4.20 removes it itself. The name was free before, so the queue is this
        // add's own.
        if let Err(Error::Failed(_)) = &added {
            if let Ok(snapshot) = self.snapshot().await {
                if snapshot.printer(&name).is_some() {
                    let _ = self.remove(&name).await;
                }
            }
        }
        added.map(|()| name)
    }

    /// Sets the description of the printer `name`, the title the page shows.
    ///
    /// # Errors
    ///
    /// As any change: see [`Error`].
    pub async fn set_info(&self, name: &str, info: &str) -> Result<(), Error> {
        let mut request = self.printer_request(op::CUPS_ADD_MODIFY_PRINTER, name);
        request.add(
            tag::PRINTER,
            "printer-info",
            vec![Value::Text(info.to_owned())],
        );
        self.change(request, ADMIN, READ_TIMEOUT, || async {
            self.admin.call("PrinterSetInfo", &(name, info)).await
        })
        .await
    }

    /// Sets where the printer `name` stands.
    ///
    /// # Errors
    ///
    /// As any change: see [`Error`].
    pub async fn set_location(&self, name: &str, location: &str) -> Result<(), Error> {
        let mut request = self.printer_request(op::CUPS_ADD_MODIFY_PRINTER, name);
        request.add(
            tag::PRINTER,
            "printer-location",
            vec![Value::Text(location.to_owned())],
        );
        self.change(request, ADMIN, READ_TIMEOUT, || async {
            self.admin
                .call("PrinterSetLocation", &(name, location))
                .await
        })
        .await
    }

    /// Shares the printer `name` on the network, or stops sharing it. CUPS shares it only while
    /// sharing is on for CUPS as a whole.
    ///
    /// # Errors
    ///
    /// As any change: see [`Error`].
    pub async fn set_shared(&self, name: &str, shared: bool) -> Result<(), Error> {
        let mut request = self.printer_request(op::CUPS_ADD_MODIFY_PRINTER, name);
        request.add(
            tag::PRINTER,
            "printer-is-shared",
            vec![Value::Boolean(shared)],
        );
        self.change(request, ADMIN, READ_TIMEOUT, || async {
            self.admin.call("PrinterSetShared", &(name, shared)).await
        })
        .await
    }

    /// Cancels the job `id`. CUPS lets the owner cancel it; another user's job needs an
    /// administrator.
    ///
    /// # Errors
    ///
    /// As any change: see [`Error`].
    pub async fn cancel_job(&self, id: i32) -> Result<(), Error> {
        let mut request = self.target_request(op::CANCEL_JOB, "ipp://localhost/");
        request.add(tag::OPERATION, "job-id", vec![Value::Integer(id)]);
        self.change(request, "/jobs/", READ_TIMEOUT, || async {
            self.admin.call("JobCancelPurge", &(id, false)).await
        })
        .await
    }

    /// Prints a test page on the printer `name`; the id of the job.
    ///
    /// # Errors
    ///
    /// As any request: see [`Error`].
    pub async fn print_test_page(&self, name: &str) -> Result<i32, Error> {
        let mut last = None;
        for document in test_page::documents() {
            let mut request = self.printer_request(op::PRINT_JOB, name);
            request.add(
                tag::OPERATION,
                "job-name",
                vec![Value::Name(tr(msg!("Test Page")).to_owned())],
            );
            request.add(
                tag::OPERATION,
                "document-format",
                vec![Value::MimeType(document.format.to_owned())],
            );
            request.data = document.bytes;
            match self.call("/printers/", request, READ_TIMEOUT).await {
                Ok(response) => {
                    return response
                        .get(tag::JOB, "job-id")
                        .and_then(Attribute::int)
                        .ok_or_else(|| Error::Failed(msg!("CUPS gave the job no number").into()));
                }
                // A document CUPS has no filter for: the next one.
                Err(CallError::Failed(status::DOCUMENT_FORMAT_NOT_SUPPORTED, message)) => {
                    last = Some(CallError::Failed(
                        status::DOCUMENT_FORMAT_NOT_SUPPORTED,
                        message,
                    ));
                }
                Err(error) => return Err(self.error(error)),
            }
        }
        Err(last.map_or_else(
            || Error::Failed(msg!("No test page to print").into()),
            |error| self.error(error),
        ))
    }

    /// Sends a change: as the user first, and if CUPS wants an administrator, through
    /// cups-pk-helper with `fallback`.
    async fn change<F, Fut>(
        &self,
        request: Message,
        path: &str,
        timeout: Duration,
        fallback: F,
    ) -> Result<(), Error>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<(), Error>>,
    {
        match self.call(path, request, timeout).await {
            Ok(_) => Ok(()),
            Err(CallError::NotAuthorized) => fallback().await,
            Err(error) => Err(self.error(error)),
        }
    }

    /// A request for `operation` with the next id and the user's name.
    fn request(&self, operation: u16) -> Message {
        let id = self.request_id.fetch_add(1, Ordering::Relaxed);
        let mut request = Message::request(operation, id);
        self.user_name(&mut request);
        request
    }

    /// Adds `requesting-user-name` to the operation group of `request`.
    fn user_name(&self, request: &mut Message) {
        if let Some(group) = request.groups.iter_mut().find(|g| g.tag == tag::OPERATION) {
            group.attributes.push(Attribute {
                name: "requesting-user-name".to_owned(),
                values: vec![Value::Name(self.user.clone())],
            });
        }
    }

    /// A request for `operation` on the printer `name`.
    fn printer_request(&self, operation: u16, name: &str) -> Message {
        self.target_request(operation, &Printer::uri(name))
    }

    /// A request for `operation` on `uri`, which comes right after the charset and language.
    fn target_request(&self, operation: u16, uri: &str) -> Message {
        let id = self.request_id.fetch_add(1, Ordering::Relaxed);
        let mut request = Message::request(operation, id);
        request.add(
            tag::OPERATION,
            "printer-uri",
            vec![Value::Uri(uri.to_owned())],
        );
        self.user_name(&mut request);
        request
    }

    /// Sends `request` to `path` and reads the answer; asks again with the peer credentials if
    /// CUPS wants them.
    async fn call(
        &self,
        path: &str,
        request: Message,
        timeout: Duration,
    ) -> Result<Message, CallError> {
        let body = request.encode();
        let mut response = http::post(&self.endpoint, path, &body, None, timeout)
            .await
            .map_err(CallError::Unreachable)?;
        if response.status == 401 {
            let peer_cred = matches!(self.endpoint, Endpoint::Socket(_))
                && response
                    .header("www-authenticate")
                    .is_some_and(|offer| offer.contains("PeerCred"));
            if !peer_cred || self.user.is_empty() {
                return Err(CallError::NotAuthorized);
            }
            let authorization = format!("PeerCred {}", self.user);
            response = http::post(&self.endpoint, path, &body, Some(&authorization), timeout)
                .await
                .map_err(CallError::Unreachable)?;
        }
        match response.status {
            200 => {}
            401 | 403 => return Err(CallError::NotAuthorized),
            other => {
                return Err(CallError::Failed(0, format!("HTTP {other}")));
            }
        }
        let message = Message::decode(&response.body)
            .map_err(|error| CallError::Failed(0, error.to_string()))?;
        match message.code {
            0x0000..=0x00FF => Ok(message),
            status::FORBIDDEN | status::NOT_AUTHENTICATED | status::NOT_AUTHORIZED => {
                Err(CallError::NotAuthorized)
            }
            status::NOT_FOUND => Err(CallError::NotFound),
            code => Err(CallError::Failed(
                code,
                message
                    .status_message()
                    .map_or_else(|| format!("IPP status {code:#06x}"), clean),
            )),
        }
    }

    /// The [`Error`] of a failed request.
    fn error(&self, error: CallError) -> Error {
        match error {
            CallError::Unreachable(error) => unreachable(&self.endpoint, &error),
            CallError::NotAuthorized => Error::NotAllowed(admin::authorization()),
            CallError::NotFound => {
                Error::Failed(msg!("The printer doesn't exist (anymore)").into())
            }
            CallError::Failed(_, message) => Error::Failed(Text::data(message)),
        }
    }
}

/// `name` without control characters and spaces, so it can go into a header line; empty if
/// nothing is left.
fn header_safe(name: &str) -> String {
    name.chars()
        .filter(|c| !c.is_control() && !c.is_whitespace())
        .collect()
}

/// The path of administrative requests.
const ADMIN: &str = "/admin/";

/// The error of a CUPS that can't be reached at `endpoint`: [`Error::NoAccess`] if the user
/// may not connect, else [`Error::Unavailable`].
fn unreachable(endpoint: &Endpoint, error: &std::io::Error) -> Error {
    use std::io::ErrorKind;
    match error.kind() {
        ErrorKind::NotFound | ErrorKind::ConnectionRefused => Error::Unavailable(
            Text::new(msg!("nothing listens at {endpoint}")).arg("endpoint", endpoint),
        ),
        // It runs: starting it wouldn't help.
        ErrorKind::PermissionDenied => {
            Error::NoAccess(Text::new(msg!("no access to {endpoint}")).arg("endpoint", endpoint))
        }
        _ => Error::Unavailable(
            Text::new(msg!("{endpoint}: {error}"))
                .arg("endpoint", endpoint)
                .arg("error", error),
        ),
    }
}

/// The values of `names` as keywords.
fn keywords(names: &[&str]) -> Vec<Value> {
    names
        .iter()
        .map(|name| Value::Keyword((*name).to_owned()))
        .collect()
}

/// The `ServerName` of a `client.conf`: the rest of its line, so a socket path may hold spaces.
fn server_name(text: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let line = line.trim();
        let (key, value) = line.split_once(char::is_whitespace)?;
        let value = value.trim();
        (key.eq_ignore_ascii_case("ServerName") && !value.is_empty()).then(|| value.to_owned())
    })
}

/// Where CUPS listens, for the server name `server` (a socket path, or `host[:port]`), or the
/// default; `exists` tells whether a socket path exists.
///
/// Only this computer counts: another host is [`Error::Remote`].
fn endpoint(server: Option<&str>, exists: impl Fn(&Path) -> bool) -> Result<Endpoint, Error> {
    let socket = SOCKETS.iter().map(Path::new).find(|path| exists(path));
    let Some(server) = server.map(str::trim) else {
        return Ok(
            socket.map_or(Endpoint::Loopback(Loopback::Localhost, IPP_PORT), |path| {
                Endpoint::Socket(path.to_owned())
            }),
        );
    };
    if server.starts_with('/') {
        return Ok(Endpoint::Socket(PathBuf::from(server)));
    }
    // `host`, `host:port`, `[v6]:port`, `::1`; a trailing `/version=…` is libcups's.
    let server = server.split('/').next().unwrap_or_default();
    let (host, port) = if server == "::1" {
        (server, None)
    } else if let Some(rest) = server.strip_prefix('[') {
        let (host, after) = rest.split_once(']').unwrap_or((rest, ""));
        (host, after.strip_prefix(':'))
    } else {
        match server.rsplit_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (server, None),
        }
    };
    let loopback = match host.to_ascii_lowercase().as_str() {
        "localhost" | "localhost.localdomain" => Loopback::Localhost,
        "127.0.0.1" => Loopback::V4,
        "::1" => Loopback::V6,
        _ => return Err(Error::Remote(clean(server))),
    };
    let port = match port {
        Some(port) => port.parse().map_err(|_| Error::Remote(clean(server)))?,
        None => IPP_PORT,
    };
    // libcups prefers the socket for localhost on the default port too.
    Ok(match socket {
        Some(path) if port == IPP_PORT => Endpoint::Socket(path.to_owned()),
        _ => Endpoint::Loopback(loopback, port),
    })
}

/// A printer from its attributes; `None` without a name.
fn parse_printer(attributes: &[Attribute]) -> Option<Printer> {
    let get = |name: &str| attributes.iter().find(|attr| attr.name == name);
    let text = |name: &str| {
        get(name)
            .and_then(Attribute::str)
            .map(clean)
            .unwrap_or_default()
    };
    let name = get("printer-name").and_then(Attribute::str)?.to_owned();
    let state = match get("printer-state").and_then(Attribute::int) {
        Some(4) => PrinterState::Processing,
        Some(5) => PrinterState::Stopped,
        _ => PrinterState::Idle,
    };
    let mut reasons: Vec<Reason> = get("printer-state-reasons")
        .map(|attr| attr.strs().filter_map(Reason::parse).collect())
        .unwrap_or_default();
    reasons.sort_by_key(|reason| std::cmp::Reverse(reason.severity));
    let ints =
        |name: &str| -> Vec<i32> { get(name).map(|a| a.ints().collect()).unwrap_or_default() };
    let strs = |name: &str| -> Vec<String> {
        get(name)
            .map(|a| a.strs().map(clean).collect())
            .unwrap_or_default()
    };
    let names = strs("marker-names");
    let kinds = strs("marker-types");
    let colors = strs("marker-colors");
    let levels = ints("marker-levels");
    let lows = ints("marker-low-levels");
    let highs = ints("marker-high-levels");
    let markers = names
        .iter()
        .enumerate()
        .map(|(i, name)| Marker {
            name: name.clone(),
            kind: kinds.get(i).cloned().unwrap_or_default(),
            colors: colors.get(i).map(|c| parse_colors(c)).unwrap_or_default(),
            level: Level::from_ipp(levels.get(i).copied().unwrap_or(-1)),
            low: lows.get(i).copied().unwrap_or(10),
            high: highs.get(i).copied().unwrap_or(100),
        })
        .collect();
    Some(Printer {
        info: text("printer-info"),
        location: text("printer-location"),
        model: text("printer-make-and-model"),
        device_uri: text("device-uri"),
        state,
        reasons,
        message: text("printer-state-message"),
        accepting: get("printer-is-accepting-jobs")
            .and_then(Attribute::bool)
            .unwrap_or(true),
        shared: get("printer-is-shared")
            .and_then(Attribute::bool)
            .unwrap_or(false),
        temporary: get("printer-is-temporary")
            .and_then(Attribute::bool)
            .unwrap_or(false),
        class: get("printer-type")
            .and_then(Attribute::int)
            .is_some_and(|kind| kind & CUPS_PRINTER_CLASS != 0),
        markers,
        name,
    })
}

/// A job from its attributes; `None` without an id.
fn parse_job(attributes: &[Attribute]) -> Option<Job> {
    let get = |name: &str| attributes.iter().find(|attr| attr.name == name);
    let text = |name: &str| {
        get(name)
            .and_then(Attribute::str)
            .map(clean)
            .unwrap_or_default()
    };
    let printer = get("job-printer-uri")
        .and_then(Attribute::str)
        .and_then(|uri| uri.rsplit('/').next())
        .map(percent_decode)
        .unwrap_or_default();
    Some(Job {
        id: get("job-id").and_then(Attribute::int)?,
        printer,
        title: text("job-name"),
        owner: text("job-originating-user-name"),
        state: JobState::from_ipp(get("job-state").and_then(Attribute::int).unwrap_or(3)),
        created: get("time-at-creation")
            .and_then(Attribute::int)
            .map(i64::from),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attr(name: &str, values: Vec<Value>) -> Attribute {
        Attribute {
            name: name.to_owned(),
            values,
        }
    }

    #[test]
    fn a_socket_without_access_is_not_a_cups_that_is_off() {
        use std::io::{Error as IoError, ErrorKind};
        let socket = Endpoint::Socket(PathBuf::from("/run/cups/cups.sock"));
        let error = |kind| unreachable(&socket, &IoError::from(kind));
        assert!(matches!(
            error(ErrorKind::PermissionDenied),
            Error::NoAccess(_)
        ));
        assert!(matches!(error(ErrorKind::NotFound), Error::Unavailable(_)));
        assert!(matches!(
            error(ErrorKind::ConnectionRefused),
            Error::Unavailable(_)
        ));
    }

    #[test]
    fn the_endpoint_is_local_only() {
        let socket = |p: &Path| p == Path::new("/run/cups/cups.sock");
        let none = |_: &Path| false;
        assert_eq!(
            endpoint(None, socket),
            Ok(Endpoint::Socket("/run/cups/cups.sock".into()))
        );
        assert_eq!(
            endpoint(None, none),
            Ok(Endpoint::Loopback(Loopback::Localhost, 631))
        );
        assert_eq!(
            endpoint(Some("/tmp/x/cups.sock"), none),
            Ok(Endpoint::Socket("/tmp/x/cups.sock".into()))
        );
        assert_eq!(
            endpoint(Some("localhost"), socket),
            Ok(Endpoint::Socket("/run/cups/cups.sock".into()))
        );
        assert_eq!(
            endpoint(Some("localhost:8631"), socket),
            Ok(Endpoint::Loopback(Loopback::Localhost, 8631))
        );
        assert_eq!(
            endpoint(Some("[::1]:632"), none),
            Ok(Endpoint::Loopback(Loopback::V6, 632))
        );
        assert_eq!(
            endpoint(Some("print.example.com"), socket),
            Err(Error::Remote("print.example.com".to_owned()))
        );
        assert_eq!(
            endpoint(Some("192.168.1.2:631/version=1.1"), socket),
            Err(Error::Remote("192.168.1.2:631".to_owned()))
        );
        assert!(endpoint(Some("localhost:x"), none).is_err());
        assert_eq!(
            endpoint(Some("::1"), none),
            Ok(Endpoint::Loopback(Loopback::V6, 631))
        );
        assert_eq!(
            endpoint(Some("127.0.0.1:8631"), socket),
            Ok(Endpoint::Loopback(Loopback::V4, 8631))
        );
    }

    #[test]
    fn user_names_are_safe_in_a_header() {
        assert_eq!(header_safe("alice"), "alice");
        assert_eq!(header_safe("al\r\nX-Evil: 1ice"), "alX-Evil:1ice");
        assert_eq!(header_safe("\u{7}"), "");
    }

    #[test]
    fn client_conf_names_the_server() {
        assert_eq!(
            server_name("# comment\nservername /run/x.sock\n"),
            Some("/run/x.sock".to_owned())
        );
        assert_eq!(server_name("Encryption Never\n"), None);
        assert_eq!(
            server_name("  ServerName   /run/my cups/cups.sock  \n"),
            Some("/run/my cups/cups.sock".to_owned())
        );
        assert_eq!(server_name("ServerName\n"), None);
    }

    #[test]
    fn printers_are_read_from_their_attributes() {
        let attributes = vec![
            attr("printer-name", vec![Value::Name("Office".to_owned())]),
            attr(
                "printer-info",
                vec![Value::Text("Office\u{1b}[31m".to_owned())],
            ),
            attr("printer-state", vec![Value::Enum(5)]),
            attr(
                "printer-state-reasons",
                vec![
                    Value::Keyword("toner-low-warning".to_owned()),
                    Value::Keyword("paused".to_owned()),
                ],
            ),
            attr("printer-type", vec![Value::Enum(0x0080_9004)]),
            attr(
                "marker-names",
                vec![
                    Value::Name("Black".to_owned()),
                    Value::Name("Color".to_owned()),
                ],
            ),
            attr(
                "marker-colors",
                vec![
                    Value::Name("#000000".to_owned()),
                    Value::Name("#00FFFF#FF00FF#FFFF00".to_owned()),
                ],
            ),
            attr(
                "marker-levels",
                vec![Value::Integer(80), Value::Integer(-3)],
            ),
            attr("marker-types", vec![Value::Keyword("toner".to_owned())]),
        ];
        let printer = parse_printer(&attributes).unwrap();
        assert_eq!(printer.name, "Office");
        assert_eq!(printer.info, "Office [31m");
        assert!(printer.paused());
        assert_eq!(printer.status().english(), "Paused · Toner low");
        assert!(!printer.class);
        assert_eq!(printer.markers.len(), 2);
        assert_eq!(printer.markers[0].level, Level::Percent(80));
        assert_eq!(printer.markers[0].kind, "toner");
        assert_eq!(printer.markers[1].colors.len(), 3);
        assert_eq!(printer.markers[1].level, Level::SomeLeft);
        assert_eq!(printer.markers[1].kind, "");
        assert!(parse_printer(&attributes[1..]).is_none());
    }

    #[test]
    fn jobs_are_read_from_their_attributes() {
        let attributes = vec![
            attr("job-id", vec![Value::Integer(42)]),
            attr("job-name", vec![Value::Name("Report.pdf".to_owned())]),
            attr(
                "job-printer-uri",
                vec![Value::Uri(
                    "ipp://localhost:631/printers/B%C3%BCro".to_owned(),
                )],
            ),
            attr("job-state", vec![Value::Enum(4)]),
            attr("time-at-creation", vec![Value::Integer(1_700_000_000)]),
        ];
        let job = parse_job(&attributes).unwrap();
        assert_eq!(job.id, 42);
        assert_eq!(job.printer, "Büro");
        assert_eq!(job.state, JobState::Held);
        assert_eq!(job.created, Some(1_700_000_000));
        assert_eq!(job.owner, "");
        assert!(parse_job(&attributes[1..]).is_none());
    }
}
