// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What this computer has for scanners, and the hints for what is missing:
//!
//! - scanimage (sane-utils on Debian and Ubuntu, sane-backends on Fedora and openSUSE, sane on
//!   Arch and Void), without which nothing is found;
//! - sane-airscan, through which SANE sees driverless scanners (eSCL, WSD), most of today's;
//! - ipp-usb, which makes a USB device that speaks IPP over USB (USB interface class 7, subclass
//!   1, protocol 4) reachable for sane-airscan and CUPS;
//! - the access to a USB scanner SANE knows (libsane's udev hwdb or rules mark it
//!   `libsane_matched`): its device file in `/dev/bus/usb` must be readable and writable, which
//!   systemd-logind gives the user at the computer's screen (`uaccess`), and some distributions
//!   a group (`scanner` on Debian, Ubuntu and Void, `lp` on openSUSE).
//!
//! Everything is read from files (`/usr/lib`, `/proc`, `/sys`, `/dev`, `/etc`), below a root
//! that tests set to a fake installation; nothing is run.

use std::collections::HashSet;
use std::ffi::OsStr;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use crate::delegate::find_program;
use crate::i18n::Text;
use crate::msg;

/// The family of the distribution, for the names of the packages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Family {
    /// Debian and Ubuntu.
    Debian,
    /// Fedora.
    Fedora,
    /// openSUSE.
    Suse,
    /// Arch Linux.
    Arch,
    /// Void Linux.
    Void,
    /// Another.
    #[default]
    Other,
}

impl Family {
    /// The family `os-release` (its `ID` and `ID_LIKE`) names.
    pub fn from_os_release(text: &str) -> Self {
        let mut ids = Vec::new();
        for line in text.lines() {
            if let Some(value) = line
                .strip_prefix("ID=")
                .or_else(|| line.strip_prefix("ID_LIKE="))
            {
                ids.extend(
                    value
                        .trim_matches(['"', '\''])
                        .split_whitespace()
                        .map(str::to_owned),
                );
            }
        }
        let has = |id: &str| ids.iter().any(|known| known == id);
        if has("debian") || has("ubuntu") {
            Self::Debian
        } else if has("fedora") || has("rhel") {
            Self::Fedora
        } else if has("opensuse") || has("suse") || ids.iter().any(|id| id.starts_with("opensuse"))
        {
            Self::Suse
        } else if has("arch") {
            Self::Arch
        } else if has("void") {
            Self::Void
        } else {
            Self::Other
        }
    }

    /// The package with scanimage.
    pub fn scanimage_package(self) -> &'static str {
        match self {
            Self::Debian => "sane-utils",
            Self::Arch | Self::Void => "sane",
            Self::Fedora | Self::Suse | Self::Other => "sane-backends",
        }
    }
}

/// A USB device the user can't use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Denied {
    /// Its name, from its maker and product.
    pub name: String,
    /// The group its device file belongs to, if not `root`.
    pub group: Option<String>,
}

/// What this computer has for scanners.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "each is a program that is there or not, which the hints look at one by one"
)]
pub struct Look {
    /// The family of the distribution.
    pub family: Family,
    /// Whether scanimage is installed.
    pub scanimage: bool,
    /// Whether sane-airscan is installed.
    pub airscan: bool,
    /// Whether ipp-usb is installed.
    pub ipp_usb: bool,
    /// Whether ipp-usb runs.
    pub ipp_usb_running: bool,
    /// The USB devices plugged in that speak IPP over USB, by name.
    pub ipp_usb_devices: Vec<String>,
    /// The USB scanners SANE knows that are plugged in, by name.
    pub usb_scanners: Vec<String>,
    /// Those of them the user can't use.
    pub denied: Vec<Denied>,
}

impl Look {
    /// Looks at this computer, with scanimage and ipp-usb looked up in `PATH`.
    pub fn read() -> Self {
        Self::read_at(Path::new("/"), std::env::var_os("PATH").as_deref())
    }

    /// Looks at the installation below `root`, with programs looked up in `path`.
    pub fn read_at(root: &Path, path: Option<&OsStr>) -> Self {
        let family = ["etc/os-release", "usr/lib/os-release"]
            .iter()
            .find_map(|file| fs::read_to_string(root.join(file)).ok())
            .map(|text| Family::from_os_release(&text))
            .unwrap_or_default();
        let ipp_usb = find_program("ipp-usb", path).is_some()
            || ["usr/sbin/ipp-usb", "usr/bin/ipp-usb"]
                .iter()
                .any(|file| root.join(file).is_file());
        let devices = usb_devices(root);
        let matched = libsane_ids(root);
        let mut usb_scanners = Vec::new();
        let mut denied = Vec::new();
        for device in devices.iter().filter(|device| matched.contains(&device.id)) {
            usb_scanners.push(device.name.clone());
            if let Some(node) = &device.node {
                if !accessible(node) {
                    denied.push(Denied {
                        name: device.name.clone(),
                        group: group_of(root, node),
                    });
                }
            }
        }
        Self {
            family,
            scanimage: find_program("scanimage", path).is_some(),
            airscan: has_backend(root, "airscan"),
            ipp_usb,
            ipp_usb_running: runs(root, "ipp-usb"),
            ipp_usb_devices: devices
                .iter()
                .filter(|device| device.ipp_usb)
                .map(|device| device.name.clone())
                .collect(),
            usb_scanners,
            denied,
        }
    }

    /// The hints for what is missing: `seen` whether SANE lists a scanner, and `unseen` the
    /// driverless scanners Avahi finds on the network that SANE doesn't list, by name.
    pub fn hints(&self, seen: bool, unseen: &[String]) -> Vec<Hint> {
        let mut hints = Vec::new();
        if !self.scanimage {
            hints.push(Hint::NoScanimage {
                package: self.family.scanimage_package(),
            });
        }
        if !self.airscan && self.scanimage && (!unseen.is_empty() || !seen) {
            hints.push(Hint::NoAirscan {
                unseen: unseen.to_vec(),
            });
        } else if self.airscan && !unseen.is_empty() {
            hints.push(Hint::NotListed {
                unseen: unseen.to_vec(),
            });
        }
        if !self.ipp_usb_devices.is_empty() && !self.ipp_usb_running {
            hints.push(Hint::IppUsb {
                devices: self.ipp_usb_devices.clone(),
                installed: self.ipp_usb,
            });
        }
        hints.extend(self.denied.iter().cloned().map(Hint::Access));
        hints
    }
}

/// What is missing for scanners, and what to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Hint {
    /// scanimage isn't installed.
    NoScanimage {
        /// The package with it.
        package: &'static str,
    },
    /// sane-airscan isn't installed: the driverless scanners Avahi finds aren't usable, or none
    /// is found at all.
    NoAirscan {
        /// The scanners Avahi finds that SANE doesn't list.
        unseen: Vec<String>,
    },
    /// sane-airscan is installed, but SANE doesn't list scanners Avahi finds.
    NotListed {
        /// Those scanners.
        unseen: Vec<String>,
    },
    /// A device that speaks IPP over USB is plugged in, but ipp-usb doesn't run.
    IppUsb {
        /// The devices.
        devices: Vec<String>,
        /// Whether ipp-usb is installed.
        installed: bool,
    },
    /// The user can't use a USB scanner.
    Access(Denied),
}

impl Hint {
    /// The hint in words.
    pub fn text(&self) -> Text {
        match self {
            Self::NoScanimage { package } => Text::new(msg!(
                "SANE's scanimage isn't installed, so no scanner can be found or used. Install \
                 the package {package}."
            ))
            .arg("package", package),
            Self::NoAirscan { unseen } if unseen.is_empty() => Text::new(msg!(
                "Most scanners today are driverless, and SANE finds them with sane-airscan, \
                 which isn't installed. If your scanner isn't listed, install the package \
                 sane-airscan."
            )),
            Self::NoAirscan { unseen } => Text::new(msg!(
                "{scanners} on the network can't be used yet: SANE needs sane-airscan for \
                 driverless scanners. Install the package sane-airscan."
            ))
            .arg("scanners", unseen.join(", ")),
            Self::NotListed { unseen } => Text::new(msg!(
                "{scanners} is on the network, but SANE doesn't list it. Search again; if it \
                 stays missing, check that sane-airscan is switched on in /etc/sane.d/dll.d."
            ))
            .arg("scanners", unseen.join(", ")),
            Self::IppUsb {
                devices,
                installed: true,
            } => Text::new(msg!(
                "{devices} is plugged in over USB and works driverless, but ipp-usb doesn't \
                 run. Plug it in again, or start ipp-usb: systemctl start ipp-usb."
            ))
            .arg("devices", devices.join(", ")),
            Self::IppUsb { devices, .. } => Text::new(msg!(
                "{devices} is plugged in over USB and works driverless with ipp-usb, which isn't \
                 installed. Install the package ipp-usb, then plug it in again."
            ))
            .arg("devices", devices.join(", ")),
            Self::Access(Denied {
                name,
                group: Some(group),
            }) => Text::new(msg!(
                "You can't use {scanner}: its device file belongs to the group “{group}”. Join \
                 it (as an administrator: usermod -aG {group} YOUR-NAME) and log in again."
            ))
            .arg("scanner", name)
            .arg("group", group),
            Self::Access(Denied { name, group: None }) => Text::new(msg!(
                "You can't use {scanner}: only the user logged in at this computer's screen may. \
                 Log in there, or log out and in again."
            ))
            .arg("scanner", name),
        }
    }
}

/// A USB device plugged in.
#[derive(Debug, Clone)]
struct UsbDevice {
    /// Its vendor and product ID, lowercase hex: `04a9:1909`.
    id: String,
    /// Its name, from its maker and product, else its ID.
    name: String,
    /// Its device file.
    node: Option<PathBuf>,
    /// Whether one of its interfaces is IPP over USB.
    ipp_usb: bool,
}

/// The USB devices in `root/sys/bus/usb/devices`.
fn usb_devices(root: &Path) -> Vec<UsbDevice> {
    let base = root.join("sys/bus/usb/devices");
    let Ok(entries) = fs::read_dir(&base) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    let read = |dir: &Path, file: &str| {
        fs::read_to_string(dir.join(file))
            .ok()
            .map(|text| super::clean(&text))
            .filter(|text| !text.is_empty())
    };
    let mut devices = Vec::new();
    for name in names.iter().filter(|name| !name.contains(':')) {
        let dir = base.join(name);
        let (Some(vendor), Some(product)) = (read(&dir, "idVendor"), read(&dir, "idProduct"))
        else {
            continue;
        };
        let id = format!(
            "{}:{}",
            vendor.to_ascii_lowercase(),
            product.to_ascii_lowercase()
        );
        let label = match (read(&dir, "manufacturer"), read(&dir, "product")) {
            (Some(maker), Some(product)) if !product.starts_with(&maker) => {
                format!("{maker} {product}")
            }
            (_, Some(product)) => product,
            (Some(maker), None) => maker,
            (None, None) => id.clone(),
        };
        let node = read(&dir, "busnum")
            .zip(read(&dir, "devnum"))
            .and_then(|(bus, dev)| Some((bus.parse::<u32>().ok()?, dev.parse::<u32>().ok()?)))
            .map(|(bus, dev)| root.join(format!("dev/bus/usb/{bus:03}/{dev:03}")));
        let prefix = format!("{name}:");
        let ipp_usb = names
            .iter()
            .filter(|other| other.starts_with(&prefix))
            .any(|interface| {
                let interface = base.join(interface);
                read(&interface, "bInterfaceClass").as_deref() == Some("07")
                    && read(&interface, "bInterfaceSubClass").as_deref() == Some("01")
                    && read(&interface, "bInterfaceProtocol").as_deref() == Some("04")
            });
        devices.push(UsbDevice {
            id,
            name: label,
            node,
            ipp_usb,
        });
    }
    devices
}

/// The USB IDs (`04a9:1909`) libsane's udev hwdb and rules mark as scanners.
fn libsane_ids(root: &Path) -> HashSet<String> {
    let mut ids = HashSet::new();
    for base in ["usr/lib/udev", "lib/udev", "etc/udev"] {
        for (dir, suffix) in [("hwdb.d", ".hwdb"), ("rules.d", ".rules")] {
            let Ok(entries) = fs::read_dir(root.join(base).join(dir)) else {
                continue;
            };
            for entry in entries.filter_map(Result::ok) {
                let name = entry.file_name().to_string_lossy().into_owned();
                if !name.contains("sane") || !name.ends_with(suffix) {
                    continue;
                }
                let Ok(text) = fs::read_to_string(entry.path()) else {
                    continue;
                };
                for line in text.lines() {
                    if let Some(id) = hwdb_id(line).or_else(|| rule_id(line)) {
                        ids.insert(id);
                    }
                }
            }
        }
    }
    ids
}

/// The ID of a hwdb match: `usb:v04A9p1909*`.
fn hwdb_id(line: &str) -> Option<String> {
    let rest = line.trim().strip_prefix("usb:v")?;
    let (vendor, rest) = rest.split_once('p')?;
    let product = rest.trim_end_matches('*');
    let hex = |text: &str| text.len() == 4 && text.chars().all(|c| c.is_ascii_hexdigit());
    (hex(vendor) && hex(product)).then(|| {
        format!(
            "{}:{}",
            vendor.to_ascii_lowercase(),
            product.to_ascii_lowercase()
        )
    })
}

/// The ID of a udev rule: `ATTRS{idVendor}=="04a9", ATTRS{idProduct}=="1909"`.
fn rule_id(line: &str) -> Option<String> {
    let value = |key: &str| {
        let rest = line.split(key).nth(1)?;
        let rest = rest.trim_start_matches(['=', '"', ' ']);
        let value: String = rest.chars().take_while(char::is_ascii_hexdigit).collect();
        (value.len() == 4).then(|| value.to_ascii_lowercase())
    };
    Some(format!(
        "{}:{}",
        value("ATTRS{idVendor}")?,
        value("ATTRS{idProduct}")?
    ))
}

/// Whether the user may read and write `node`.
fn accessible(node: &Path) -> bool {
    use rustix::fs::{Access, access};
    access(node, Access::READ_OK | Access::WRITE_OK).is_ok()
}

/// The name of the group of `node`, from `root/etc/group`; `None` for `root` or unknown.
fn group_of(root: &Path, node: &Path) -> Option<String> {
    let gid = fs::metadata(node).ok()?.gid();
    if gid == 0 {
        return None;
    }
    let groups = fs::read_to_string(root.join("etc/group")).ok()?;
    groups.lines().find_map(|line| {
        let mut fields = line.split(':');
        let name = fields.next()?;
        let id: u32 = fields.nth(1)?.parse().ok()?;
        (id == gid).then(|| super::clean(name))
    })
}

/// Whether SANE's backend `name` is installed: `libsane-NAME.so` in one of the places the
/// distributions keep the backends.
fn has_backend(root: &Path, name: &str) -> bool {
    let file = format!("libsane-{name}.so");
    let mut dirs = vec![
        root.join("usr/lib/sane"),
        root.join("usr/lib64/sane"),
        root.join("usr/local/lib/sane"),
    ];
    // Debian's and Ubuntu's multiarch directories.
    if let Ok(entries) = fs::read_dir(root.join("usr/lib")) {
        dirs.extend(
            entries
                .filter_map(Result::ok)
                .map(|entry| entry.path().join("sane"))
                .filter(|dir| dir.is_dir()),
        );
    }
    dirs.iter().any(|dir| {
        fs::read_dir(dir).is_ok_and(|entries| {
            entries
                .filter_map(Result::ok)
                .any(|entry| entry.file_name().to_string_lossy().starts_with(&file))
        })
    })
}

/// Whether a process named `name` runs, from `root/proc/*/comm`.
fn runs(root: &Path, name: &str) -> bool {
    let Ok(entries) = fs::read_dir(root.join("proc")) else {
        return false;
    };
    entries.filter_map(Result::ok).any(|entry| {
        entry
            .file_name()
            .to_string_lossy()
            .bytes()
            .all(|b| b.is_ascii_digit())
            && fs::read_to_string(entry.path().join("comm"))
                .is_ok_and(|comm| comm.trim_end() == name)
    })
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    /// A fake installation: the files at their paths with their texts.
    fn install(files: &[(&str, &str)]) -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        for (path, text) in files {
            let path = root.path().join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, text).unwrap();
            if path.parent().is_some_and(|dir| dir.ends_with("bin")) {
                fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
            }
        }
        root
    }

    /// The `PATH` of a fake installation.
    fn path(root: &Path) -> std::ffi::OsString {
        std::env::join_paths([root.join("usr/bin"), root.join("usr/sbin")]).unwrap()
    }

    /// A USB device in sysfs, with its interfaces of class, subclass and protocol.
    fn usb(
        name: &str,
        id: (&str, &str),
        product: &str,
        interfaces: &[(&str, &str, &str)],
    ) -> Vec<(String, String)> {
        let base = format!("sys/bus/usb/devices/{name}");
        let mut files = vec![
            (format!("{base}/idVendor"), format!("{}\n", id.0)),
            (format!("{base}/idProduct"), format!("{}\n", id.1)),
            (format!("{base}/manufacturer"), "Canon\n".to_owned()),
            (format!("{base}/product"), format!("{product}\n")),
            (format!("{base}/busnum"), "1\n".to_owned()),
            (format!("{base}/devnum"), "5\n".to_owned()),
        ];
        for (i, (class, subclass, protocol)) in interfaces.iter().enumerate() {
            let interface = format!("sys/bus/usb/devices/{name}:1.{i}");
            files.push((format!("{interface}/bInterfaceClass"), format!("{class}\n")));
            files.push((
                format!("{interface}/bInterfaceSubClass"),
                format!("{subclass}\n"),
            ));
            files.push((
                format!("{interface}/bInterfaceProtocol"),
                format!("{protocol}\n"),
            ));
        }
        files
    }

    fn borrowed(files: &[(String, String)]) -> Vec<(&str, &str)> {
        files
            .iter()
            .map(|(a, b)| (a.as_str(), b.as_str()))
            .collect()
    }

    #[test]
    fn nothing_installed_says_to_install_scanimage() {
        let root = install(&[("etc/os-release", "NAME=Debian\nID=debian\n")]);
        let look = Look::read_at(root.path(), Some(&path(root.path())));
        assert_eq!(look.family, Family::Debian);
        assert!(!look.scanimage && !look.airscan && !look.ipp_usb);
        let hints = look.hints(false, &[]);
        assert_eq!(
            hints,
            [Hint::NoScanimage {
                package: "sane-utils"
            }]
        );
        assert!(hints[0].text().english().contains("sane-utils"));
    }

    #[test]
    fn driverless_scanners_need_sane_airscan() {
        let root = install(&[
            ("etc/os-release", "ID=fedora\n"),
            ("usr/bin/scanimage", ""),
            ("usr/lib64/sane/libsane-escl.so.1", ""),
        ]);
        let look = Look::read_at(root.path(), Some(&path(root.path())));
        assert!(look.scanimage && !look.airscan);
        assert_eq!(
            look.hints(false, &[]),
            [Hint::NoAirscan { unseen: Vec::new() }]
        );
        // A scanner SANE lists, and none on the network it misses: nothing to say.
        assert_eq!(look.hints(true, &[]), []);
        let unseen = vec!["HP LaserJet MFP M28w".to_owned()];
        let hints = look.hints(true, &unseen);
        assert_eq!(
            hints,
            [Hint::NoAirscan {
                unseen: unseen.clone()
            }]
        );
        assert!(
            hints[0]
                .text()
                .english()
                .starts_with("HP LaserJet MFP M28w on the network")
        );
    }

    #[test]
    fn sane_airscan_is_found_in_multiarch_directories() {
        let root = install(&[
            ("etc/os-release", "ID=ubuntu\nID_LIKE=debian\n"),
            ("usr/bin/scanimage", ""),
            ("usr/lib/x86_64-linux-gnu/sane/libsane-airscan.so.1", ""),
        ]);
        let look = Look::read_at(root.path(), Some(&path(root.path())));
        assert!(look.airscan);
        assert_eq!(look.hints(false, &[]), []);
        let unseen = vec!["Office".to_owned()];
        assert_eq!(look.hints(true, &unseen), [Hint::NotListed { unseen }]);
    }

    #[test]
    fn an_ipp_usb_device_needs_ipp_usb_running() {
        let mut files = usb(
            "1-2",
            ("03f0", "2b2a"),
            "HP LaserJet MFP M28w",
            &[("ff", "04", "01"), ("07", "01", "04")],
        );
        files.push(("usr/bin/scanimage".to_owned(), String::new()));
        files.push((
            "usr/lib/sane/libsane-airscan.so.1".to_owned(),
            String::new(),
        ));
        files.push(("etc/os-release".to_owned(), "ID=arch\n".to_owned()));
        let root = install(&borrowed(&files));
        let look = Look::read_at(root.path(), Some(&path(root.path())));
        assert_eq!(look.ipp_usb_devices, ["Canon HP LaserJet MFP M28w"]);
        assert_eq!(
            look.hints(true, &[]),
            [Hint::IppUsb {
                devices: vec!["Canon HP LaserJet MFP M28w".to_owned()],
                installed: false
            }]
        );
        // Installed and running: nothing to say.
        files.push(("usr/sbin/ipp-usb".to_owned(), String::new()));
        files.push(("proc/412/comm".to_owned(), "ipp-usb\n".to_owned()));
        let root = install(&borrowed(&files));
        let look = Look::read_at(root.path(), Some(&path(root.path())));
        assert!(look.ipp_usb && look.ipp_usb_running);
        assert_eq!(look.hints(true, &[]), []);
    }

    #[test]
    fn a_usb_scanner_without_access_says_which_group() {
        let mut files = usb(
            "1-3",
            ("04a9", "1909"),
            "CanoScan LiDE 110",
            &[("ff", "ff", "ff")],
        );
        files.push(("usr/bin/scanimage".to_owned(), String::new()));
        files.push((
            "usr/lib/sane/libsane-airscan.so.1".to_owned(),
            String::new(),
        ));
        files.push((
            "usr/lib/udev/hwdb.d/20-sane.hwdb".to_owned(),
            "# CanoScan LiDE 110\nusb:v04A9p1909*\n libsane_matched=yes\n".to_owned(),
        ));
        files.push(("dev/bus/usb/001/005".to_owned(), String::new()));
        // The device file is the test's, so its group is the test's, named "scanner" here.
        let own_gid = rustix::process::getgid().as_raw();
        files.push((
            "etc/group".to_owned(),
            format!("root:x:0:\nscanner:x:{own_gid}:\n"),
        ));
        let root = install(&borrowed(&files));
        let node = root.path().join("dev/bus/usb/001/005");
        let look = Look::read_at(root.path(), Some(&path(root.path())));
        // Readable and writable: nothing to say.
        assert_eq!(look.usb_scanners, ["Canon CanoScan LiDE 110"]);
        assert_eq!(look.denied, []);
        fs::set_permissions(&node, fs::Permissions::from_mode(0o000)).unwrap();
        let look = Look::read_at(root.path(), Some(&path(root.path())));
        if rustix::process::geteuid().is_root() {
            return;
        }
        assert_eq!(
            look.denied,
            [Denied {
                name: "Canon CanoScan LiDE 110".to_owned(),
                group: (own_gid != 0).then(|| "scanner".to_owned()),
            }]
        );
        let text = look.hints(true, &[])[0].text().english();
        assert!(
            text.starts_with("You can't use Canon CanoScan LiDE 110"),
            "{text}"
        );
    }

    #[test]
    fn udev_rules_and_hwdb_lines_are_read() {
        assert_eq!(hwdb_id("usb:v04A9p1909*"), Some("04a9:1909".to_owned()));
        assert_eq!(hwdb_id(" libsane_matched=yes"), None);
        assert_eq!(hwdb_id("usb:v04A9p19*"), None);
        assert_eq!(
            rule_id(
                r#"ATTRS{idVendor}=="04a9", ATTRS{idProduct}=="1909", MODE="0664", GROUP="scanner""#
            ),
            Some("04a9:1909".to_owned())
        );
        assert_eq!(rule_id(r#"ENV{libsane_matched}=="yes", GROUP="lp""#), None);
    }

    #[test]
    fn families_are_told_from_os_release() {
        let family = |text: &str| Family::from_os_release(text);
        assert_eq!(family("ID=debian"), Family::Debian);
        assert_eq!(
            family("ID=linuxmint\nID_LIKE=\"ubuntu debian\""),
            Family::Debian
        );
        assert_eq!(
            family("ID=\"opensuse-tumbleweed\"\nID_LIKE=\"opensuse suse\""),
            Family::Suse
        );
        assert_eq!(family("ID=fedora"), Family::Fedora);
        assert_eq!(family("ID=arch"), Family::Arch);
        assert_eq!(family("ID=void"), Family::Void);
        assert_eq!(family("ID=nixos"), Family::Other);
        assert_eq!(Family::Suse.scanimage_package(), "sane-backends");
        assert_eq!(Family::Arch.scanimage_package(), "sane");
    }
}
