// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The rows of the Sound page: the outputs and inputs with their volume and ports, the apps that
//! play or record, and the sound cards with their profiles, updated in place from each snapshot.
use std::cell::RefCell;

use gtk::prelude::*;
use gtk::subclass::prelude::*;
use sway_things_core::sound::{Card, Device, LOUDEST, Snapshot, Sound, Stream};

use super::volume::{Kind, VolumeControl};
use super::{CardRow, Side, SoundPage, StreamRow, fill, report, shorten_names};
use crate::i18n::{msg, tr, trf};
use crate::style;
use crate::widgets::list::{list_row, row, set_hint};

/// Updates the row of `stream`: its icon, name, what it plays and its volume.
pub(super) fn update_stream_row(
    row: &StreamRow,
    stream: &Stream,
    recording: bool,
    snapshot: &Snapshot,
    loudest: f64,
) {
    let fallback = if recording {
        "audio-input-microphone-symbolic"
    } else {
        "audio-x-generic-symbolic"
    };
    let theme = gtk::IconTheme::for_display(&row.icon.display());
    let icon = stream
        .icon
        .as_deref()
        .filter(|icon| theme.has_icon(icon))
        .unwrap_or(fallback);
    row.icon.set_icon_name(Some(icon));
    row.name.set_text(&stream.app);
    row.volume.set_names(
        &trf(msg!("Volume of {app}"), &[("app", &stream.app)]),
        &trf(msg!("Mute {app}"), &[("app", &stream.app)]),
    );
    row.detail
        .set_text(&stream_detail(stream, recording, snapshot));
    row.volume.widget().set_visible(stream.volume.is_some());
    if let Some(volume) = &stream.volume {
        row.volume.show(volume.level(), stream.muted, loudest);
    }
}

/// What the row of `stream` says under the app's name: what it plays, that it records, and the
/// device when it isn't the default.
pub(super) fn stream_detail(stream: &Stream, recording: bool, snapshot: &Snapshot) -> String {
    let (device, default) = if recording {
        (
            snapshot.input_by_index(stream.device),
            snapshot.input().map(|d| d.index),
        )
    } else {
        (
            snapshot.output_by_index(stream.device),
            snapshot.output().map(|d| d.index),
        )
    };
    let elsewhere = device.filter(|device| Some(device.index) != default);
    let what = match (&stream.title, recording) {
        (Some(title), _) => title.clone(),
        (None, true) => tr(msg!("Recording")).to_owned(),
        (None, false) => tr(msg!("Playing")).to_owned(),
    };
    match elsewhere {
        Some(device) => trf(
            msg!("{what} · on {device}"),
            &[("what", &what), ("device", &device.description)],
        ),
        None => what,
    }
}

impl SoundPage {
    /// Puts the last snapshot into the widgets.
    pub(super) fn render(&self) {
        let imp = self.imp();
        let snapshot = imp.snapshot.borrow();
        let Some(snapshot) = snapshot.as_ref() else {
            return;
        };
        let loudest = if imp.above_100.get() { LOUDEST } else { 1.0 };
        imp.updating.set(true);
        Self::render_side(
            &imp.output,
            &snapshot.outputs,
            snapshot.default_output(),
            loudest,
            tr(msg!(
                "No output device. Plug in speakers or headphones; this page updates by itself."
            )),
        );
        Self::render_side(
            &imp.input,
            &snapshot.inputs,
            snapshot.default_input(),
            loudest,
            tr(msg!(
                "No input device. Plug in a microphone or headset; this page updates by itself."
            )),
        );
        imp.meter_row
            .set_visible(snapshot.default_input().is_some());
        imp.updating.set(false);
        self.render_apps(snapshot, loudest);
        self.render_cards(snapshot);
    }

    /// Fills the section of the outputs or inputs: the devices, the connectors, the volume.
    pub(super) fn render_side(
        side: &Side,
        devices: &[Device],
        current: Option<&Device>,
        loudest: f64,
        empty: &str,
    ) {
        side.list.set_visible(!devices.is_empty());
        set_hint(&side.hint, devices.is_empty().then_some(empty));
        let mut names: Vec<String> = devices.iter().map(|device| device.name.clone()).collect();
        let mut labels: Vec<&str> = devices
            .iter()
            .map(|device| device.description.as_str())
            .collect();
        // With the default unknown (a monitor of an output, or a device not listed yet), the
        // drop-down says so, and every device listed can be chosen to make it the default.
        if current.is_none() && !devices.is_empty() {
            names.insert(0, String::new());
            labels.insert(0, tr(msg!("Another device")));
        }
        let chosen = current.map_or("", |device| device.name.as_str());
        fill(&side.choice, &labels, &side.names, names, chosen);
        side.choice
            .set_sensitive(devices.len() > 1 || current.is_none());
        side.volume_row.set_visible(current.is_some());
        let Some(current) = current else {
            side.port_row.set_visible(false);
            return;
        };

        let ports: Vec<_> = current.usable_ports().collect();
        side.port_row.set_visible(ports.len() > 1);
        let port_names: Vec<String> = ports.iter().map(|port| port.name.clone()).collect();
        let port_labels: Vec<&str> = ports.iter().map(|port| port.description.as_str()).collect();
        let active = current.active_port.clone().unwrap_or_default();
        fill(
            &side.ports,
            &port_labels,
            &side.port_names,
            port_names,
            &active,
        );

        if let Some(volume) = side.volume.get() {
            volume.show(current.volume.level(), current.muted, loudest);
        }
    }

    /// Shows a row per app that plays or records, keeping the rows that stay.
    pub(super) fn render_apps(&self, snapshot: &Snapshot, loudest: f64) {
        let imp = self.imp();
        let streams: Vec<(bool, &Stream)> = snapshot
            .playing
            .iter()
            .map(|stream| (false, stream))
            .chain(snapshot.recording.iter().map(|stream| (true, stream)))
            .collect();
        let mut rows = imp.streams.borrow_mut();
        rows.retain(|key, row| {
            let keep = streams
                .iter()
                .any(|(recording, stream)| (*recording, stream.index) == *key);
            if !keep {
                imp.apps.remove(&row.row);
            }
            keep
        });
        // A new row goes where it sorts; the rows there stay where they are, so a slider being
        // dragged or focused keeps its place.
        for (position, (recording, stream)) in streams.iter().enumerate() {
            let key = (*recording, stream.index);
            let row = rows.entry(key).or_insert_with(|| {
                let row = self.new_stream_row(*recording, stream.index);
                imp.apps
                    .insert(&row.row, i32::try_from(position).unwrap_or(-1));
                row
            });
            update_stream_row(row, stream, *recording, snapshot, loudest);
        }
        imp.apps.set_visible(!streams.is_empty());
        set_hint(
            &imp.apps_hint,
            streams
                .is_empty()
                .then(|| tr(msg!("No app is playing or recording sound."))),
        );
    }

    /// A row for the stream with the server's `index`.
    pub(super) fn new_stream_row(&self, recording: bool, index: u32) -> StreamRow {
        let icon = gtk::Image::new();
        icon.set_pixel_size(24);
        let name = gtk::Label::new(None);
        let detail = gtk::Label::new(None);
        detail.add_css_class(style::DIM);
        let kind = if recording {
            Kind::Recording
        } else {
            Kind::Playing
        };
        let volume = VolumeControl::new(tr(msg!("App volume")), kind);
        let weak = self.downgrade();
        let level = std::rc::Rc::new(move |level: f64| {
            let weak = weak.clone();
            let sent: std::pin::Pin<Box<dyn Future<Output = ()>>> = Box::pin(async move {
                let Some((sound, stream)) = weak
                    .upgrade()
                    .and_then(|page| page.stream(recording, index))
                else {
                    return;
                };
                let Some(volume) = stream.volume else {
                    return;
                };
                let volume = volume.with_level(level);
                let result = if recording {
                    sound.set_recording_volume(index, &volume).await
                } else {
                    sound.set_playing_volume(index, &volume).await
                };
                report(&weak, result, msg!("The volume wasn’t changed: {error}"));
            });
            sent
        });
        let weak = self.downgrade();
        let muted = std::rc::Rc::new(move |muted: bool| {
            let weak = weak.clone();
            let sent: std::pin::Pin<Box<dyn Future<Output = ()>>> = Box::pin(async move {
                let Some(sound) = weak
                    .upgrade()
                    .and_then(|page| page.imp().sound.borrow().clone())
                else {
                    return;
                };
                let result = if recording {
                    sound.set_recording_muted(index, muted).await
                } else {
                    sound.set_playing_muted(index, muted).await
                };
                report(&weak, result, msg!("The mute wasn’t changed: {error}"));
            });
            sent
        });
        volume.on_change(level, muted);
        let texts = gtk::Box::new(gtk::Orientation::Vertical, 2);
        for label in [&name, &detail] {
            label.set_xalign(0.0);
            label.set_ellipsize(gtk::pango::EllipsizeMode::End);
            texts.append(label);
        }
        texts.set_width_request(120);
        let line = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        line.append(&icon);
        line.append(&texts);
        line.append(volume.widget());
        StreamRow {
            row: list_row(&line),
            icon,
            name,
            detail,
            volume,
        }
    }

    /// The connection and the stream with the server's `index`, as last read.
    pub(super) fn stream(&self, recording: bool, index: u32) -> Option<(Sound, Stream)> {
        let imp = self.imp();
        let sound = imp.sound.borrow().clone()?;
        let snapshot = imp.snapshot.borrow();
        let streams = if recording {
            &snapshot.as_ref()?.recording
        } else {
            &snapshot.as_ref()?.playing
        };
        let stream = streams.iter().find(|stream| stream.index == index)?;
        Some((sound, stream.clone()))
    }

    /// Shows a row with the profile choice per card that offers one.
    pub(super) fn render_cards(&self, snapshot: &Snapshot) {
        let imp = self.imp();
        let cards: Vec<&Card> = snapshot.cards_with_choice().collect();
        let mut rows = imp.card_rows.borrow_mut();
        rows.retain(|index, row| {
            let keep = cards.iter().any(|card| card.index == *index);
            if !keep {
                imp.cards.remove(&row.row);
            }
            keep
        });
        imp.updating.set(true);
        for card in &cards {
            let row = rows.entry(card.index).or_insert_with(|| {
                let row = self.new_card_row(card.index);
                imp.cards.append(&row.row);
                row
            });
            row.name.set_text(&card.description);
            row.choice
                .update_property(&[gtk::accessible::Property::Label(&trf(
                    msg!("Profile of {card}"),
                    &[("card", &card.description)],
                ))]);
            let offered: Vec<_> = card.offered().collect();
            let labels: Vec<&str> = offered.iter().map(|p| p.description.as_str()).collect();
            let names = offered.iter().map(|p| p.name.clone()).collect();
            let active = card.active_profile.as_deref().unwrap_or_default();
            fill(&row.choice, &labels, &row.profiles, names, active);
        }
        imp.updating.set(false);
        imp.cards_box.set_visible(!cards.is_empty());
    }

    /// A row for the card with the server's `index`.
    pub(super) fn new_card_row(&self, index: u32) -> CardRow {
        let name = gtk::Label::new(None);
        let detail = gtk::Label::new(Some(tr(msg!("Connectors and sound quality"))));
        detail.add_css_class(style::DIM);
        let choice = gtk::DropDown::from_strings(&[]);
        choice.set_valign(gtk::Align::Center);
        shorten_names(&choice);
        let weak = self.downgrade();
        choice.connect_selected_notify(move |choice| {
            if let Some(page) = weak.upgrade() {
                page.profile_chosen(index, choice.selected());
            }
        });
        CardRow {
            row: list_row(&row(&name, &detail, &[choice.upcast_ref()])),
            name,
            choice,
            profiles: RefCell::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use sway_things_core::sound::{Device, Server, Snapshot, Stream};

    use super::stream_detail;

    #[test]
    fn an_app_on_another_device_says_so() {
        let device = |index, name: &str, description: &str| Device {
            index,
            name: name.to_owned(),
            description: description.to_owned(),
            ..Device::default()
        };
        let snapshot = Snapshot {
            server: Server {
                default_output: Some("speakers".to_owned()),
                ..Server::default()
            },
            outputs: vec![device(1, "hdmi", "HDMI"), device(2, "speakers", "Speakers")],
            ..Snapshot::default()
        };
        let mut stream = Stream {
            device: 2,
            title: Some("Song".to_owned()),
            ..Stream::default()
        };
        assert_eq!(stream_detail(&stream, false, &snapshot), "Song");
        stream.device = 1;
        assert_eq!(stream_detail(&stream, false, &snapshot), "Song · on HDMI");
        stream.title = None;
        assert_eq!(stream_detail(&stream, true, &snapshot), "Recording");
    }
}
