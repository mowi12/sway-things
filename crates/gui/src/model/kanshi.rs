// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What the model sees of kanshi: whether it sets up the displays and what its config says,
//! looked at on a thread of its own and followed through kanshi's files.
use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;

use super::{Model, Op};

impl Model {
    /// Looks again whether kanshi sets up the displays, as when displays come and go. At most
    /// every two seconds: the model changes often while a display is dragged, and a look reads
    /// `/proc`. kanshi's config and sway's are watched, so a change of those is seen at once
    /// ([`Model::look_at_kanshi`]).
    pub fn look_for_kanshi(&self) {
        let recently = self.imp().kanshi_looked.get().is_some_and(|then| {
            std::time::Instant::now().duration_since(then) < std::time::Duration::from_secs(2)
        });
        if !recently {
            self.look_at_kanshi();
        }
    }

    /// Looks now whether kanshi sets up the displays and what its config says: on a thread of
    /// its own, since that reads `/proc` and kanshi's config. Once it knows, the model takes
    /// what it saw and says `changed` if anything did. A model made for tests never looks.
    pub(super) fn look_at_kanshi(&self) {
        self.imp()
            .kanshi_looked
            .set(Some(std::time::Instant::now()));
        if !self.imp().looks_for_kanshi.get() {
            self.saw_kanshi(KanshiLook::default());
            return;
        }
        glib::spawn_future_local(glib::clone!(
            #[weak(rename_to = model)]
            self,
            async move {
                let look = gtk::gio::spawn_blocking(KanshiLook::read)
                    .await
                    .unwrap_or_default();
                if model.saw_kanshi(look) {
                    model.emit_by_name::<()>("changed", &[]);
                }
            }
        ));
    }

    /// Takes `look` as what the model sees of kanshi; says whether that changed what it saw or
    /// who sets up the displays.
    pub(super) fn saw_kanshi(&self, look: KanshiLook) -> bool {
        let seen = *self.imp().kanshi.borrow() != look;
        self.imp().kanshi.replace(look);
        let displays = self.displays();
        let changed = self.imp().displays.replace(displays) != displays;
        if changed {
            // The rows show what the displays run with, or what is stored, and the lines for
            // the displays go from the generated files or come back.
            self.queue(Op::Persist);
        }
        changed || seen
    }

    /// Follows the user's kanshi config and ours: the include line that was added, moved or
    /// removed shows on the Displays page at once, as the one of the sway config does.
    pub(super) fn watch_kanshi(&self) {
        let Some(files) = sway_things_core::outputs::kanshi::KanshiFiles::from_env() else {
            return;
        };
        let Ok(watcher) = sway_things_core::watch::Watcher::new(
            [files.config.clone(), files.ours],
            sway_things_core::watch::DEBOUNCE,
        ) else {
            return;
        };
        glib::spawn_future_local(glib::clone!(
            #[weak(rename_to = model)]
            self,
            async move {
                while let Some(changed) = watcher.changed().await {
                    let theirs = changed.contains(&files.config);
                    model.look_at_kanshi();
                    // kanshi doesn't follow its config by itself: the line the user just put
                    // there counts at once, and the arrangement made here comes back.
                    if theirs && model.kanshi_runs() {
                        sway_things_core::outputs::kanshi::reload().await;
                    }
                }
            }
        ));
    }
}

/// What the model saw of kanshi when it last looked, for the note of the Displays page: read
/// from the machine once, not each time the page draws.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KanshiLook {
    /// Whether kanshi is installed.
    pub installed: bool,
    /// Whether one runs in this session.
    pub runs: bool,
    /// Whether the user's kanshi config is there without the include of ours.
    pub include_missing: bool,
    /// The include line of ours.
    pub include_line: String,
    /// The user's profiles before the include, which kanshi tries before ours.
    pub before_ours: Vec<(String, Vec<String>)>,
    /// The file of ours for kanshi, if someone else's file stands in its place: nothing can be
    /// written there.
    pub foreign: Option<String>,
}

impl KanshiLook {
    /// Looks at the machine: `$PATH`, `/proc` and the user's kanshi config.
    fn read() -> Self {
        use sway_things_core::outputs::kanshi;
        let Some(files) = kanshi::KanshiFiles::from_env() else {
            return Self::default();
        };
        Self {
            installed: kanshi::installed(),
            runs: sway_things_core::outputs::kanshi_runs(),
            include_missing: files.inclusion() == kanshi::Inclusion::Missing,
            include_line: files.include_line(),
            before_ours: files.before_ours(),
            foreign: files
                .ours_is_foreign()
                .then(|| files.ours.display().to_string()),
        }
    }
}
