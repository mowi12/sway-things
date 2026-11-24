// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Asking the systemd user manager: `systemctl --user` with an argument array, never a shell,
//! and never waiting for long.

use std::time::Duration;

use futures_lite::FutureExt;

/// The output of `systemctl --user <args>`, if it ran within two seconds.
pub(crate) async fn output(args: &[&str]) -> Option<String> {
    let run = async {
        let output = async_process::Command::new("systemctl")
            .arg("--user")
            .args(args)
            .stdin(async_process::Stdio::null())
            .stderr(async_process::Stdio::null())
            .output()
            .await
            .ok()?;
        Some(String::from_utf8_lossy(&output.stdout).into_owned())
    };
    let timeout = async {
        async_io::Timer::after(Duration::from_secs(2)).await;
        None
    };
    run.or(timeout).await
}

/// Runs `systemctl --user <args>` and waits for it, at most ten seconds.
///
/// # Errors
///
/// What `systemctl` reported, if it failed, couldn't be run or didn't answer.
pub(crate) async fn run(args: &[&str]) -> Result<(), String> {
    let run = async {
        let output = async_process::Command::new("systemctl")
            .arg("--user")
            .args(args)
            .stdin(async_process::Stdio::null())
            .output()
            .await
            .map_err(|error| format!("systemctl: {error}"))?;
        if output.status.success() {
            Ok(())
        } else {
            Err(format!(
                "systemctl --user {}: {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr).trim()
            ))
        }
    };
    let timeout = async {
        async_io::Timer::after(Duration::from_secs(10)).await;
        Err(format!("systemctl --user {}: no answer", args.join(" ")))
    };
    run.or(timeout).await
}
