// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The scanners SANE lists: `scanimage -f` with a format of our own, and the lines of
//! `scanimage -L` (``device `NAME' is a VENDOR MODEL TYPE``).
//!
//! What a scanner says about itself, and the names sane-airscan makes of what the network
//! announces, are data: they are cleaned of control characters and only ever shown as text.

use super::clean;
use crate::i18n::Text;
use crate::msg;

/// The field separator of our `-f` format: a character no scanner puts into its name.
pub(super) const SEPARATOR: char = '\u{1f}';

/// The format `scanimage -f` prints each scanner in: device name, vendor, model and type,
/// separated by [`SEPARATOR`], one scanner a line.
pub(super) fn format() -> String {
    format!("%d{SEPARATOR}%v{SEPARATOR}%m{SEPARATOR}%t%n")
}

/// A scanner SANE lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scanner {
    /// Its SANE device name (`airscan:e0:Office`, `pixma:04A9176D_4F0BA3`), which scanimage
    /// takes with `-d`.
    pub device: String,
    /// Its vendor (`Canon`); sane-airscan says the protocol here (`eSCL`, `WSD`).
    pub vendor: String,
    /// Its model (`CanoScan LiDE 300`); sane-airscan puts the name the scanner announces on the
    /// network here.
    pub model: String,
    /// What SANE says it is (`flatbed scanner`, `virtual device`); sane-airscan puts the
    /// address here (`ip=192.0.2.7`).
    pub kind: String,
}

/// How a scanner is connected, as its SANE device name tells.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Connection {
    /// Driverless over the network (eSCL or WSD, through sane-airscan or SANE's escl backend).
    Driverless {
        /// The protocol: `eSCL` or `WSD`.
        protocol: &'static str,
    },
    /// Driverless over USB, made reachable by ipp-usb on this computer.
    DriverlessUsb,
    /// On USB, with a backend for its model.
    Usb,
    /// On the network, with a backend for its model or through another computer's saned.
    Network,
    /// A camera (v4l), which SANE lists too.
    Camera,
    /// Something else, such as SANE's test backend.
    Other,
}

impl Connection {
    /// How it is connected, in words.
    pub fn text(&self) -> Text {
        match self {
            Self::Driverless { protocol } => {
                Text::new(msg!("Network, driverless ({protocol})")).arg("protocol", protocol)
            }
            Self::DriverlessUsb => msg!("USB, driverless").into(),
            Self::Usb => msg!("USB").into(),
            Self::Network => msg!("Network").into(),
            Self::Camera => msg!("Camera").into(),
            Self::Other => msg!("Other connection").into(),
        }
    }
}

impl Scanner {
    /// Its SANE backend: what comes before the first `:` of its device name.
    pub fn backend(&self) -> &str {
        self.device
            .split_once(':')
            .map_or(self.device.as_str(), |(backend, _)| backend)
    }

    /// How it is connected.
    pub fn connection(&self) -> Connection {
        let device = self.device.to_ascii_lowercase();
        match self.backend() {
            "airscan" => {
                let protocol = if device.starts_with("airscan:w") {
                    "WSD"
                } else {
                    "eSCL"
                };
                if self.address().is_some_and(|address| is_loopback(&address)) {
                    Connection::DriverlessUsb
                } else {
                    Connection::Driverless { protocol }
                }
            }
            "escl" => {
                if self.address().is_some_and(|address| is_loopback(&address)) {
                    Connection::DriverlessUsb
                } else {
                    Connection::Driverless { protocol: "eSCL" }
                }
            }
            "v4l" => Connection::Camera,
            "test" => Connection::Other,
            "net" => Connection::Network,
            _ if device.contains("usb") => Connection::Usb,
            _ if device.contains("net") || device.contains("tcp") || device.contains("://") => {
                Connection::Network
            }
            // Most backends of scanners with a driver of their own name USB devices by their
            // bus position or vendor and product IDs.
            _ => Connection::Usb,
        }
    }

    /// Its address on the network, if its name or model says: sane-airscan's `ip=`, or the
    /// host of an escl URL.
    pub fn address(&self) -> Option<String> {
        if let Some(ip) = self.kind.trim().strip_prefix("ip=") {
            let ip = ip.split([',', ' ']).next().unwrap_or(ip);
            return (!ip.is_empty()).then(|| ip.to_owned());
        }
        // In `-L`'s lines, the type follows the model.
        if let Some((_, ip)) = self.model.rsplit_once(" ip=") {
            let ip = ip.split_whitespace().next().unwrap_or(ip);
            return (!ip.is_empty()).then(|| ip.to_owned());
        }
        let rest = self.device.strip_prefix("escl:")?;
        let (_, rest) = rest.split_once("://")?;
        let authority = rest.split('/').next().unwrap_or(rest);
        let host = match authority.strip_prefix('[') {
            Some(v6) => v6.split(']').next().unwrap_or(v6),
            None => authority
                .rsplit_once(':')
                .map_or(authority, |(host, _)| host),
        };
        (!host.is_empty()).then(|| host.to_owned())
    }

    /// Its title: vendor and model, without the protocol and address sane-airscan puts there.
    pub fn title(&self) -> String {
        let model = self
            .model
            .rsplit_once(" ip=")
            .map_or(self.model.as_str(), |(model, _)| model)
            .trim();
        let vendor = self.vendor.trim();
        let protocol = matches!(vendor, "eSCL" | "WSD" | "ESCL");
        if vendor.is_empty()
            || protocol
            || model
                .to_ascii_lowercase()
                .starts_with(&vendor.to_ascii_lowercase())
        {
            if model.is_empty() {
                return self.device.clone();
            }
            return model.to_owned();
        }
        if model.is_empty() {
            return vendor.to_owned();
        }
        format!("{vendor} {model}")
    }

    /// Whether it is a camera rather than a scanner: SANE's v4l backend lists webcams.
    pub fn is_camera(&self) -> bool {
        self.connection() == Connection::Camera
    }
}

/// Whether `address` is this computer: ipp-usb serves USB devices there.
fn is_loopback(address: &str) -> bool {
    let address = address.trim_matches(['[', ']']);
    address == "localhost"
        || address.starts_with("127.")
        || address == "::1"
        || address.eq_ignore_ascii_case("ip6-localhost")
}

/// `name` as a device name to pass to scanimage as it is: `None` if empty or with control
/// characters, which no backend names a device with.
fn device_name(name: &str) -> Option<String> {
    (!name.is_empty() && !name.chars().any(char::is_control)).then(|| name.to_owned())
}

/// The scanners in what `scanimage -f` printed with [`format`]: lines that aren't ours (SANE's
/// note when there are none, a backend's messages) are left out, and so is a device twice.
pub fn parse_formatted(output: &str) -> Vec<Scanner> {
    let mut scanners: Vec<Scanner> = Vec::new();
    for line in output.lines() {
        let fields: Vec<&str> = line.split(SEPARATOR).collect();
        let [device, vendor, model, kind] = fields.as_slice() else {
            continue;
        };
        let Some(device) = device_name(device) else {
            continue;
        };
        if scanners.iter().any(|known| known.device == device) {
            continue;
        }
        scanners.push(Scanner {
            device,
            vendor: clean(vendor),
            model: clean(model),
            kind: clean(kind),
        });
    }
    scanners
}

/// The scanners in what `scanimage -L` printed: ``device `NAME' is a VENDOR MODEL TYPE`` a
/// line. The name is between the backquote and the last `' is a `; the vendor is the first word after
/// it, and the model and type the rest, which can't be told apart: the model gets all of it.
pub fn parse_list(output: &str) -> Vec<Scanner> {
    let mut scanners: Vec<Scanner> = Vec::new();
    for line in output.lines() {
        let Some(rest) = line.trim().strip_prefix("device `") else {
            continue;
        };
        let Some((device, description)) = rest.rsplit_once("' is a ") else {
            continue;
        };
        let Some(device) = device_name(device) else {
            continue;
        };
        if scanners.iter().any(|known| known.device == device) {
            continue;
        }
        let description = description.trim();
        let (vendor, model) = description.split_once(' ').unwrap_or((description, ""));
        scanners.push(Scanner {
            device,
            vendor: clean(vendor),
            model: clean(model),
            kind: String::new(),
        });
    }
    scanners
}

#[cfg(test)]
mod tests {
    use super::*;

    fn formatted(lines: &[[&str; 4]]) -> String {
        let mut text = String::new();
        for fields in lines {
            text.push_str(&fields.join(&SEPARATOR.to_string()));
            text.push('\n');
        }
        text
    }

    #[test]
    fn formatted_lines_are_read() {
        let output = formatted(&[
            [
                "airscan:e0:HP LaserJet MFP M28w (5E1A2B)",
                "eSCL",
                "HP LaserJet MFP M28w",
                "ip=192.168.1.20",
            ],
            [
                "airscan:w1:Kyocera MFP Scanner",
                "WSD",
                "Kyocera ECOSYS M2040dn",
                "ip=192.168.1.21,fe80::1",
            ],
            [
                "escl:http://127.0.0.1:60000",
                "Brother",
                "MFC-L2710DW",
                "flatbed scanner",
            ],
            [
                "pixma:04A9176D_4F0BA3",
                "CANON",
                "Canon PIXMA MG5700 Series",
                "multi-function peripheral",
            ],
            [
                "epsonds:libusb:001:005",
                "Epson",
                "ES-50",
                "sheetfed scanner",
            ],
            [
                "net:scanserver:genesys:libusb:002:004",
                "Canon",
                "LiDE 220",
                "flatbed scanner",
            ],
            [
                "v4l:/dev/video0",
                "Noname",
                "Integrated Camera",
                "virtual device",
            ],
            ["test:0", "Noname", "frontend-tester", "virtual device"],
        ]);
        let scanners = parse_formatted(&output);
        let seen: Vec<(String, Connection, Option<String>)> = scanners
            .iter()
            .map(|s| (s.title(), s.connection(), s.address()))
            .collect();
        assert_eq!(
            seen,
            [
                (
                    "HP LaserJet MFP M28w".to_owned(),
                    Connection::Driverless { protocol: "eSCL" },
                    Some("192.168.1.20".to_owned())
                ),
                (
                    "Kyocera ECOSYS M2040dn".to_owned(),
                    Connection::Driverless { protocol: "WSD" },
                    Some("192.168.1.21".to_owned())
                ),
                (
                    "Brother MFC-L2710DW".to_owned(),
                    Connection::DriverlessUsb,
                    Some("127.0.0.1".to_owned())
                ),
                (
                    "Canon PIXMA MG5700 Series".to_owned(),
                    Connection::Usb,
                    None
                ),
                ("Epson ES-50".to_owned(), Connection::Usb, None),
                ("Canon LiDE 220".to_owned(), Connection::Network, None),
                (
                    "Noname Integrated Camera".to_owned(),
                    Connection::Camera,
                    None
                ),
                ("Noname frontend-tester".to_owned(), Connection::Other, None),
            ]
        );
        assert_eq!(scanners[3].backend(), "pixma");
        assert!(scanners[6].is_camera());
    }

    #[test]
    fn no_scanners_and_noise_give_an_empty_list() {
        let none = "\nNo scanners were identified. If you were expecting something different,\n\
                    check that the scanner is plugged in, turned on and detected by the\n\
                    sane-find-scanner tool (if appropriate).\n";
        assert_eq!(parse_formatted(none), []);
        assert_eq!(parse_list(none), []);
        assert_eq!(parse_formatted(""), []);
        // A backend's message, a broken line, a device twice, a device without a name.
        let noisy = format!(
            "[bjnp] create_broadcast_socket: bind failed\n{}{}{}{}",
            formatted(&[["test:0", "Noname", "frontend-tester", "virtual device"]]),
            "test:1\u{1f}Noname\n",
            formatted(&[["test:0", "Noname", "frontend-tester", "virtual device"]]),
            formatted(&[["", "Noname", "nothing", "virtual device"]]),
        );
        let scanners = parse_formatted(&noisy);
        assert_eq!(scanners.len(), 1);
        assert_eq!(scanners[0].device, "test:0");
    }

    #[test]
    fn listed_lines_are_read() {
        let output = "device `test:0' is a Noname frontend-tester virtual device\n\
                      device `airscan:e0:Office MFP (it's new)' is a eSCL Office MFP ip=10.0.0.9 scanner\n\
                      device `hpaio:/usb/OfficeJet_Pro_8020?serial=TH12' is a Hewlett-Packard OfficeJet_Pro_8020 all-in-one\n\
                      scanimage: sane_init failed\n";
        let scanners = parse_list(output);
        assert_eq!(scanners.len(), 3);
        assert_eq!(scanners[0].device, "test:0");
        assert_eq!(scanners[0].vendor, "Noname");
        assert_eq!(scanners[1].device, "airscan:e0:Office MFP (it's new)");
        assert_eq!(scanners[1].address().as_deref(), Some("10.0.0.9"));
        assert_eq!(scanners[2].connection(), Connection::Usb);
    }

    #[test]
    fn hostile_names_are_cleaned_or_left_out() {
        let output = formatted(&[
            ["test:0\u{1b}[2J", "Noname", "model", "x"],
            ["test:1", "No\u{7}name", "model\u{0}", "x"],
        ]);
        let scanners = parse_formatted(&output);
        assert_eq!(scanners.len(), 1);
        assert_eq!(scanners[0].device, "test:1");
        assert_eq!(scanners[0].vendor, "No name");
        assert_eq!(scanners[0].model, "model");
    }
}
