// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Sound through the PulseAudio API, which pipewire-pulse serves as well as PulseAudio itself:
//! the output and input devices, the sound cards and their profiles, the streams of the apps that
//! play or record.
//!
//! [`Sound`] is the connection; the types here are plain data, free of libpulse. Device and app
//! names come from drivers and apps: they are data, shown as plain text and never put into
//! commands.
//!
//! Nothing here is stored by Sway Things: the sound server (WirePlumber with PipeWire) remembers
//! the default devices, the volumes and the profiles itself.

mod meter;
mod pulse;
#[cfg(any(test, feature = "test-support"))]
pub mod stand_in;

use std::fmt;

use crate::i18n::Text;
use crate::msg;

pub use meter::Meter;
pub use pulse::Sound;

/// The volume the sound server calls 100 %, in its own units (`PA_VOLUME_NORM`).
pub const NORMAL: u32 = 0x1_0000;

/// The highest volume the page offers when going above 100 % is allowed: 150 % (pavucontrol goes
/// to about 153 %).
pub const LOUDEST: f64 = 1.5;

/// What the sound server knows, read at one moment.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Snapshot {
    /// The server and its defaults.
    pub server: Server,
    /// The devices sounds can play on (sinks), sorted by name to show.
    pub outputs: Vec<Device>,
    /// The devices that record (sources), without the monitors of the outputs, sorted.
    pub inputs: Vec<Device>,
    /// The sound cards, with their profiles, sorted.
    pub cards: Vec<Card>,
    /// The streams apps play (sink inputs), sorted by app.
    pub playing: Vec<Stream>,
    /// The streams apps record (source outputs), sorted by app.
    pub recording: Vec<Stream>,
}

impl Snapshot {
    /// The output sounds play on: the default one, or the first if the default is unknown.
    pub fn output(&self) -> Option<&Device> {
        pick(&self.outputs, self.server.default_output.as_deref())
    }

    /// The input apps record from: the default one, or the first if the default is unknown.
    pub fn input(&self) -> Option<&Device> {
        pick(&self.inputs, self.server.default_input.as_deref())
    }

    /// The default output, if the server has one among the outputs listed (a default that is
    /// gone, or not listed yet, is none).
    pub fn default_output(&self) -> Option<&Device> {
        let name = self.server.default_output.as_deref()?;
        self.outputs.iter().find(|device| device.name == name)
    }

    /// The default input, if it is among the inputs listed: a monitor of an output, which
    /// apps can record from too, isn't.
    pub fn default_input(&self) -> Option<&Device> {
        let name = self.server.default_input.as_deref()?;
        self.inputs.iter().find(|device| device.name == name)
    }

    /// The output with the server's `index`.
    pub fn output_by_index(&self, index: u32) -> Option<&Device> {
        self.outputs.iter().find(|device| device.index == index)
    }

    /// The input with the server's `index`.
    pub fn input_by_index(&self, index: u32) -> Option<&Device> {
        self.inputs.iter().find(|device| device.index == index)
    }

    /// The cards that offer a choice: more than one profile offered, or one that isn't active.
    pub fn cards_with_choice(&self) -> impl Iterator<Item = &Card> {
        self.cards.iter().filter(|card| {
            let active = card.active_profile.as_deref();
            card.offered()
                .any(|profile| Some(profile.name.as_str()) != active)
        })
    }
}

fn pick<'a>(devices: &'a [Device], default: Option<&str>) -> Option<&'a Device> {
    default
        .and_then(|name| devices.iter().find(|device| device.name == name))
        .or_else(|| devices.first())
}

/// The sound server.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Server {
    /// Its name, e.g. "PulseAudio (on PipeWire 1.6.9)".
    pub name: String,
    /// The version of the PulseAudio protocol it speaks, e.g. "15.0.0".
    pub version: String,
    /// The name of the default output, if there is one.
    pub default_output: Option<String>,
    /// The name of the default input, if there is one.
    pub default_input: Option<String>,
}

impl Server {
    /// What to show for the server: its name, which carries the version of PipeWire behind
    /// pipewire-pulse (`PulseAudio (on PipeWire 1.6.9)`); a name without a version (PulseAudio's
    /// own) gets the version.
    pub fn label(&self) -> String {
        if self.version.is_empty() || self.name.chars().any(|c| c.is_ascii_digit()) {
            self.name.clone()
        } else {
            format!("{} {}", self.name, self.version)
        }
    }
}

/// A volume per channel, in the server's units ([`NORMAL`] is 100 %).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Volume {
    /// The volume of each channel.
    pub channels: Vec<u32>,
}

impl Volume {
    /// The same `level` on `channels` channels.
    pub fn flat(level: f64, channels: usize) -> Self {
        Self {
            channels: vec![units(level); channels.max(1)],
        }
    }

    /// The loudest channel as a fraction: 1.0 is 100 %.
    pub fn level(&self) -> f64 {
        f64::from(self.channels.iter().copied().max().unwrap_or(0)) / f64::from(NORMAL)
    }

    /// This volume set to `level` (a fraction, 1.0 is 100 %) on its loudest channel, the others
    /// keeping their balance to it.
    #[must_use]
    pub fn with_level(&self, level: f64) -> Self {
        let target = units(level);
        let loudest = self.channels.iter().copied().max().unwrap_or(0);
        if loudest == 0 || self.channels.is_empty() {
            return Self::flat(level, self.channels.len());
        }
        let channels = self
            .channels
            .iter()
            .map(|&channel| {
                let scaled = u64::from(channel) * u64::from(target) / u64::from(loudest);
                u32::try_from(scaled).unwrap_or(u32::MAX)
            })
            .collect();
        Self { channels }
    }
}

/// A fraction (1.0 is 100 %) in the server's units, clamped to 0 … [`LOUDEST`] × 100 %.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped to 0 … 1.5 × NORMAL first, which fits u32"
)]
fn units(level: f64) -> u32 {
    let level = if level.is_finite() { level } else { 0.0 };
    (level.clamp(0.0, LOUDEST) * f64::from(NORMAL)).round() as u32
}

/// A device that plays (an output, a sink) or records (an input, a source).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Device {
    /// The server's index, which the actions take.
    pub index: u32,
    /// The server's name of it, e.g. `alsa_output.pci-0000_00_1f.3.analog-stereo`.
    pub name: String,
    /// The name to show, e.g. "Built-in Audio Analog Stereo".
    pub description: String,
    /// Its volume.
    pub volume: Volume,
    /// Whether it is muted.
    pub muted: bool,
    /// Its ports (speakers, headphones, …), sorted by priority, highest first.
    pub ports: Vec<Port>,
    /// The name of the port in use.
    pub active_port: Option<String>,
    /// The index of the card it belongs to.
    pub card: Option<u32>,
    /// The icon the driver names (`device.icon_name`), if any.
    pub icon: Option<String>,
}

impl Device {
    /// The ports worth offering: those not known to be unplugged, and the one in use.
    pub fn usable_ports(&self) -> impl Iterator<Item = &Port> {
        self.ports.iter().filter(|port| {
            port.available != Availability::No
                || self.active_port.as_deref() == Some(port.name.as_str())
        })
    }
}

/// A connector of a device, like speakers or headphones.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Port {
    /// The server's name of it.
    pub name: String,
    /// The name to show.
    pub description: String,
    /// Whether something is plugged in.
    pub available: Availability,
}

/// Whether something is plugged into a port.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Availability {
    /// The driver can't tell.
    #[default]
    Unknown,
    /// Plugged in.
    Yes,
    /// Not plugged in.
    No,
}

/// A sound card, with the profiles it can run in (analog, HDMI, a headset's A2DP or HFP, …).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Card {
    /// The server's index, which the actions take.
    pub index: u32,
    /// The server's name of it.
    pub name: String,
    /// The name to show (`device.description`), else its name.
    pub description: String,
    /// The profiles, sorted by priority, highest first; "off" last.
    pub profiles: Vec<Profile>,
    /// The name of the profile in use.
    pub active_profile: Option<String>,
}

/// The profile that hands out the card's raw channels, for studio setups. Its devices don't route
/// HDMI or headphones the usual way, so it is offered only while it is in use.
const PRO_AUDIO: &str = "pro-audio";

impl Card {
    /// The profiles to offer: the available ones but Pro Audio, and the one in use.
    pub fn offered(&self) -> impl Iterator<Item = &Profile> {
        let active = self.active_profile.as_deref();
        self.profiles.iter().filter(move |profile| {
            Some(profile.name.as_str()) == active
                || (profile.available && profile.name != PRO_AUDIO)
        })
    }
}

/// A mode a card can run in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    /// The server's name of it, e.g. `output:hdmi-stereo`.
    pub name: String,
    /// The name to show, e.g. "Digital Stereo (HDMI) Output".
    pub description: String,
    /// Whether it can be used now (a profile for an unplugged output can't).
    pub available: bool,
}

/// What an app plays or records.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Stream {
    /// The server's index, which the actions take.
    pub index: u32,
    /// The app's name (`application.name`), else the stream's.
    pub app: String,
    /// What it plays, e.g. a song's title (`media.name`), unless it only repeats the app's name.
    pub title: Option<String>,
    /// The app's icon (`application.icon_name`), else its binary's name.
    pub icon: Option<String>,
    /// Its volume, or `None` if it has none that can be set.
    pub volume: Option<Volume>,
    /// Whether it is muted.
    pub muted: bool,
    /// The index of the device it plays on or records from.
    pub device: u32,
    /// Whether it is paused.
    pub paused: bool,
}

/// Why sound can't be read or an action failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// No PulseAudio-compatible sound server runs (yet): the connection waits for one.
    Unavailable,
    /// The sound server refused or failed: its error, as data.
    Failed(Text),
}

impl Error {
    /// What went wrong, to show.
    pub fn text(&self) -> Text {
        match self {
            Self::Unavailable => msg!("no sound server is running").into(),
            Self::Failed(reason) => reason.clone(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text().english())
    }
}

impl std::error::Error for Error {}

/// Sorts devices and cards by the name shown, ignoring case; the server's name decides a tie.
fn sort_by_description<T>(items: &mut [T], key: impl Fn(&T) -> (&str, &str)) {
    items.sort_by(|a, b| {
        let (a_shown, a_name) = key(a);
        let (b_shown, b_name) = key(b);
        a_shown
            .to_lowercase()
            .cmp(&b_shown.to_lowercase())
            .then_with(|| a_name.cmp(b_name))
    });
}

#[cfg(test)]
mod tests {
    use super::{Availability, Card, Device, LOUDEST, NORMAL, Port, Profile, Snapshot, Volume};

    #[test]
    fn a_level_keeps_the_balance_of_the_channels() {
        let volume = Volume {
            channels: vec![NORMAL, NORMAL / 2],
        };
        assert!((volume.level() - 1.0).abs() < 1e-9);
        let half = volume.with_level(0.5);
        assert_eq!(half.channels, [NORMAL / 2, NORMAL / 4]);
        // Silence has no balance left: every channel gets the level.
        let silent = Volume {
            channels: vec![0, 0],
        };
        assert_eq!(silent.with_level(0.5).channels, [NORMAL / 2, NORMAL / 2]);
        // Never above the loudest offered, nor below silence.
        assert!((volume.with_level(9.0).level() - LOUDEST).abs() < 1e-9);
        assert_eq!(volume.with_level(-1.0).channels, [0, 0]);
        assert_eq!(volume.with_level(f64::NAN).channels, [0, 0]);
    }

    #[test]
    fn the_server_is_named_with_one_version() {
        let server = |name: &str, version: &str| super::Server {
            name: name.to_owned(),
            version: version.to_owned(),
            ..super::Server::default()
        };
        assert_eq!(
            server("PulseAudio (on PipeWire 1.6.9)", "15.0.0").label(),
            "PulseAudio (on PipeWire 1.6.9)"
        );
        assert_eq!(server("pulseaudio", "17.0").label(), "pulseaudio 17.0");
        assert_eq!(server("pulseaudio", "").label(), "pulseaudio");
    }

    #[test]
    fn the_default_device_is_picked_else_the_first() {
        let device = |index, name: &str| Device {
            index,
            name: name.to_owned(),
            ..Device::default()
        };
        let mut snapshot = Snapshot {
            outputs: vec![device(1, "a"), device(2, "b")],
            ..Snapshot::default()
        };
        assert_eq!(snapshot.output().map(|output| output.index), Some(1));
        snapshot.server.default_output = Some("b".to_owned());
        assert_eq!(snapshot.output().map(|output| output.index), Some(2));
        snapshot.server.default_output = Some("gone".to_owned());
        assert_eq!(snapshot.output().map(|output| output.index), Some(1));
        assert_eq!(snapshot.input(), None);
    }

    #[test]
    fn unplugged_ports_are_left_out_unless_in_use() {
        let port = |name: &str, available| Port {
            name: name.to_owned(),
            description: name.to_owned(),
            available,
        };
        let device = Device {
            ports: vec![
                port("speakers", Availability::Unknown),
                port("headphones", Availability::No),
                port("line", Availability::Yes),
            ],
            active_port: Some("speakers".to_owned()),
            ..Device::default()
        };
        let names: Vec<&str> = device
            .usable_ports()
            .map(|port| port.name.as_str())
            .collect();
        assert_eq!(names, ["speakers", "line"]);
        let unplugged = Device {
            active_port: Some("headphones".to_owned()),
            ..device
        };
        assert_eq!(unplugged.usable_ports().count(), 3);
    }

    #[test]
    fn only_cards_with_a_choice_are_offered() {
        let profile = |name: &str, available| Profile {
            name: name.to_owned(),
            description: name.to_owned(),
            available,
        };
        let card = |profiles, active: &str| Card {
            profiles,
            active_profile: Some(active.to_owned()),
            ..Card::default()
        };
        let snapshot = Snapshot {
            cards: vec![
                card(vec![profile("a", true), profile("off", true)], "a"),
                card(vec![profile("a", true), profile("hdmi", false)], "a"),
                card(vec![profile("a", true)], "off"),
                card(vec![profile("a", true), profile("pro-audio", true)], "a"),
            ],
            ..Snapshot::default()
        };
        assert_eq!(snapshot.cards_with_choice().count(), 2);
        let pro = card(
            vec![profile("a", true), profile("pro-audio", true)],
            "pro-audio",
        );
        let offered: Vec<_> = pro.offered().map(|p| p.name.as_str()).collect();
        assert_eq!(offered, ["a", "pro-audio"]);
        let usual = card(vec![profile("a", true), profile("pro-audio", true)], "a");
        assert_eq!(usual.offered().count(), 1);
    }
}
