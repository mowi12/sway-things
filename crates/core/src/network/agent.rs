// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The secret agent: the D-Bus object (`org.freedesktop.NetworkManager.SecretAgent`)
//! NetworkManager asks for a Wi-Fi password it doesn't have or that turned out wrong, and for the
//! secrets of a VPN. A Wi-Fi question becomes a [`SecretPrompt`] on a channel, which the page
//! shows as a dialog and answers. For a VPN it runs the plugin's auth dialog (see [`vpn`]); in
//! external UI mode the fields it describes become a [`VpnPrompt`].
//!
//! The page registers it only while it is open. It answers only for WPA and WPA3 Personal
//! passwords, only when NetworkManager allows a question, and for VPNs whose plugin has an auth
//! dialog: everything else (802.1X, WireGuard) it leaves to other agents, such as nm-applet's. It
//! keeps no secrets: those it gets are kept as their flags tell NetworkManager, or asked for
//! again. It only answers calls from NetworkManager itself: another program on the system bus
//! can't make it show dialogs or run programs.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError, RwLock};

use async_channel::{Receiver, Sender};
use zbus::Connection;
use zbus::message::Header;
use zbus::zvariant::OwnedValue;

use super::nm::{SERVICE, value_bytes};
use super::vpn::{self, Failure, Outcome, Plugin, Request, VpnField};
use super::{clean, ssid_text};

/// Where NetworkManager looks for an agent on its connection: a fixed path.
pub(super) const PATH: &str = "/org/freedesktop/NetworkManager/SecretAgent";
/// The name the agent registers with.
pub(super) const IDENTIFIER: &str = "io.github.mowi12.SwayThings";

/// The setting with a Wi-Fi network's password.
const WIRELESS_SECURITY: &str = "802-11-wireless-security";
/// The setting with a VPN's secrets.
const VPN: &str = "vpn";
/// `NM_SECRET_AGENT_CAPABILITY_VPN_HINTS`: the agent passes NetworkManager's hints on to the VPN
/// plugins' auth dialogs.
pub(super) const VPN_HINTS: u32 = 0x1;
/// `NM_SECRET_AGENT_GET_SECRETS_FLAG_ALLOW_INTERACTION`: the agent may ask the user.
const ALLOW_INTERACTION: u32 = 0x1;
/// `NM_SECRET_AGENT_GET_SECRETS_FLAG_REQUEST_NEW`: the password NetworkManager has failed.
const REQUEST_NEW: u32 = 0x2;

/// The settings of a connection, as NetworkManager sends them.
pub(super) type Settings = HashMap<String, HashMap<String, OwnedValue>>;

/// A question from NetworkManager: the password of a Wi-Fi network.
#[derive(Debug)]
pub struct SecretPrompt {
    /// The path of the saved connection it is for.
    pub connection: String,
    /// The network's name (its SSID as text, else the connection's name). Untrusted: show it as
    /// plain text only.
    pub network: String,
    /// The SSID, to tell apart networks with the same name.
    pub ssid: Vec<u8>,
    /// Whether the password NetworkManager had was wrong (or there was none to try).
    pub retry: bool,
    /// Whether it is a WPA3 Personal (SAE) network, which takes passwords of any length.
    pub wpa3: bool,
    /// Where the answer goes.
    reply: Sender<String>,
}

impl SecretPrompt {
    /// Answers with `password`. Dropping the prompt cancels the question, and NetworkManager
    /// gives up connecting.
    pub fn answer(self, password: String) {
        // NetworkManager may have stopped waiting; then nobody does.
        let _ = self.reply.try_send(password);
    }
}

/// A question from NetworkManager in a VPN plugin's words: the fields its auth dialog asks the
/// agent to show (external UI mode).
#[derive(Debug)]
pub struct VpnPrompt {
    /// The path of the saved connection it is for.
    pub connection: String,
    /// The connection's name. Untrusted: show it as plain text only.
    pub name: String,
    /// What the plugin tells the user, plain text; may be empty.
    pub description: String,
    /// Whether the secrets NetworkManager had failed.
    pub retry: bool,
    /// The fields to fill in, in the plugin's order.
    pub fields: Vec<VpnField>,
    /// Where the answer goes.
    reply: Sender<Vec<String>>,
}

impl VpnPrompt {
    /// Answers with the `values` of the [`fields`](Self::fields), in their order. Dropping the
    /// prompt cancels the question, and NetworkManager gives up connecting.
    pub fn answer(self, values: Vec<String>) {
        // NetworkManager may have stopped waiting; then nobody does.
        let _ = self.reply.try_send(values);
    }
}

/// What the agent tells the page.
#[derive(Debug)]
pub enum AgentEvent {
    /// A password to ask for.
    Prompt(SecretPrompt),
    /// A VPN's fields to ask for.
    Vpn(VpnPrompt),
    /// NetworkManager stopped waiting for the password of the connection at this path (it
    /// timed out or the connection was given up): close its dialog.
    Cancel(String),
}

/// A new channel for the agent's events.
pub(super) fn channel() -> (Sender<AgentEvent>, Receiver<AgentEvent>) {
    async_channel::unbounded()
}

/// The errors the agent answers NetworkManager with.
#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.freedesktop.NetworkManager.SecretAgent")]
enum AgentError {
    /// A zbus error.
    #[zbus(error)]
    ZBus(zbus::Error),
    /// The call doesn't come from NetworkManager.
    PermissionDenied(String),
    /// The user canceled the dialog, or the page went away.
    UserCanceled(String),
    /// Not a question this agent answers: another agent may.
    NoSecrets(String),
}

/// What the agent and the [`NetworkManager`](super::NetworkManager) handle share about VPNs.
#[derive(Debug, Default)]
pub(super) struct VpnState {
    /// Where the plugins' `.name` files are; `None` for where libnm looks.
    dirs: RwLock<Option<Vec<PathBuf>>>,
    /// The connections (by path) whose secrets were asked for while their plugin has no auth
    /// dialog, with the plugin's name.
    missing: Mutex<HashMap<String, String>>,
}

impl VpnState {
    /// Looks for the plugins in `dirs` instead.
    pub(super) fn use_dirs(&self, dirs: Vec<PathBuf>) {
        *self.dirs.write().unwrap_or_else(PoisonError::into_inner) = Some(dirs);
    }

    fn dirs(&self) -> Vec<PathBuf> {
        self.dirs
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .unwrap_or_else(vpn::plugin_dirs)
    }

    /// The name of the plugin without an auth dialog, if the connection at `path` needed one
    /// since the last call; forgets it.
    pub(super) fn take_missing(&self, path: &str) -> Option<String> {
        lock(&self.missing).remove(path)
    }

    fn note_missing(&self, path: &str, plugin: String) {
        lock(&self.missing).insert(path.to_owned(), plugin);
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The D-Bus object.
pub(super) struct Agent {
    /// Where the prompts go.
    pub(super) events: Sender<AgentEvent>,
    /// What it shares with the handle about VPNs.
    pub(super) vpn: std::sync::Arc<VpnState>,
    /// The VPN questions under way, by connection path: a message stops one.
    pub(super) running: Mutex<HashMap<String, Sender<()>>>,
}

/// What the agent answers NetworkManager with.
type Reply = HashMap<String, HashMap<String, OwnedValue>>;

impl Agent {
    /// The secrets of the VPN in `settings` at `path`, from its plugin's auth dialog.
    async fn vpn_secrets(
        &self,
        settings: &Settings,
        path: &str,
        hints: Vec<String>,
        flags: u32,
    ) -> Result<Reply, AgentError> {
        let interactive = flags & ALLOW_INTERACTION != 0;
        let Some(request) = Request::of(settings, hints, interactive, flags & REQUEST_NEW != 0)
        else {
            return Err(no_secrets());
        };
        let plugin = vpn::find_plugin(&self.vpn.dirs(), &request.service);
        let Some((program, plugin)) = plugin
            .as_ref()
            .and_then(|plugin| plugin.auth_dialog.clone().map(|program| (program, plugin)))
        else {
            let name = plugin.map_or_else(|| vpn::short_name(&request.service), |p| p.name);
            self.vpn.note_missing(path, name);
            return Err(no_secrets());
        };
        self.vpn.take_missing(path);
        let (stop, stopped) = async_channel::bounded::<()>(1);
        lock(&self.running).insert(path.to_owned(), stop.clone());
        let ask = self.ask_vpn(&program, plugin, &request, path);
        let canceled = async {
            let _ = stopped.recv().await;
            Err(canceled())
        };
        let secrets = futures_lite::future::or(ask, canceled).await;
        {
            let mut running = lock(&self.running);
            if running.get(path).is_some_and(|s| s.same_channel(&stop)) {
                running.remove(path);
            }
        }
        let secrets: HashMap<String, String> = secrets?.into_iter().collect();
        let secrets = OwnedValue::try_from(zbus::zvariant::Value::from(secrets))
            .map_err(|error| AgentError::ZBus(error.into()))?;
        Ok(HashMap::from([(
            VPN.to_owned(),
            HashMap::from([("secrets".to_owned(), secrets)]),
        )]))
    }

    /// Runs the auth dialog `program` of `plugin` for `request`, and asks the page for the fields
    /// it describes in external UI mode.
    async fn ask_vpn(
        &self,
        program: &Path,
        plugin: &Plugin,
        request: &Request,
        path: &str,
    ) -> Result<Vec<(String, String)>, AgentError> {
        let form = match vpn::run(program, plugin, request).await {
            Ok(Outcome::Secrets(secrets)) => return Ok(secrets),
            Ok(Outcome::Ask(form)) => form,
            Err(Failure::Canceled) => return Err(canceled()),
            Err(Failure::Start | Failure::Garbled) => return Err(no_secrets()),
        };
        let mut secrets = form.known;
        if form.fields.is_empty() {
            return Ok(secrets);
        }
        if !request.interactive {
            return Err(no_secrets());
        }
        let (reply, answer) = async_channel::bounded(1);
        let mut name = clean(&request.name);
        if name.is_empty() {
            name.clone_from(&plugin.name);
        }
        let keys: Vec<String> = form.fields.iter().map(|field| field.key.clone()).collect();
        let prompt = VpnPrompt {
            connection: path.to_owned(),
            name,
            description: form.description,
            retry: request.renew,
            fields: form.fields,
            reply,
        };
        self.events
            .send(AgentEvent::Vpn(prompt))
            .await
            .map_err(|_| canceled())?;
        let values = answer.recv().await.map_err(|_| canceled())?;
        secrets.extend(keys.into_iter().zip(values));
        Ok(secrets)
    }

    /// Stops the VPN question about the connection at `path`, if one is under way.
    fn stop_vpn(&self, path: &str) {
        if let Some(stop) = lock(&self.running).remove(path) {
            let _ = stop.try_send(());
        }
    }
}

/// What the agent asks for, read from the connection's settings: `None` if it doesn't ask.
#[derive(Debug, PartialEq, Eq)]
struct Question {
    network: String,
    ssid: Vec<u8>,
    wpa3: bool,
}

/// The question about `settings` for the secrets of `setting`, or `None` if the agent leaves
/// it to others.
fn question(settings: &Settings, setting: &str, flags: u32) -> Option<Question> {
    if setting != WIRELESS_SECURITY || flags & ALLOW_INTERACTION == 0 {
        return None;
    }
    let key_mgmt = settings
        .get(WIRELESS_SECURITY)?
        .get("key-mgmt")
        .and_then(|value| <&str>::try_from(value).ok())?;
    let wpa3 = match key_mgmt {
        "wpa-psk" => false,
        "sae" => true,
        _ => return None,
    };
    let ssid: Vec<u8> = settings
        .get("802-11-wireless")
        .and_then(|wireless| wireless.get("ssid"))
        .and_then(|value| value_bytes(value))
        .unwrap_or_default();
    let mut network = ssid_text(&ssid);
    if network.is_empty() {
        network = settings
            .get("connection")
            .and_then(|connection| connection.get("id"))
            .and_then(|value| <&str>::try_from(value).ok())
            .map(super::clean)
            .unwrap_or_default();
    }
    Some(Question {
        network,
        ssid,
        wpa3,
    })
}

/// The D-Bus interface, apart: zbus's macro reads every argument, and clippy counts that as
/// using the ones the agent ignores.
#[allow(
    clippy::used_underscore_binding,
    reason = "zbus's interface macro reads every argument, also those the agent ignores"
)]
mod dbus {
    use std::collections::HashMap;

    use zbus::message::Header;
    use zbus::zvariant::{ObjectPath, OwnedValue};
    use zbus::{Connection, interface};

    use super::{
        Agent, AgentError, AgentEvent, REQUEST_NEW, SecretPrompt, Settings, VPN, WIRELESS_SECURITY,
        canceled, from_nm, no_secrets, question,
    };

    #[interface(name = "org.freedesktop.NetworkManager.SecretAgent")]
    impl Agent {
        #[allow(
            clippy::too_many_arguments,
            reason = "the five arguments of the D-Bus method, plus what zbus passes"
        )]
        async fn get_secrets(
            &self,
            connection: Settings,
            connection_path: ObjectPath<'_>,
            setting_name: String,
            hints: Vec<String>,
            flags: u32,
            #[zbus(header)] header: Header<'_>,
            #[zbus(connection)] bus: &Connection,
        ) -> Result<HashMap<String, HashMap<String, OwnedValue>>, AgentError> {
            from_nm(&header, bus).await?;
            if setting_name == VPN {
                return self
                    .vpn_secrets(&connection, connection_path.as_str(), hints, flags)
                    .await;
            }
            let Some(question) = question(&connection, &setting_name, flags) else {
                return Err(no_secrets());
            };
            let (reply, answer) = async_channel::bounded(1);
            let prompt = SecretPrompt {
                connection: connection_path.to_string(),
                network: question.network,
                ssid: question.ssid,
                retry: flags & REQUEST_NEW != 0,
                wpa3: question.wpa3,
                reply,
            };
            self.events
                .send(AgentEvent::Prompt(prompt))
                .await
                .map_err(|_| canceled())?;
            let password = answer.recv().await.map_err(|_| canceled())?;
            let psk = OwnedValue::try_from(zbus::zvariant::Value::from(password))
                .map_err(|error| AgentError::ZBus(error.into()))?;
            Ok(HashMap::from([(
                WIRELESS_SECURITY.to_owned(),
                HashMap::from([("psk".to_owned(), psk)]),
            )]))
        }

        async fn cancel_get_secrets(
            &self,
            connection_path: ObjectPath<'_>,
            _setting_name: String,
            #[zbus(header)] header: Header<'_>,
            #[zbus(connection)] bus: &Connection,
        ) -> Result<(), AgentError> {
            from_nm(&header, bus).await?;
            self.stop_vpn(connection_path.as_str());
            let _ = self
                .events
                .send(AgentEvent::Cancel(connection_path.to_string()))
                .await;
            Ok(())
        }

        /// The Wi-Fi passwords the page gets are kept by NetworkManager (`psk-flags=0`); a VPN's
        /// secrets that its flags leave to the agent aren't kept: they are asked for again.
        async fn save_secrets(
            &self,
            _connection: Settings,
            _connection_path: ObjectPath<'_>,
            #[zbus(header)] header: Header<'_>,
            #[zbus(connection)] bus: &Connection,
        ) -> Result<(), AgentError> {
            from_nm(&header, bus).await
        }

        /// The agent keeps no passwords: nothing to delete.
        async fn delete_secrets(
            &self,
            _connection: Settings,
            _connection_path: ObjectPath<'_>,
            #[zbus(header)] header: Header<'_>,
            #[zbus(connection)] bus: &Connection,
        ) -> Result<(), AgentError> {
            from_nm(&header, bus).await
        }
    }
}

/// Fails unless the call comes from the program that owns NetworkManager's name.
async fn from_nm(header: &Header<'_>, connection: &Connection) -> Result<(), AgentError> {
    let sender = header.sender().ok_or_else(denied)?;
    let owner: String = connection
        .call_method(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            Some("org.freedesktop.DBus"),
            "GetNameOwner",
            &(SERVICE,),
        )
        .await
        .map_err(|_| denied())?
        .body()
        .deserialize()
        .map_err(|_| denied())?;
    if sender.as_str() == owner {
        Ok(())
    } else {
        Err(denied())
    }
}

fn denied() -> AgentError {
    AgentError::PermissionDenied("Only NetworkManager may ask".to_owned())
}

fn no_secrets() -> AgentError {
    AgentError::NoSecrets("No secrets".to_owned())
}

fn canceled() -> AgentError {
    AgentError::UserCanceled("Canceled".to_owned())
}

#[cfg(test)]
mod tests {
    use zbus::zvariant::Value;

    use super::*;

    fn settings(key_mgmt: &str, ssid: &[u8], id: &str) -> Settings {
        let value = |v: Value<'_>| OwnedValue::try_from(v).unwrap();
        HashMap::from([
            (
                "connection".to_owned(),
                HashMap::from([("id".to_owned(), value(Value::from(id)))]),
            ),
            (
                "802-11-wireless".to_owned(),
                HashMap::from([("ssid".to_owned(), value(Value::from(ssid.to_vec())))]),
            ),
            (
                WIRELESS_SECURITY.to_owned(),
                HashMap::from([("key-mgmt".to_owned(), value(Value::from(key_mgmt)))]),
            ),
        ])
    }

    #[test]
    fn only_personal_passwords_are_asked_for() {
        let home = settings("wpa-psk", b"Home\n", "Home 1");
        assert_eq!(
            question(&home, WIRELESS_SECURITY, ALLOW_INTERACTION),
            Some(Question {
                network: "Home".to_owned(),
                ssid: b"Home\n".to_vec(),
                wpa3: false
            })
        );
        let sae = settings("sae", b"", "Hidden one");
        assert_eq!(
            question(&sae, WIRELESS_SECURITY, ALLOW_INTERACTION | REQUEST_NEW)
                .map(|q| (q.network, q.wpa3)),
            Some(("Hidden one".to_owned(), true))
        );
        // Not allowed to ask, another setting, another kind of network.
        assert_eq!(question(&home, WIRELESS_SECURITY, 0), None);
        assert_eq!(question(&home, "802-1x", ALLOW_INTERACTION), None);
        let eap = settings("wpa-eap", b"Work", "Work");
        assert_eq!(question(&eap, WIRELESS_SECURITY, ALLOW_INTERACTION), None);
    }

    #[test]
    fn a_dropped_prompt_cancels_and_an_answer_arrives() {
        let (reply, answer) = async_channel::bounded(1);
        let prompt = SecretPrompt {
            connection: String::new(),
            network: String::new(),
            ssid: Vec::new(),
            retry: false,
            wpa3: false,
            reply,
        };
        prompt.answer("secret".to_owned());
        assert_eq!(answer.try_recv(), Ok("secret".to_owned()));

        let (reply, answer) = async_channel::bounded::<String>(1);
        drop(SecretPrompt {
            connection: String::new(),
            network: String::new(),
            ssid: Vec::new(),
            retry: true,
            wpa3: false,
            reply,
        });
        assert!(answer.is_closed());
    }
}
