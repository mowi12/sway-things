# Sway Things

A settings app for the [sway](https://swaywm.org) Wayland compositor and the tools around it.

Sway Things gives sway the familiar shape of a desktop settings app: a searchable sidebar of pages, controls that apply
instantly, and a safety net for risky changes. It doesn't pull in a desktop environment, and it doesn't take over your
hand-written config.

> **Status: planning.** There is no code to install yet. See the [Roadmap](wiki/Roadmap.md) for what comes first.

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
- **Made for sway.** Built for sway and SwayFX, not ported from another desktop. It follows your GTK theme, and its
  accent color is your sway focus color.
- **No root, no daemon.** It runs as your user. System changes go through polkit, and nothing keeps running in the
  background.
- **Shareable.** One settings file you can keep in your dotfiles, with optional per-machine overrides.

## Planned features

**Devices**

- **Mouse & Touchpad:** speed, acceleration, scrolling, tap to click, per-device settings, a test area
- **Keyboard:** layouts and variants, layout switching shortcut, Caps Lock and other XKB options, key repeat
- **Cursor:** cursor theme and size, hiding the cursor
- **Displays:** drag-and-drop arrangement, resolution, scale, rotation, profiles (kanshi), lid handling
- **Sound:** output and input devices, volume, per-app volume
- **Bluetooth & Network:** pairing devices, Wi-Fi

**Appearance**

- **Windows:** focus colors, borders, gaps, title bars, color themes, SwayFX effects (blur, shadows, rounded corners)
- **Wallpaper:** per output, with a folder browser
- **Apps Look & Feel:** GTK and Qt themes, icons, fonts, dark mode
- **Bar:** waybar or swaybar layout, modules and style

**Desktop**

- **Window Behavior** · **Workspaces** · **Keyboard Shortcuts** (with conflict detection and a printable cheat sheet)
- **Notifications** (Do Not Disturb) · **Apps & Autostart** · **Screenshots & Clipboard**

**System**

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
sway-things get appearance.gaps.inner     # read a setting
sway-things doctor                        # check setup, missing tools and conflicts
```

_(Planned interface. Key names may still change.)_

## Requirements (planned)

- sway ≥ 1.10, or SwayFX ≥ 0.5
- GTK 4
- Optional companion tools unlock their pages, for example kanshi, swayidle, swaylock, waybar, mako or swaync, gammastep
  or wlsunset, brightnessctl, PipeWire, NetworkManager and BlueZ. Pages for missing tools stay visible and tell you how
  to install them.

## Installation

Not released yet. Planned: native packages for openSUSE, Fedora, Debian and Ubuntu via the openSUSE Build Service, plus
the AUR for Arch Linux.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). The developer documentation lives in the [wiki](wiki/Home.md).

## License

[GPL-3.0-or-later](LICENSE). Every file declares its copyright and license following [REUSE](https://reuse.software/).
