// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Stand-ins for the printing services, for tests. Built with the `test-support` feature.
//!
//! [`StandIn`] runs the distribution's own `cupsd` as the user, with a configuration in a
//! temporary directory: its own domain socket, no network port, no access to the machine's
//! printers, and the default policies of CUPS. [`FakePrinter`] is a driverless printer on the
//! loopback address that answers what CUPS asks when it adds a printer with the `everywhere`
//! model and takes jobs. [`Services`] puts stand-ins for Avahi and cups-pk-helper on a private
//! bus, made with python-dbusmock.
//!
//! They need `cupsd`, `dbus-daemon` and python-dbusmock, which the integration containers have;
//! the tests that use them are ignored by a plain `cargo test`.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use async_io::Timer;
use tempfile::TempDir;
use zbus::Connection;
use zbus::names::BusName;
use zbus::zvariant::{OwnedValue, Value};

use super::ipp::{Message, Value as IppValue, tag};
use super::{Bus, Cups, Endpoint};

/// The error of a stand-in.
pub type Error = Box<dyn std::error::Error + Send + Sync>;

/// Where the distributions keep `cupsd`.
const CUPSD: [&str; 3] = ["/usr/sbin/cupsd", "/usr/bin/cupsd", "/sbin/cupsd"];
/// Where they keep CUPS's filters and backends.
const SERVER_BIN: [&str; 4] = [
    "/usr/lib/cups",
    "/usr/libexec/cups",
    "/usr/lib64/cups",
    "/usr/local/lib/cups",
];
/// Where they keep CUPS's data.
const DATA_DIR: [&str; 2] = ["/usr/share/cups", "/usr/local/share/cups"];

/// The policies of CUPS's default `cupsd.conf`: users print and cancel their own jobs, the
/// members of `SystemGroup` change printers.
const POLICY: &str = "\
<Location />
  Order allow,deny
  Allow localhost
</Location>
<Location /admin>
  AuthType Default
  Require user @SYSTEM
  Order allow,deny
  Allow localhost
</Location>
<Policy default>
  JobPrivateAccess default
  JobPrivateValues default
  SubscriptionPrivateAccess default
  SubscriptionPrivateValues default
  <Limit Create-Job Print-Job Print-URI Validate-Job>
    Order deny,allow
  </Limit>
  <Limit Send-Document Send-URI Hold-Job Release-Job Restart-Job Purge-Jobs \
Set-Job-Attributes Create-Job-Subscription Renew-Subscription Cancel-Subscription \
Get-Notifications Reprocess-Job Cancel-Current-Job Suspend-Current-Job Resume-Job \
Cancel-My-Jobs Close-Job CUPS-Move-Job CUPS-Get-Document>
    Require user @OWNER @SYSTEM
    Order deny,allow
  </Limit>
  <Limit CUPS-Add-Modify-Printer CUPS-Delete-Printer CUPS-Add-Modify-Class \
CUPS-Delete-Class CUPS-Set-Default CUPS-Get-Devices>
    AuthType Default
    Require user @SYSTEM
    Order deny,allow
  </Limit>
  <Limit Pause-Printer Resume-Printer Enable-Printer Disable-Printer \
Pause-Printer-After-Current-Job Hold-New-Jobs Release-Held-New-Jobs Deactivate-Printer \
Activate-Printer Restart-Printer Shutdown-Printer Startup-Printer Promote-Job \
Schedule-Job-After Cancel-Jobs CUPS-Accept-Jobs CUPS-Reject-Jobs>
    AuthType Default
    Require user @SYSTEM
    Order deny,allow
  </Limit>
  <Limit Cancel-Job CUPS-Authenticate-Job>
    Require user @OWNER @SYSTEM
    Order deny,allow
  </Limit>
  <Limit All>
    Order deny,allow
  </Limit>
</Policy>
";

/// A private `cupsd`. Stops when dropped.
#[derive(Debug)]
pub struct StandIn {
    cupsd: Child,
    socket: PathBuf,
    dir: TempDir,
}

impl StandIn {
    /// Starts a CUPS without printers. With `admin`, the user is in its `SystemGroup` (the
    /// user's own group), so CUPS lets them change printers; without, it is `root`'s group.
    ///
    /// # Errors
    ///
    /// If `cupsd` is missing or doesn't come up within ten seconds.
    pub fn start(admin: bool) -> Result<Self, Error> {
        Self::start_with(admin, "")
    }

    /// Starts a CUPS with the printers of `printers_conf`, in the format of CUPS's
    /// `printers.conf` (`<Printer Name> … </Printer>`), as if it had them from before.
    ///
    /// # Errors
    ///
    /// As [`start`](Self::start).
    pub fn start_with(admin: bool, printers_conf: &str) -> Result<Self, Error> {
        let cupsd = CUPSD
            .iter()
            .map(Path::new)
            .find(|path| path.exists())
            .ok_or("cupsd isn't installed")?;
        let server_bin = SERVER_BIN
            .iter()
            .map(Path::new)
            .find(|dir| dir.join("backend/ipp").exists())
            .ok_or("CUPS's ipp backend isn't installed")?;
        let data_dir = DATA_DIR
            .iter()
            .map(Path::new)
            .find(|dir| dir.exists())
            .unwrap_or(Path::new("/usr/share/cups"));
        let dir = tempfile::tempdir()?;
        let root = dir.path();
        for sub in [
            "bin/backend",
            "bin/filter",
            "spool/tmp",
            "cache",
            "state",
            "log",
        ] {
            std::fs::create_dir_all(root.join(sub))?;
        }
        server_dir(server_bin, &root.join("bin"))?;
        let socket = root.join("cups.sock");
        let group = if admin {
            own_group()?
        } else {
            "root".to_owned()
        };
        write_config(root, &socket, &group, data_dir)?;
        if !printers_conf.is_empty() {
            std::fs::write(root.join("printers.conf"), printers_conf)?;
        }
        let child = Command::new(cupsd)
            .arg("-f")
            .arg("-c")
            .arg(root.join("cupsd.conf"))
            .arg("-s")
            .arg(root.join("cups-files.conf"))
            .env_remove("CUPS_SERVER")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| format!("can't start cupsd: {error}"))?;
        let mut stand_in = Self {
            cupsd: child,
            socket,
            dir,
        };
        let start = Instant::now();
        while std::os::unix::net::UnixStream::connect(&stand_in.socket).is_err() {
            if start.elapsed() > Duration::from_secs(10)
                || stand_in.cupsd.try_wait().ok().flatten().is_some()
            {
                return Err(format!("cupsd didn't start; its log:\n{}", stand_in.log()).into());
            }
            thread::sleep(Duration::from_millis(50));
        }
        Ok(stand_in)
    }

    /// Its domain socket.
    pub fn socket(&self) -> &Path {
        &self.socket
    }

    /// A client of it, without cups-pk-helper.
    pub fn cups(&self) -> Cups {
        Cups::at(Endpoint::Socket(self.socket.clone())).with_admin_bus(Bus::Off)
    }

    /// What CUPS logged.
    pub fn log(&self) -> String {
        std::fs::read_to_string(self.dir.path().join("log/error_log")).unwrap_or_default()
    }

    /// Stops it, as when `cups.service` stops.
    pub fn stop(&mut self) {
        let _ = self.cupsd.kill();
        let _ = self.cupsd.wait();
    }
}

impl Drop for StandIn {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Fills `bin`, the stand-in's `ServerBin`, from the distribution's `server_bin`. The backends
/// run as the user, so they are copies the user may run.
fn server_dir(server_bin: &Path, bin: &Path) -> Result<(), Error> {
    for backend in ["ipp", "ipps"] {
        let target = bin.join("backend").join(backend);
        std::fs::copy(server_bin.join("backend/ipp"), &target)?;
        std::fs::set_permissions(&target, std::os::unix::fs::PermissionsExt::from_mode(0o755))?;
    }
    if let Ok(filters) = std::fs::read_dir(server_bin.join("filter")) {
        for filter in filters.flatten() {
            let _ = std::os::unix::fs::symlink(
                filter.path(),
                bin.join("filter").join(filter.file_name()),
            );
        }
    }
    for sub in ["daemon", "notifier", "monitor", "command", "driver"] {
        if server_bin.join(sub).exists() {
            std::os::unix::fs::symlink(server_bin.join(sub), bin.join(sub))?;
        }
    }
    Ok(())
}

/// Writes `cupsd.conf` and `cups-files.conf` into `root`: listening on `socket` only, with
/// `group` as the `SystemGroup`.
fn write_config(root: &Path, socket: &Path, group: &str, data_dir: &Path) -> Result<(), Error> {
    std::fs::write(
        root.join("cupsd.conf"),
        format!(
            "Listen {}\nLogLevel debug\nWebInterface No\nBrowsing No\n\
             DefaultShared Yes\nMaxLogSize 0\n{POLICY}",
            socket.display()
        ),
    )?;
    let mut files = String::new();
    let _ = writeln!(files, "SystemGroup {group}");
    for (key, value) in [
        ("ServerRoot", root.to_owned()),
        ("ServerBin", root.join("bin")),
        ("DataDir", data_dir.to_owned()),
        ("RequestRoot", root.join("spool")),
        ("TempDir", root.join("spool/tmp")),
        ("CacheDir", root.join("cache")),
        ("StateDir", root.join("state")),
        ("AccessLog", root.join("log/access_log")),
        ("ErrorLog", root.join("log/error_log")),
        ("PageLog", root.join("log/page_log")),
    ] {
        let _ = writeln!(files, "{key} {}", value.display());
    }
    std::fs::write(root.join("cups-files.conf"), files)?;
    Ok(())
}

/// The name of the user's own group, which `id -gn` prints.
fn own_group() -> Result<String, Error> {
    let output = Command::new("id").arg("-gn").output()?;
    let group = String::from_utf8(output.stdout)?.trim().to_owned();
    if group.is_empty() {
        return Err("id -gn printed no group".into());
    }
    Ok(group)
}

/// A driverless printer on the loopback address: it answers `Get-Printer-Attributes` as an IPP
/// Everywhere printer does and takes every job. Stops when dropped.
#[derive(Debug)]
pub struct FakePrinter {
    uri: String,
    jobs: Arc<AtomicUsize>,
    stop: Arc<Mutex<bool>>,
    port: u16,
}

impl FakePrinter {
    /// Starts the printer with the make and model `model`.
    ///
    /// # Errors
    ///
    /// If no port on the loopback address is free.
    pub fn start(model: &str) -> Result<Self, Error> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        let uri = format!("ipp://127.0.0.1:{port}/ipp/print");
        let jobs = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(Mutex::new(false));
        let attributes = Arc::new(printer_attributes(model, &uri));
        {
            let jobs = Arc::clone(&jobs);
            let stop = Arc::clone(&stop);
            thread::spawn(move || {
                for stream in listener.incoming() {
                    if stop.lock().map_or(true, |stop| *stop) {
                        return;
                    }
                    let Ok(stream) = stream else { continue };
                    let jobs = Arc::clone(&jobs);
                    let attributes = Arc::clone(&attributes);
                    thread::spawn(move || serve(stream, &attributes, &jobs));
                }
            });
        }
        Ok(Self {
            uri,
            jobs,
            stop,
            port,
        })
    }

    /// Its device URI.
    pub fn uri(&self) -> &str {
        &self.uri
    }

    /// How many jobs it got.
    pub fn jobs(&self) -> usize {
        self.jobs.load(Ordering::SeqCst)
    }
}

impl Drop for FakePrinter {
    fn drop(&mut self) {
        if let Ok(mut stop) = self.stop.lock() {
            *stop = true;
        }
        // Wakes the listener up, so it sees the stop.
        let _ = TcpStream::connect(("127.0.0.1", self.port));
    }
}

/// What the fake printer reports about itself: enough for CUPS to make an IPP Everywhere
/// driver.
fn printer_attributes(model: &str, uri: &str) -> Vec<(&'static str, Vec<IppValue>)> {
    let keywords = |values: &[&str]| -> Vec<IppValue> {
        values
            .iter()
            .map(|v| IppValue::Keyword((*v).to_owned()))
            .collect()
    };
    let mimes = |values: &[&str]| -> Vec<IppValue> {
        values
            .iter()
            .map(|v| IppValue::MimeType((*v).to_owned()))
            .collect()
    };
    let resolution = vec![
        IppValue::Resolution(300, 300, 3),
        IppValue::Resolution(600, 600, 3),
    ];
    vec![
        ("printer-uri-supported", vec![IppValue::Uri(uri.to_owned())]),
        ("uri-security-supported", keywords(&["none"])),
        ("uri-authentication-supported", keywords(&["none"])),
        ("printer-name", vec![IppValue::Name(model.to_owned())]),
        (
            "printer-make-and-model",
            vec![IppValue::Text(model.to_owned())],
        ),
        ("printer-state", vec![IppValue::Enum(3)]),
        ("printer-state-reasons", keywords(&["none"])),
        ("printer-is-accepting-jobs", vec![IppValue::Boolean(true)]),
        ("ipp-versions-supported", keywords(&["1.1", "2.0"])),
        (
            "operations-supported",
            [2, 4, 5, 6, 8, 9, 10, 11]
                .into_iter()
                .map(IppValue::Enum)
                .collect(),
        ),
        (
            "document-format-supported",
            mimes(&[
                "application/octet-stream",
                "application/pdf",
                "image/pwg-raster",
            ]),
        ),
        (
            "document-format-default",
            mimes(&["application/octet-stream"]),
        ),
        (
            "pwg-raster-document-resolution-supported",
            resolution.clone(),
        ),
        (
            "pwg-raster-document-type-supported",
            keywords(&["sgray_8", "srgb_8"]),
        ),
        ("pwg-raster-document-sheet-back", keywords(&["normal"])),
        ("printer-resolution-supported", resolution),
        (
            "printer-resolution-default",
            vec![IppValue::Resolution(600, 600, 3)],
        ),
        (
            "media-supported",
            keywords(&["iso_a4_210x297mm", "na_letter_8.5x11in"]),
        ),
        ("media-default", keywords(&["iso_a4_210x297mm"])),
        ("media-ready", keywords(&["iso_a4_210x297mm"])),
        (
            "sides-supported",
            keywords(&["one-sided", "two-sided-long-edge", "two-sided-short-edge"]),
        ),
        ("sides-default", keywords(&["one-sided"])),
        (
            "print-color-mode-supported",
            keywords(&["auto", "monochrome", "color"]),
        ),
        ("print-color-mode-default", keywords(&["auto"])),
        ("color-supported", vec![IppValue::Boolean(true)]),
        (
            "print-quality-supported",
            vec![IppValue::Enum(3), IppValue::Enum(4), IppValue::Enum(5)],
        ),
        ("marker-names", vec![IppValue::Name("Black".to_owned())]),
        ("marker-types", keywords(&["toner"])),
        ("marker-colors", vec![IppValue::Name("#000000".to_owned())]),
        ("marker-levels", vec![IppValue::Integer(60)]),
    ]
}

/// Answers the IPP requests on one connection until it closes.
fn serve(stream: TcpStream, attributes: &[(&'static str, Vec<IppValue>)], jobs: &AtomicUsize) {
    let Ok(write_half) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(stream);
    let mut writer = write_half;
    while let Some(body) = read_request(&mut reader, &mut writer) {
        let Ok(request) = Message::decode(&body) else {
            return;
        };
        let mut response = Message::request(0, request.request_id);
        response.version = (2, 0);
        match request.code {
            // Get-Printer-Attributes.
            0x000B => {
                for (name, values) in attributes {
                    response.add(tag::PRINTER, name, values.clone());
                }
            }
            // Print-Job, Create-Job.
            0x0002 | 0x0005 => {
                let id = jobs.fetch_add(1, Ordering::SeqCst) + 1;
                let id = i32::try_from(id).unwrap_or(i32::MAX);
                response.add(tag::JOB, "job-id", vec![IppValue::Integer(id)]);
                response.add(tag::JOB, "job-state", vec![IppValue::Enum(9)]);
            }
            // Get-Job-Attributes: done.
            0x0009 => {
                response.add(tag::JOB, "job-state", vec![IppValue::Enum(9)]);
                response.add(
                    tag::JOB,
                    "job-state-reasons",
                    vec![IppValue::Keyword("job-completed-successfully".to_owned())],
                );
            }
            _ => {}
        }
        let bytes = response.encode();
        let head = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/ipp\r\nContent-Length: {}\r\n\r\n",
            bytes.len()
        );
        if writer.write_all(head.as_bytes()).is_err() || writer.write_all(&bytes).is_err() {
            return;
        }
    }
}

/// Reads one HTTP request and returns its body; `None` once the connection ends.
fn read_request(reader: &mut BufReader<TcpStream>, writer: &mut TcpStream) -> Option<Vec<u8>> {
    let mut headers = HashMap::new();
    let mut line = String::new();
    // The request line.
    loop {
        line.clear();
        if reader.read_line(&mut line).ok()? == 0 {
            return None;
        }
        if !line.trim().is_empty() {
            break;
        }
    }
    loop {
        line.clear();
        if reader.read_line(&mut line).ok()? == 0 {
            return None;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            break;
        }
        if let Some((name, value)) = trimmed.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_owned());
        }
    }
    if headers
        .get("expect")
        .is_some_and(|expect| expect.contains("100-continue"))
    {
        writer.write_all(b"HTTP/1.1 100 Continue\r\n\r\n").ok()?;
    }
    let mut body = Vec::new();
    if headers
        .get("transfer-encoding")
        .is_some_and(|encoding| encoding.contains("chunked"))
    {
        loop {
            line.clear();
            reader.read_line(&mut line).ok()?;
            let size = usize::from_str_radix(line.trim().split(';').next()?, 16).ok()?;
            let mut chunk = vec![0; size + 2];
            reader.read_exact(&mut chunk).ok()?;
            if size == 0 {
                break;
            }
            body.extend_from_slice(chunk.get(..size)?);
        }
    } else if let Some(length) = headers.get("content-length") {
        let mut chunk = vec![0; length.parse().ok()?];
        reader.read_exact(&mut chunk).ok()?;
        body = chunk;
    }
    Some(body)
}

/// `ServiceBrowserNew`: a browser object that reports the services of its type announced so
/// far, and those announced later.
const BROWSER_NEW: &str = "\
self.browsers = getattr(self, 'browsers', {})
self.services = getattr(self, 'services', {})
path = '/Client1/ServiceBrowser%d' % (len(self.browsers) + 1)
self.AddObject(path, 'org.freedesktop.Avahi.ServiceBrowser', {}, [('Free', '', '', '')])
self.browsers[path] = args[2]
for (name, kind), (flags, txt) in self.services.items():
    if kind == args[2]:
        objects[path].EmitSignal('org.freedesktop.Avahi.ServiceBrowser', 'ItemNew', 'iisssu',
            [2, 0, name, kind, 'local', dbus.UInt32(flags)])
ret = dbus.ObjectPath(path)
";

/// The test helper `Announce(name, type, flags, txt)`.
const ANNOUNCE: &str = "\
self.browsers = getattr(self, 'browsers', {})
self.services = getattr(self, 'services', {})
self.services[(args[0], args[1])] = (int(args[2]), list(args[3]))
for path, kind in self.browsers.items():
    if kind == args[1]:
        objects[path].EmitSignal('org.freedesktop.Avahi.ServiceBrowser', 'ItemNew', 'iisssu',
            [2, 0, args[0], kind, 'local', dbus.UInt32(args[2])])
";

/// The test helper `Withdraw(name, type)`.
const WITHDRAW: &str = "\
self.browsers = getattr(self, 'browsers', {})
self.services = getattr(self, 'services', {})
self.services.pop((args[0], args[1]), None)
for path, kind in self.browsers.items():
    if kind == args[1]:
        objects[path].EmitSignal('org.freedesktop.Avahi.ServiceBrowser', 'ItemRemove', 'iisssu',
            [2, 0, args[0], kind, 'local', dbus.UInt32(0)])
";

/// `ResolveService`: the TXT record of an announced service.
const RESOLVE: &str = "\
services = getattr(self, 'services', {})
if (args[2], args[3]) not in services:
    raise dbus.exceptions.DBusException('Timeout reached', name='org.freedesktop.Avahi.TimeoutError')
flags, txt = services[(args[2], args[3])]
if 'unresolvable' in txt:
    raise dbus.exceptions.DBusException('Timeout reached', name='org.freedesktop.Avahi.TimeoutError')
records = dbus.Array([dbus.Array([dbus.Byte(b) for b in t.encode()], signature='y') for t in txt],
    signature='ay')
ret = (args[0], args[1], args[2], args[3], args[4], 'printer.local', args[5], '',
    dbus.UInt16(631), records, dbus.UInt32(flags))
";

/// What cups-pk-helper's methods do in the stand-in: nothing, or refuse as polkit does.
const PK_HELPER_METHODS: [(&str, &str); 9] = [
    ("PrinterAdd", "sssss"),
    ("PrinterDelete", "s"),
    ("PrinterSetDefault", "s"),
    ("PrinterSetEnabled", "sb"),
    ("PrinterSetAcceptJobs", "sbs"),
    ("PrinterSetInfo", "ss"),
    ("PrinterSetLocation", "ss"),
    ("PrinterSetShared", "sb"),
    ("JobCancelPurge", "ib"),
];

/// The bus name of cups-pk-helper.
const PK_HELPER: &str = "org.opensuse.CupsPkHelper.Mechanism";
/// The bus name of Avahi.
const AVAHI: &str = "org.freedesktop.Avahi";
/// The interface dbusmock puts on every object.
const DBUS_MOCK: &str = "org.freedesktop.DBus.Mock";

/// A private bus with stand-ins for Avahi and cups-pk-helper. Stops them when dropped.
#[derive(Debug)]
pub struct Services {
    address: String,
    connection: Connection,
    daemon: Child,
    mocks: Vec<Child>,
    dir: TempDir,
}

impl Services {
    /// Starts the bus with Avahi's stand-in, and with cups-pk-helper's if `pk_helper`; it
    /// refuses every change as polkit does when the user doesn't give the password if `deny`.
    ///
    /// # Errors
    ///
    /// If `dbus-daemon` or python-dbusmock is missing, or a stand-in doesn't come up within ten
    /// seconds.
    pub async fn start(pk_helper: bool, deny: bool) -> Result<Self, Error> {
        let crate::dbus::TestBus {
            dir,
            address,
            daemon,
            connection,
        } = crate::dbus::TestBus::start().await?;
        let mut services = Self {
            address,
            connection,
            daemon,
            mocks: Vec::new(),
            dir,
        };
        services
            .mock(AVAHI, "org.freedesktop.Avahi.Server", "avahi")
            .await?;
        let server = "org.freedesktop.Avahi.Server";
        services
            .add_method(
                AVAHI,
                server,
                "ServiceBrowserNew",
                "iissu",
                "o",
                BROWSER_NEW,
            )
            .await?;
        services
            .add_method(
                AVAHI,
                server,
                "ResolveService",
                "iisssiu",
                "iissssisqaayu",
                RESOLVE,
            )
            .await?;
        let test = "org.freedesktop.Avahi.Test";
        services
            .add_method(AVAHI, test, "Announce", "ssuas", "", ANNOUNCE)
            .await?;
        services
            .add_method(AVAHI, test, "Withdraw", "ss", "", WITHDRAW)
            .await?;
        if pk_helper {
            services.mock(PK_HELPER, PK_HELPER, "pk-helper").await?;
            let code = if deny {
                "raise dbus.exceptions.DBusException('Not authorized', \
                 name='org.opensuse.CupsPkHelper.Mechanism.NotPrivileged')"
            } else {
                "ret = ''"
            };
            for (method, inputs) in PK_HELPER_METHODS {
                services
                    .add_method(PK_HELPER, PK_HELPER, method, inputs, "s", code)
                    .await?;
            }
        }
        Ok(services)
    }

    /// The address of the bus.
    pub fn address(&self) -> &str {
        &self.address
    }

    /// The bus, for [`Cups::with_admin_bus`] and [`Discovery::start`](super::Discovery::start).
    pub fn bus(&self) -> Bus {
        Bus::Address(self.address.clone())
    }

    /// Starts a dbusmock named `name` with its main object at `/` with `interface`.
    async fn mock(&mut self, name: &str, interface: &str, log: &str) -> Result<(), Error> {
        let mock = Command::new("python3")
            .args([
                "-m",
                "dbusmock",
                "--session",
                name,
                "/",
                interface,
                "--logfile",
            ])
            .arg(self.dir.path().join(format!("{log}.log")))
            .env("DBUS_SESSION_BUS_ADDRESS", &self.address)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| format!("can't start python3 -m dbusmock: {error}"))?;
        self.mocks.push(mock);
        let dbus = zbus::fdo::DBusProxy::new(&self.connection).await?;
        for _ in 0..100 {
            if dbus.name_has_owner(BusName::try_from(name)?).await? {
                return Ok(());
            }
            Timer::after(Duration::from_millis(100)).await;
        }
        Err(format!("the stand-in for {name} didn't start").into())
    }

    async fn add_method(
        &self,
        name: &str,
        interface: &str,
        method: &str,
        inputs: &str,
        outputs: &str,
        code: &str,
    ) -> Result<(), Error> {
        self.connection
            .call_method(
                Some(name),
                "/",
                Some(DBUS_MOCK),
                "AddMethod",
                &(interface, method, inputs, outputs, code),
            )
            .await?;
        Ok(())
    }

    /// Announces the printer service `name` of the type `kind` (`_ipp._tcp`) with the TXT
    /// record `txt` (`ty=Model`) and the Avahi result flags `flags` (8: from this computer). A
    /// TXT record with `unresolvable` makes resolving the service fail.
    ///
    /// # Errors
    ///
    /// If the stand-in refuses.
    pub async fn announce(
        &self,
        name: &str,
        kind: &str,
        flags: u32,
        txt: &[&str],
    ) -> Result<(), Error> {
        self.connection
            .call_method(
                Some(AVAHI),
                "/",
                Some("org.freedesktop.Avahi.Test"),
                "Announce",
                &(name, kind, flags, txt),
            )
            .await?;
        Ok(())
    }

    /// Withdraws the service `name` of the type `kind`.
    ///
    /// # Errors
    ///
    /// If the stand-in refuses.
    pub async fn withdraw(&self, name: &str, kind: &str) -> Result<(), Error> {
        self.connection
            .call_method(
                Some(AVAHI),
                "/",
                Some("org.freedesktop.Avahi.Test"),
                "Withdraw",
                &(name, kind),
            )
            .await?;
        Ok(())
    }

    /// The calls of cups-pk-helper's `method` so far, each with its arguments.
    ///
    /// # Errors
    ///
    /// If the stand-in doesn't answer.
    pub async fn pk_helper_calls(&self, method: &str) -> Result<Vec<Vec<OwnedValue>>, Error> {
        let reply = self
            .connection
            .call_method(
                Some(PK_HELPER),
                "/",
                Some(DBUS_MOCK),
                "GetMethodCalls",
                &(method,),
            )
            .await?;
        let calls: Vec<(u64, Vec<OwnedValue>)> = reply.body().deserialize()?;
        Ok(calls.into_iter().map(|(_, args)| args).collect())
    }
}

impl Drop for Services {
    fn drop(&mut self) {
        for mock in &mut self.mocks {
            let _ = mock.kill();
            let _ = mock.wait();
        }
        let _ = self.daemon.kill();
        let _ = self.daemon.wait();
    }
}

/// The string in `value`, for checking the arguments of a call.
pub fn string(value: &OwnedValue) -> Option<String> {
    match &**value {
        Value::Str(s) => Some(s.to_string()),
        _ => None,
    }
}
