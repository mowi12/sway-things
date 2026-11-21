// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The auth dialogs of the VPN plugins, which the secret agent runs for a VPN's secrets as
//! nm-applet and GNOME Shell do.
//!
//! A plugin describes itself in a `.name` file (a key file): its D-Bus service, and in the
//! `[GNOME]` group its `auth-dialog` program and whether that supports `--external-ui-mode`
//! (it then describes the fields to ask for, and we ask) and hints. The files are looked up as
//! libnm does: in `NM_VPN_PLUGIN_DIR`, `/usr/lib/NetworkManager/VPN` and
//! `/etc/NetworkManager/VPN`, only files owned by root or the user that nobody else may write.
//!
//! The dialog is started with an argument array, never a shell. It reads the VPN's data and
//! secrets from stdin (`DATA_KEY=`/`DATA_VAL=`, `SECRET_KEY=`/`SECRET_VAL=`, then `DONE`) and
//! answers on stdout: a key line and a value line per secret, ended by an empty line, after which
//! it waits for `QUIT`; or in external UI mode a key file with the fields, and exits.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Stdio;

use async_process::{Child, ChildStdout, Command};
use futures_lite::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use zbus::zvariant::OwnedValue;

use super::agent::Settings;
use super::clean;

/// Where the plugins' `.name` files are, after the directory in `NM_VPN_PLUGIN_DIR`.
const PLUGIN_DIRS: [&str; 2] = ["/usr/lib/NetworkManager/VPN", "/etc/NetworkManager/VPN"];
/// Where an auth dialog named without a directory is.
const LIBEXEC: &str = "/usr/libexec";
/// The group of the `.name` file that describes the plugin.
const CONNECTION_GROUP: &str = "VPN Connection";
/// The group of the `.name` file with the auth dialog.
const GNOME_GROUP: &str = "GNOME";
/// The group of an external UI key file that describes the dialog.
const UI_GROUP: &str = "VPN Plugin UI";
/// The most an auth dialog may write in external UI mode, and the most secrets it may answer.
const MAX_OUTPUT: u64 = 64 * 1024;
const MAX_SECRETS: usize = 256;

/// The directories with the plugins' `.name` files, in the order libnm reads them.
pub(super) fn plugin_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var_os("NM_VPN_PLUGIN_DIR")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .into_iter()
        .collect();
    dirs.extend(PLUGIN_DIRS.iter().map(PathBuf::from));
    dirs
}

/// A VPN plugin, from its `.name` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Plugin {
    /// Its short name (`openvpn`).
    pub(super) name: String,
    /// Its auth dialog, if it has one that can be run.
    pub(super) auth_dialog: Option<PathBuf>,
    /// Whether the auth dialog takes `--external-ui-mode`.
    pub(super) external_ui: bool,
    /// Whether the auth dialog takes hints (`-t`).
    pub(super) hints: bool,
}

/// The plugin for the VPN `service` (`org.freedesktop.NetworkManager.openvpn`), from the first of
/// `dirs` that has one.
pub(super) fn find_plugin(dirs: &[PathBuf], service: &str) -> Option<Plugin> {
    let uid = rustix::process::getuid().as_raw();
    dirs.iter().find_map(|dir| {
        let mut files: Vec<PathBuf> = fs::read_dir(dir)
            .ok()?
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| is_name_file(path, uid))
            .collect();
        files.sort();
        files.iter().find_map(|path| {
            let text = fs::read_to_string(path).ok()?;
            plugin_of(&parse_key_file(&text), service)
        })
    })
}

/// Whether `path` is a `.name` file libnm would read for the user `uid`: a regular file, owned by
/// root or `uid`, that neither its group nor others may write, and not setuid.
fn is_name_file(path: &Path, uid: u32) -> bool {
    let named = path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| !name.starts_with('.'))
        && path
            .extension()
            .is_some_and(|extension| extension == "name");
    named
        && fs::metadata(path).is_ok_and(|meta| {
            meta.is_file()
                && (meta.uid() == 0 || meta.uid() == uid)
                && meta.permissions().mode() & 0o4022 == 0
        })
}

/// The plugin a `.name` file describes, if it is the one for `service`.
fn plugin_of(file: &KeyFile, service: &str) -> Option<Plugin> {
    let connection = group(file, CONNECTION_GROUP)?;
    let serves = value(connection, "service") == Some(service)
        || value(connection, "aliases")
            .is_some_and(|aliases| aliases.split(';').any(|alias| alias.trim() == service));
    if !serves {
        return None;
    }
    let gnome = group(file, GNOME_GROUP);
    let flag = |key| gnome.and_then(|g| value(g, key)).is_some_and(name_boolean);
    // A program without an absolute path is the file of its name in libexec.
    let auth_dialog = gnome
        .and_then(|g| value(g, "auth-dialog"))
        .map(Path::new)
        .and_then(|program| {
            if program.is_absolute() {
                Some(program.to_owned())
            } else {
                program
                    .file_name()
                    .map(|name| Path::new(LIBEXEC).join(name))
            }
        })
        .filter(|program| is_executable(program));
    Some(Plugin {
        name: value(connection, "name")
            .map(clean)
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| short_name(service)),
        auth_dialog,
        external_ui: flag("supports-external-ui-mode"),
        hints: flag("supports-hints"),
    })
}

/// The last part of a VPN service's name (`openvpn`), for a plugin without a `.name` file.
pub(super) fn short_name(service: &str) -> String {
    clean(service.rsplit('.').next().unwrap_or(service))
}

fn is_executable(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

/// What the agent was asked for a VPN, read from its settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Request {
    /// The connection's UUID, name and VPN service.
    pub(super) uuid: String,
    pub(super) name: String,
    pub(super) service: String,
    /// The VPN's data and the secrets NetworkManager has, sorted by key.
    pub(super) data: Vec<(String, String)>,
    pub(super) secrets: Vec<(String, String)>,
    /// NetworkManager's hints: the secrets it needs, or a message for the user.
    pub(super) hints: Vec<String>,
    /// Whether the dialog may ask the user.
    pub(super) interactive: bool,
    /// Whether the secrets NetworkManager had failed.
    pub(super) renew: bool,
}

impl Request {
    /// The request for the VPN in `settings`; `None` if they aren't a VPN's (WireGuard keeps its
    /// keys in the connection and isn't one).
    pub(super) fn of(
        settings: &Settings,
        hints: Vec<String>,
        interactive: bool,
        renew: bool,
    ) -> Option<Self> {
        let connection = settings.get("connection")?;
        let text = |value: Option<&OwnedValue>| {
            value
                .and_then(|value| <&str>::try_from(value).ok())
                .map(ToOwned::to_owned)
        };
        if text(connection.get("type")).as_deref() != Some("vpn") {
            return None;
        }
        let vpn = settings.get("vpn")?;
        let service = text(vpn.get("service-type")).filter(|s| !s.is_empty())?;
        let pairs = |key: &str| {
            let mut pairs: Vec<(String, String)> = vpn
                .get(key)
                .and_then(|value| value.try_clone().ok())
                .and_then(|value| HashMap::<String, String>::try_from(value).ok())
                .unwrap_or_default()
                .into_iter()
                .collect();
            pairs.sort();
            pairs
        };
        Some(Self {
            uuid: text(connection.get("uuid")).unwrap_or_default(),
            name: text(connection.get("id")).unwrap_or_default(),
            service,
            data: pairs("data"),
            secrets: pairs("secrets"),
            hints,
            interactive,
            renew,
        })
    }

    /// The auth dialog's arguments, as nm-applet and GNOME Shell pass them.
    pub(super) fn arguments(&self, plugin: &Plugin) -> Vec<String> {
        let mut args = vec![
            "-u".to_owned(),
            self.uuid.clone(),
            "-n".to_owned(),
            self.name.clone(),
            "-s".to_owned(),
            self.service.clone(),
        ];
        if plugin.external_ui {
            args.push("--external-ui-mode".to_owned());
        }
        if self.interactive {
            args.push("-i".to_owned());
        }
        if self.renew {
            args.push("-r".to_owned());
        }
        if plugin.hints {
            for hint in &self.hints {
                args.push("-t".to_owned());
                args.push(hint.clone());
            }
        }
        args
    }

    /// What the auth dialog reads on stdin. A key or value with a line break would break the
    /// line protocol: it is left out.
    pub(super) fn input(&self) -> String {
        let mut input = String::new();
        let fits = |(key, value): &&(String, String)| !key.contains('\n') && !value.contains('\n');
        for (key, value) in self.data.iter().filter(fits) {
            let _ = write!(input, "DATA_KEY={key}\nDATA_VAL={value}\n\n");
        }
        for (key, value) in self.secrets.iter().filter(fits) {
            let _ = write!(input, "SECRET_KEY={key}\nSECRET_VAL={value}\n\n");
        }
        input.push_str("DONE\n\n");
        input
    }
}

/// A field the agent asks for in external UI mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VpnField {
    /// The secret's key in the VPN's settings.
    pub key: String,
    /// What the plugin calls it (`Password`), as plain text.
    pub label: String,
    /// What it holds now.
    pub value: String,
    /// Whether it is a secret, to type in hidden.
    pub secret: bool,
}

/// What an auth dialog in external UI mode asks for.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(super) struct Form {
    /// The plugin's text for the user, plain text.
    pub(super) description: String,
    /// The fields to ask for, in the plugin's order.
    pub(super) fields: Vec<VpnField>,
    /// The secrets it has without asking.
    pub(super) known: Vec<(String, String)>,
}

/// What came of running an auth dialog.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Outcome {
    /// The secrets.
    Secrets(Vec<(String, String)>),
    /// The fields to ask for (external UI mode).
    Ask(Form),
}

/// Why running an auth dialog gave nothing.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Failure {
    /// It couldn't be started.
    Start,
    /// The user canceled it, or it failed.
    Canceled,
    /// It answered in a way we don't understand.
    Garbled,
}

/// Runs `plugin`'s auth dialog at `program` for `request`. Dropping the future ends the dialog.
pub(super) async fn run(
    program: &Path,
    plugin: &Plugin,
    request: &Request,
) -> Result<Outcome, Failure> {
    let child = Command::new(program)
        .args(request.arguments(plugin))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|_| Failure::Start)?;
    let mut child = Running(Some(child));
    let (mut stdin, stdout) = child.pipes().ok_or(Failure::Start)?;
    stdin
        .write_all(request.input().as_bytes())
        .await
        .map_err(|_| Failure::Canceled)?;
    stdin.flush().await.map_err(|_| Failure::Canceled)?;
    if plugin.external_ui {
        let mut output = String::new();
        stdout
            .take(MAX_OUTPUT)
            .read_to_string(&mut output)
            .await
            .map_err(|_| Failure::Garbled)?;
        drop(stdin);
        let status = child.status().await?;
        if !status.success() {
            return Err(Failure::Canceled);
        }
        // Nothing to ask: the plugin keeps the secrets itself.
        if output.trim().is_empty() {
            return Ok(Outcome::Secrets(Vec::new()));
        }
        return form(&parse_key_file(&output)).map(Outcome::Ask);
    }
    let secrets = read_secrets(stdout).await?;
    // The dialog waits for this before it exits by itself.
    let _ = stdin.write_all(b"QUIT\n\n").await;
    let _ = stdin.flush().await;
    child.release();
    Ok(Outcome::Secrets(secrets))
}

/// A running auth dialog, ended when it is dropped unless it was released: the question was
/// given up.
struct Running(Option<Child>);

impl Running {
    fn pipes(&mut self) -> Option<(async_process::ChildStdin, ChildStdout)> {
        let child = self.0.as_mut()?;
        Some((child.stdin.take()?, child.stdout.take()?))
    }

    async fn status(&mut self) -> Result<std::process::ExitStatus, Failure> {
        let child = self.0.as_mut().ok_or(Failure::Canceled)?;
        let status = child.status().await.map_err(|_| Failure::Canceled)?;
        self.0 = None;
        Ok(status)
    }

    /// Lets the dialog end by itself; async-process reaps it.
    fn release(mut self) {
        self.0 = None;
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        if let Some(child) = self.0.as_mut() {
            let _ = child.kill();
        }
    }
}

/// Reads the secrets an auth dialog writes: a key line and a value line each, up to an empty
/// key line.
async fn read_secrets(stdout: ChildStdout) -> Result<Vec<(String, String)>, Failure> {
    let mut lines = BufReader::new(stdout.take(MAX_OUTPUT)).lines();
    let mut secrets = Vec::new();
    loop {
        // It ended before the empty line: canceled.
        let key = next_line(&mut lines)
            .await
            .map_err(|_| Failure::Garbled)?
            .ok_or(Failure::Canceled)?;
        if key.is_empty() {
            return Ok(secrets);
        }
        let value = next_line(&mut lines)
            .await
            .map_err(|_| Failure::Garbled)?
            .ok_or(Failure::Canceled)?;
        if secrets.len() == MAX_SECRETS {
            return Err(Failure::Garbled);
        }
        secrets.push((key, value));
    }
}

/// The next line of `lines`, without its line break; `None` at the end.
async fn next_line<R: futures_lite::AsyncBufRead + Unpin>(
    lines: &mut futures_lite::io::Lines<R>,
) -> std::io::Result<Option<String>> {
    futures_lite::StreamExt::next(lines).await.transpose()
}

/// The form an auth dialog describes in external UI mode (version 2, as GNOME Shell reads it).
fn form(file: &KeyFile) -> Result<Form, Failure> {
    let ui = group(file, UI_GROUP).ok_or(Failure::Garbled)?;
    if value(ui, "Version").map(str::trim) != Some("2") {
        return Err(Failure::Garbled);
    }
    let mut form = Form {
        description: value(ui, "Description").unwrap_or_default().to_owned(),
        ..Form::default()
    };
    for (name, entries) in file {
        if name == UI_GROUP {
            continue;
        }
        let flag = |key| value(entries, key).is_some_and(boolean);
        let current = value(entries, "Value").unwrap_or_default().to_owned();
        if flag("ShouldAsk") {
            let label = value(entries, "Label").unwrap_or(name);
            form.fields.push(VpnField {
                key: name.clone(),
                label: clean(label.trim_end().trim_end_matches(':')),
                value: current,
                secret: flag("IsSecret"),
            });
        } else if flag("IsSecret") && !current.is_empty() {
            form.known.push((name.clone(), current));
        }
    }
    Ok(form)
}

/// A key file: its groups in order, each with its keys in order.
type KeyFile = Vec<(String, Vec<(String, String)>)>;

/// Reads a key file as `GLib` writes it: `[group]` lines, `key=value` lines with `\s`, `\n`, `\t`,
/// `\r` and `\\` escaped in the value, `#` comments. Translated keys (`Label[de]`) are left out.
fn parse_key_file(text: &str) -> KeyFile {
    let mut file: KeyFile = Vec::new();
    for line in text.lines() {
        let line = line.trim_start();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line
            .strip_prefix('[')
            .and_then(|l| l.trim_end().strip_suffix(']'))
        {
            file.push((name.to_owned(), Vec::new()));
            continue;
        }
        let Some((key, raw)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim_end();
        if key.contains('[') {
            continue;
        }
        if let Some((_, entries)) = file.last_mut() {
            entries.push((key.to_owned(), unescape(raw.trim_start())));
        }
    }
    file
}

fn unescape(raw: &str) -> String {
    let mut text = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            text.push(c);
            continue;
        }
        match chars.next() {
            Some('s') => text.push(' '),
            Some('n') => text.push('\n'),
            Some('t') => text.push('\t'),
            Some('r') => text.push('\r'),
            Some(other) => text.push(other),
            None => {}
        }
    }
    text
}

fn group<'a>(file: &'a KeyFile, name: &str) -> Option<&'a [(String, String)]> {
    file.iter()
        .find(|(group, _)| group == name)
        .map(|(_, entries)| entries.as_slice())
}

fn value<'a>(entries: &'a [(String, String)], key: &str) -> Option<&'a str> {
    entries
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, value)| value.as_str())
}

/// A boolean of a `.name` file, as libnm and GNOME Shell read it.
fn name_boolean(value: &str) -> bool {
    ["true", "yes", "on", "1"]
        .iter()
        .any(|word| value.trim().eq_ignore_ascii_case(word))
}

/// A key file's boolean, as `GLib` reads it.
fn boolean(value: &str) -> bool {
    matches!(value.trim(), "true" | "1")
}

#[cfg(test)]
mod tests {
    use std::fs::File;
    use std::io::Write;

    use zbus::zvariant::Value;

    use super::*;

    const OPENVPN: &str = "\
[VPN Connection]
name=openvpn
service=org.freedesktop.NetworkManager.openvpn
program=/usr/libexec/nm-openvpn-service

[GNOME]
auth-dialog=nm-openvpn-auth-dialog
supports-external-ui-mode=Yes
supports-hints=1
";

    fn request() -> Request {
        Request {
            uuid: "1b2c".to_owned(),
            name: "-Office".to_owned(),
            service: "org.freedesktop.NetworkManager.openvpn".to_owned(),
            data: vec![
                ("remote".to_owned(), "vpn.example.org".to_owned()),
                ("evil".to_owned(), "x\nSECRET_KEY=y".to_owned()),
            ],
            secrets: vec![("password".to_owned(), String::new())],
            hints: vec!["x-vpn-message:Code?".to_owned()],
            interactive: true,
            renew: false,
        }
    }

    fn write(dir: &Path, name: &str, text: &str, mode: u32) -> PathBuf {
        let path = dir.join(name);
        let mut file = File::create(&path).unwrap();
        file.write_all(text.as_bytes()).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
        path
    }

    #[test]
    fn a_plugin_is_found_by_its_service_or_alias() {
        let dir = tempfile::tempdir().unwrap();
        let dialog = write(dir.path(), "auth-dialog", "", 0o755);
        let text = OPENVPN.replace("nm-openvpn-auth-dialog", dialog.to_str().unwrap());
        write(
            dir.path(),
            "nm-openvpn-service.name",
            &text.replace(
                "program=",
                "aliases=org.example.ovpn;org.example.other\nprogram=",
            ),
            0o644,
        );
        let dirs = [PathBuf::from("/nonexistent"), dir.path().to_owned()];
        let plugin = find_plugin(&dirs, "org.freedesktop.NetworkManager.openvpn").unwrap();
        assert_eq!(
            plugin,
            Plugin {
                name: "openvpn".to_owned(),
                auth_dialog: Some(dialog),
                external_ui: true,
                hints: true,
            }
        );
        assert_eq!(find_plugin(&dirs, "org.example.other"), Some(plugin));
        assert_eq!(
            find_plugin(&dirs, "org.freedesktop.NetworkManager.l2tp"),
            None
        );
    }

    #[test]
    fn a_plugin_without_its_dialog_has_none() {
        let dir = tempfile::tempdir().unwrap();
        let only_kde = "[VPN Connection]\nname=openconnect\n\
                        service=org.freedesktop.NetworkManager.openconnect\n";
        write(dir.path(), "openconnect.name", only_kde, 0o644);
        let missing = OPENVPN.replace("nm-openvpn-auth-dialog", "/nonexistent/auth-dialog");
        write(dir.path(), "openvpn.name", &missing, 0o644);
        let dirs = [dir.path().to_owned()];
        let plugin = find_plugin(&dirs, "org.freedesktop.NetworkManager.openconnect").unwrap();
        assert_eq!(
            (plugin.name.as_str(), plugin.auth_dialog),
            ("openconnect", None)
        );
        let plugin = find_plugin(&dirs, "org.freedesktop.NetworkManager.openvpn").unwrap();
        assert_eq!(plugin.auth_dialog, None);
        // A program without an absolute path is looked for in libexec only.
        let plugin = plugin_of(
            &parse_key_file(&OPENVPN.replace("nm-openvpn-auth-dialog", "../../bin/sh")),
            "org.freedesktop.NetworkManager.openvpn",
        )
        .unwrap();
        assert_eq!(plugin.auth_dialog, None);
    }

    #[test]
    fn name_files_others_may_write_are_ignored() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "openvpn.name", OPENVPN, 0o666);
        write(dir.path(), ".hidden.name", OPENVPN, 0o644);
        write(dir.path(), "openvpn.conf", OPENVPN, 0o644);
        write(dir.path(), "setuid.name", OPENVPN, 0o4644);
        let dirs = [dir.path().to_owned()];
        assert_eq!(
            find_plugin(&dirs, "org.freedesktop.NetworkManager.openvpn"),
            None
        );
    }

    #[test]
    fn the_dialog_gets_arguments_and_input_as_nm_applet_passes_them() {
        let mut plugin = Plugin {
            name: "openvpn".to_owned(),
            auth_dialog: None,
            external_ui: true,
            hints: true,
        };
        assert_eq!(
            request().arguments(&plugin),
            [
                "-u",
                "1b2c",
                "-n",
                "-Office",
                "-s",
                "org.freedesktop.NetworkManager.openvpn",
                "--external-ui-mode",
                "-i",
                "-t",
                "x-vpn-message:Code?"
            ]
        );
        plugin.external_ui = false;
        plugin.hints = false;
        let renew = Request {
            interactive: false,
            renew: true,
            ..request()
        };
        assert_eq!(renew.arguments(&plugin)[6..], ["-r"]);
        assert_eq!(
            request().input(),
            "DATA_KEY=remote\nDATA_VAL=vpn.example.org\n\n\
             SECRET_KEY=password\nSECRET_VAL=\n\nDONE\n\n"
        );
    }

    #[test]
    fn the_request_is_read_from_a_vpns_settings_only() {
        let value = |v: Value<'_>| OwnedValue::try_from(v).unwrap();
        let mut settings: Settings = HashMap::from([
            (
                "connection".to_owned(),
                HashMap::from([
                    ("id".to_owned(), value(Value::from("Office"))),
                    ("uuid".to_owned(), value(Value::from("1b2c"))),
                    ("type".to_owned(), value(Value::from("vpn"))),
                ]),
            ),
            (
                "vpn".to_owned(),
                HashMap::from([
                    (
                        "service-type".to_owned(),
                        value(Value::from("org.freedesktop.NetworkManager.openvpn")),
                    ),
                    (
                        "data".to_owned(),
                        value(Value::from(HashMap::from([("b", "2"), ("a", "1")]))),
                    ),
                ]),
            ),
        ]);
        let request = Request::of(&settings, Vec::new(), true, true).unwrap();
        assert_eq!(request.name, "Office");
        assert_eq!(
            request.data,
            [("a".into(), "1".into()), ("b".into(), "2".into())]
        );
        assert_eq!(request.secrets, []);
        if let Some(connection) = settings.get_mut("connection") {
            connection.insert("type".to_owned(), value(Value::from("wireguard")));
        }
        assert_eq!(Request::of(&settings, Vec::new(), true, false), None);
    }

    #[test]
    fn an_external_ui_key_file_becomes_a_form() {
        let text = "\
[VPN Plugin UI]
Version=2
Description=You need to authenticate to access the Virtual Private Network \u{201c}Office\u{201d}.
Title=Authenticate VPN

[username]
Value=alice
Label=Username:
IsSecret=false
ShouldAsk=true

[password]
Value=
Label=Password:
Label[de]=Passwort:
IsSecret=true
ShouldAsk=true

[cert-pass]
Value=s3cret\\swith space
IsSecret=true
ShouldAsk=false
";
        let form = form(&parse_key_file(text)).unwrap();
        assert!(form.description.starts_with("You need to authenticate"));
        assert_eq!(
            form.fields,
            [
                VpnField {
                    key: "username".to_owned(),
                    label: "Username".to_owned(),
                    value: "alice".to_owned(),
                    secret: false,
                },
                VpnField {
                    key: "password".to_owned(),
                    label: "Password".to_owned(),
                    value: String::new(),
                    secret: true,
                },
            ]
        );
        assert_eq!(
            form.known,
            [("cert-pass".into(), "s3cret with space".into())]
        );
        let old = text.replace("Version=2", "Version=1");
        assert_eq!(super::form(&parse_key_file(&old)), Err(Failure::Garbled));
    }

    #[test]
    fn key_files_unescape_values() {
        let file = parse_key_file("# note\n[a]\nk = \\sx\\ty\\\\z\\n\nbad line\n");
        assert_eq!(
            file,
            [(
                "a".to_owned(),
                vec![("k".to_owned(), " x\ty\\z\n".to_owned())]
            )]
        );
        assert_eq!(
            short_name("org.freedesktop.NetworkManager.fortisslvpn"),
            "fortisslvpn"
        );
    }
}
