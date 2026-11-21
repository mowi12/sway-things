// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Our own sign-in for OpenConnect VPNs: instead of the plugin's auth dialog the agent runs
//! `openconnect --authenticate` as nmcli does, shows each of its prompts in the page's sign-in
//! dialog, and answers NetworkManager with the `cookie`, `gateway`, `gwcert` and `resolve` it
//! prints.
//!
//! openconnect is started with an argument array, never a shell, the profile's settings as
//! `--option=value` arguments and the server after `--`, so no value becomes an option. It asks
//! one field at a time on stderr: the prompt (`Username:`, or `GROUP: [a|b]:` for a choice)
//! without a line break, then it reads a line from stdin. Its progress lines go to stderr too
//! and are told apart by their (untranslated) beginnings; what is left is the server's message,
//! shown with the next prompt. Whether a field is secret isn't in the output: a field is secret
//! unless its label names a user, account or group. A web sign-in opens the user's browser
//! (`--external-browser=xdg-open`). A server certificate that fails verification is refused,
//! never accepted by a prompt.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use async_channel::{Receiver, Sender};
use async_io::Timer;
use async_process::{Child, ChildStderr, Command};
use futures_lite::io::{AsyncReadExt, AsyncWriteExt};

use super::vpn::VpnField;

/// The VPN service of NetworkManager-openconnect.
pub(super) const SERVICE: &str = "org.freedesktop.NetworkManager.openconnect";
/// The secrets a sign-in gives NetworkManager.
pub(super) const SECRETS: [&str; 4] = ["cookie", "gateway", "gwcert", "resolve"];
/// Where nmcli looks for openconnect, in its order.
const PROGRAM_DIRS: [&str; 6] = [
    "/usr/sbin",
    "/sbin",
    "/usr/local/sbin",
    "/usr/bin",
    "/bin",
    "/usr/local/bin",
];
/// The profile's settings that become arguments, as nmcli passes them.
const ARGUMENTS: [(&str, &str); 11] = [
    ("usercert", "--certificate"),
    ("cacert", "--cafile"),
    ("userkey", "--sslkey"),
    ("key_pass", "--key-password"),
    ("protocol", "--protocol"),
    ("proxy", "--proxy"),
    ("useragent", "--useragent"),
    ("reported_os", "--os"),
    ("mca_cert", "--mca-certificate"),
    ("mca_key", "--mca-key"),
    ("mca_key_pass", "--mca-key-password"),
];
/// How long a prompt without a colon must stay unanswered before it counts as one.
const PROMPT_WAIT: Duration = Duration::from_millis(500);
/// The most openconnect may write on stdout (its result).
const MAX_OUTPUT: u64 = 64 * 1024;
/// The beginnings of openconnect's own progress lines (in the C locale), which aren't the
/// server's message.
const PROGRESS: &[&str] = &[
    "POST ",
    "GET ",
    "Attempting to connect",
    "Connected to ",
    "SSL negotiation",
    "Got HTTP response",
    "HTTP body",
    "XML POST enabled",
    "TLS/DTLS",
    "Unexpected empty response",
    "Spawning external browser",
    "Accepted incoming external-browser",
    "Got CONNECT response",
    "Server certificate",
    "Certificate from VPN server",
    "To trust this server",
    "--servercert",
    "Enter 'yes' to accept",
    "Failed to complete authentication",
];
/// Words that make a field's label say it isn't secret.
const NOT_SECRET: &[&str] = &[
    "user", "name", "login", "mail", "account", "group", "realm", "domain", "benutzer", "konto",
];

/// Where openconnect is installed, if it is.
pub(super) fn find_program() -> Option<PathBuf> {
    PROGRAM_DIRS
        .iter()
        .map(|dir| Path::new(dir).join("openconnect"))
        .find(|path| super::vpn::is_executable(path))
}

/// openconnect's arguments for the VPN with `data` (the `vpn` setting's data); `None` without a
/// server.
pub(super) fn arguments(data: &[(String, String)]) -> Option<Vec<String>> {
    let get = |key: &str| {
        data.iter()
            .find(|(k, _)| k == key)
            .map(|(_, value)| value.as_str())
            .filter(|value| !value.is_empty())
    };
    let gateway = get("gateway")?;
    let mut args = vec![
        "--authenticate".to_owned(),
        "--external-browser=xdg-open".to_owned(),
    ];
    for (key, flag) in ARGUMENTS {
        if let Some(value) = get(key) {
            args.push(format!("{flag}={value}"));
        }
    }
    if get("pem_passphrase_fsid") == Some("yes") {
        args.push("--key-password-from-fsid".to_owned());
    }
    if get("enable_csd_trojan") == Some("yes") {
        if let Some(wrapper) = get("csd_wrapper") {
            args.push(format!("--csd-wrapper={wrapper}"));
        }
    }
    if let Some(mode) = get("stoken_source") {
        let mut secret = get("stoken_string");
        let mode = match mode {
            "manual" if secret.is_some() => Some("rsa"),
            "stokenrc" => {
                secret = None;
                Some("rsa")
            }
            "totp" | "hotp" | "yubioath" => Some(mode),
            _ => None,
        };
        if let Some(mode) = mode {
            args.push(format!("--token-mode={mode}"));
            if let Some(secret) = secret {
                args.push(format!("--token-secret={secret}"));
            }
        }
    }
    args.push("--".to_owned());
    args.push(gateway.to_owned());
    Some(args)
}

/// What openconnect writes on stderr, in pieces.
#[derive(Debug, PartialEq, Eq)]
enum Output {
    /// A whole line.
    Line(String),
    /// A prompt: openconnect waits for an answer.
    Prompt(String),
}

/// Splits what openconnect writes on stderr into lines and prompts.
#[derive(Debug, Default)]
struct Reader {
    /// The start of a line that hasn't ended yet.
    partial: String,
}

impl Reader {
    /// The lines and prompts in `text`, the next piece of stderr. A line that hasn't ended and
    /// ends in a colon is a prompt.
    fn feed(&mut self, text: &str) -> Vec<Output> {
        self.partial.push_str(text);
        let mut out = Vec::new();
        while let Some(end) = self.partial.find('\n') {
            let line: String = self.partial.drain(..=end).collect();
            out.push(Output::Line(line.trim_end_matches(['\n', '\r']).to_owned()));
        }
        if self.partial.trim_end().ends_with(':') {
            out.push(Output::Prompt(std::mem::take(&mut self.partial)));
        }
        out
    }

    /// The line that hasn't ended, as a prompt: nothing more came for a while.
    fn idle(&mut self) -> Option<Output> {
        (!self.partial.trim().is_empty()).then(|| Output::Prompt(std::mem::take(&mut self.partial)))
    }
}

/// Whether `line` is one of openconnect's progress lines.
fn is_progress(line: &str) -> bool {
    PROGRESS
        .iter()
        .any(|start| line.trim_start().starts_with(start))
}

/// The field a `prompt` asks for: a choice for `LABEL [a|b]:`, else a text, secret unless its
/// label names a user, account or group.
fn field(prompt: &str) -> VpnField {
    let prompt = prompt.trim();
    let prompt = prompt.strip_suffix(':').unwrap_or(prompt).trim_end();
    if let Some((label, choices)) = prompt
        .strip_suffix(']')
        .and_then(|rest| rest.rsplit_once(" ["))
    {
        let label = super::clean(label.trim_end_matches(':'));
        return VpnField {
            key: label.clone(),
            label,
            value: String::new(),
            secret: false,
            choices: choices.split('|').map(super::clean).collect(),
        };
    }
    let label = super::clean(prompt);
    let lower = label.to_lowercase();
    VpnField {
        key: label.clone(),
        secret: !NOT_SECRET.iter().any(|word| lower.contains(word)),
        label,
        value: String::new(),
        choices: Vec::new(),
    }
}

/// A step of a sign-in, for the page's dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignInStep {
    /// openconnect asks for `field`, with the server's `message` (may be empty). Both are plain
    /// text, from the server.
    Ask {
        /// What the server says.
        message: String,
        /// The field.
        field: VpnField,
    },
    /// The sign-in continues in the user's browser.
    Browser,
}

/// What a sign-in gives NetworkManager.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct SignedIn {
    /// The secrets, by key: `cookie`, `gateway`, `gwcert` and maybe `resolve`.
    pub(super) secrets: Vec<(String, String)>,
}

/// Why a sign-in failed.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum SignInFailure {
    /// openconnect couldn't be started.
    Start,
    /// The user canceled.
    Canceled,
    /// The server's certificate failed verification.
    Certificate,
    /// openconnect gave up; its last message (untrusted text), if any.
    Failed(String),
}

/// Runs `openconnect` with `args`, sending each step to `steps` and reading the answers from
/// `answers`. Dropping the future, or closing `answers`, ends openconnect.
pub(super) async fn sign_in(
    program: &Path,
    args: &[String],
    steps: &Sender<SignInStep>,
    answers: &Receiver<String>,
) -> Result<SignedIn, SignInFailure> {
    let child = Command::new(program)
        .args(args)
        // Its own messages untranslated, so its progress lines are recognized; text in UTF-8.
        .env("LC_ALL", "C.UTF-8")
        .env_remove("LANGUAGE")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| SignInFailure::Start)?;
    let mut child = Running(Some(child));
    let (mut stdin, stdout, stderr) = child.pipes().ok_or(SignInFailure::Start)?;
    let talk = async {
        let mut stderr = stderr;
        let mut reader = Reader::default();
        let mut message: Vec<String> = Vec::new();
        let mut last = String::new();
        let mut buffer = [0_u8; 4096];
        loop {
            let read = async { Some(stderr.read(&mut buffer).await) };
            let waited = async {
                if reader.partial.is_empty() {
                    futures_lite::future::pending::<()>().await;
                }
                Timer::after(PROMPT_WAIT).await;
                None
            };
            let pieces = match futures_lite::future::or(read, waited).await {
                Some(Ok(0) | Err(_)) => return Talk::Ended(last),
                Some(Ok(n)) => reader.feed(&String::from_utf8_lossy(&buffer[..n])),
                None => reader.idle().into_iter().collect(),
            };
            for piece in pieces {
                match piece {
                    Output::Line(line) => {
                        if line.starts_with("Spawning external browser") {
                            let _ = steps.send(SignInStep::Browser).await;
                        }
                        if line.trim().is_empty() || is_progress(&line) {
                            continue;
                        }
                        last = super::clean(&line);
                        message.push(last.clone());
                    }
                    Output::Prompt(prompt) => {
                        if prompt.contains("Enter 'yes' to accept") {
                            return Talk::Certificate;
                        }
                        let step = SignInStep::Ask {
                            message: message.join("\n"),
                            field: field(&prompt),
                        };
                        message.clear();
                        if steps.send(step).await.is_err() {
                            return Talk::Canceled;
                        }
                        let Ok(answer) = answers.recv().await else {
                            return Talk::Canceled;
                        };
                        // One line: a line break would answer the next prompt too.
                        let answer = answer.replace(['\n', '\r'], "");
                        if stdin
                            .write_all(format!("{answer}\n").as_bytes())
                            .await
                            .is_err()
                            || stdin.flush().await.is_err()
                        {
                            return Talk::Ended(last);
                        }
                    }
                }
            }
        }
    };
    let talked = talk.await;
    let failure = match talked {
        Talk::Ended(last) => SignInFailure::Failed(last),
        Talk::Canceled => SignInFailure::Canceled,
        Talk::Certificate => SignInFailure::Certificate,
    };
    let SignInFailure::Failed(last) = failure else {
        // Ended before its stdin closes, so it never takes the end of input for an answer.
        drop(child);
        return Err(failure);
    };
    // The result is a few lines, which wait in the pipe until openconnect's stderr ended.
    let mut output = String::new();
    let _ = stdout.take(MAX_OUTPUT).read_to_string(&mut output).await;
    let status = child.status().await?;
    if !status.success() {
        return Err(SignInFailure::Failed(last));
    }
    let signed_in = result(&output);
    if signed_in.secrets.is_empty() {
        return Err(SignInFailure::Failed(last));
    }
    Ok(signed_in)
}

/// How the conversation with openconnect ended.
enum Talk {
    /// Its stderr ended; its last message.
    Ended(String),
    /// The user canceled.
    Canceled,
    /// It asked to accept a certificate that failed verification.
    Certificate,
}

/// The secrets in what `openconnect --authenticate` prints, read as nmcli reads them; none if
/// any of the cookie, the certificate's fingerprint and the server is missing.
fn result(output: &str) -> SignedIn {
    let value = |tag: &str| {
        output.lines().find_map(|line| {
            let value = line.strip_prefix(tag)?;
            let value = value
                .strip_prefix('\'')
                .and_then(|v| v.strip_suffix('\''))
                .or_else(|| value.strip_prefix('"').and_then(|v| v.strip_suffix('"')))
                .unwrap_or(value);
            Some(value.to_owned())
        })
    };
    let (Some(cookie), Some(gwcert)) = (value("COOKIE="), value("FINGERPRINT=")) else {
        return SignedIn::default();
    };
    let Some(gateway) = value("CONNECT_URL=").or_else(|| value("HOST=")) else {
        return SignedIn::default();
    };
    let mut secrets = vec![
        ("cookie".to_owned(), cookie),
        ("gateway".to_owned(), gateway),
        ("gwcert".to_owned(), gwcert),
    ];
    if let Some(resolve) = value("RESOLVE=") {
        secrets.push(("resolve".to_owned(), resolve));
    }
    SignedIn { secrets }
}

/// A running openconnect, ended when it is dropped before it ended by itself.
struct Running(Option<Child>);

impl Running {
    fn pipes(
        &mut self,
    ) -> Option<(
        async_process::ChildStdin,
        async_process::ChildStdout,
        ChildStderr,
    )> {
        let child = self.0.as_mut()?;
        Some((
            child.stdin.take()?,
            child.stdout.take()?,
            child.stderr.take()?,
        ))
    }

    async fn status(&mut self) -> Result<std::process::ExitStatus, SignInFailure> {
        let child = self.0.as_mut().ok_or(SignInFailure::Canceled)?;
        let status = child
            .status()
            .await
            .map_err(|_| SignInFailure::Failed(String::new()))?;
        self.0 = None;
        Ok(status)
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        if let Some(child) = self.0.as_mut() {
            let _ = child.kill();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    #[test]
    fn the_arguments_come_from_the_profile_with_the_server_last() {
        let args = arguments(&data(&[
            ("gateway", "-vpn.example.org"),
            ("protocol", "anyconnect"),
            ("useragent", "AnyConnect"),
            ("stoken_source", "totp"),
            ("stoken_string", "base32:ABC"),
            ("enable_csd_trojan", "no"),
            ("csd_wrapper", "/usr/libexec/csd"),
        ]))
        .unwrap();
        assert_eq!(
            args,
            [
                "--authenticate",
                "--external-browser=xdg-open",
                "--protocol=anyconnect",
                "--useragent=AnyConnect",
                "--token-mode=totp",
                "--token-secret=base32:ABC",
                "--",
                "-vpn.example.org",
            ]
        );
        assert_eq!(arguments(&data(&[("protocol", "gp")])), None);
        let stokenrc = arguments(&data(&[
            ("gateway", "vpn"),
            ("stoken_source", "stokenrc"),
            ("stoken_string", "x"),
        ]))
        .unwrap();
        assert_eq!(stokenrc[2..4], ["--token-mode=rsa", "--"]);
    }

    #[test]
    fn stderr_is_split_into_lines_and_prompts() {
        let mut reader = Reader::default();
        assert_eq!(
            reader.feed("POST https://vpn/\nXML POST "),
            [Output::Line("POST https://vpn/".to_owned())]
        );
        assert_eq!(
            reader.feed("enabled\nUsername:"),
            [
                Output::Line("XML POST enabled".to_owned()),
                Output::Prompt("Username:".to_owned())
            ]
        );
        // A choice comes in pieces; a prompt without a colon only after a while.
        assert_eq!(reader.feed("GROUP: [Students"), []);
        assert_eq!(
            reader.feed("|Staff]:"),
            [Output::Prompt("GROUP: [Students|Staff]:".to_owned())]
        );
        assert_eq!(reader.feed("Passcode"), []);
        assert_eq!(reader.idle(), Some(Output::Prompt("Passcode".to_owned())));
        assert_eq!(reader.idle(), None);
    }

    #[test]
    fn a_field_is_secret_unless_it_names_a_user_and_choices_are_split() {
        let secret = |prompt: &str| field(prompt).secret;
        assert!(!secret("Username:"));
        assert!(!secret("E-Mail:"));
        assert!(!secret("Benutzername:"));
        assert!(secret("Password:"));
        assert!(secret("Response:"));
        assert!(secret("Passcode"));
        assert_eq!(field("Response:").label, "Response");
        let group = field("GROUP: [Students|Staff]:");
        assert_eq!(
            (group.label.as_str(), group.choices, group.secret),
            (
                "GROUP",
                vec!["Students".to_owned(), "Staff".to_owned()],
                false
            )
        );
    }

    #[test]
    fn progress_lines_are_not_the_message() {
        assert!(is_progress("POST https://vpn.uni-ulm.de/"));
        assert!(is_progress("Connected to [2001:db8::1]:443"));
        assert!(is_progress("SSL negotiation with vpn.example.org"));
        assert!(!is_progress("MFA: Enter your TOTP as the second factor"));
        assert!(!is_progress("Login failed."));
    }

    #[test]
    fn the_result_is_read_as_nmcli_reads_it() {
        let output = "COOKIE='webvpn=abc'\nHOST='[2001:db8::1]'\n\
                      CONNECT_URL='https://vpn.example.org/'\n\
                      FINGERPRINT='pin-sha256:xyz='\nRESOLVE='vpn.example.org:2001:db8::1'\n";
        assert_eq!(
            result(output).secrets,
            [
                ("cookie".to_owned(), "webvpn=abc".to_owned()),
                ("gateway".to_owned(), "https://vpn.example.org/".to_owned()),
                ("gwcert".to_owned(), "pin-sha256:xyz=".to_owned()),
                (
                    "resolve".to_owned(),
                    "vpn.example.org:2001:db8::1".to_owned()
                ),
            ]
        );
        let old = "COOKIE='c'\nHOST='1.2.3.4'\nFINGERPRINT='sha1:00'\n";
        assert_eq!(result(old).secrets[1].1, "1.2.3.4");
        assert_eq!(result("COOKIE='c'\nHOST='h'\n"), SignedIn::default());
    }
}
