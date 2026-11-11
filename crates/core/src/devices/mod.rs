// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The input devices of the session: what is connected, what comes and goes, and which devices
//! only have stored settings.
//!
//! [`Devices`] is the list, built from `GET_INPUTS`. [`DeviceWatch`] keeps it current: it reads
//! the devices again whenever sway sends an `input` event, and never polls. Devices that aren't connected but
//! have values in the settings files stay listed, so their values can still be seen and removed.
//!
//! The name of a device is an external string. It is only shown; commands select a device by its
//! [identifier](Device::identifier) through
//! [`InputSelector::device`](crate::command::input::InputSelector::device).
//!
//! ```no_run
//! use sway_things_core::command::input::InputType;
//! use sway_things_core::devices::DeviceWatch;
//! use sway_things_core::ipc::Client;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let sway = Client::connect().await?;
//! let mut watch = DeviceWatch::start(&sway).await?;
//! for touchpad in watch.devices().of_type(InputType::Touchpad) {
//!     println!("{} ({})", touchpad.name(), touchpad.identifier());
//! }
//! while let Some(change) = watch.changed().await {
//!     println!("{:?}", change?);
//! }
//! # Ok(())
//! # }
//! ```

#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, VecDeque};

use crate::command::input::InputType;
use crate::ipc::types::{Event, EventType, Input, SendEvents};
use crate::ipc::{Client, Events, IpcError};
use crate::registry::Scope;
use crate::state::State;

/// Whether a device sends its events: libinput's `send_events` setting, which the `events`
/// option of sway's `input` command changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Sending {
    /// The device works.
    Enabled,
    /// The device is switched off.
    Disabled,
    /// The device is switched off while an external mouse is connected.
    DisabledOnExternalMouse,
}

/// What a change would leave the user without.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Lockout {
    /// No keyboard to type with.
    Keyboard,
    /// No mouse, trackpoint or touchpad to point with.
    PointingDevice,
}

impl Lockout {
    /// What switching the devices of type `kind` (all of them, or only the one with the
    /// identifier `device`) to `sending` could leave the user without, when it isn't known which
    /// devices are connected: switching off a whole type of keyboards or pointing devices could
    /// leave none, switching off one device leaves the others.
    pub fn without_devices(
        kind: InputType,
        device: Option<&str>,
        sending: Sending,
    ) -> Option<Self> {
        if device.is_some() || sending != Sending::Disabled {
            return None;
        }
        match kind {
            InputType::Keyboard => Some(Self::Keyboard),
            InputType::Pointer | InputType::Touchpad => Some(Self::PointingDevice),
            _ => None,
        }
    }
}

impl std::fmt::Display for Lockout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Keyboard => {
                f.write_str("this would turn off the last keyboard you can type with")
            }
            Self::PointingDevice => f.write_str(
                "this would turn off the last mouse, trackpoint or touchpad you can point with",
            ),
        }
    }
}

/// The keyboard layouts of a keyboard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layouts {
    /// The names of the configured layouts, as XKB describes them (`English (US)`).
    pub names: Vec<String>,
    /// The index of the layout in use, if sway reports one that is in `names`.
    pub active: Option<usize>,
}

impl Layouts {
    /// The name of the layout in use.
    pub fn active_name(&self) -> Option<&str> {
        self.active
            .and_then(|index| self.names.get(index))
            .map(String::as_str)
    }
}

/// An input device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    identifier: String,
    name: String,
    kind: Option<InputType>,
    connected: bool,
    sending: Option<Sending>,
    layouts: Option<Layouts>,
}

impl Device {
    /// A connected device, from sway's description of it.
    fn connected(input: &Input) -> Self {
        let kind = InputType::from_name(&input.input_type);
        let sending = input
            .libinput
            .as_ref()
            .and_then(|libinput| libinput.send_events)
            .and_then(|send_events| match send_events {
                SendEvents::Enabled => Some(Sending::Enabled),
                SendEvents::Disabled => Some(Sending::Disabled),
                SendEvents::DisabledOnExternalMouse => Some(Sending::DisabledOnExternalMouse),
                // A mode this build doesn't know.
                _ => None,
            });
        let layouts = (kind == Some(InputType::Keyboard)).then(|| Layouts {
            names: input.xkb_layout_names.clone(),
            active: input
                .xkb_active_layout_index
                .and_then(|index| usize::try_from(index).ok())
                .filter(|index| *index < input.xkb_layout_names.len()),
        });
        Self {
            identifier: input.identifier.clone(),
            name: input.name.clone(),
            kind,
            connected: true,
            sending,
            layouts,
        }
    }

    /// A device that isn't connected, of which only the identifier in the settings files and the
    /// type of the settings stored for it are known.
    fn disconnected(identifier: &str, kind: InputType) -> Self {
        // `<vendor>:<product>:<name>`, the name with `_` for spaces.
        let name = identifier.splitn(3, ':').nth(2).unwrap_or(identifier);
        Self {
            identifier: identifier.to_owned(),
            name: name.to_owned(),
            kind: Some(kind),
            connected: false,
            sending: None,
            layouts: None,
        }
    }

    /// The identifier sway selects the device by: `<vendor>:<product>:<name>`. Devices of the
    /// same model share it.
    pub fn identifier(&self) -> &str {
        &self.identifier
    }

    /// The name to show. For a device that isn't connected, the name part of the identifier.
    ///
    /// This is an external string: never put it into a command.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The type sway reports, or `None` for one this build doesn't know. For a device that isn't
    /// connected, the type its stored settings are for.
    pub fn kind(&self) -> Option<InputType> {
        self.kind
    }

    /// Whether the device is connected. If not, it is listed for its stored settings.
    pub fn is_connected(&self) -> bool {
        self.connected
    }

    /// Whether the device sends its events, or `None` if that can't be switched for it (it isn't
    /// a libinput device) or it isn't connected.
    pub fn sending(&self) -> Option<Sending> {
        self.sending
    }

    /// Whether the device is connected and not switched off. A device that is switched off while
    /// an external mouse is connected counts: it works as soon as the mouse is gone.
    pub fn is_enabled(&self) -> bool {
        self.connected && self.sending != Some(Sending::Disabled)
    }

    /// The layouts of a connected keyboard.
    pub fn layouts(&self) -> Option<&Layouts> {
        self.layouts.as_ref()
    }

    /// Whether this is hardware with a vendor ID, going by the identifier. Vendor 0 are the
    /// buttons of the system (`0:1:Power_Button`, `0:6:Video_Bus`), which sway lists as
    /// keyboards, and virtual devices of other programs. Nobody can type or point with those.
    pub fn is_physical(&self) -> bool {
        self.identifier
            .split(':')
            .next()
            .and_then(|vendor| vendor.parse::<u32>().ok())
            .is_some_and(|vendor| vendor != 0)
    }

    /// Whether the user can type with this device right now: an enabled keyboard with a vendor ID.
    ///
    /// The extra keys some mice register as a keyboard of their own count too; they can't be told
    /// apart from a real keyboard.
    pub fn is_usable_keyboard(&self) -> bool {
        self.kind == Some(InputType::Keyboard) && self.is_physical() && self.is_enabled()
    }

    /// Whether the user can move the pointer with this device right now: an enabled mouse,
    /// trackpoint or touchpad with a vendor ID.
    pub fn is_usable_pointing_device(&self) -> bool {
        matches!(self.kind, Some(InputType::Pointer | InputType::Touchpad))
            && self.is_physical()
            && self.is_enabled()
    }
}

/// How the list changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// A device was connected. The identifier is new in the list, or was listed as not connected.
    Added(String),
    /// A device was disconnected. It is gone from the list, or stays as not connected if it has
    /// stored settings.
    Removed(String),
    /// Something about a connected device changed: its layouts, the layout in use, or whether it
    /// is switched off.
    Changed(String),
}

impl Change {
    /// The identifier of the device.
    pub fn identifier(&self) -> &str {
        match self {
            Self::Added(identifier) | Self::Removed(identifier) | Self::Changed(identifier) => {
                identifier
            }
        }
    }
}

/// The input devices: the connected ones in sway's order, then those that only have stored
/// settings, by identifier.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Devices {
    connected: Vec<Device>,
    /// The devices with stored settings, with the type the settings are for.
    stored: BTreeMap<String, InputType>,
    disconnected: Vec<Device>,
}

impl Devices {
    /// The devices of a reply to `GET_INPUTS`.
    pub fn new(inputs: &[Input]) -> Self {
        let mut devices = Self::default();
        devices.update(inputs);
        devices
    }

    /// Replaces the connected devices with those of a newer reply to `GET_INPUTS` and returns
    /// what changed: first the devices that are gone, then the others in sway's order.
    pub fn update(&mut self, inputs: &[Input]) -> Vec<Change> {
        let mut connected: Vec<Device> = Vec::with_capacity(inputs.len());
        for input in inputs {
            // Devices of the same model share an identifier, and with it every setting.
            if !connected
                .iter()
                .any(|device| device.identifier == input.identifier)
            {
                connected.push(Device::connected(input));
            }
        }
        let mut changes: Vec<Change> = self
            .connected
            .iter()
            .filter(|old| !connected.iter().any(|new| new.identifier == old.identifier))
            .map(|old| Change::Removed(old.identifier.clone()))
            .collect();
        for new in &connected {
            match self
                .connected
                .iter()
                .find(|old| old.identifier == new.identifier)
            {
                None => changes.push(Change::Added(new.identifier.clone())),
                Some(old) if old != new => changes.push(Change::Changed(new.identifier.clone())),
                Some(_) => {}
            }
        }
        self.connected = connected;
        self.list_disconnected();
        changes
    }

    /// Lists the devices that have values in `state` and aren't connected, and drops those whose
    /// values are gone. Call it after the state changed. Returns whether the list changed.
    pub fn set_stored(&mut self, state: &State) -> bool {
        let mut stored = BTreeMap::new();
        for entry in state.entries() {
            if let (Some(device), Scope::PerDevice(kind)) = (entry.device, entry.setting.scope()) {
                stored.entry(device.to_owned()).or_insert(kind);
            }
        }
        if stored == self.stored {
            return false;
        }
        self.stored = stored;
        let before = std::mem::take(&mut self.disconnected);
        self.list_disconnected();
        before != self.disconnected
    }

    fn list_disconnected(&mut self) {
        self.disconnected = self
            .stored
            .iter()
            .filter(|(identifier, _)| !self.is_connected(identifier))
            .map(|(identifier, kind)| Device::disconnected(identifier, *kind))
            .collect();
    }

    fn is_connected(&self, identifier: &str) -> bool {
        self.connected
            .iter()
            .any(|device| device.identifier == identifier)
    }

    /// Every device: the connected ones in sway's order, then those that only have stored
    /// settings.
    pub fn iter(&self) -> impl Iterator<Item = &Device> {
        self.connected.iter().chain(&self.disconnected)
    }

    /// The device with this identifier.
    pub fn get(&self, identifier: &str) -> Option<&Device> {
        self.iter().find(|device| device.identifier == identifier)
    }

    /// The devices of one type, for the device selector of a page.
    pub fn of_type(&self, kind: InputType) -> impl Iterator<Item = &Device> {
        self.iter().filter(move |device| device.kind == Some(kind))
    }

    /// Whether switching off the device `identifier` would leave the user without a keyboard:
    /// it is a [usable keyboard](Device::is_usable_keyboard), and no other one is connected.
    pub fn is_last_enabled_keyboard(&self, identifier: &str) -> bool {
        self.is_last(identifier, Device::is_usable_keyboard)
    }

    /// Whether switching off the device `identifier` would leave the user without a pointer: it
    /// is a [usable pointing device](Device::is_usable_pointing_device), and no other one is
    /// connected.
    pub fn is_last_enabled_pointing_device(&self, identifier: &str) -> bool {
        self.is_last(identifier, Device::is_usable_pointing_device)
    }

    /// What switching the connected devices of type `kind` (all of them, or only the one with the
    /// identifier `device`) to `sending` would leave the user without: the last usable keyboard
    /// or pointing device. `None` if one of each remains, or if there was none before (a session
    /// without input devices, which this change doesn't make worse).
    pub fn lockout(
        &self,
        kind: InputType,
        device: Option<&str>,
        sending: Sending,
    ) -> Option<Lockout> {
        let after: Vec<Device> = self
            .connected
            .iter()
            .map(|existing| {
                let covered = existing.kind == Some(kind)
                    && device.is_none_or(|identifier| existing.identifier == identifier);
                let mut changed = existing.clone();
                if covered && changed.sending.is_some() {
                    changed.sending = Some(sending);
                }
                changed
            })
            .collect();
        let lost = |usable: fn(&Device) -> bool| {
            self.connected.iter().any(usable) && !after.iter().any(usable)
        };
        if lost(Device::is_usable_keyboard) {
            Some(Lockout::Keyboard)
        } else if lost(Device::is_usable_pointing_device) {
            Some(Lockout::PointingDevice)
        } else {
            None
        }
    }

    fn is_last(&self, identifier: &str, usable: impl Fn(&Device) -> bool) -> bool {
        let mut devices = self.connected.iter().filter(|device| usable(device));
        devices.clone().count() == 1 && devices.all(|device| device.identifier == identifier)
    }
}

/// The device list of a session, kept current with sway's `input` events.
#[derive(Debug)]
pub struct DeviceWatch {
    devices: Devices,
    events: Events,
    /// A connection of its own, to read the devices after each event.
    client: Client,
    /// Changes that weren't returned yet.
    pending: VecDeque<Change>,
}

impl DeviceWatch {
    /// Subscribes to the `input` events of the session of `client` and reads its devices, on a
    /// connection of its own.
    ///
    /// # Errors
    ///
    /// The errors of [`Client::connect_to`], [`Client::subscribe`] and [`Client::inputs`].
    pub async fn start(client: &Client) -> Result<Self, IpcError> {
        let mut client = Client::connect_to(client.socket()).await?;
        // First the subscription, so a device that is plugged in meanwhile isn't missed.
        let events = client.subscribe(&[EventType::Input]).await?;
        let devices = Devices::new(&client.inputs().await?);
        Ok(Self {
            devices,
            events,
            client,
            pending: VecDeque::new(),
        })
    }

    /// The devices as of the last event.
    pub fn devices(&self) -> &Devices {
        &self.devices
    }

    /// Lists the devices that only have values in `state`; see [`Devices::set_stored`].
    pub fn set_stored(&mut self, state: &State) -> bool {
        self.devices.set_stored(state)
    }

    /// Waits until the list changes and returns how; several changes at once come one by one.
    /// `None` once sway closed the connection, which it does when it exits.
    ///
    /// An event only says that something changed: the devices are read again, so the list is
    /// exact also for two devices of the same model, which share an identifier.
    ///
    /// # Errors
    ///
    /// [`IpcError::InvalidReply`] for an event or a device list that can't be read; the list may
    /// be out of date then, and the events after it still come. After [`IpcError::Io`] or
    /// [`IpcError::Protocol`] there are no more events.
    pub async fn changed(&mut self) -> Option<Result<Change, IpcError>> {
        loop {
            if let Some(change) = self.pending.pop_front() {
                return Some(Ok(change));
            }
            match self.events.next().await? {
                Ok(Event::Input(_)) => match self.client.inputs().await {
                    Ok(inputs) => self.pending.extend(self.devices.update(&inputs)),
                    Err(error) => return Some(Err(error)),
                },
                Ok(_) => {}
                Err(error) => return Some(Err(error)),
            }
        }
    }
}
