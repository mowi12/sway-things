// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The displays part of the [`Applier`]: the profiles kanshi sets up the displays with, following
//! the combination of displays connected, mirroring, forgetting a combination and placing the
//! workspaces.

use std::time::{Duration, Instant};

use super::{Applier, ApplyError, Effect};
use crate::command::SwayCmd;
use crate::generate::{self, Validate};
use crate::i18n::Text;
use crate::include::WriteAccess;
use crate::msg;
use crate::outputs::kanshi::KanshiFiles;
use crate::outputs::profiles::{Profile, Profiles};
use crate::outputs::{ENABLED, MODE, Outputs, POSITION, SCALE, TRANSFORM};
use crate::registry::{Scope, Value, workspaces};

impl<V: Validate> Applier<V> {
    /// While kanshi sets up the displays: has the file of ours for kanshi there and saying what
    /// the remembered displays do, whatever changed or didn't. kanshi rejects its whole config
    /// over an include it can't find, so the file must be there before anything else works, also
    /// when no display was ever set up here; and one that someone emptied is filled again.
    pub(super) async fn keep_kanshi_file(&mut self) {
        if !self.displays.is_kanshi() {
            return;
        }
        let places = crate::outputs::workspaces::Placing::of(&self.state);
        let (Some(files), Some(profiles)) = (self.kanshi_files.as_ref(), self.profiles.as_ref())
        else {
            return;
        };
        if let Some(problem) = Self::write_kanshi(Some(files), profiles, places).await {
            self.displays_problem = Some(problem);
        }
    }

    /// While kanshi sets up the displays: remembers how the connected ones are set up here (the
    /// stored values, and what they run with where nothing is stored) for their combination,
    /// writes the profiles for kanshi and has kanshi read them again. Not while a risky change
    /// waits (that isn't kept yet), nor without a session, nor for a combination other than the
    /// one the stored values were made for ([`Applier::follow_combination`]). Best effort: what
    /// can't be written is tried again at the next save.
    pub(super) async fn remember_displays(&mut self) {
        if !self.displays.is_kanshi() || self.pending.is_some() || !self.displays_touched {
            return;
        }
        self.reread_profiles();
        let (Some(client), Some(profiles)) = (&mut self.client, &mut self.profiles) else {
            return;
        };
        let Ok(found) = client.outputs().await else {
            return;
        };
        let outputs = Outputs::new(&found);
        if self
            .combination
            .as_ref()
            .is_some_and(|combination| *combination != combination_of(&outputs))
        {
            return;
        }
        if let Some(profile) = Profile::stored(&outputs, &self.state) {
            if profiles.remember(profile) {
                if let Err(error) = profiles.save() {
                    self.displays_problem = Some(
                        Text::new(msg!("the displays can't be remembered in {file}: {error}"))
                            .arg("file", profiles.path().display())
                            .arg("error", error),
                    );
                    return;
                }
            }
        }
        self.displays_problem = Self::write_kanshi(
            self.kanshi_files.as_ref(),
            profiles,
            crate::outputs::workspaces::Placing::of(&self.state),
        )
        .await;
        // The generated config starts kanshi, but it doesn't run yet (the displays are set up
        // here for the first time): sway starts it now, so the arrangement doesn't wait for the
        // next login to come back with the displays.
        let starts = matches!(self.displays, generate::Displays::Kanshi { start: true });
        // Once: a kanshi the user stopped, or one that ends at once, isn't started again and again.
        if self.displays_problem.is_none()
            && starts
            && !self.kanshi_started
            && !crate::outputs::kanshi_runs()
        {
            self.kanshi_started = true;
            let _ = client.run(&SwayCmd::new("exec").keyword("kanshi")).await;
        }
        // Written: nothing more to remember until a setting of the displays changes; else tried
        // again at the next save.
        self.displays_touched = self.displays_problem.is_some();
    }

    /// While kanshi sets up the displays: has the stored values of the displays follow the
    /// combination that is connected, so the page shows what kanshi set up. When the
    /// combination changes, the values of its profile take the place of what was stored for the
    /// displays (for a combination never set up here: what the displays run with now), and the
    /// values of displays that aren't connected are removed. Until a setting of the displays
    /// changes here, the values keep following the combination's profile, or what the displays
    /// run with. Nothing is sent: kanshi sets the displays up. Not while a risky change waits.
    /// Returns whether the stored values changed.
    ///
    /// # Errors
    ///
    /// The IPC errors of asking sway, and [`ApplyError::State`] if a value can't be stored.
    pub async fn follow_combination(&mut self) -> Result<bool, ApplyError> {
        if !self.displays.is_kanshi() || self.pending.is_some() {
            return Ok(false);
        }
        let Some(client) = &mut self.client else {
            return Ok(false);
        };
        let outputs = Outputs::new(&client.outputs().await.map_err(ApplyError::Ipc)?);
        let combination = combination_of(&outputs);
        if combination.is_empty() {
            return Ok(false);
        }
        if self.combination.as_ref() == Some(&combination) {
            // Changed here, or long enough after the combination came that kanshi has set it up:
            // a change from elsewhere (`swaymsg`) is left as it is, as without kanshi.
            let settled = self
                .combination_since
                .is_none_or(|since| since.elapsed() > FOLLOW_TIME);
            if self.displays_touched || settled {
                return Ok(false);
            }
        } else {
            self.combination_since = Some(Instant::now());
            // A change made here for the displays connected before, not remembered yet (the
            // combination changed within the delay before saving): remembered for theirs now,
            // before the values follow the new one.
            let before = self.combination.replace(combination.clone());
            if let Some(before) = before.filter(|_| self.displays_touched) {
                self.remember_combination(&before).await;
            }
            self.displays_touched = false;
        }
        let names: Vec<&str> = combination.iter().map(String::as_str).collect();
        let profile = self
            .profiles
            .as_ref()
            .and_then(|profiles| profiles.of(&names))
            .cloned()
            .or_else(|| Profile::capture(&outputs));
        let Some(profile) = profile else {
            return Ok(false);
        };
        let mut changed = false;
        let stale: Vec<(&'static str, String)> = self
            .state
            .entries()
            .into_iter()
            .filter(|entry| entry.setting.scope() == Scope::PerOutput)
            .filter_map(|entry| Some((entry.setting.key(), entry.device?.to_owned())))
            .filter(|(_, device)| !combination.contains(device))
            // Only what a remembered combination holds: it comes back with it. The values of a
            // display set up before kanshi did it stay.
            .filter(|(_, device)| {
                self.profiles.as_ref().is_some_and(|profiles| {
                    profiles
                        .iter()
                        .any(|profile| profile.combination().contains(&device.as_str()))
                })
            })
            .collect();
        for (key, device) in &stale {
            changed |= self.state.reset(key, Some(device))?;
        }
        for (name, setup) in profile.outputs() {
            let values = setup.values();
            for key in [ENABLED, MODE, POSITION, SCALE, TRANSFORM] {
                let wanted = values
                    .iter()
                    .find(|(wanted, _)| *wanted == key)
                    .map(|(_, value)| value);
                match wanted {
                    Some(value) if self.state.get(key, Some(name)) != Some(value) => {
                        self.state.set(key, Some(name), value.clone(), None)?;
                        changed = true;
                    }
                    Some(_) => {}
                    None => changed |= self.state.reset(key, Some(name))?,
                }
            }
        }
        // Not saved for this alone: the values come back whenever the app follows again, and a
        // save per dock and undock would push the user's own changes out of the history.
        if changed {
            self.waiting.settle(&self.state);
        }
        Ok(changed)
    }

    /// Remembers the stored values of the displays `names`, a combination that isn't connected
    /// any more, for that combination, and writes the profiles for kanshi. Best effort, like
    /// [`Applier::remember_displays`].
    async fn remember_combination(&mut self, names: &[String]) {
        self.reread_profiles();
        let Some(profiles) = &mut self.profiles else {
            return;
        };
        let Some(profile) = Profile::stored_for(names, &self.state) else {
            return;
        };
        if profiles.remember(profile) {
            if let Err(error) = profiles.save() {
                self.displays_problem = Some(
                    Text::new(msg!("the displays can't be remembered in {file}: {error}"))
                        .arg("file", profiles.path().display())
                        .arg("error", error),
                );
                return;
            }
        }
        self.displays_problem = Self::write_kanshi(
            self.kanshi_files.as_ref(),
            profiles,
            crate::outputs::workspaces::Placing::of(&self.state),
        )
        .await;
    }

    /// Reads the remembered displays again: `sway-things hook mirror`, run by kanshi, may have
    /// noted something in them since (the workspaces a picture took along), which a save from
    /// what was read before would drop. Kept as they are if the file can't be read now.
    fn reread_profiles(&mut self) {
        let fresh = self
            .profiles
            .as_ref()
            .and_then(|profiles| Profiles::load(profiles.path()).ok());
        if let Some(fresh) = fresh {
            self.profiles = Some(fresh);
        }
    }

    /// Writes the profiles for kanshi and has kanshi read them again, if anything changed.
    /// Returns what went wrong.
    async fn write_kanshi(
        files: Option<&KanshiFiles>,
        profiles: &Profiles,
        places: Option<crate::outputs::workspaces::Placing>,
    ) -> Option<Text> {
        files?
            .refresh(profiles, places)
            .await
            .err()
            .map(|error| error.text())
    }

    /// Has the display `target` (by its name in the settings) show another one instead of its
    /// own content (`mirror`), or with `None` its own again, for the combination connected now.
    /// Saved at once, written for kanshi, and the mirrors are started or ended now.
    ///
    /// # Errors
    ///
    /// [`ApplyError::NoSession`] without a session or profiles, and the IPC errors.
    pub async fn set_mirror(
        &mut self,
        target: &str,
        mirror: Option<crate::outputs::mirror::Mirror>,
    ) -> Result<(), ApplyError> {
        self.reread_profiles();
        let (Some(client), Some(profiles)) = (&mut self.client, &mut self.profiles) else {
            return Err(ApplyError::NoSession {
                key: "displays.mirror",
            });
        };
        let outputs = Outputs::new(&client.outputs().await.map_err(ApplyError::Ipc)?);
        let Some(captured) = Profile::stored(&outputs, &self.state) else {
            return Ok(());
        };
        let combination: Vec<String> = captured
            .combination()
            .into_iter()
            .map(str::to_owned)
            .collect();
        // Where the display stands among the others, to go back to from apart.
        let place = captured
            .outputs()
            .find(|(name, _)| *name == target)
            .and_then(|(_, setup)| setup.position);
        let names: Vec<&str> = combination.iter().map(String::as_str).collect();
        if profiles.of(&names).is_none() {
            profiles.remember(captured);
        }
        let gone = !names.contains(&target);
        let ending = mirror.is_none();
        // The workspaces that left the display when its picture started, to give back.
        let session = client.socket().display().to_string();
        let given_back = profiles
            .of(&names)
            .map(|profile| profile.gathered(target, &session))
            .filter(|_| ending)
            .unwrap_or_default();
        let set = profiles
            .of_mut(&names)
            .is_some_and(|profile| profile.set_mirror(target, mirror.clone()));
        if !set {
            // A display that went meanwhile shows nothing to end.
            if gone && ending {
                return Ok(());
            }
            return Err(ApplyError::Mirror);
        }
        // The workspaces of a display that shows another one's picture go to that one first, so
        // they stay within reach; noted (and saved before kanshi reads the profiles again and runs
        // its own hook), so they come back. Best effort, like the mirror itself.
        let _ = crate::outputs::mirror::gather_workspaces(client, profiles).await;
        // Saved onto the file as it is now: kanshi's hook may have noted workspaces in it while
        // sway was asked.
        let noted = profiles
            .of(&names)
            .map(|profile| profile.gathered(target, &session))
            .unwrap_or_default();
        if let Ok(mut fresh) = Profiles::load(profiles.path()) {
            if fresh.of(&names).is_none() {
                if let Some(profile) = profiles.of(&names) {
                    fresh.remember(profile.clone());
                }
            }
            if let Some(profile) = fresh.of_mut(&names) {
                profile.set_mirror(target, mirror);
                profile.note_gathered(target, &noted, &session);
            }
            *profiles = fresh;
        }
        let path = profiles.path().to_owned();
        profiles
            .save()
            .map_err(|source| ApplyError::Write(generate::WriteError::Io { path, source }))?;
        self.displays_problem = Self::write_kanshi(
            self.kanshi_files.as_ref(),
            profiles,
            crate::outputs::workspaces::Placing::of(&self.state),
        )
        .await;
        // The picture doesn't take the focus when it starts (the generated config says so from
        // the next reload of sway on).
        let _ = client
            .run(&crate::outputs::mirror::no_focus_command())
            .await;
        // Idempotent: kanshi's own `hook mirror` after the reload changes nothing more.
        crate::outputs::mirror::apply(profiles, &outputs);
        // Its own content again: its workspaces come back.
        if ending {
            let _ =
                crate::outputs::mirror::give_back_workspaces(client, target, &given_back, place)
                    .await;
        }
        // No workspace named after a picture that ended stays behind on its display.
        let _ = crate::outputs::mirror::leave_ended(client, profiles).await;
        // The display takes workspaces again, or no longer: placed anew. sway keeps the lists of
        // displays it was told before until it reloads, so the workspaces that aren't open
        // follow then.
        if crate::outputs::workspaces::Rule::of(&self.state).places() {
            let _ = self.place_now().await;
            if let Some(setting) = self
                .state
                .registry()
                .get(crate::registry::workspaces::PLACEMENT)
            {
                let was = self.state.get(setting.key(), None).cloned();
                self.note(setting, None, &Effect::AfterReload, was);
            }
        }
        Ok(())
    }

    /// Forgets everything set here for the displays connected now, while kanshi sets them up:
    /// their stored values (the risky ones too), the profile of their combination with its
    /// pictures and its placement of the workspaces. kanshi's file is written without it and
    /// kanshi reads it again, so what set the displays up before Sway Things did applies again:
    /// a profile of the user's own for these displays, or what the displays run with. The
    /// pictures end and their workspaces come back. Says whether anything was forgotten.
    ///
    /// # Errors
    ///
    /// [`ApplyError::PendingChange`] while a risky change waits, the IPC errors, and the errors
    /// of writing.
    pub async fn forget_displays(&mut self, access: &WriteAccess) -> Result<bool, ApplyError> {
        if let Some(pending) = &self.pending {
            return Err(ApplyError::PendingChange { key: pending.key() });
        }
        self.reread_profiles();
        let places = crate::outputs::workspaces::Placing::of(&self.state);
        let (Some(client), Some(profiles)) = (&mut self.client, &mut self.profiles) else {
            return Ok(false);
        };
        let outputs = Outputs::new(&client.outputs().await.map_err(ApplyError::Ipc)?);
        let connected: Vec<String> = outputs
            .iter()
            .filter(|output| output.is_connected())
            .map(|output| output.identifier().to_owned())
            .collect();
        let names: Vec<&str> = connected.iter().map(String::as_str).collect();
        let session = client.socket().display().to_string();
        // The workspaces each picture took along, to give back, and where its display stood.
        let pictures: Vec<Picture> = profiles
            .of(&names)
            .map(|profile| {
                profile
                    .outputs()
                    .filter(|(_, setup)| setup.mirror.is_some())
                    .map(|(name, setup)| {
                        (
                            name.to_owned(),
                            profile.gathered(name, &session),
                            setup.position,
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        let mut forgotten = profiles.forget(&names);
        if forgotten {
            let path = profiles.path().to_owned();
            profiles
                .save()
                .map_err(|source| ApplyError::Write(generate::WriteError::Io { path, source }))?;
        }
        // The stored values of these displays, whatever they are for.
        let stored: Vec<(&'static str, String)> = self
            .state
            .entries()
            .into_iter()
            .filter(|entry| entry.setting.scope() == Scope::PerOutput)
            .filter_map(|entry| Some((entry.setting.key(), entry.device?.to_owned())))
            .filter(|(_, device)| connected.contains(device))
            .collect();
        for (key, device) in stored {
            forgotten |= self.state.reset(key, Some(&device))?;
        }
        self.state.save(access)?;
        // Nothing made here is kept for this combination: it is read from the displays again
        // when it comes back.
        self.combination = None;
        self.displays_touched = false;
        let (Some(client), Some(profiles)) = (&mut self.client, &self.profiles) else {
            return Ok(forgotten);
        };
        // The pictures end, their displays go back among the others and take their workspaces.
        crate::outputs::mirror::apply(profiles, &outputs);
        for (target, gathered, place) in &pictures {
            let _ = crate::outputs::mirror::give_back_workspaces(client, target, gathered, *place)
                .await;
        }
        let _ = crate::outputs::mirror::leave_ended(client, profiles).await;
        // kanshi reads the file without the profile: another one that matches applies.
        self.displays_problem =
            Self::write_kanshi(self.kanshi_files.as_ref(), profiles, places).await;
        if let Some(client) = &mut self.client {
            let _ = crate::outputs::ensure_one_on(client).await;
        }
        Ok(forgotten)
    }

    /// kanshi's files, if kanshi is installed (not for an applier made for tests).
    pub fn kanshi_files(&self) -> Option<&KanshiFiles> {
        self.kanshi_files.as_ref()
    }

    /// Uses `profiles` as the setups of the displays remembered per combination.
    #[cfg(test)]
    pub(crate) fn set_profiles(&mut self, profiles: Profiles) {
        self.profiles = Some(profiles);
    }

    /// The setups of the displays remembered per combination, if they could be loaded.
    pub fn profiles(&self) -> Option<&Profiles> {
        self.profiles.as_ref()
    }

    /// Puts `workspace` on `display` (by its name in the settings) in the placement as it is now
    /// for the displays connected, remembers that as their fixed placement, saved at once, and
    /// makes the rule fixed, which places the workspaces.
    ///
    /// # Errors
    ///
    /// [`ApplyError::NoSession`] without a session or profiles, a write error of `displays.toml`,
    /// and the errors of the change and the IPC.
    pub async fn fix_placement(
        &mut self,
        workspace: &str,
        display: &str,
        access: &WriteAccess,
    ) -> Result<(), ApplyError> {
        self.reread_profiles();
        // No displays remembered yet: the file is made now. One that can't be read stays as it is.
        let path = self.state.paths().displays();
        if self.profiles.is_none() && !path.exists() {
            self.profiles = Some(Profiles::empty(path));
        }
        let (Some(client), Some(profiles)) = (&mut self.client, &mut self.profiles) else {
            return Err(ApplyError::NoSession {
                key: crate::registry::workspaces::PLACEMENT,
            });
        };
        let outputs = Outputs::new(&client.outputs().await.map_err(ApplyError::Ipc)?);
        // The move is made on the placement as it is now, so moves made one after the other
        // all count, however fast they come.
        let rule = crate::outputs::workspaces::Rule::of(&self.state);
        let mirrors = crate::outputs::workspaces::mirrored(Some(profiles), &outputs);
        let mut placement = crate::outputs::workspaces::place(
            if rule.places() {
                rule
            } else {
                crate::outputs::workspaces::Rule::RoundRobin
            },
            &outputs,
            &mirrors,
            crate::outputs::workspaces::count(&self.state),
            &crate::outputs::workspaces::fixed(Some(profiles), &outputs),
        );
        placement.insert(workspace.to_owned(), display.to_owned());
        let Some(mut profile) = Profile::stored(&outputs, &self.state) else {
            return Ok(());
        };
        let names: Vec<&str> = profile.combination();
        if let Some(known) = profiles.of(&names) {
            profile = known.clone();
        }
        profile.set_workspaces(placement);
        profiles.remember(profile);
        let path = profiles.path().to_owned();
        profiles
            .save()
            .map_err(|source| ApplyError::Write(generate::WriteError::Io { path, source }))?;
        // The rule becomes fixed, which places them; or it was, and a reload settles the
        // workspaces that aren't open (sway only appends to their displays).
        let fixed = Value::Choice("fixed".to_owned());
        if self.state.get(workspaces::PLACEMENT, None) == Some(&fixed) {
            self.write_profiles_for_kanshi().await;
            self.place_now().await?;
            if let Some(setting) = self.state.registry().get(workspaces::PLACEMENT) {
                self.note(setting, None, &Effect::AfterReload, Some(fixed));
            }
            Ok(())
        } else {
            self.set(workspaces::PLACEMENT, None, fixed, access)
                .await
                .map(drop)
        }
    }

    /// Writes kanshi's profiles again, after the placement of the workspaces changed: they run
    /// the hook only while it places.
    pub(super) async fn write_profiles_for_kanshi(&mut self) {
        if let Some(profiles) = &self.profiles {
            let places = crate::outputs::workspaces::Placing::of(&self.state);
            self.displays_problem =
                Self::write_kanshi(self.kanshi_files.as_ref(), profiles, places).await;
        }
    }

    /// Puts the workspaces on the displays connected now as the Workspaces page says
    /// ([`crate::outputs::workspaces::apply`]). Nothing without a session.
    ///
    /// # Errors
    ///
    /// The IPC errors.
    pub async fn place_now(&mut self) -> Result<(), ApplyError> {
        let Some(client) = &mut self.client else {
            return Ok(());
        };
        crate::outputs::workspaces::apply(client, &self.state, self.profiles.as_ref())
            .await
            .map(drop)
            .map_err(ApplyError::Ipc)
    }
}

/// A display that showed a picture: its name in the settings, the workspaces the picture took
/// along, and where the display stood among the others.
type Picture = (String, Vec<String>, Option<(i32, i32)>);

/// How long after a combination of displays came the stored values follow what the displays
/// run with, while kanshi sets them up.
const FOLLOW_TIME: Duration = Duration::from_secs(10);

/// The names of the connected displays of `outputs`, sorted: their combination.
fn combination_of(outputs: &Outputs) -> Vec<String> {
    let mut names: Vec<String> = outputs
        .iter()
        .filter(|output| output.is_connected())
        .map(|output| output.identifier().to_owned())
        .collect();
    names.sort_unstable();
    names
}
