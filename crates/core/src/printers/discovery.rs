// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Driverless printers around: what Avahi finds of the services `_ipp._tcp` and `_ipps._tcp`, on
//! the network and on USB (ipp-usb announces IPP-over-USB printers on this computer).
//!
//! Avahi does the looking, on the system bus; Sway Things never contacts a printer. A printer
//! found becomes a device URI made of its service name (`ipps://Name._ipps._tcp.local/`), which
//! CUPS resolves itself when it adds the printer. The queues our own CUPS shares are left out.
//! Everything Avahi reports comes from the network, so it is data.

use std::collections::{HashMap, HashSet, VecDeque};
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use async_io::Timer;
use futures_lite::{Stream, future};
use zbus::message::Type;
use zbus::zvariant::OwnedObjectPath;
use zbus::{Connection, MatchRule, MessageStream};

use super::{Bus, Error, clean, percent_encode};
use crate::i18n::Text;
use crate::msg;

/// Avahi's bus name.
const SERVICE: &str = "org.freedesktop.Avahi";
/// Its server's interface, at `/`.
const SERVER: &str = "org.freedesktop.Avahi.Server";
/// The interface of a service browser.
const BROWSER: &str = "org.freedesktop.Avahi.ServiceBrowser";
/// The service types of IPP printers: secure first.
const TYPES: [&str; 2] = ["_ipps._tcp", "_ipp._tcp"];
/// Any interface, any protocol.
const UNSPEC: i32 = -1;
/// `AVAHI_LOOKUP_NO_ADDRESS`: resolve the TXT record only.
const NO_ADDRESS: u32 = 8;
/// `AVAHI_LOOKUP_RESULT_LOCAL`: announced by this computer.
const LOCAL: u32 = 8;
/// How long resolving a service may take.
const RESOLVE_TIMEOUT: Duration = Duration::from_secs(5);

/// A driverless printer Avahi found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// Its service name, which tells it from the others.
    pub key: String,
    /// Its service name, cleaned to show.
    pub name: String,
    /// Its make and model, if it says (`ty`); else empty.
    pub model: String,
    /// Where it stands, if it says (`note`); else empty.
    pub location: String,
    /// The device URI to add it with.
    pub uri: String,
    /// Its host name and its resource path (`rp`), as Avahi resolved them: CUPS keeps the
    /// resolved address as the device URI of a printer it added, so this tells whether it is
    /// set up already. Empty if Avahi didn't resolve it.
    pub host: String,
    /// Its resource path, see [`host`](Self::host).
    pub resource: String,
    /// Whether it is reached over IPPS (encrypted).
    pub secure: bool,
    /// Whether this computer announces it: a device on USB that ipp-usb makes reachable.
    pub local: bool,
}

impl Found {
    /// Whether a printer with the device URI `uri` prints to it.
    pub fn is_at(&self, uri: &str) -> bool {
        if uri == self.uri {
            return true;
        }
        if self.host.is_empty() {
            return false;
        }
        let rest = uri.split_once("://").map_or(uri, |(_, rest)| rest);
        let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
        let host = authority
            .rsplit_once(':')
            .map_or(authority, |(host, _)| host);
        host.trim_end_matches('.')
            .eq_ignore_ascii_case(self.host.trim_end_matches('.'))
            && path.trim_matches('/') == self.resource.trim_matches('/')
    }

    /// Its title: the model, else the service name.
    pub fn title(&self) -> &str {
        if self.model.is_empty() {
            &self.name
        } else {
            &self.model
        }
    }
}

/// A change of what Avahi finds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Discovered {
    /// A printer appeared, or what is known of it changed.
    Added(Found),
    /// The printer with this key went away.
    Removed(String),
}

/// One announcement of a service: interface, protocol, type and domain.
type Announcement = (i32, i32, String, String);

/// The most printers (found, being resolved or left out) a discovery keeps track of: far more
/// than any network has, so a flood of announcements can't grow it without bound.
const MAX_PRINTERS: usize = 200;

/// What resolving a service gives: its key, and its host name and TXT record if Avahi answered.
type Resolved = (String, Option<(String, HashMap<String, String>)>);

/// A service being resolved.
type Resolving = Pin<Box<dyn Future<Output = Resolved>>>;

/// What Avahi finds, for as long as it lives. Dropping it closes its own connection to the bus,
/// which makes Avahi stop looking.
pub struct Discovery {
    connection: Connection,
    stream: MessageStream,
    browsers: Vec<OwnedObjectPath>,
    /// The announcements of each printer (found or being resolved), by its key, with the result
    /// flags of its first one.
    announced: HashMap<String, (HashSet<Announcement>, u32)>,
    /// The printers reported, by their key.
    found: HashMap<String, Found>,
    /// The services being resolved, side by side, so one that doesn't answer holds up none.
    resolving: Vec<Resolving>,
    /// The keys of services left out (our own CUPS's queues).
    ignored: HashSet<String>,
    pending: VecDeque<Discovered>,
}

impl std::fmt::Debug for Discovery {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Discovery")
            .field("browsers", &self.browsers)
            .field("found", &self.found)
            .field("resolving", &self.resolving.len())
            .finish_non_exhaustive()
    }
}

impl Discovery {
    /// Starts looking for printers through Avahi on `bus`.
    ///
    /// # Errors
    ///
    /// [`Error::Unavailable`] if Avahi isn't running there.
    pub async fn start(bus: &Bus) -> Result<Self, Error> {
        let unavailable = |error: &dyn std::fmt::Display| {
            Error::Unavailable(Text::new(msg!("Avahi isn't running ({error})")).arg("error", error))
        };
        let connection = match bus {
            Bus::System => zbus::connection::Builder::system(),
            Bus::Address(address) => zbus::connection::Builder::address(address.as_str()),
            Bus::Off => return Err(Error::Unavailable(msg!("no bus").into())),
        }
        .map_err(|error| unavailable(&error))?
        .build()
        .await
        .map_err(|error| unavailable(&error))?;
        // Subscribed before the browsers exist, so their first signals don't get lost.
        let rule = MatchRule::builder()
            .msg_type(Type::Signal)
            .sender(SERVICE)
            .and_then(|rule| rule.interface(BROWSER))
            .map_err(|error| unavailable(&error))?
            .build();
        let stream = MessageStream::for_match_rule(rule, &connection, Some(64))
            .await
            .map_err(|error| unavailable(&error))?;
        let mut browsers = Vec::new();
        for kind in TYPES {
            let reply = connection
                .call_method(
                    Some(SERVICE),
                    "/",
                    Some(SERVER),
                    "ServiceBrowserNew",
                    &(UNSPEC, UNSPEC, kind, "", 0u32),
                )
                .await
                .map_err(|error| unavailable(&error))?;
            let path: OwnedObjectPath = reply
                .body()
                .deserialize()
                .map_err(|error| unavailable(&error))?;
            browsers.push(path);
        }
        Ok(Self {
            connection,
            stream,
            browsers,
            announced: HashMap::new(),
            found: HashMap::new(),
            resolving: Vec::new(),
            ignored: HashSet::new(),
            pending: VecDeque::new(),
        })
    }

    /// The next change; `None` once Avahi is gone.
    pub async fn next(&mut self) -> Option<Discovered> {
        loop {
            if let Some(event) = self.pending.pop_front() {
                return Some(event);
            }
            match future::poll_fn(|cx| self.poll_event(cx)).await {
                Event::Resolved(key, resolved) => self.resolved(key, resolved),
                Event::Message(message) => self.message(&message),
                Event::Gone => return None,
            }
        }
    }

    /// The next service resolved or signal of Avahi.
    fn poll_event(&mut self, cx: &mut Context<'_>) -> Poll<Event> {
        for i in 0..self.resolving.len() {
            if let Some(resolving) = self.resolving.get_mut(i) {
                if let Poll::Ready((key, resolved)) = resolving.as_mut().poll(cx) {
                    drop(self.resolving.swap_remove(i));
                    return Poll::Ready(Event::Resolved(key, resolved));
                }
            }
        }
        loop {
            match Pin::new(&mut self.stream).poll_next(cx) {
                Poll::Ready(Some(Ok(message))) => return Poll::Ready(Event::Message(message)),
                Poll::Ready(Some(Err(_))) => {}
                Poll::Ready(None) => return Poll::Ready(Event::Gone),
                Poll::Pending => return Poll::Pending,
            }
        }
    }

    /// Takes a signal of one of our browsers.
    fn message(&mut self, message: &zbus::Message) {
        let header = message.header();
        let Some(path) = header.path() else {
            return;
        };
        if !self
            .browsers
            .iter()
            .any(|browser| browser.as_str() == path.as_str())
        {
            return;
        }
        let member = header.member().map(ToString::to_string).unwrap_or_default();
        let item: Result<(i32, i32, String, String, String, u32), _> = message.body().deserialize();
        match (member.as_str(), item) {
            ("ItemNew", Ok((interface, protocol, name, kind, domain, flags))) => {
                self.item_new((interface, protocol, kind, domain), name, flags);
            }
            ("ItemRemove", Ok((interface, protocol, name, kind, domain, _))) => {
                self.item_remove(&(interface, protocol, kind, domain), &name);
            }
            _ => {}
        }
    }

    fn item_new(&mut self, announcement: Announcement, name: String, flags: u32) {
        let (interface, protocol, kind, domain) = announcement.clone();
        // A domain that can't be part of a host name can't be part of a device URI.
        if self.ignored.contains(&name) || service_uri(&name, &kind, &domain).is_none() {
            return;
        }
        if let Some((announcements, _)) = self.announced.get_mut(&name) {
            announcements.insert(announcement);
            // The secure address wins.
            if let Some(found) = self.found.get_mut(&name) {
                if kind == "_ipps._tcp" && !found.secure {
                    if let Some(uri) = service_uri(&name, &kind, &domain) {
                        found.secure = true;
                        found.uri = uri;
                        self.pending.push_back(Discovered::Added(found.clone()));
                    }
                }
            }
            return;
        }
        if self.announced.len() + self.ignored.len() >= MAX_PRINTERS {
            return;
        }
        self.announced
            .insert(name.clone(), (HashSet::from([announcement]), flags));
        let connection = self.connection.clone();
        self.resolving.push(Box::pin(async move {
            let resolved = resolve(&connection, interface, protocol, &name, &kind, &domain).await;
            (name, resolved)
        }));
    }

    /// Reports the printer `key` once its service is resolved, or not.
    fn resolved(&mut self, key: String, resolved: Option<(String, HashMap<String, String>)>) {
        // Gone while it was resolved.
        let Some((announcements, flags)) = self.announced.get(&key) else {
            return;
        };
        let local = flags & LOCAL != 0;
        // The queues of our own CUPS: adding one would make a queue that prints to itself.
        // Without the TXT record they can't be told from a printer on USB (ipp-usb), so a
        // service of this computer that doesn't answer is left out too.
        let own = match &resolved {
            Some((_, txt)) => local && txt.contains_key("printer-type"),
            None => local,
        };
        if own {
            self.announced.remove(&key);
            if self.ignored.len() < MAX_PRINTERS {
                self.ignored.insert(key);
            }
            return;
        }
        // The secure announcement, else any.
        let Some((_, _, kind, domain)) = announcements
            .iter()
            .find(|(_, _, kind, _)| kind == "_ipps._tcp")
            .or_else(|| announcements.iter().next())
            .cloned()
        else {
            return;
        };
        let Some(uri) = service_uri(&key, &kind, &domain) else {
            return;
        };
        let (host, txt) = resolved.unwrap_or_default();
        let value = |key: &str| txt.get(key).map(|v| clean(v)).unwrap_or_default();
        let found = Found {
            key: key.clone(),
            name: clean(&key),
            model: value("ty"),
            location: value("note"),
            uri,
            host,
            resource: txt.get("rp").cloned().unwrap_or_default(),
            secure: kind == "_ipps._tcp",
            local,
        };
        self.pending.push_back(Discovered::Added(found.clone()));
        self.found.insert(key, found);
    }

    fn item_remove(&mut self, announcement: &Announcement, name: &str) {
        let Some((announcements, _)) = self.announced.get_mut(name) else {
            return;
        };
        announcements.remove(announcement);
        if announcements.is_empty() {
            self.announced.remove(name);
            if self.found.remove(name).is_some() {
                self.pending.push_back(Discovered::Removed(name.to_owned()));
            }
        }
    }
}

/// What the discovery waits for.
enum Event {
    /// A service was resolved, or not.
    Resolved(String, Option<(String, HashMap<String, String>)>),
    /// A signal of Avahi.
    Message(zbus::Message),
    /// Avahi's signals ended.
    Gone,
}

/// The host name and the TXT record of a service, keys in lowercase; `None` if Avahi can't
/// resolve it in time.
async fn resolve(
    connection: &Connection,
    interface: i32,
    protocol: i32,
    name: &str,
    kind: &str,
    domain: &str,
) -> Option<(String, HashMap<String, String>)> {
    let args = (interface, protocol, name, kind, domain, UNSPEC, NO_ADDRESS);
    let call = connection.call_method(Some(SERVICE), "/", Some(SERVER), "ResolveService", &args);
    let reply = future::or(async { call.await.ok() }, async {
        Timer::after(RESOLVE_TIMEOUT).await;
        None
    })
    .await?;
    let resolved: ResolveReply = reply.body().deserialize().ok()?;
    Some((clean(&resolved.5), parse_txt(&resolved.9)))
}

/// What `ResolveService` answers: interface, protocol, name, type, domain, host, address
/// protocol, address, port, TXT record and flags.
type ResolveReply = (
    i32,
    i32,
    String,
    String,
    String,
    String,
    i32,
    String,
    u16,
    Vec<Vec<u8>>,
    u32,
);

/// The key-value pairs of a TXT record, keys in lowercase.
fn parse_txt(records: &[Vec<u8>]) -> HashMap<String, String> {
    records
        .iter()
        .filter_map(|record| {
            let record = String::from_utf8_lossy(record);
            let (key, value) = record.split_once('=').unwrap_or((&record, ""));
            (!key.is_empty()).then(|| (key.to_ascii_lowercase(), value.to_owned()))
        })
        .collect()
}

/// The device URI of the service `name` of the type `kind` in `domain`, which CUPS resolves
/// through Avahi itself (`ipps://Office%20Printer._ipps._tcp.local/`).
///
/// `None` for a domain that isn't made of `A-Z`, `a-z`, `0-9`, `.` and `-`.
fn service_uri(name: &str, kind: &str, domain: &str) -> Option<String> {
    let scheme = if kind == "_ipps._tcp" { "ipps" } else { "ipp" };
    let domain = if domain.is_empty() { "local" } else { domain };
    let valid = domain
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-');
    valid.then(|| format!("{scheme}://{}.{kind}.{domain}/", percent_encode(name)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_uris_hold_the_encoded_name() {
        assert_eq!(
            service_uri("HP LaserJet [1A2B3C]", "_ipps._tcp", "local").as_deref(),
            Some("ipps://HP%20LaserJet%20%5B1A2B3C%5D._ipps._tcp.local/")
        );
        assert_eq!(
            service_uri("a/b", "_ipp._tcp", "").as_deref(),
            Some("ipp://a%2Fb._ipp._tcp.local/")
        );
        assert_eq!(
            service_uri("a", "_ipp._tcp", "office.example-1.org").as_deref(),
            Some("ipp://a._ipp._tcp.office.example-1.org/")
        );
        assert_eq!(service_uri("a", "_ipp._tcp", "evil/../x"), None);
        assert_eq!(service_uri("a", "_ipp._tcp", "a b"), None);
    }

    #[test]
    fn a_printer_is_found_at_its_service_or_its_resolved_address() {
        let found = Found {
            key: "Acme".to_owned(),
            name: "Acme".to_owned(),
            model: String::new(),
            location: String::new(),
            uri: "ipps://Acme._ipps._tcp.local/".to_owned(),
            host: "acme.local".to_owned(),
            resource: "ipp/print".to_owned(),
            secure: true,
            local: false,
        };
        assert!(found.is_at("ipps://Acme._ipps._tcp.local/"));
        assert!(found.is_at("ipps://acme.local.:443/ipp/print"));
        assert!(found.is_at("ipp://ACME.local/ipp/print"));
        assert!(!found.is_at("ipps://acme.local:443/ipp/print/2"));
        assert!(!found.is_at("ipps://other.local:443/ipp/print"));
    }

    #[test]
    fn txt_records_are_read() {
        let txt = parse_txt(&[
            b"txtvers=1".to_vec(),
            b"TY=Acme Laser 9000".to_vec(),
            b"note=".to_vec(),
            b"=x".to_vec(),
            b"air".to_vec(),
        ]);
        assert_eq!(txt.get("ty").map(String::as_str), Some("Acme Laser 9000"));
        assert_eq!(txt.get("note").map(String::as_str), Some(""));
        assert_eq!(txt.get("air").map(String::as_str), Some(""));
        assert_eq!(txt.len(), 4);
    }
}
