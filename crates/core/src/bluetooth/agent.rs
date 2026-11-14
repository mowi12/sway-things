// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The pairing agent: the D-Bus object (`org.bluez.Agent1`) BlueZ asks while pairing for a PIN, a
//! passkey or a confirmation. Each question becomes a [`Prompt`] on a channel, which the page
//! shows as a dialog and answers.
//!
//! It is registered without asking to be BlueZ's default agent, so BlueZ only asks it for the
//! pairings this connection starts; a running blueman-applet keeps handling the rest. It only
//! answers calls from BlueZ itself: another program on the system bus can't make it show dialogs.

use async_channel::{Receiver, Sender};
use zbus::message::Header;
use zbus::zvariant::ObjectPath;
use zbus::{Connection, interface};

use super::bluez::SERVICE;

/// Where the agent lives on our connection.
pub(super) const PATH: &str = "/io/github/mowi12/SwayThings/agent";
/// What the agent can do: show a code and take one typed in.
pub(super) const CAPABILITY: &str = "KeyboardDisplay";

/// The longest PIN code Bluetooth takes, in characters.
const MAX_PIN: usize = 16;
/// The largest passkey: passkeys have six digits.
const MAX_PASSKEY: u32 = 999_999;

/// What BlueZ wants to know or show while pairing a device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// Type in a PIN code (1 to 16 letters or digits) that the device expects; old devices.
    PinCode,
    /// Type in the six-digit passkey the device shows.
    Passkey,
    /// Show this PIN code, to be typed on the device. Needs no answer.
    DisplayPinCode(String),
    /// Show this passkey, to be typed on the device (a keyboard); `entered` digits are typed so
    /// far. Comes again with each digit. Needs no answer.
    DisplayPasskey {
        /// The passkey.
        passkey: u32,
        /// How many of its digits were typed on the device.
        entered: u16,
    },
    /// Confirm that the device shows this passkey too.
    Confirmation(u32),
    /// Allow pairing with a device that offers no way to check it.
    Authorization,
    /// Allow the device to use a service (its UUID).
    Service(String),
}

impl Request {
    /// Whether the request waits for an answer (all but the displays).
    pub fn needs_answer(&self) -> bool {
        !matches!(self, Self::DisplayPinCode(_) | Self::DisplayPasskey { .. })
    }
}

/// An answer to a [`Request`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    /// Yes, to a [`Request::Confirmation`], [`Request::Authorization`] or [`Request::Service`].
    Accept,
    /// No, to any request; pairing fails.
    Reject,
    /// The PIN code for a [`Request::PinCode`].
    PinCode(String),
    /// The passkey for a [`Request::Passkey`].
    Passkey(u32),
}

/// A question from BlueZ, about the device at [`device`](Self::device) (its path).
#[derive(Debug)]
pub struct Prompt {
    /// The path of the device being paired.
    pub device: String,
    /// What BlueZ asks.
    pub request: Request,
    /// Where the answer goes; `None` for a display.
    reply: Option<Sender<Answer>>,
}

impl Prompt {
    /// Answers the prompt. Dropping a prompt that needs an answer cancels the pairing.
    pub fn answer(self, answer: Answer) {
        if let Some(reply) = self.reply {
            // The pairing may have ended already; then nobody waits for the answer.
            let _ = reply.try_send(answer);
        }
    }
}

/// What the agent tells the page.
#[derive(Debug)]
pub enum AgentEvent {
    /// A question or something to show.
    Prompt(Prompt),
    /// BlueZ gave up the current question (the device canceled or it timed out): close its
    /// dialog.
    Cancel,
}

/// The six digits of `passkey`, as a device shows it (`012345`).
pub fn passkey_text(passkey: u32) -> String {
    format!("{passkey:06}")
}

/// Whether `pin` is a PIN code Bluetooth takes: 1 to 16 ASCII letters or digits.
pub fn valid_pin(pin: &str) -> bool {
    (1..=MAX_PIN).contains(&pin.len()) && pin.chars().all(|c| c.is_ascii_alphanumeric())
}

/// `text` as a passkey, if it is one: up to six digits.
pub fn parse_passkey(text: &str) -> Option<u32> {
    let text = text.trim();
    if text.is_empty() || text.len() > 6 || !text.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    text.parse().ok().filter(|passkey| *passkey <= MAX_PASSKEY)
}

/// A new channel for the agent's events.
pub(super) fn channel() -> (Sender<AgentEvent>, Receiver<AgentEvent>) {
    async_channel::unbounded()
}

/// The errors the agent answers BlueZ with.
#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.bluez.Error")]
enum AgentError {
    /// A zbus error.
    #[zbus(error)]
    ZBus(zbus::Error),
    /// The user said no, or gave an answer that doesn't fit.
    Rejected(String),
    /// Nobody answered: the page went away.
    Canceled(String),
}

/// The D-Bus object.
pub(super) struct Agent {
    /// Where the prompts go.
    pub(super) events: Sender<AgentEvent>,
}

impl Agent {
    /// Asks the page and waits for the answer.
    async fn ask(&self, device: &ObjectPath<'_>, request: Request) -> Result<Answer, AgentError> {
        let (reply, answer) = async_channel::bounded(1);
        let prompt = Prompt {
            device: device.to_string(),
            request,
            reply: Some(reply),
        };
        self.events
            .send(AgentEvent::Prompt(prompt))
            .await
            .map_err(|_| canceled())?;
        answer.recv().await.map_err(|_| canceled())
    }

    /// Shows something without waiting.
    async fn show(&self, device: &ObjectPath<'_>, request: Request) {
        let prompt = Prompt {
            device: device.to_string(),
            request,
            reply: None,
        };
        // Without a page there's nobody to show it to; the pairing goes on.
        let _ = self.events.send(AgentEvent::Prompt(prompt)).await;
    }

    /// Asks a yes-or-no question.
    async fn confirm(&self, device: &ObjectPath<'_>, request: Request) -> Result<(), AgentError> {
        match self.ask(device, request).await? {
            Answer::Accept => Ok(()),
            _ => Err(rejected()),
        }
    }
}

#[interface(name = "org.bluez.Agent1")]
impl Agent {
    /// BlueZ no longer uses the agent; nothing to clean up.
    #[allow(
        clippy::unused_self,
        reason = "zbus calls interface methods on the object"
    )]
    fn release(&self) {}

    async fn request_pin_code(
        &self,
        device: ObjectPath<'_>,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &Connection,
    ) -> Result<String, AgentError> {
        from_bluez(&header, connection).await?;
        match self.ask(&device, Request::PinCode).await? {
            Answer::PinCode(pin) if valid_pin(&pin) => Ok(pin),
            _ => Err(rejected()),
        }
    }

    async fn display_pin_code(
        &self,
        device: ObjectPath<'_>,
        pincode: String,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &Connection,
    ) -> Result<(), AgentError> {
        from_bluez(&header, connection).await?;
        if !valid_pin(&pincode) {
            return Err(rejected());
        }
        self.show(&device, Request::DisplayPinCode(pincode)).await;
        Ok(())
    }

    async fn request_passkey(
        &self,
        device: ObjectPath<'_>,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &Connection,
    ) -> Result<u32, AgentError> {
        from_bluez(&header, connection).await?;
        match self.ask(&device, Request::Passkey).await? {
            Answer::Passkey(passkey) if passkey <= MAX_PASSKEY => Ok(passkey),
            _ => Err(rejected()),
        }
    }

    async fn display_passkey(
        &self,
        device: ObjectPath<'_>,
        passkey: u32,
        entered: u16,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &Connection,
    ) -> Result<(), AgentError> {
        from_bluez(&header, connection).await?;
        self.show(&device, Request::DisplayPasskey { passkey, entered })
            .await;
        Ok(())
    }

    async fn request_confirmation(
        &self,
        device: ObjectPath<'_>,
        passkey: u32,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &Connection,
    ) -> Result<(), AgentError> {
        from_bluez(&header, connection).await?;
        self.confirm(&device, Request::Confirmation(passkey)).await
    }

    async fn request_authorization(
        &self,
        device: ObjectPath<'_>,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &Connection,
    ) -> Result<(), AgentError> {
        from_bluez(&header, connection).await?;
        self.confirm(&device, Request::Authorization).await
    }

    async fn authorize_service(
        &self,
        device: ObjectPath<'_>,
        uuid: String,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &Connection,
    ) -> Result<(), AgentError> {
        from_bluez(&header, connection).await?;
        self.confirm(&device, Request::Service(uuid)).await
    }

    async fn cancel(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &Connection,
    ) -> Result<(), AgentError> {
        from_bluez(&header, connection).await?;
        let _ = self.events.send(AgentEvent::Cancel).await;
        Ok(())
    }
}

/// Fails unless the call comes from the program that owns BlueZ's name.
async fn from_bluez(header: &Header<'_>, connection: &Connection) -> Result<(), AgentError> {
    let sender = header.sender().ok_or_else(rejected)?;
    let owner: String = connection
        .call_method(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            Some("org.freedesktop.DBus"),
            "GetNameOwner",
            &(SERVICE,),
        )
        .await
        .map_err(|_| rejected())?
        .body()
        .deserialize()
        .map_err(|_| rejected())?;
    if sender.as_str() == owner {
        Ok(())
    } else {
        Err(rejected())
    }
}

fn rejected() -> AgentError {
    AgentError::Rejected("Rejected".to_owned())
}

fn canceled() -> AgentError {
    AgentError::Canceled("Canceled".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pins_are_one_to_sixteen_letters_or_digits() {
        assert!(valid_pin("0000"));
        assert!(valid_pin("abcDEF0123456789"));
        assert!(!valid_pin(""));
        assert!(!valid_pin("abcDEF01234567890"));
        assert!(!valid_pin("12 34"));
        assert!(!valid_pin("12ä4"));
    }

    #[test]
    fn passkeys_have_up_to_six_digits() {
        assert_eq!(parse_passkey("012345"), Some(12_345));
        assert_eq!(parse_passkey(" 999999 "), Some(999_999));
        assert_eq!(parse_passkey("1000000"), None);
        assert_eq!(parse_passkey("12a"), None);
        assert_eq!(parse_passkey("-1"), None);
        assert_eq!(parse_passkey(""), None);
        assert_eq!(passkey_text(42), "000042");
    }

    #[test]
    fn only_displays_need_no_answer() {
        assert!(!Request::DisplayPinCode("0000".to_owned()).needs_answer());
        assert!(
            !Request::DisplayPasskey {
                passkey: 1,
                entered: 0
            }
            .needs_answer()
        );
        assert!(Request::Confirmation(1).needs_answer());
        assert!(Request::PinCode.needs_answer());
    }

    #[test]
    fn a_dropped_prompt_cancels_and_an_answer_arrives() {
        let (reply, answer) = async_channel::bounded(1);
        let prompt = Prompt {
            device: String::new(),
            request: Request::Authorization,
            reply: Some(reply),
        };
        prompt.answer(Answer::Accept);
        assert_eq!(answer.try_recv(), Ok(Answer::Accept));

        let (reply, answer) = async_channel::bounded::<Answer>(1);
        drop(Prompt {
            device: String::new(),
            request: Request::Authorization,
            reply: Some(reply),
        });
        assert!(answer.try_recv().is_err());
        assert!(answer.is_closed());
    }
}
