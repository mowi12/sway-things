// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A display plugged in to a combination that is set up here, the projector or the TV at a
//! meeting: Sway Things asks once whether it shows the same picture as another display or
//! extends the desktop, and remembers the answer for the new combination.
//!
//! No process watches for it. kanshi finds the case itself: for each remembered combination,
//! the file of ours has a profile with the same displays and `output *`, which matches exactly
//! one display more ([`super::profiles::kanshi_config_with`]) and runs
//! `sway-things hook new-display`. That hook settles what to do with [`decide`]: it asks only
//! about a display never seen here, and not when several displays come at once (a dock) or the
//! display is the only one. A display seen before shows the picture it showed then, if that
//! display is there. [`ask`] asks through the notification server
//! (`org.freedesktop.Notifications`) with the actions _Mirror_ and _Extend_ and waits for the
//! answer or a timeout; without an answer, the display extends the desktop, as it does anyway.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::time::Duration;

use futures_lite::StreamExt as _;
use sha2::{Digest, Sha256};
use zbus::message::Type;
use zbus::zvariant::Value;
use zbus::{Connection, MatchRule, MessageStream};

use super::mirror::{Mirror, Scaling};
use super::profiles::{Profile, Profiles};
use super::{Output, Outputs};
use crate::i18n::Text;
use crate::msg;

/// How long the question waits for an answer.
pub const WAIT: Duration = Duration::from_secs(120);

/// The notification server's name, object and interface.
const SERVICE: &str = "org.freedesktop.Notifications";
const PATH: &str = "/org/freedesktop/Notifications";

/// The app's id, which names its icon and its desktop entry.
const APP_ID: &str = "io.github.mowi12.SwayThings";

/// What came of asking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Asked {
    /// The user answered.
    Answer(Answer),
    /// No answer: the notification was dismissed or timed out.
    Nothing,
    /// The notification server shows no actions, so nothing was asked.
    NoActions,
}

/// What the user answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    /// The new display shows the same picture as [`Question::source`].
    Mirror,
    /// The new display extends the desktop.
    Extend,
}

/// What to ask about a display plugged in to a remembered combination.
#[derive(Debug, Clone, PartialEq)]
pub struct Question {
    /// The new display, by its name in the settings.
    pub display: String,
    /// Its name to show.
    pub display_label: String,
    /// The display it would show the picture of, by its name in the settings.
    pub source: String,
    /// Its name to show.
    pub source_label: String,
    /// The remembered combination with the new display, as it runs now.
    combined: Profile,
}

impl Question {
    /// The profile to remember for the combination with the new display after `answer`.
    pub fn profile(&self, answer: Answer) -> Profile {
        let mut profile = self.combined.clone();
        if answer == Answer::Mirror {
            profile.set_mirror(&self.display, Some(mirror_of(&self.source)));
        }
        profile
    }

    /// The title of the notification.
    pub fn summary(&self) -> Text {
        Text::new(msg!("New display: {display}")).arg("display", &self.display_label)
    }

    /// The text of the notification.
    pub fn body(&self) -> Text {
        Text::new(msg!(
            "Mirror shows the same picture as {source} on it, and so does a click on this notification; Extend makes it part of the desktop. Sway Things remembers your choice for these displays."
        ))
        .arg("source", &self.source_label)
    }
}

/// What `sway-things hook new-display` does.
#[derive(Debug, Clone, PartialEq)]
pub enum Decision {
    /// Ask about a display never seen here.
    Ask(Question),
    /// Remember this profile for the combination connected now and set it up: the new display
    /// was seen before and shows the picture it showed then.
    Repeat(Profile),
    /// Nothing: the combination is known, several displays are new, or the display was seen
    /// before and extended the desktop.
    Nothing,
}

/// A picture of `source` that fits whole.
fn mirror_of(source: &str) -> Mirror {
    Mirror {
        of: source.to_owned(),
        scaling: Scaling::Fit,
        gathered: Vec::new(),
        session: String::new(),
    }
}

/// What to do about the displays connected in `outputs`, with the combinations `profiles`
/// remembers: ask only when exactly one display is added to a combination set up here, it was
/// never seen in any combination, it is on, and a display of the combination can be shown on it
/// (one that is on and shows its own content).
pub fn decide(profiles: &Profiles, outputs: &Outputs) -> Decision {
    let connected: Vec<&Output> = outputs
        .iter()
        .filter(|output| output.is_connected())
        .collect();
    let names: Vec<&str> = connected.iter().map(|output| output.identifier()).collect();
    // Known already, or a display alone (a laptop with its lid closed): nothing to ask.
    if connected.len() < 2 || profiles.of(&names).is_some() {
        return Decision::Nothing;
    }
    // Each display whose leaving would give a combination set up here, with that combination.
    let added: Vec<(&Output, &Profile)> = connected
        .iter()
        .filter_map(|new| {
            let rest: Vec<&str> = names
                .iter()
                .copied()
                .filter(|name| *name != new.identifier())
                .collect();
            profiles.arranged(&rest).map(|known| (*new, known))
        })
        .collect();
    let (unseen, seen): (Vec<_>, Vec<_>) = added
        .into_iter()
        .partition(|(new, _)| !profiles.seen(new.identifier()));
    match unseen.as_slice() {
        [(new, known)] if new.is_enabled() => {
            let Some(source) = source(known, outputs) else {
                return Decision::Nothing;
            };
            Decision::Ask(Question {
                display: new.identifier().to_owned(),
                display_label: new.label(),
                source: source.identifier().to_owned(),
                source_label: source.label(),
                combined: known.with_one_more(new),
            })
        }
        [] => seen
            .into_iter()
            .filter(|(new, _)| new.is_enabled())
            .find_map(|(new, known)| {
                let of = profiles.mirrored(new.identifier())?;
                // The display it showed is here and can be shown.
                let shown = outputs
                    .find(of)
                    .filter(|output| can_be_shown(known, output))?;
                let mut profile = known.with_one_more(new);
                profile
                    .set_mirror(new.identifier(), Some(mirror_of(shown.identifier())))
                    .then_some(profile)
            })
            .map_or(Decision::Nothing, Decision::Repeat),
        _ => Decision::Nothing,
    }
}

/// Whether `output` can be shown on another display in the combination `known`: it is on, and
/// shows its own content.
fn can_be_shown(known: &Profile, output: &Output) -> bool {
    output.is_connected()
        && output.is_enabled()
        && known
            .outputs()
            .any(|(name, setup)| name == output.identifier() && setup.mirror.is_none())
}

/// The display of `known` whose picture a new display would show: the laptop's own panel, else
/// the one with the focus, else the leftmost one; of those that are on and show their own
/// content.
fn source<'a>(known: &Profile, outputs: &'a Outputs) -> Option<&'a Output> {
    let mut candidates: Vec<&Output> = outputs
        .iter()
        .filter(|output| can_be_shown(known, output))
        .collect();
    candidates.sort_by_key(|output| {
        (
            !output.is_built_in(),
            !output.is_focused(),
            output.rect().map(|rect| (rect.x, rect.y)),
        )
    });
    candidates.first().copied()
}

/// The note of the display asked about last, in `$XDG_RUNTIME_DIR`: kanshi applies the profile
/// that asks again whenever it reads its config, and a display that is still connected is
/// asked about only once. sway gives an output a new id each time it is plugged in.
const ASKED: &str = "sway-things-new-display";

/// Notes that the display with sway's output `id` in the sway `session` (its IPC socket) is
/// asked about now. Says whether it was asked about already, in which case nothing is noted.
/// Without `$XDG_RUNTIME_DIR`, `true`: nothing could tell a second question from the first.
pub fn note_asked(session: &str, id: i64) -> bool {
    match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(dir) => note_asked_in(std::path::Path::new(&dir), session, id),
        None => true,
    }
}

/// [`note_asked`] with the note in `dir`.
fn note_asked_in(dir: &std::path::Path, session: &str, id: i64) -> bool {
    // One hook at a time between reading and writing the note.
    let Some(_lock) = crate::session::Lock::take_in(dir, "sway-things-new-display.lock") else {
        return true;
    };
    let path = dir.join(ASKED);
    let note = format!("{session}\n{id}\n");
    if std::fs::read_to_string(&path).is_ok_and(|old| old == note) {
        return true;
    }
    std::fs::write(&path, note).is_err()
}

/// The lock held while the display `display` (by its name in the settings) is asked about in
/// the sway `session`: plugged in again meanwhile, it isn't asked about a second time, and the
/// answer to the first question counts. `None` while another process asks about it.
pub fn asking(session: &str, display: &str) -> Option<crate::session::Lock> {
    crate::session::Lock::take(&asking_lock(session, display)).ok()
}

/// The name of the lock of [`asking`].
fn asking_lock(session: &str, display: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(session.as_bytes());
    hash.update([0]);
    hash.update(display.as_bytes());
    let mut name = String::from("sway-things-asking-");
    for byte in hash.finalize().iter().take(4) {
        let _ = write!(name, "{byte:02x}");
    }
    name.push_str(".lock");
    name
}

/// Asks `question` on the session bus ([`ask_on`]).
///
/// # Errors
///
/// Without a session bus, and the errors of [`ask_on`].
pub async fn ask(question: &Question, wait: Duration) -> Result<Asked, zbus::Error> {
    let asked = async {
        let connection = Connection::session().await?;
        ask_on(&connection, question, wait).await
    };
    // A bus or a server that doesn't answer at all doesn't keep the hook.
    let hung = async {
        async_io::Timer::after(wait + Duration::from_secs(10)).await;
        Ok(Asked::Nothing)
    };
    futures_lite::future::or(asked, hung).await
}

/// Shows `question` as a notification with the actions _Mirror_ (also a click on it) and
/// _Extend_ and waits for the answer, at most `wait`; the notification is closed then. A server
/// that shows no actions isn't asked.
///
/// # Errors
///
/// Without a notification server, or if the bus refuses the subscriptions.
pub async fn ask_on(
    connection: &Connection,
    question: &Question,
    wait: Duration,
) -> Result<Asked, zbus::Error> {
    let reply = connection
        .call_method(Some(SERVICE), PATH, Some(SERVICE), "GetCapabilities", &())
        .await?;
    let capabilities: Vec<String> = reply.body().deserialize()?;
    if !capabilities
        .iter()
        .any(|capability| capability == "actions")
    {
        return Ok(Asked::NoActions);
    }
    // Subscribed before the notification goes out, so no answer is missed.
    let rule = MatchRule::builder()
        .msg_type(Type::Signal)
        .interface(SERVICE)?
        .path(PATH)?
        .build();
    let mut signals = MessageStream::for_match_rule(rule, connection, None).await?;
    let mirror = crate::i18n::tr(msg!("Mirror"));
    let extend = crate::i18n::tr(msg!("Extend"));
    let actions = ["default", mirror, "mirror", mirror, "extend", extend];
    let mut hints: HashMap<&str, Value<'_>> = HashMap::new();
    hints.insert("desktop-entry", Value::from(APP_ID));
    let timeout = i32::try_from(wait.as_millis()).unwrap_or(i32::MAX);
    let reply = connection
        .call_method(
            Some(SERVICE),
            PATH,
            Some(SERVICE),
            "Notify",
            &(
                "Sway Things",
                0_u32,
                APP_ID,
                question.summary().translated(),
                question.body().translated(),
                &actions[..],
                hints,
                timeout,
            ),
        )
        .await?;
    let id: u32 = reply.body().deserialize()?;
    let answer = async {
        while let Some(message) = signals.next().await {
            let Ok(message) = message else { continue };
            let member = message.header().member().map(ToString::to_string);
            match member.as_deref() {
                Some("ActionInvoked") => {
                    let Ok((of, key)) = message.body().deserialize::<(u32, String)>() else {
                        continue;
                    };
                    if of != id {
                        continue;
                    }
                    match key.as_str() {
                        "default" | "mirror" => return Some(Some(Answer::Mirror)),
                        "extend" => return Some(Some(Answer::Extend)),
                        _ => {}
                    }
                }
                Some("NotificationClosed")
                    if message
                        .body()
                        .deserialize::<(u32, u32)>()
                        .is_ok_and(|(of, _)| of == id) =>
                {
                    return Some(None);
                }
                _ => {}
            }
        }
        // The bus went away.
        Some(None)
    };
    let timed_out = async {
        async_io::Timer::after(wait).await;
        None
    };
    let answer = futures_lite::future::or(answer, timed_out).await;
    // Answered or timed out: the notification goes (a server may leave it after an action).
    // One that was dismissed is gone already.
    if answer.is_none_or(|answer| answer.is_some()) {
        close(connection, id).await;
    }
    Ok(answer.flatten().map_or(Asked::Nothing, Asked::Answer))
}

/// Closes the notification `id`; best effort.
async fn close(connection: &Connection, id: u32) {
    let _ = connection
        .call_method(
            Some(SERVICE),
            PATH,
            Some(SERVICE),
            "CloseNotification",
            &(id,),
        )
        .await;
}

#[cfg(test)]
mod tests {
    use super::{Answer, Decision, decide, note_asked_in};
    use crate::outputs::Outputs;
    use crate::outputs::profiles::{Profile, Profiles};

    /// A display of sway's `GET_OUTPUTS`, on at `x`.
    fn display(name: &str, model: &str, x: i32, focused: bool) -> String {
        format!(
            r#"{{"name": "{name}", "active": true, "primary": false, "make": "Make",
               "model": "{model}", "serial": "S{model}", "scale": 1.0, "transform": "normal",
               "focused": {focused},
               "modes": [{{"width": 1920, "height": 1080, "refresh": 60000}}],
               "current_mode": {{"width": 1920, "height": 1080, "refresh": 60000}},
               "rect": {{"x": {x}, "y": 0, "width": 1920, "height": 1080}}}}"#
        )
    }

    fn outputs(displays: &[String]) -> Outputs {
        Outputs::new(&serde_json::from_str::<Vec<_>>(&format!("[{}]", displays.join(","))).unwrap())
    }

    /// Profiles with the combination of `displays` remembered as it runs.
    fn remembered(displays: &[String]) -> Profiles {
        let mut profiles = Profiles::empty(std::path::Path::new("/nonexistent/displays.toml"));
        profiles.remember(Profile::capture(&outputs(displays)).unwrap());
        profiles
    }

    #[test]
    fn a_display_plugged_in_to_a_known_combination_is_asked_about_once() {
        let laptop = display("eDP-1", "Panel", 0, false);
        let monitor = display("DP-1", "Desk", 1920, true);
        let projector = display("HDMI-A-1", "Beamer", 3840, false);
        let profiles = remembered(&[laptop.clone(), monitor.clone()]);
        let Decision::Ask(question) = decide(
            &profiles,
            &outputs(&[laptop.clone(), monitor.clone(), projector.clone()]),
        ) else {
            panic!("no question");
        };
        assert_eq!(question.display, "Make Beamer SBeamer");
        assert_eq!(question.display_label, "Make Beamer");
        // The laptop's own panel is shown, though the other display has the focus.
        assert_eq!(question.source, "Make Panel SPanel");
        let mirrored = question.profile(Answer::Mirror);
        assert_eq!(mirrored.combination().len(), 3);
        assert!(mirrored.mirror_runs("Make Beamer SBeamer"));
        let extended = question.profile(Answer::Extend);
        assert!(!extended.mirrors());
        assert!(extended.is_arranged());
        // Once its answer is remembered, the combination is known: nothing more to ask.
        let mut answered = profiles.clone();
        answered.remember(mirrored);
        let all = outputs(&[laptop.clone(), monitor.clone(), projector.clone()]);
        assert_eq!(decide(&answered, &all), Decision::Nothing);
    }

    #[test]
    fn without_the_laptop_the_focused_display_is_shown() {
        let left = display("DP-1", "Left", 0, false);
        let right = display("DP-2", "Right", 1920, true);
        let projector = display("HDMI-A-1", "Beamer", 3840, false);
        let profiles = remembered(&[left.clone(), right.clone()]);
        let Decision::Ask(question) = decide(&profiles, &outputs(&[left, right, projector])) else {
            panic!("no question");
        };
        assert_eq!(question.source, "Make Right SRight");
    }

    #[test]
    fn a_dock_a_lone_display_and_an_unknown_combination_ask_nothing() {
        let laptop = display("eDP-1", "Panel", 0, true);
        let one = display("DP-1", "One", 1920, false);
        let two = display("DP-2", "Two", 3840, false);
        let profiles = remembered(std::slice::from_ref(&laptop));
        // Two displays at once: a desk.
        let docked = outputs(&[laptop.clone(), one.clone(), two.clone()]);
        assert_eq!(decide(&profiles, &docked), Decision::Nothing);
        // A display alone: nothing to show on it.
        assert_eq!(
            decide(&profiles, &outputs(std::slice::from_ref(&one))),
            Decision::Nothing
        );
        // Nothing remembered for the others.
        let empty = Profiles::empty(std::path::Path::new("/nonexistent/displays.toml"));
        let plugged = outputs(&[laptop, one]);
        assert_eq!(decide(&empty, &plugged), Decision::Nothing);
        // Only a placement remembered: the displays aren't set up here.
        let mut placed = Profiles::empty(std::path::Path::new("/nonexistent/displays.toml"));
        placed.remember(
            Profile::capture(&outputs(&[display("eDP-1", "Panel", 0, true)]))
                .unwrap()
                .placement_only(),
        );
        assert_eq!(decide(&placed, &plugged), Decision::Nothing);
    }

    #[test]
    fn a_display_seen_before_shows_the_picture_it_showed() {
        let laptop = display("eDP-1", "Panel", 0, true);
        let monitor = display("DP-1", "Desk", 1920, false);
        let projector = display("HDMI-A-1", "Beamer", 3840, false);
        let mut profiles = remembered(std::slice::from_ref(&laptop));
        // At the meeting: the projector showed the laptop's panel.
        let mut meeting = Profile::capture(&outputs(&[laptop.clone(), projector.clone()])).unwrap();
        assert!(meeting.set_mirror(
            "Make Beamer SBeamer",
            Some(crate::outputs::mirror::Mirror {
                of: "Make Panel SPanel".to_owned(),
                scaling: crate::outputs::mirror::Scaling::Cover,
                gathered: Vec::new(),
                session: String::new(),
            })
        ));
        profiles.remember(meeting);
        profiles.remember(Profile::capture(&outputs(&[laptop.clone(), monitor.clone()])).unwrap());
        // Now with the monitor too: not asked again, the picture comes back.
        let all = outputs(&[laptop.clone(), monitor.clone(), projector.clone()]);
        let Decision::Repeat(profile) = decide(&profiles, &all) else {
            panic!("not repeated");
        };
        assert_eq!(profile.combination().len(), 3);
        assert!(profile.mirror_runs("Make Beamer SBeamer"));
        // A display that extended the desktop before does so again, without a question.
        let mut extended = remembered(std::slice::from_ref(&laptop));
        extended
            .remember(Profile::capture(&outputs(&[laptop.clone(), projector.clone()])).unwrap());
        extended.remember(Profile::capture(&outputs(&[laptop, monitor])).unwrap());
        assert_eq!(decide(&extended, &all), Decision::Nothing);
    }

    #[test]
    fn one_question_at_a_time_per_display_and_session() {
        let first = super::asking_lock("/run/sway.sock", "Make Beamer 1");
        assert_eq!(first, super::asking_lock("/run/sway.sock", "Make Beamer 1"));
        assert_ne!(first, super::asking_lock("/run/sway.sock", "Make Beamer 2"));
        assert_ne!(
            first,
            super::asking_lock("/run/other.sock", "Make Beamer 1")
        );
        let dir = tempfile::tempdir().unwrap();
        let held = crate::session::Lock::take_in(dir.path(), &first).unwrap();
        assert!(crate::session::Lock::take_in(dir.path(), &first).is_none());
        drop(held);
        assert!(crate::session::Lock::take_in(dir.path(), &first).is_some());
    }

    #[test]
    fn a_display_is_asked_about_once_while_it_stays_plugged_in() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!note_asked_in(dir.path(), "/run/sway.sock", 7));
        assert!(note_asked_in(dir.path(), "/run/sway.sock", 7));
        // Plugged in again (sway gives it a new id), or another session: asked again.
        assert!(!note_asked_in(dir.path(), "/run/sway.sock", 9));
        assert!(!note_asked_in(dir.path(), "/run/other.sock", 9));
    }
}
