// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Changes for users CUPS doesn't let make them: cups-pk-helper on the system bus
//! (`org.opensuse.CupsPkHelper.Mechanism`), which polkit guards. polkit's agent asks for an
//! administrator's password; Sway Things never sees it.

use std::path::Path;

use zbus::proxy::MethodFlags;

use super::{Authorization, Error};
use crate::i18n::Text;
use crate::msg;

/// cups-pk-helper's bus name, path and interface.
const SERVICE: &str = "org.opensuse.CupsPkHelper.Mechanism";
/// Its object.
const PATH: &str = "/";

/// The bus where cups-pk-helper and Avahi are.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Bus {
    /// The system bus, where it runs.
    #[default]
    System,
    /// Another bus, with a stand-in for it (tests).
    Address(String),
    /// None: changes CUPS refuses stay refused, and no printers are found.
    Off,
}

/// The groups CUPS lets change printers on the distributions, in the order to suggest them:
/// Debian and Ubuntu use `lpadmin`, the others `wheel` or `sys`.
const GROUPS: [&str; 3] = ["lpadmin", "wheel", "sys"];

/// The group to suggest joining: the first of [`GROUPS`] in `/etc/group`.
pub(super) fn authorization() -> Authorization {
    authorization_in(Path::new("/etc/group"))
}

fn authorization_in(file: &Path) -> Authorization {
    let text = std::fs::read_to_string(file).unwrap_or_default();
    let exists = |group: &str| {
        text.lines()
            .any(|line| line.split(':').next() == Some(group))
    };
    let group = GROUPS
        .into_iter()
        .find(|group| exists(group))
        .unwrap_or(GROUPS[0]);
    Authorization {
        group: group.to_owned(),
    }
}

impl Bus {
    /// Calls cups-pk-helper's `method` with `args`; it answers with an error text, empty when it
    /// worked. polkit may ask the user for a password first, which the call waits for.
    pub(super) async fn call<B>(&self, method: &str, args: &B) -> Result<(), Error>
    where
        B: serde::Serialize + zbus::zvariant::DynamicType,
    {
        let not_allowed = || Error::NotAllowed(authorization());
        let connection = match self {
            Self::Off => return Err(not_allowed()),
            Self::System => zbus::Connection::system().await,
            Self::Address(address) => match zbus::connection::Builder::address(address.as_str()) {
                Ok(builder) => builder.build().await,
                Err(error) => Err(error),
            },
        }
        .map_err(|_| not_allowed())?;
        let proxy = zbus::Proxy::new(&connection, SERVICE, PATH, SERVICE)
            .await
            .map_err(|_| not_allowed())?;
        let reply: Result<Option<String>, zbus::Error> = proxy
            .call_with_flags(method, MethodFlags::AllowInteractiveAuth.into(), args)
            .await;
        match reply {
            Ok(None) => Ok(()),
            Ok(Some(error)) if error.is_empty() => Ok(()),
            Ok(Some(error)) => Err(Error::Failed(Text::data(super::clean(&error)))),
            Err(zbus::Error::MethodError(name, message, _)) => {
                let name = name.as_str();
                if crate::dbus::is_gone(name) {
                    Err(not_allowed())
                } else if name.ends_with("NotPrivileged")
                    || name.starts_with("org.freedesktop.PolicyKit1.Error")
                    || name == "org.freedesktop.DBus.Error.AccessDenied"
                {
                    Err(Error::Denied(super::clean(&message.unwrap_or_default())))
                } else {
                    Err(Error::Failed(
                        Text::new(msg!("cups-pk-helper failed: {error}"))
                            .arg("error", super::clean(&message.unwrap_or_default())),
                    ))
                }
            }
            Err(error) => Err(Error::Failed(
                Text::new(msg!("cups-pk-helper failed: {error}")).arg("error", error),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_group_to_suggest_is_one_that_exists() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("group");
        std::fs::write(&file, "root:x:0:\nwheel:x:10:alice\nsys:x:3:\n").unwrap();
        assert_eq!(authorization_in(&file).group, "wheel");
        std::fs::write(&file, "lpadmin:x:120:\nwheel:x:10:\n").unwrap();
        assert_eq!(authorization_in(&file).group, "lpadmin");
        assert_eq!(
            authorization_in(&dir.path().join("missing")).group,
            "lpadmin"
        );
    }
}
