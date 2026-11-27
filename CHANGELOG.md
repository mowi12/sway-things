# Changelog

All notable changes to Sway Things, written for users. git-cliff drafts each section from the Conventional Commits.

## 0.6.0 (2026-11-27)

A new Bar page for waybar and swaybar, and three System pages: Brightness & Night Light, Region & Time and Environment.
Printers became Printers & Scanners and scans documents into files.

### Pages

- **Bar:** set up waybar, or swaybar as the alternative, from a preview of the bar over your wallpaper. Add items from a
  picker with search and a sample of each, drag them between the Left, Center and Right lanes (or move them with Alt and
  the arrow keys), and change each item's basic settings in plain words, with every option under _Advanced_. Group items
  into one pill, with a drawer that shows the others only on hover. Pick a look (Simple, GNOME-like, Floating islands,
  Minimal, Large & high contrast, Like your windows, Classic Waybar, Glass on SwayFX), the size and the position, and
  show the bar always or only while a key is held. Your own commands run as items with a test run, and templates cover a
  pomodoro timer with its phases, the camera, updates, a battery warning, VPN, system load, a power menu and the
  notification center. Your existing waybar config can be imported as a starting point. Sway Things' bar can take the
  place of the `bar { … }` block of your config without editing it, and _Switch Back to My Own Bar_ brings yours back.
- **Usage limits of Claude Code and Codex in the bar:** three templates in the picker's _Custom_ category show how much
  of their usage limits the two command line agents have used, each on its own or as one pill that always shows your
  main agent and the other one only at 80 % or at its limit; hovering shows the details and a click refreshes. These
  templates, and the check for updates, reach the network, so the page asks before it adds them and says what they send
  where and how often. Waybar runs them, never Sway Things. Each sends your login only to its own vendor and asks it at
  most every five minutes.
- **Brightness & Night Light:** the brightness of the screen, of external monitors through ddcutil and of the keyboard
  light, also as `sway-things brightness`; a night light with wlsunset or gammastep, from sunset to sunrise where you
  are, at fixed times or always on, also as `sway-things night-light`.
- **Region & Time:** the time zone, automatic time and the clock through systemd-timedated; a time zone that follows
  where the computer is, through geoclue and off by default; and the language and formats for you, with a preview, and
  for the whole system through systemd-localed.
- **Environment:** the Wayland variables for older Qt 5, SDL 2, Java and Electron apps, each falling back to X11; an
  editor for your environment variables in Sway Things' own `environment.d` file, with the variables of the other files
  shown read only; a check of the XDG desktop portals for screen sharing and file pickers, with fixes in Sway Things'
  own files; and the choice of how screen sharing picks a display.
- **Printers & Scanners:** the Printers page, redesigned after GNOME Settings, now lists the scanners SANE finds and the
  driverless ones on the network, says what is missing for the others (scanimage, sane-airscan, ipp-usb, access to a USB
  scanner), and scans into PNG, JPEG, TIFF or PDF, with a preview and every sheet of the document feeder in one PDF. A
  printer's queue opens from the jobs on its line, and new printers are added unshared. `sway-things open printers`
  still opens the page.

### Displays and Wallpaper

- The Wallpaper page shows the displays in the same arrangement and look as the Displays page.
- The Wallpaper page's tabs, and the text of rows beside their controls, get the width they need.

### Bluetooth and Network

- Bluetooth shows whether a device is charging, from UPower or the kernel, and its battery level no longer jumps after
  charging.
- A VPN that fails before it is connected says why.

### Screenshots & Clipboard

- Recording the screen works where ffmpeg has no libx264: wf-recorder is asked for a codec and falls back to the next.
- A second press of the record key while a region is being selected no longer starts a second recording.
- The window no longer freezes for up to three seconds after the clipboard history is switched.

### App and command line

- `sway-things doctor` checks the XDG desktop portals in an Environment section, names UPower in the system details, and
  reports what is missing for scanners.

### Packages

- Same distributions as 0.5: openSUSE Tumbleweed, Fedora 43 and 44, Debian 13 and Ubuntu 26.04 from the openSUSE Build
  Service; Fedora Rawhide and Debian unstable on a best-effort basis.
- The new pages use optional tools when installed and say what to install otherwise: waybar, curl and jq for the usage
  templates, ddcutil, wlsunset or gammastep, geoclue, systemd's timedated and localed, xdg-desktop-portal-wlr and
  xdg-desktop-portal-gtk, SANE's scanimage with sane-airscan and ipp-usb, and UPower.

## 0.5.0 (2026-11-23)

Five new pages for the desktop around your windows: Keyboard Shortcuts, Screenshots & Clipboard, Apps & Autostart,
Notifications and Printers. Every key binding in one place, the daily tools set up from the app, and the printers on the
network added in a click.

### Pages

- **Keyboard Shortcuts:** every key binding sway has, yours with the file and line they come from and those of Sway
  Things, in words grouped by what they do, and searchable by keys or command. Add, change or remove a binding by
  pressing the new keys; a binding of your own config is changed or removed in Sway Things' files, never in yours, and
  the row says so, with a way back. Two bindings on the same keys are shown as a conflict. Ready-made choices cover the
  media keys (volume, brightness, playback), with an on-screen display through SwayOSD, and the main modifier of the
  bindings Sway Things makes.
- **Screenshots & Clipboard:** keys for a screenshot of an area, a window, a display or all displays (grim and slurp),
  copied, saved to a folder with a name pattern of your choice, or opened in swappy or satty; a clipboard history with
  cliphist and a key that picks from it; and a key that starts and stops a screen recording (wf-recorder or
  wl-screenrec). The same works from the command line: `sway-things screenshot`, `record` and `clipboard pick`.
- **Apps & Autostart:** the terminal and the launcher, taken from the installed ones, with their keys; the default apps
  (web browser, file manager, mail, image viewer, PDF viewer, video and music player, text editor) in Sway Things' own
  `sway-mimeapps.list`, so your `mimeapps.list` stays as it is; and one list of everything that starts at login: the
  entries of Sway Things, the `exec` lines of your config, and the XDG autostart entries, with a note and the fix when
  nothing in your session starts the latter.
- **Notifications:** Do Not Disturb for mako, swaync and dunst, also as `sway-things notifications dnd` for a key or a
  bar. The page says which daemon runs and what starts it, can start one with sway, and sends a test notification. For
  mako and swaync it sets the position, timeout, size, font and colors (matching the windows or your own), swaync's
  panel with its widgets and buttons, and whether clicking a notification brings its app to the front; an existing
  swaync config can be imported. On SwayFX the notifications can be blurred behind.
- **Printers:** the printers of CUPS with their state, ink and toner levels and print queue; set the default, pause and
  resume, rename, share, print a test page, cancel jobs, and remove a printer. Driverless printers on the network or on
  USB are found and added in a click, or by their address. Changes work as a member of CUPS's administrator group or
  through cups-pk-helper; _Advanced…_ hands printers that need a driver to system-config-printer or CUPS's web
  interface.

### Bluetooth

- A connection or pairing that takes long can be canceled from the device's row.
- Turning Bluetooth off during a connection attempt cancels the attempt first, so the switch no longer hangs
  half-changed.

### App and command line

- Resetting a single setting brings back the value of your own config, not sway's default: gaps set in your config no
  longer drop to 0.
- `sway-things doctor` names CUPS and cups-pk-helper in the system details, reports the notification daemon that runs
  and what starts it, and says when nothing starts the XDG autostart entries.

### Packages

- Same distributions as 0.4: openSUSE Tumbleweed, Fedora 43 and 44, Debian 13 and Ubuntu 26.04 from the openSUSE Build
  Service; Fedora Rawhide and Debian unstable on a best-effort basis.
- The new pages use optional tools when installed and say what to install otherwise: grim, slurp, wl-clipboard,
  cliphist, swappy or satty, wf-recorder or wl-screenrec, SwayOSD, dex, mako, swaync or dunst, CUPS with cups-pk-helper
  and Avahi, and system-config-printer.

## 0.4.0 (2026-11-23)

A new Wallpaper page, VPN sign-ins inside the app, and displays that look after themselves: a mirror that comes back by
itself, one question when a projector is plugged in, and workspaces placed while your own kanshi profiles set up the
displays.

### Pages

- **Wallpaper:** a picture or a color for all displays or for each one, with a preview of the displays as they are
  arranged. Pick from your picture folder (the folder is a preference, `~/Pictures` by default) or any image, choose how
  it fits, or make a collection of pictures and folders that take turns at every login, daily or after a while. _Next
  Picture_ and `sway-things wallpaper next` move on at once. The _Default_ tile brings back the wallpaper of your own
  config. When your config starts a wallpaper tool such as swaybg, swww or wpaperd, the page names the line instead of
  fighting it, and `sway-things doctor` reports a `swaybg_command` that draws no wallpaper.

### Displays and Workspaces

- A mirrored display comes back by itself when wl-mirror crashed or its window was closed, and `sway-things hook mirror`
  can be bound to a key to bring it back at once.
- When a display Sway Things has never seen is plugged in, such as the projector at a meeting, a notification asks once
  whether it mirrors or extends the desktop, and the answer is remembered.
- The workspaces are placed on the displays even while your own kanshi profiles set them up: Sway Things follows your
  profiles instead of replacing them.

### Network

- A VPN that needs a password or other secrets to connect asks for them in a dialog of the app, with the fields its VPN
  plugin describes.
- OpenConnect VPNs sign in with openconnect itself, step by step in one dialog, including servers that ask for a
  one-time code and sign-ins in the browser.

### App and command line

- _Reset page…_ brings back the values of your own config at once, instead of sway's defaults until the next reload.
- `sway-things get` shows the value in effect, as the app does, and lists the values of one display under its name.
- The notices of every page look alike.
- Writing a file still works after Sway Things crashed while writing it, and the generated config that lands is exactly
  the one `sway --validate` checked.

### Packages

- Same distributions as 0.3: openSUSE Tumbleweed, Fedora 43 and 44, Debian 13 and Ubuntu 26.04 from the openSUSE Build
  Service; Fedora Rawhide and Debian unstable on a best-effort basis.
- The packages recommend the gdk-pixbuf loader for WebP wallpapers (webp-pixbuf-loader) where the distribution has it;
  the Wallpaper page warns when it is missing.

## 0.3.0 (2026-11-20)

Five new pages for the devices and the look of the desktop: Displays, Workspaces, Sound, Cursor & Tablet and Apps Look &
Feel. And one command installs Sway Things on every supported distribution.

### Pages

- **Displays:** arrange your displays by dragging them, with snapping, and set the resolution, refresh rate, scale and
  rotation of each, or switch one off. Changes apply live, a new mode or a display switched off reverts after 15 seconds
  unless you keep it, and the last display can't be switched off. _Identify_ shows each display's number on it. Every
  combination of displays is remembered on its own as a kanshi profile, without naming or picking profiles, and a
  display can mirror another with wl-mirror.
- **Workspaces:** place the numbered workspaces on your displays by a rule (sway's own, round robin, a block per
  display, all on the built-in one) or by hand on a map of the displays, remembered per combination. A workspace moves
  to the next display when its own is unplugged, and back when it returns. The settings for new workspaces moved here
  from Window Behavior.
- **Sound:** output and input devices, volume and mute, the microphone level with a meter, the volume of each app that
  plays or records, and device profiles (HDMI, Bluetooth headsets). _Advanced…_ opens pwvucontrol or pavucontrol, and a
  preference allows volumes up to 150 %.
- **Cursor & Tablet:** the cursor theme with a preview and its sizes, hiding the cursor while typing or after a while,
  and mapping a drawing tablet or touchscreen to one display, with absolute or relative mode.
- **Apps Look & Feel:** the GTK theme, the icon theme with a few of its icons, the interface, document and monospace
  fonts, dark or light, font rendering and the text size, for GTK apps through GSettings.

### Bluetooth and Network

- Searching for Bluetooth devices and Wi-Fi networks starts with _Search_, or automatically while the page is open, as a
  preference sets.
- The Bluetooth adapter can be renamed and made visible to other devices.
- A placeholder replaces the list while Bluetooth or Wi-Fi is off.

### App

- Every page fits a third of a laptop screen.
- Lines separate the groups of the sidebar, and the search field and the back button.
- Every message of the app and the command line is ready for translation.

### Packages

- One command finds your distribution, adds the repository after checking its signing key, and installs Sway Things:
  `curl -fsSL https://sway-things.org/install | sh`. `--dry-run` shows what it would do, `--uninstall` removes it all
  again.
- Same distributions as 0.2: openSUSE Tumbleweed, Fedora 43 and 44, Debian 13 and Ubuntu 26.04 from the openSUSE Build
  Service; Fedora Rawhide and Debian unstable on a best-effort basis.
- Now links gtk4-layer-shell and libpulse. The Sound page uses the PulseAudio API (pipewire-pulse or PulseAudio); kanshi
  and wl-mirror are optional, for display profiles and mirroring.

## 0.2.1 (2026-11-15)

A packaging fix: the openSUSE Build Service couldn't download the 0.2.0 sources and built no packages. The app is the
same as in 0.2.0.

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
