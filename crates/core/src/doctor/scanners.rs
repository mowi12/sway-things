// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The checks of scanners, the same as the hints of the Printers & Scanners page that need no
//! scanner to be asked: scanimage missing while a scanner is plugged in, sane-airscan missing,
//! ipp-usb not running for a device that speaks IPP over USB, and a USB scanner the user can't
//! use. Nothing is run: SANE's listing takes seconds and talks to the network, which the page
//! does on request.

use super::{Code, Finding, Severity};
use crate::i18n::Text;
use crate::msg;
use crate::scanners::{Hint, Look};

/// The findings about `look`.
pub(super) fn findings(look: &Look, findings: &mut Vec<Finding>) {
    let plugged = !look.usb_scanners.is_empty() || !look.ipp_usb_devices.is_empty();
    let before = findings.len();
    // As if SANE listed a scanner: whether it does isn't asked here, so no hint is given for
    // an empty list.
    for hint in look.hints(true, &[]) {
        let (code, severity) = match &hint {
            Hint::NoScanimage { .. } if !plugged => continue,
            Hint::NoScanimage { .. } => (Code::ScanimageMissing, Severity::Problem),
            Hint::NoAirscan { .. } | Hint::NotListed { .. } => continue,
            Hint::IppUsb { .. } => (Code::IppUsbNotRunning, Severity::Problem),
            Hint::Access(_) => (Code::ScannerAccess, Severity::Problem),
        };
        findings.push(Finding::new(code, severity, hint.text()));
    }
    if look.scanimage && !look.airscan {
        findings.push(
            Finding::new(
                Code::AirscanMissing,
                Severity::Warning,
                msg!("sane-airscan isn't installed, so SANE doesn't see driverless scanners."),
            )
            .with_hint(msg!(
                "Most scanners today are driverless (eSCL, also on USB through ipp-usb): install \
                 the package sane-airscan."
            )),
        );
    }
    if look.scanimage && findings.len() == before {
        findings.push(Finding::new(
            Code::ScannersReady,
            Severity::Ok,
            Text::new(msg!(
                "SANE's scanimage is installed; the Printers & Scanners page lists the scanners."
            )),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scanners::look::Denied;

    fn codes(look: &Look) -> Vec<(Code, Severity)> {
        let mut found = Vec::new();
        findings(look, &mut found);
        found
            .iter()
            .map(|finding| (finding.code, finding.severity))
            .collect()
    }

    #[test]
    fn a_machine_without_scanners_and_sane_has_nothing_to_report() {
        assert_eq!(codes(&Look::default()), []);
    }

    #[test]
    fn what_is_missing_is_reported() {
        let ready = Look {
            scanimage: true,
            airscan: true,
            ..Look::default()
        };
        assert_eq!(codes(&ready), [(Code::ScannersReady, Severity::Ok)]);
        let no_airscan = Look {
            airscan: false,
            ..ready.clone()
        };
        assert_eq!(
            codes(&no_airscan),
            [(Code::AirscanMissing, Severity::Warning)]
        );
        let plugged = Look {
            scanimage: false,
            usb_scanners: vec!["CanoScan LiDE 110".to_owned()],
            denied: vec![Denied {
                name: "CanoScan LiDE 110".to_owned(),
                group: Some("scanner".to_owned()),
            }],
            ipp_usb_devices: vec!["HP LaserJet MFP M28w".to_owned()],
            ..ready
        };
        assert_eq!(
            codes(&plugged),
            [
                (Code::ScanimageMissing, Severity::Problem),
                (Code::IppUsbNotRunning, Severity::Problem),
                (Code::ScannerAccess, Severity::Problem),
            ]
        );
    }
}
