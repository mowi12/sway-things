// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What the lines and the details do: resume, pause, set as default, print a test page, cancel
//! jobs, remove a printer and change its details, add a printer, and open the specialist tool.

use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use sway_things_core::delegate;
use sway_things_core::printers::{Cups, Error};

use super::{DetailsDialog, PrintersPage, details, dialogs, title_of};
use crate::i18n::{Msg, Text, msg, tr, trf};

impl PrintersPage {
    /// Runs `action` on CUPS for `key` (a printer's name, or `job:ID`), which shows `label`
    /// meanwhile; its error shows at the top, `failed` saying what failed.
    pub(super) fn run<F, Fut>(&self, key: &str, label: Msg, failed: Text, action: F)
    where
        F: FnOnce(Cups) -> Fut + 'static,
        Fut: Future<Output = Result<(), Error>> + 'static,
    {
        let Some(cups) = self.imp().cups.borrow().clone() else {
            return;
        };
        let weak = self.downgrade();
        let key = key.to_owned();
        self.set_running(&key, Some(tr(label)));
        self.show_error("");
        glib::spawn_future_local(async move {
            let result = action(cups).await;
            if let Some(page) = weak.upgrade() {
                page.set_running(&key, None);
                if let Err(error) = result {
                    page.show_error(&failed.text("error", error.text()).translated());
                }
                page.refresh();
            }
        });
    }

    /// Marks what runs on `key` and shows it.
    pub(super) fn set_running(&self, key: &str, label: Option<&'static str>) {
        {
            let mut running = self.imp().running.borrow_mut();
            match label {
                Some(label) => running.insert(key.to_owned(), label),
                None => running.remove(key),
            };
        }
        self.render();
    }

    /// The title of the printer `name` in the snapshot.
    pub(super) fn title(&self, name: &str) -> String {
        self.imp()
            .snapshot
            .borrow()
            .as_ref()
            .map_or_else(|| name.to_owned(), |s| title_of(s, name))
    }

    pub(super) fn resume(&self, name: &str) {
        let failed = Text::new(msg!("Couldn't resume “{printer}”: {error}"))
            .arg("printer", self.title(name));
        let id = name.to_owned();
        self.run(name, msg!("Resuming…"), failed, move |cups| async move {
            cups.resume(&id).await
        });
    }

    pub(super) fn pause(&self, name: &str) {
        let failed =
            Text::new(msg!("Couldn't pause “{printer}”: {error}")).arg("printer", self.title(name));
        let id = name.to_owned();
        self.run(name, msg!("Pausing…"), failed, move |cups| async move {
            cups.pause(&id).await
        });
    }

    pub(super) fn set_default(&self, name: &str) {
        let failed = Text::new(msg!("Couldn't make “{printer}” the default: {error}"))
            .arg("printer", self.title(name));
        let id = name.to_owned();
        self.run(
            name,
            msg!("Setting as default…"),
            failed,
            move |cups| async move { cups.set_default(&id).await },
        );
    }

    pub(super) fn print_test_page(&self, name: &str) {
        let failed = Text::new(msg!("Couldn't print a test page on “{printer}”: {error}"))
            .arg("printer", self.title(name));
        let id = name.to_owned();
        self.run(
            name,
            msg!("Sending a test page…"),
            failed,
            move |cups| async move { cups.print_test_page(&id).await.map(|_| ()) },
        );
    }

    pub(super) fn cancel_job(&self, id: i32) {
        let failed = Text::new(msg!("Couldn't cancel the job: {error}"));
        self.run(
            &format!("job:{id}"),
            msg!("Canceling…"),
            failed,
            move |cups| async move { cups.cancel_job(id).await },
        );
    }

    pub(super) fn cancel_all(&self, name: &str) {
        let ids: Vec<i32> = self
            .imp()
            .snapshot
            .borrow()
            .as_ref()
            .map(|s| s.jobs_of(name).map(|job| job.id).collect())
            .unwrap_or_default();
        let failed = Text::new(msg!("Couldn't cancel the jobs of “{printer}”: {error}"))
            .arg("printer", self.title(name));
        self.run(name, msg!("Canceling…"), failed, move |cups| async move {
            for id in ids {
                cups.cancel_job(id).await?;
            }
            Ok(())
        });
    }

    /// Removes the printer `name`, asking first.
    pub(super) fn remove(&self, name: &str) {
        let title = self.title(name);
        // Over the details, if they are open, which close once it is removed.
        let window = self
            .imp()
            .details
            .borrow()
            .as_ref()
            .map(|details| details.window.clone())
            .or_else(|| self.root().and_downcast::<gtk::Window>());
        let weak = self.downgrade();
        let id = name.to_owned();
        glib::spawn_future_local(async move {
            if !dialogs::confirm_remove(&title, window.as_ref()).await {
                return;
            }
            if let Some(page) = weak.upgrade() {
                let failed =
                    Text::new(msg!("Couldn't remove “{printer}”: {error}")).arg("printer", &title);
                page.run(
                    &id.clone(),
                    msg!("Removing…"),
                    failed,
                    move |cups| async move { cups.remove(&id).await },
                );
            }
        });
    }

    /// Opens the details of the printer `name`, or brings them to the front.
    pub(super) fn show_details(&self, name: &str) {
        let imp = self.imp();
        let open = imp.details.borrow().clone();
        if let Some(open) = open {
            if open.name == name && open.window.is_visible() {
                open.window.present();
                return;
            }
            imp.details.replace(None);
            open.window.destroy();
        }
        let printer = imp
            .snapshot
            .borrow()
            .as_ref()
            .and_then(|s| s.printer(name).cloned());
        let Some(printer) = printer else {
            return;
        };
        let window = self.root().and_downcast::<gtk::Window>();
        let dialog = DetailsDialog::open(window.as_ref(), self, &printer);
        imp.details.replace(Some(dialog.clone()));
        self.update_details();
        dialog.window.present();
    }

    /// Makes the changes of the details dialog.
    pub(super) fn apply_details(&self, changes: details::DetailsChange) {
        let failed = Text::new(msg!("Couldn't change “{printer}”: {error}"))
            .arg("printer", self.title(&changes.name));
        let name = changes.name.clone();
        self.run(&name, msg!("Saving…"), failed, move |cups| async move {
            if let Some(info) = &changes.info {
                cups.set_info(&changes.name, info).await?;
            }
            if let Some(location) = &changes.location {
                cups.set_location(&changes.name, location).await?;
            }
            if let Some(shared) = changes.shared {
                cups.set_shared(&changes.name, shared).await?;
            }
            Ok(())
        });
    }

    /// Opens the dialog for adding a printer.
    pub(super) fn open_add_dialog(&self) {
        let imp = self.imp();
        // An open dialog comes to the front; one that is gone (whatever closed it) is
        // forgotten, never shown again empty.
        let open = imp.adding.borrow().clone();
        if let Some(open) = open {
            if open.window.is_visible() {
                open.window.present();
                return;
            }
            imp.adding.replace(None);
        }
        let Some(cups) = imp.cups.borrow().clone() else {
            return;
        };
        let taken: Vec<String> = imp
            .snapshot
            .borrow()
            .as_ref()
            .map(|s| s.printers.iter().map(|p| p.name.clone()).collect())
            .unwrap_or_default();
        let known: Vec<String> = imp
            .snapshot
            .borrow()
            .as_ref()
            .map(|s| s.printers.iter().map(|p| p.device_uri.clone()).collect())
            .unwrap_or_default();
        let window = self.root().and_downcast::<gtk::Window>();
        let bus = imp.service.borrow().bus();
        let weak = self.downgrade();
        let dialog =
            dialogs::AddDialog::open(window.as_ref(), cups, bus, taken, known, move |added| {
                if let Some(page) = weak.upgrade() {
                    page.imp().adding.replace(None);
                    if added {
                        page.refresh();
                    }
                }
            });
        imp.adding.replace(Some(dialog));
    }

    /// Starts system-config-printer, or opens CUPS's web interface without it.
    pub(super) fn open_advanced(&self) {
        if let Ok(mut child) = delegate::SYSTEM_CONFIG_PRINTER.launch() {
            // Waiting reaps it once it ends, so it doesn't stay behind as a zombie.
            glib::spawn_future_local(async move {
                let _ = child.status().await;
            });
            return;
        }
        // At the port this CUPS is reached on.
        let address = self.imp().cups.borrow().as_ref().map_or_else(
            || "http://localhost:631/".to_owned(),
            |cups| cups.endpoint().web_interface(),
        );
        let window = self.root().and_downcast::<gtk::Window>();
        let weak = self.downgrade();
        gtk::UriLauncher::new(&address).launch(
            window.as_ref(),
            gtk::gio::Cancellable::NONE,
            move |result| {
                if let (Err(error), Some(page)) = (result, weak.upgrade()) {
                    page.show_error(&trf(
                        msg!("CUPS's web interface can't be opened: {error}"),
                        &[("error", &error)],
                    ));
                }
            },
        );
    }
}
