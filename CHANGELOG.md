# Changelog

All notable changes to Sway Things, written for users. git-cliff drafts each section from the Conventional Commits.

## 0.2.0 (2026-11-15)

Bluetooth and Wi-Fi join the app: two new pages that talk to BlueZ and NetworkManager directly, and system details for
bug reports in the app and in `sway-things doctor`.

### Pages

- **Bluetooth:** turn the radio on and off, see your devices with their state and battery level, and find new ones while
  the page is open. _Connect_ on a new device pairs, trusts and connects it in one step, with dialogs for comparing or
  typing a code. Devices can be renamed and forgotten, a soft block of the radio is lifted for you, and a computer with
  two adapters gets a choice between them.
- **Network:** Wi-Fi on and off, your known networks (also those out of range) and the other networks nearby with signal
  and security, connecting with a password dialog, hidden networks, and a note with a login button behind a captive
  portal. Wired connections and VPNs (also WireGuard) each get a switch, and the ⋮ menu shows a connection's details or
  forgets it.
- What these pages don't cover opens in the specialist tool: blueman for Bluetooth, nm-connection-editor for Network
  (802.1X networks, VPN settings, IP settings).

### Bug reports

- About & Maintenance lists the system details: the versions of Sway Things, the compositor, GTK, the distribution,
  NetworkManager and the kernel, and whether blueman and nm-connection-editor are installed. _Copy System Details_
  copies them as text.
- `sway-things doctor` starts its report with the same block (also in `--json`), and the bug form now just asks for its
  output.
- About & Maintenance also links the website, the source code and the issue tracker.

### Website

- Sway Things has a home at [sway-things.org](https://sway-things.org), with screenshots of every released page in light
  and dark.

### Packages

- openSUSE Tumbleweed, Fedora 43 and 44, Debian 13 and Ubuntu 26.04 from the openSUSE Build Service; Fedora Rawhide and
  Debian unstable on a best-effort basis.
- Requires sway 1.10 or newer, or SwayFX 0.5 or newer, and GTK 4.18 or newer. The Bluetooth page uses BlueZ, the Network
  page NetworkManager.

## 0.1.0 (2026-11-14)

The first release: a settings app for sway and SwayFX that never edits your hand-written config. It writes sway config
files of its own, which your config includes with one line, and applies every change live.

### Pages

- **Mouse & Touchpad:** pointer speed and acceleration profile, natural scrolling, scroll speed and method, left-handed
  buttons, middle-click emulation; for touchpads also tap to click, tap and drag, drag lock, disable while typing and
  the click method. Settings apply to all devices of a type or to one device, and a device can be turned off.
- **Keyboard:** layouts and variants in a reorderable list, keyboard model, XKB options (such as Caps Lock as Escape),
  key repeat delay and rate, Num Lock and Caps Lock at start, and switching the active layout.
- **Windows:** the colors of focused, unfocused, urgent and tabbed windows, borders, gaps, title bars and font. On
  SwayFX also rounded corners, blur, shadows, dimming of inactive windows and animations.
- **Window Behavior:** focus follows mouse, mouse warping, focus wrapping, workspace layout and orientation, the
  floating modifier, tiling drag, behavior in fullscreen, and Xwayland.

### Safe and transparent

- Sway Things stays read-only and shows the line to add until your config includes its files.
- Risky changes (keyboard layouts, turning off a device) revert after 15 seconds unless you keep them, and the last
  pointer or keyboard can't be turned off.
- Lines of your own config that override a setting are shown with file and line, and open in your editor.
- Every generated file can be viewed before it is written, and every setting can be reset to its default.
- Values can be stored for one machine only, next to the settings you share through your dotfiles.

### App and command line

- A searchable sidebar, keyboard navigation, a narrow mode for half-screen windows, light and dark, and the sway focus
  color as the accent.
- `sway-things get`, `set`, `reset`, `show-generated`, `doctor` and `open`, with man pages and completions for bash, zsh
  and fish.

### Packages

- openSUSE Tumbleweed, Fedora 43 and 44, Debian 13 and Ubuntu 26.04 from the openSUSE Build Service; Fedora Rawhide and
  Debian unstable on a best-effort basis.
- Requires sway 1.10 or newer, or SwayFX 0.5 or newer, and GTK 4.18 or newer.
