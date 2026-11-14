<p align="center">
  <img
    src="data/icons/hicolor/scalable/apps/io.github.mowi12.SwayThings.svg"
    width="128"
    height="128"
    alt="Sway Things app icon"
  >
</p>

<p align="center">
  <a href="https://github.com/mowi12/sway-things/actions/workflows/ci.yml">
    <img src="https://github.com/mowi12/sway-things/actions/workflows/ci.yml/badge.svg?branch=main" alt="CI status">
  </a>
  <a href="https://github.com/mowi12/sway-things/releases">
    <img src="https://img.shields.io/github/v/release/mowi12/sway-things?include_prereleases" alt="Latest release">
  </a>
  <a href="LICENSES/GPL-3.0-or-later.txt">
    <img src="https://img.shields.io/badge/license-GPL--3.0--or--later-blue" alt="License: GPL-3.0-or-later">
  </a>
  <a href="https://build.opensuse.org/package/show/home:mowi12:sway-things/sway-things">
    <img
      src="https://build.opensuse.org/projects/home:mowi12:sway-things/packages/sway-things/badge.svg?type=default"
      alt="openSUSE Build Service status"
    >
  </a>
  <a href="https://api.reuse.software/info/github.com/mowi12/sway-things">
    <img src="https://api.reuse.software/badge/github.com/mowi12/sway-things" alt="REUSE status">
  </a>
</p>

# Sway Things

A settings app for the [sway](https://swaywm.org) Wayland compositor and the tools around it.

Sway Things gives sway the familiar shape of a desktop settings app: a searchable sidebar of pages, controls that apply
instantly, and a safety net for risky changes. It doesn't pull in a desktop environment, and it doesn't take over your
hand-written config.

> **Status: 0.1.0, the first release.** It covers the pages listed under [Features](#features); packages are in
> [Installation](#installation), and the [changes](CHANGELOG.md) per release are in the changelog.

## Why

Sway is configured entirely through text files. That's great, until you want to try a different pointer speed, find the
XKB option that turns Caps Lock into Escape, or arrange three monitors. Today's graphical tools each cover one area, or
come as an all-in-one shell that rewrites your config.

Sway Things covers sway itself, its usual companion tools (kanshi, waybar, swayidle, swaylock, notification daemons, …)
and the everyday system basics (sound, Bluetooth, Wi-Fi, power) in one window.

## Principles

- **Your config stays yours.** Sway Things never edits a file it didn't create. It writes its own include files, and you
  add a single `include` line to your sway config yourself. The app shows you exactly which line.
- **Instant, with a safety net.** Changes apply live. Risky ones (display mode, turning off an output, keyboard layout)
  revert after 15 seconds unless you press _Keep_.
- **Transparent.** See every file the app generates, which of your own config lines conflict with a setting, and reset
  any setting to its default.
- **Made for sway.** Built for sway and SwayFX, not ported from another desktop. It follows your GTK theme (light or
  dark), and its accent color is your sway focus color.
- **No root, no daemon.** It runs as your user. System changes go through polkit, and nothing keeps running in the
  background.
- **Shareable.** One settings file you can keep in your dotfiles, with optional per-machine overrides.

## Features

The first release, v0.1, brings the pages **Mouse & Touchpad**, **Keyboard**, **Windows** (with the SwayFX effects) and
**Window Behavior**, the view of the generated config, and the app's own preferences on About & Maintenance. The other
pages below are planned.

### Devices

- **Mouse & Touchpad:** speed, acceleration, scrolling, tap to click, per-device settings, a test area
- **Keyboard:** layouts and variants, layout switching shortcut, Caps Lock and other XKB options, key repeat
- **Cursor:** cursor theme and size, hiding the cursor
- **Displays:** drag-and-drop arrangement, resolution, scale, rotation, profiles (kanshi), lid handling
- **Sound:** output and input devices, volume, per-app volume
- **Bluetooth & Network:** pairing devices, Wi-Fi

### Appearance

- **Windows:** focus colors, borders, gaps, title bars, color themes, SwayFX effects (blur, shadows, rounded corners)
- **Wallpaper:** per output, with a folder browser
- **Apps Look & Feel:** GTK and Qt themes, icons, fonts, dark mode
- **Bar:** waybar or swaybar layout, modules and style

### Desktop

- **Window Behavior** · **Workspaces** · **Keyboard Shortcuts** (with conflict detection and a printable cheat sheet)
- **Notifications** (Do Not Disturb) · **Apps & Autostart** · **Screenshots & Clipboard**

### System

- **Power & Lock:** idle timeouts, lock screen, power profile, battery
- **Brightness & Night Light** · **Region & Time** · **Accessibility**
- **Environment:** Wayland compatibility toggles, environment variables
- **About & Maintenance:** versions, dependency check, generated config, export / import

The complete list is in the [Feature Catalog](wiki/Feature-Catalog.md).

## Also a command line tool

Everything the app can set is also scriptable:

```sh
sway-things open keyboard                 # open the app on a page
sway-things set input.touchpad.tap true   # change a setting
sway-things get windows.gaps.inner        # read a setting
sway-things doctor                        # check setup, missing tools and conflicts
```

## Requirements

- sway ≥ 1.10, or SwayFX ≥ 0.5
- GTK ≥ 4.18
- Later pages will use optional companion tools, for example kanshi, swayidle, swaylock, waybar, mako or swaync,
  gammastep or wlsunset, brightnessctl, PipeWire, NetworkManager and BlueZ. Pages for missing tools stay visible and
  tell you how to install them.

## Installation

Packages for openSUSE Tumbleweed (x86_64), Fedora, Debian 13 and Ubuntu 26.04 (x86_64 and aarch64) come from the
[openSUSE Build Service](https://build.opensuse.org) project `home:mowi12:sway-things`. Fedora Rawhide and Debian
unstable are built too, on a best-effort basis: their packages may lag behind when those distributions change under
them. The AUR for Arch Linux is planned.

### openSUSE Tumbleweed

```sh
repo=https://download.opensuse.org/repositories/home:/mowi12:/sway-things
sudo zypper addrepo "$repo/openSUSE_Tumbleweed/home:mowi12:sway-things.repo"
sudo zypper install sway-things
```

### Fedora

```sh
repo=https://download.opensuse.org/repositories/home:/mowi12:/sway-things
sudo dnf config-manager addrepo --from-repofile="$repo/Fedora_$(rpm -E %fedora)/home:mowi12:sway-things.repo"
sudo dnf install sway-things
```

On Fedora Rawhide, use `Fedora_Rawhide` instead of `Fedora_$(rpm -E %fedora)`.

Both ask you to trust the repository's signing key on first use.

### Debian and Ubuntu

```sh
distribution=Debian_13 # or Debian_Unstable, xUbuntu_26.04
repo=https://download.opensuse.org/repositories/home:/mowi12:/sway-things/$distribution
curl -fsSL "$repo/Release.key" | gpg --dearmor | sudo tee /etc/apt/keyrings/sway-things.gpg >/dev/null
echo "deb [signed-by=/etc/apt/keyrings/sway-things.gpg] $repo/ /" | sudo tee /etc/apt/sources.list.d/sway-things.list
sudo apt update
sudo apt install sway-things
```

### From source

You need Rust 1.85 or newer, the GTK 4.18 development files, gettext and [just](https://just.systems/):

```sh
just build
sudo just install      # sudo just uninstall removes it again
```

Without `just`, run `scripts/build.sh` and `scripts/install.sh` (`--uninstall`) directly. `PREFIX` defaults to
`/usr/local`, and `DESTDIR` installs into a staging directory. Packages use `PREFIX=/usr` for both steps. fish only
finds the completions under `/usr/local` if your session's `XDG_DATA_DIRS` includes `/usr/local/share`.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). The developer documentation lives in the [wiki](wiki/Home.md).

## License

[GPL-3.0-or-later](LICENSE). Every file declares its copyright and license following [REUSE](https://reuse.software/).
