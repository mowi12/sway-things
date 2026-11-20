// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What the commands need to know of the displays: the device a command names, and what kanshi
//! does with a change of one.

use sway_things_core::generate::Validate;
use sway_things_core::i18n::{tr, trf};
use sway_things_core::msg;
use sway_things_core::outputs::{self, Outputs};
use sway_things_core::registry::{Registry, Scope};
use sway_things_core::state::StateError;

use super::{Command, Exit, Io, Session, error_line, hook::kanshi_files};

/// What the command line needs to know of the displays to find the device a command names.
pub(super) struct Displays {
    /// The outputs of the session, if there is one and the command reads or writes settings.
    pub(super) outputs: Option<Outputs>,
}

impl Displays {
    /// The displays for `command`. A display named by its name can't be told apart from one
    /// stored by that name if sway can't list its displays: a command that writes for one then
    /// fails, with the error on `io`.
    pub(super) async fn of<V: Validate>(
        command: &Command,
        session: &mut Session<V>,
        io: &mut Io<'_>,
    ) -> Result<Self, Exit> {
        let listed = match (command, session.applier.client()) {
            (
                Command::Get { .. }
                | Command::Set { .. }
                | Command::Reset { .. }
                | Command::Wallpaper(_),
                Some(client),
            ) => Some(client.outputs().await.map(|found| Outputs::new(&found))),
            _ => None,
        };
        if let (
            Some(Err(error)),
            Command::Set { key, device, .. } | Command::Reset { key, device, .. },
        ) = (&listed, command)
        {
            if device.is_some() && is_for_displays(Some(key)) {
                io.error(error_line(&error.text().translated()));
                return Err(Exit::Failure);
            }
        }
        Ok(Self {
            outputs: listed.and_then(Result::ok),
        })
    }

    /// `device` as settings are stored for it ([`resolve_display`]): a connected display named
    /// by its name is that display; anything else, such as a value stored by a name, stays as
    /// given.
    pub(super) fn resolve(&self, key: Option<&str>, device: Option<&str>) -> Option<String> {
        resolve_display(key, device, self.outputs.as_ref())
    }

    /// What a value of `key` is stored for: `collection` for a setting of a wallpaper collection,
    /// else `device` as [`Displays::resolve`] makes it.
    ///
    /// # Errors
    ///
    /// [`StateError::NotPerCollection`] for `collection` with another setting,
    /// [`StateError::NoCollection`] for a setting of a collection without one.
    pub(super) fn stored_for(
        &self,
        key: &str,
        device: Option<&str>,
        collection: Option<&str>,
    ) -> Result<Option<String>, StateError> {
        let setting = Registry::builtin().get(key);
        let of_collection = setting.is_some_and(|setting| setting.scope() == Scope::PerCollection);
        match (collection, setting) {
            (Some(collection), _) if of_collection => Ok(Some(collection.to_owned())),
            (Some(_), Some(setting)) => Err(StateError::NotPerCollection { key: setting.key() }),
            (None, Some(setting)) if of_collection => {
                Err(StateError::NoCollection { key: setting.key() })
            }
            // An unknown key is reported by the state.
            _ => Ok(self.resolve(Some(key), device)),
        }
    }
}

/// `device` as settings are stored for it: for a setting of displays (or without a key), a
/// connected display named by its name (`DP-4`) is its identifier (make, model and serial).
/// Anything else stays as given.
pub(super) fn resolve_display(
    key: Option<&str>,
    device: Option<&str>,
    outputs: Option<&Outputs>,
) -> Option<String> {
    let device = device?;
    let identifier = outputs
        .filter(|_| is_for_displays(key))
        .and_then(|outputs| outputs.find(device))
        .map(|output| output.identifier().to_owned());
    Some(identifier.unwrap_or_else(|| device.to_owned()))
}

/// After a change (or a `reset`) of a display's setting that went through while kanshi sets up
/// the displays: says what that means.
pub(super) async fn note_kanshi<V: Validate>(
    session: &mut Session<V>,
    key: &str,
    exit: Exit,
    reset: bool,
    io: &mut Io<'_>,
) {
    if exit != Exit::Ok || !session.applier.displays().is_kanshi() || !is_for_displays(Some(key)) {
        return;
    }
    // Someone else's file in the place of ours: nothing reaches kanshi.
    if let Some(files) = kanshi_files().filter(outputs::kanshi::KanshiFiles::ours_is_foreign) {
        io.warn(trf(
            msg!(
                "the change isn't remembered: {file} wasn't made by Sway Things, so it is left \
                 alone and kanshi never sees what is set. Move that file away."
            ),
            &[("file", &files.ours.display())],
        ));
        return;
    }
    // A profile of the user's own that kanshi applies before ours: the change didn't last.
    if let Some((names, how)) = overriding_profiles(session).await {
        io.warn(trf(
            msg!(
                "the change doesn't last: a kanshi profile of your own ({name}) sets up these \
                 displays, as kanshi takes the first profile that matches and it comes before \
                 the include of Sway Things. {how}"
            ),
            &[("name", &names), ("how", &how)],
        ));
        return;
    }
    if reset {
        io.note(tr(msg!(
            "kanshi sets up the displays: what the display runs with now is remembered for the \
             displays connected now."
        )));
    } else {
        io.note(tr(msg!(
            "kanshi sets up the displays: the change is remembered for the displays connected \
             now, and kanshi brings it back whenever they are."
        )));
    }
}

/// The user's kanshi profiles that kanshi applies instead of ours for the displays connected
/// now, by name, and how to put our include first. `None` if none does.
pub(super) async fn overriding_profiles<V: Validate>(
    session: &mut Session<V>,
) -> Option<(String, String)> {
    let files = kanshi_files()?;
    let found = Outputs::new(&session.applier.client()?.outputs().await.ok()?);
    let connected: Vec<&str> = found
        .iter()
        .filter(|output| output.is_connected())
        .map(outputs::Output::identifier)
        .collect();
    let names = outputs::kanshi::overriding(&files.before_ours(), &connected, |criteria| {
        found.find(criteria).map_or_else(
            || criteria.to_owned(),
            |output| output.identifier().to_owned(),
        )
    });
    (!names.is_empty()).then(|| (names.join(", "), files.how_to_include().translated()))
}

/// Before a change of a display's setting while kanshi sets up the displays: has the stored
/// values of the displays follow the combination connected now, so the change is remembered for
/// it with the rest as kanshi set it up. Best effort: says on stderr what went wrong.
pub(super) async fn follow_combination<V: Validate>(
    session: &mut Session<V>,
    key: &str,
    io: &mut Io<'_>,
) {
    if !session.applier.displays().is_kanshi() || !is_for_displays(Some(key)) {
        return;
    }
    if let Err(error) = session.applier.follow_combination().await {
        io.note(error_line(&error.text().translated()));
    }
}

/// Whether `key` is a setting of displays, or no key is given (every setting of a device).
pub(super) fn is_for_displays(key: Option<&str>) -> bool {
    key.is_none_or(|key| {
        Registry::builtin()
            .get(key)
            .is_some_and(|setting| setting.scope() == Scope::PerOutput)
    })
}
