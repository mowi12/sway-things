# Vision & Scope

## Problem

Sway is configured entirely through text files and man pages. Experienced users like that, but even they spend time
looking up `pointer_accel` ranges, XKB option names or output mode strings. Newcomers from GNOME, KDE or macOS hit a
wall. The existing graphical tools each cover one area (inputs, displays, waybar), or come as an all-in-one shell that
takes over and rewrites the config.

## Vision

> One settings app for sway, its companion tools and the everyday system basics. It feels like the settings app of a
> mainstream desktop, and it respects a hand-written config completely.

"Feels like GNOME Settings or macOS System Settings" refers to the **structure and behavior** (sidebar of pages, search,
instant apply, safety net), **not the look**. Sway Things follows the user's GTK theme and uses the sway focus color as
its accent.

## Target users

**Both newcomers and experienced users, layered**.

- Experienced users (hand-written config, often in a dotfiles repo) are the **first design check**. If they trust Sway
  Things on their setup, it works for everyone.
- Newcomers get the same app, starting from an empty config.

**Acceptance test for every feature:** a sway veteran can enable Sway Things on an existing dotfiles setup, and not a
single hand-written line changes.

## Goals

1. **Cover sway completely**: every common sway option has a control, live preview where it makes sense, and conflict
   detection against the user's own config.
2. **Cover the companion tools** (kanshi, waybar/swaybar, swayidle, swaylock, notification daemons, night light,
   wallpaper, launchers) through **pluggable, auto-detected backends**.
3. **Cover the system basics natively** (sound, Bluetooth, Wi-Fi, power profile, battery, time zone). "Advanced…"
   buttons hand off to specialist tools.
4. **Be safe**: memory-safe code, no privileged code, no shell strings, never write user files, a 15 s revert for risky
   changes.
5. **Be transparent**: every generated line is visible, every conflict points to file:line, every setting can be reset.
6. **Be packaged properly**: native packages for openSUSE, Fedora, Debian, Ubuntu (via OBS) and Arch (AUR), aiming for
   the official distro repositories after 1.0.

## Non-goals

| Not doing | Why |
| --- | --- |
| Editing the user's sway config or other hand-written files | Core trust promise |
| Other compositors (Hyprland, niri, Wayfire, river) | Focus. SwayFX is the only fork in scope |
| Non-Linux systems, cross-platform toolkits | Sway only runs on Linux (and the BSDs) |
| A resident daemon | Smaller attack surface. Runtime behavior goes to existing tools |
| Root helpers or setuid code | Only polkit-guarded D-Bus services |
| Flatpak | The sandbox would need holes that make it pointless |
| Full network, printer or user-account editors | Delegate to nm-connection-editor, system-config-printer, … |
| Magnifier, sticky keys, color inversion | Not supported by sway/wlroots |
| Comment-preserving edits of user config files | Unnecessary, because we never edit user files |
| A "desktop shell" (own bar, launcher, notification daemon) | We configure existing tools, we don't replace them |

## Scope by sidebar group

| Group | Pages | Depth |
| --- | --- | --- |
| **Devices** | Mouse & Touchpad · Keyboard · Cursor & Tablet · Displays · Sound · Bluetooth & Network | sway-native pages are complete; Sound and Bluetooth & Network cover the basics natively, then delegate |
| **Appearance** | Windows (incl. SwayFX effects) · Wallpaper · Apps Look & Feel · Bar | complete, through backends |
| **Desktop** | Window Behavior · Workspaces · Keyboard Shortcuts · Notifications · Apps & Autostart · Screenshots & Clipboard | complete, through backends |
| **System** | Power & Lock · Brightness & Night Light · Region & Time · Accessibility · Environment · About & Maintenance | basics natively; anything that needs a root file edit is read-only with instructions |

The item-level scope is in the [Feature Catalog](Feature-Catalog.md).

## Prior art

Researched on 2026-10-17. Not re-verified.

| Project | Covers | Stack | Difference from Sway Things |
| --- | --- | --- | --- |
| [SwaySettings](https://github.com/ErikReider/SwaySettings) | Inputs, keyboard layout, wallpaper, GTK theme, default apps, autostart, lock screen | Vala, GTK4, libadwaita | Similar idea and include pattern; small scope; AUR only |
| [Sway Input Configurator](https://flathub.org/apps/io.github.Sunderland93.sway-input-config) | Keyboard, touchpad, pointer | Python, Qt | Inputs only |
| [nwg-shell](https://wiki.archlinux.org/title/Nwg-shell) | Full GTK shell (displays, look, panel, config) | Python/Go, GTK3 | All-or-nothing; the installer can overwrite the sway config |
| nwg-displays, wdisplays | Monitor arrangement | GTK | Displays only |
| [kanshi_gui](https://github.com/nurkert/kanshi_gui) | kanshi profiles, layout editor, workspace placement, lid handling | Flutter | By nurkert. **Inspiration** for the Displays page |
| [vsWaybar Studio](https://aur.archlinux.org/packages/vswaybar-studio) | Visual waybar editor | Python, GTK, WebKit | Waybar only, very new |

**What makes Sway Things different:** one app for sway, its ecosystem and the system basics. It never takes over your
config, it's built for safety (Rust, no root, no daemon), and it's packaged for the major distros.
