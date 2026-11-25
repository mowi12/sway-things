// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The command that installs packages on this distribution, for the notes of a page about a
//! program that is missing: zypper (openSUSE), dnf (Fedora), apt (Debian, Ubuntu) or pacman
//! (Arch), told by `ID` and `ID_LIKE` of os-release(5). Only shown, never run.

use std::path::Path;

use crate::system::{OS_RELEASE, os_release_field};

/// A package manager of the distributions Sway Things is packaged for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Manager {
    /// openSUSE.
    Zypper,
    /// Fedora.
    Dnf,
    /// Debian and Ubuntu.
    Apt,
    /// Arch Linux.
    Pacman,
}

impl Manager {
    /// The manager of this system, from its os-release file; `None` for another distribution.
    pub fn detect() -> Option<Self> {
        let text = OS_RELEASE
            .iter()
            .find_map(|file| std::fs::read_to_string(Path::new(file)).ok())?;
        Self::of(&text)
    }

    /// The manager of the distribution the os-release text `text` describes: by its `ID`, else
    /// the first of `ID_LIKE` that is known.
    pub fn of(text: &str) -> Option<Self> {
        let id = os_release_field(text, "ID").unwrap_or_default();
        let like = os_release_field(text, "ID_LIKE").unwrap_or_default();
        std::iter::once(id.as_str())
            .chain(like.split_whitespace())
            .find_map(|id| match id {
                "opensuse" | "opensuse-tumbleweed" | "opensuse-leap" | "suse" | "sles" => {
                    Some(Self::Zypper)
                }
                "fedora" | "rhel" | "centos" => Some(Self::Dnf),
                "debian" | "ubuntu" => Some(Self::Apt),
                "arch" | "archlinux" | "manjaro" | "endeavouros" => Some(Self::Pacman),
                _ => None,
            })
    }

    /// The command that installs `packages`, to show: `sudo zypper install wlsunset`.
    pub fn install_command(self, packages: &[&str]) -> String {
        let command = match self {
            Self::Zypper => "sudo zypper install",
            Self::Dnf => "sudo dnf install",
            Self::Apt => "sudo apt install",
            Self::Pacman => "sudo pacman -S",
        };
        let mut line = command.to_owned();
        for package in packages {
            line.push(' ');
            line.push_str(package);
        }
        line
    }
}

#[cfg(test)]
mod tests {
    use super::Manager;

    #[test]
    fn the_manager_follows_id_then_id_like() {
        assert_eq!(
            Manager::of("ID=\"opensuse-tumbleweed\"\nID_LIKE=\"opensuse suse\"\n"),
            Some(Manager::Zypper)
        );
        assert_eq!(Manager::of("ID=fedora\n"), Some(Manager::Dnf));
        assert_eq!(Manager::of("ID=debian\n"), Some(Manager::Apt));
        assert_eq!(
            Manager::of("ID=linuxmint\nID_LIKE=\"ubuntu debian\"\n"),
            Some(Manager::Apt)
        );
        assert_eq!(Manager::of("ID=arch\n"), Some(Manager::Pacman));
        assert_eq!(Manager::of("ID=gentoo\n"), None);
    }

    #[test]
    fn the_command_names_every_package() {
        assert_eq!(
            Manager::Pacman.install_command(&["wlsunset", "ddcutil"]),
            "sudo pacman -S wlsunset ddcutil"
        );
        assert_eq!(
            Manager::Zypper.install_command(&["gammastep"]),
            "sudo zypper install gammastep"
        );
    }
}
