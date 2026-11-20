// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What the live pages (Bluetooth, Network, Sound) share: following the change stream of the
//! service they show, reading a new snapshot at most every so often.

use std::future::Future;
use std::time::Duration;

use futures_lite::future::poll_once;
use futures_lite::{Stream, StreamExt};
use gtk::glib;

use crate::i18n::{msg, tr};

/// Calls `refresh` after each change of `changes`, at most every `throttle`: the changes that
/// come while it waits are taken together, so a burst (a scan, a volume dragged elsewhere) reads
/// one snapshot. Ends when the stream ends or `refresh` returns false (the page is gone).
pub async fn follow_changes<S, F, R>(mut changes: S, throttle: Duration, mut refresh: F)
where
    S: Stream + Unpin,
    F: FnMut() -> R,
    R: Future<Output = bool>,
{
    while changes.next().await.is_some() {
        glib::timeout_future(throttle).await;
        while let Some(Some(_)) = poll_once(changes.next()).await {}
        if !refresh().await {
            return;
        }
    }
}

/// Asks whether to forget something a page lists (a paired device, a saved network): `message`
/// names it, `detail` says what forgetting does. Plain text: the dialog doesn't read markup.
/// True if the user chose _Forget_; _Cancel_ is the default.
pub async fn confirm_forget(message: &str, detail: &str, window: Option<&gtk::Window>) -> bool {
    let dialog = gtk::AlertDialog::builder()
        .message(message)
        .detail(detail)
        .buttons([tr(msg!("Cancel")), tr(msg!("Forget"))])
        .cancel_button(0)
        .default_button(0)
        .modal(true)
        .build();
    matches!(dialog.choose_future(window).await, Ok(1))
}
