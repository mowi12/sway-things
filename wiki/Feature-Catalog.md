# Feature Catalog

The complete planned scope, per page, **unordered**, and the global status of every item. The next item to build is
picked from this list; open a GitHub issue when you start or plan it (see [Roadmap](Roadmap.md)).

## Status legend

- [x] Done: released, or merged for the next release
- [ ] Open: planned, not built yet
- ~~Discarded~~: dropped, with the reason after it (e.g. "doesn't work on sway"). The checkbox is removed

Tick an item in the PR that implements it, and strike it through in the PR or issue that drops it.

## Effort legend

- 🟢 Easy: native sway option, applied live over IPC and generated into our include
- 🟡 Medium: another tool's config, D-Bus, parsing, or a custom widget
- 🔴 Hard: big effort, fragile, or only partly possible on sway

Each page also lists **Maybe** items: ideas that weren't selected, kept for later discussion.

---

## Devices

### Mouse & Touchpad

`man sway-input`. Settings apply to all devices of a type (`type:pointer`, `type:touchpad`) or to one device (per-device
scope → host overlay by default).

- [x] 🟢 Pointer speed (`pointer_accel`, −1.0 … 1.0)
- [x] 🟢 Acceleration profile: flat / adaptive (`accel_profile`)
- [x] 🟢 Natural scrolling (`natural_scroll`)
- [x] 🟢 Scroll speed (`scroll_factor`)
- [x] 🟢 Left-handed mode (`left_handed`)
- [x] 🟢 Middle-click emulation (`middle_emulation`)
- [x] 🟢 Scroll method: two-finger / edge (touchpads) / on-button-down / none (`scroll_method`, `scroll_button`)
- [x] 🟢 Touchpad: tap to click (`tap`), tap button map lrm / lmr (`tap_button_map`)
- [x] 🟢 Touchpad: tap-and-drag, drag lock (`drag`, `drag_lock`; sticky drag lock needs sway ≥ 1.11)
- [x] 🟢 Touchpad: disable while typing / while trackpointing (`dwt`, `dwtp`)
- [x] 🟢 Touchpad: click method, button areas vs. clickfinger (`click_method`, `clickfinger_button_map`)
- [x] 🟢 Disable touchpad while an external mouse is connected (`events disabled_on_external_mouse`)
- [x] 🟢 Enable / disable individual devices (`events`), **risky → safety net**, the last pointing device is protected
- [x] 🟢 Per-device overrides (device list from `GET_INPUTS`, hotplug via events)
- [x] 🟢 Live test area for clicking and scrolling

### Keyboard

- [x] 🟢 Layouts: add, remove, reorder (`xkb_layout`), **risky → safety net**
- [x] 🟢 Variants, e.g. `nodeadkeys`, `dvorak`, `colemak` (`xkb_variant`)
- [x] 🟢 Layout switching shortcut (`xkb_options grp:*`)
- [x] 🟢 Common XKB options as toggles: Caps→Ctrl, Caps→Esc, swap Ctrl/Alt, Compose key, … (`xkb_options`)
- [x] 🟢 Full XKB option browser (parsed from `/usr/share/X11/xkb/rules/evdev.xml`)
- [x] 🟢 Keyboard model (`xkb_model`)
- [x] 🟢 Repeat delay and rate (`repeat_delay`, `repeat_rate`)
- [x] 🟢 NumLock / CapsLock state at startup (`xkb_numlock`, `xkb_capslock`)
- [x] 🟢 Per-keyboard layouts (e.g. a different layout on an external keyboard)
- [x] 🟢 Switch the active layout from the app (`xkb_switch_layout`)
- [ ] 🟡 Layout preview image (`xkbcli` render or bundled drawing)
- [x] 🟢 "Try it" input field
- **Maybe:** 🟡 custom `.xkb` keymap file (`xkb_file`)

### Cursor & Tablet

- [x] 🟢 Cursor theme and size (`seat * xcursor_theme`, copied to GSettings for GTK apps), with a preview of each theme
      and the sizes it ships
- [x] 🟢 Hide the cursor after inactivity / while typing (`seat * hide_cursor`)
- [x] 🟢 Map a drawing tablet or touchscreen to one screen or all of them (`map_to_output`)
- [x] 🟢 Tablet tool mode absolute / relative (`tool_mode`)
- **Maybe:** 🟢 map to a region (`map_to_region`, `map_from_region`; it shares the mapping with `map_to_output`, so it
  fits the Displays canvas) · 🟢 `XCURSOR_*` in our `environment.d` file for apps started outside sway (→ Environment) ·
  🟡 touchscreen calibration wizard (`calibration_matrix`)

### Displays

The requirements below include the proven ideas from kanshi_gui. Display profile backend: kanshi (reference); without
kanshi, static `output` lines in our include (no profiles).

#### Layout editor

- [x] 🟡 Drag-and-drop arrangement on a canvas, with smart snapping (guide lines, edge/corner/center alignment, a magnet
      that backs off)
- [x] 🟢 Resolution and refresh rate (**risky → safety net**)
- [x] 🟢 Scale and rotation per output
- [x] 🟢 Enable / disable outputs (**risky**). The last enabled output is protected
- [x] 🟢 Live apply over IPC, 15 s safety net for risky changes
- [x] 🟢 Hotplug awareness (output events)
- [x] 🟡 Outputs stored by make, model and serial instead of connector (stable across docks and ports); those without a
      serial by connector
- [x] 🟢 Never without a display: while Sway Things stores anything for a display, a hook at every start and reload of
      sway switches one on if none is
- [x] 🟢 Identify displays (numbers flashed on each screen)
- [x] 🟡 Mirror one output onto another (`wl-mirror`), with a scaling mode, remembered per combination

#### Profiles (kanshi backend)

- [x] 🟡 Each combination of displays remembered on its own as a kanshi profile, without naming or picking profiles;
      generated into our kanshi include (`~/.config/kanshi/sway-things.conf`)
- [x] 🟢 A profile that switches on any one display alone, so no combination stays dark
- [x] 🟢 `kanshictl reload` after saving
- [x] 🟢 Our include goes above the user's own kanshi profiles, so the arrangement made here comes first; a warning with
      the command to move it while one of theirs wins for the displays connected
- [x] 🟢 Generate `exec kanshi` into **our** sway include when the user's config doesn't start kanshi

Workspace placement: see [Workspaces](#workspaces).

#### Laptop lid (opt-in)

- [ ] 🟡 Lid closed with another screen active → turn off the built-in panel and move its workspaces; restore on open.
      `bindswitch` lines in our include call `sway-things hook lid closed|open`

**Maybe:** 🟡 adaptive sync / VRR per output · 🟡 sway-only output options (allow tearing, 10-bit, subpixel, scale
filter, ICC profile) · 🟡 custom modes / modelines · 🟡 show which profile is active and why · 🟡 per-profile custom
`exec` commands · 🟡 shikane backend

### Sound

Native basics, delegate advanced cases. Backend: PulseAudio API via pipewire-pulse (or PulseAudio).

- [x] 🟡 Output and input device selection, with the connector (speakers, headphones)
- [x] 🟡 Master volume and mute, microphone level, and a level meter for the input
- [x] 🟡 Per-application volume, for apps that play and apps that record
- [x] 🟡 Device profiles (HDMI, A2DP/HFP for Bluetooth headsets)
- [ ] 🟡 Alert sound volume / event sounds: waits for `libpulse-binding`'s stream-restore write, which passes Rust
      structs to libpulse (see [Architecture → Backends](Architecture.md#backends))
- [x] 🟢 "Advanced…" → pwvucontrol, else pavucontrol
- [x] 🟢 Volume above 100 % (up to 150 %) as an app preference

### Bluetooth

- [x] 🟡 Bluetooth on/off, scan, pair, connect, forget (BlueZ D-Bus). Device names are attacker-controlled data
- [x] 🟡 Pairing dialogs through our own agent: compare a code, type a code or PIN, authorize
- [x] 🟢 Lift an rfkill soft block when switching on; explain a hard block
- [x] 🟢 Several adapters: pick the one the page shows
- [x] 🟢 Battery level of connected devices
- [x] 🟢 Rename a device
- [x] 🟢 Rename the adapter: the name other devices see
- [x] 🟢 Make the computer visible to other devices for a while
- [x] 🟢 "Advanced…" → blueman-manager
- [x] 🟢 Search for devices automatically while the page is open, or only with a Search button (app preference)

### Network

- [x] 🟡 Wi-Fi list and connect (NetworkManager D-Bus). SSIDs are attacker-controlled data
- [x] 🟡 Wired / VPN status; "Advanced…" → `nm-connection-editor`
- [x] 🟡 Asking again for a wrong or changed Wi-Fi password, through our own secret agent
- [x] 🟢 Wi-Fi on/off (`WirelessEnabled`); explain a hardware block
- [x] 🟢 Connect to a hidden network
- [x] 🟢 Forget a saved network; edit a connection in `nm-connection-editor`
- [x] 🟢 Search for networks automatically while the page is open, or only with a Search button (app preference)
- [x] 🟢 Several Wi-Fi adapters: pick the one the page shows
- [x] 🟢 Network details: addresses, gateway, DNS, signal, speed
- [x] 🟢 Captive portal note with the login page
- [x] 🟢 Switches for the wired devices and the VPN and WireGuard connections
- [ ] 🟡 WPA-Enterprise (802.1X) dialog: PEAP and TTLS with MSCHAPv2 or PAP, TLS with certificates, a CA file
- [ ] 🟡 Ask for VPN passwords through the VPN plugin's dialog
- **Not planned:** a full network editor (delegate)

---

## Appearance

### Windows

- [x] 🟢 Focused window colors: border, background, text, indicator, child border (`client.focused`). The child border
      also drives the app accent
- [x] 🟢 Colors for unfocused / focused-inactive / urgent windows and the focused tab title (`client.*`)
- [x] 🟢 Border style: normal / pixel / none, plus width (`default_border`, `default_floating_border`)
- [x] 🟢 Hide edge borders / smart borders (`hide_edge_borders`, `smart_borders`)
- [x] 🟢 Gaps inner/outer (`gaps`)
- [ ] 🟢 Gaps per workspace (`workspace <name> gaps`)
- [x] 🟢 Smart gaps (`smart_gaps`)
- [x] 🟢 Title bar font and size (`font`)
- [x] 🟢 Title alignment and padding (`title_align`, `titlebar_padding`, `titlebar_border_thickness`)
- [ ] 🟢 Color presets (e.g. Catppuccin, Nord, Gruvbox) applied in one click
- [ ] 🟡 Match the accent color to the wallpaper (palette extraction)
- [ ] 🟢 Per-app window opacity (`for_window [...] opacity` for new windows, `[...] opacity` for open ones)
- ~~🟡 Live preview of focused and unfocused windows (colors, borders, gaps, title bars)~~: sway applies these changes
  live, so the real windows already show them

**Effects (SwayFX only)**, generated into `90-effects.conf`

- [x] 🟢 Corner radius, smart corner radius
- [x] 🟢 Blur on/off, passes, radius (and noise / brightness / contrast / saturation)
- [x] 🟢 Shadows on/off, color, blur radius, offset; inactive shadow color
- [x] 🟢 Dim inactive windows (amount, colors)
- [x] 🟢 Animations (`animation_duration_ms`, SwayFX ≥ 0.6)
- ~~🟡 Effects in the live preview~~: no live preview (see above); SwayFX applies the effects live
- Directive names are taken from the man page of the oldest supported SwayFX release (0.5) and gated by version

### Wallpaper

- [x] 🟢 Image per output or for all outputs (`output * bg`)
- [x] 🟢 Fill mode: fill / fit / stretch / center / tile
- [x] 🟢 Solid color background
- [x] 🟢 Folder browser with thumbnails (`~/Pictures` by default, subfolders as filters)
- [x] 🟢 Preview of the displays as laid out, one or all edited at a time
- [x] 🟢 "Managed externally" detection when a custom wallpaper script runs (handover)
- [x] 🟡 Collections: named sets of pictures and folders whose pictures take turns at login, daily or after an interval,
      shuffled or in order (systemd user timer, no daemon)

### Apps Look & Feel

- [ ] 🟢 GTK theme, icon theme, font (gsettings)
- [ ] 🟢 Dark / light preference (`color-scheme`)
- [ ] 🟡 Qt theming (`QT_QPA_PLATFORMTHEME`, qt5ct/qt6ct, Kvantum) via our `environment.d` file
- [ ] 🟢 Font rendering: antialiasing, hinting, subpixel order
- [ ] 🟢 Text scaling factor
- [ ] 🟡 Automatic dark mode by time of day (darkman backend)

### Bar

Waybar via **handover** (waybar's include lets the main file win, see [Architecture](Architecture.md#other-tools));
swaybar via our sway include.

- [ ] 🟡 Position, height, output(s), layer, margins
- [ ] 🟡 Module layout: drag modules between left / center / right
- [ ] 🟡 Module catalog: clock, battery, network, pulseaudio, tray, cpu, memory, …
- [ ] 🟡 Per-module settings form, driven by a schema per module
- [ ] 🟡 Custom script modules (`custom/<name>`)
- [ ] 🟡 Styling: colors, font, size, rounded corners (generated `style.css`)
- [ ] 🟢 Reload waybar (`SIGUSR2`)
- [ ] 🟡 "Import as starting point" from the user's existing waybar config (read-only)
- [ ] 🟢 swaybar alternative: `bar { position, colors, font, status_command }`
- [ ] 🟢 SwayFX: blur behind the bar (`layer_effects`)
- **Maybe:** 🟡 multiple bars

---

## Desktop

### Window Behavior

- [x] 🟢 Focus follows mouse: yes / no / always (`focus_follows_mouse`)
- [x] 🟢 Mouse warping (`mouse_warping`)
- [x] 🟢 Focus wrapping (`focus_wrapping`)
- [x] 🟢 Focus on window activation (`focus_on_window_activation`)
- [x] 🟢 Default layout for new workspaces (`workspace_layout`): on Workspaces
- [x] 🟢 Default split orientation (`default_orientation`): on Workspaces
- [x] 🟢 Floating modifier and drag behavior (`floating_modifier`)
- [x] 🟢 Floating min/max size (`floating_minimum_size`, `floating_maximum_size`)
- [x] 🟢 Tiling drag and threshold (`tiling_drag`, `tiling_drag_threshold`)
- [x] 🟢 Popups during fullscreen (`popup_during_fullscreen`)
- [x] 🟢 Workspace back-and-forth (`workspace_auto_back_and_forth`): on Workspaces
- [x] 🟢 XWayland on/off (`xwayland`)
- [x] 🟢 Inhibit idle for fullscreen apps (`for_window [...] inhibit_idle fullscreen`)

### Workspaces

- [x] 🟡 Placement on the displays by a rule worked out for the displays connected: sway's own, round robin from left to
      right, a block per display, all on the built-in display
- [x] 🟡 Fixed placement remembered per combination of displays: a map of the displays with the workspace numbers; click
      a number to send it to the next display, drag it to another
- [x] 🟢 Fallback displays: each workspace lists the other displays after its own, so it moves away when its display is
      unplugged and back when it returns
- [x] 🟢 Number of workspaces placed (1–20)
- [x] 🟡 Applied at every start and reload of sway and each kanshi profile (`sway-things hook workspaces`), and at once
      from the page
- [x] 🟢 Layout, split and back-and-forth of new workspaces (moved from Window Behavior)
- [ ] 🟡 Keys for workspaces 11–20 and to move a workspace to another display, with conflict detection for the user's
      own bindings (→ shares the binding parser with Keyboard Shortcuts)
- [ ] 🟢 Workspace names / icons

### Keyboard Shortcuts

- [ ] 🟢 Searchable list of all bindings: hand-written ones (from the parser, with file:line) and ours
- [ ] 🟡 Add, edit, remove bindings with a "press shortcut" capture field. Changing a hand-written binding works through
      `unbindsym` + a new `bindsym` in our include (no edit of the user's file)
- [ ] 🟡 Conflict detection between bindings
- [ ] 🟡 Main modifier (`$mod`) for **our generated** bindings. Sway substitutes variables when a line is read, so this
      doesn't rebind hand-written lines; the page explains this
- [ ] 🟢 Media keys: volume, mute, brightness, play/pause/next (`wpctl`, `brightnessctl`, `playerctl`)
- [ ] 🟢 Screenshot shortcuts (grim + slurp / grimshot)
- [ ] 🟡 Touchpad gestures (`bindgesture`)
- [ ] 🟡 Binding modes (resize mode, custom modes)
- [ ] 🟢 Printable / exportable cheat sheet

### Notifications

- [ ] 🟢 Do Not Disturb toggle (`swaync-client -d`, `makoctl mode`)
- [ ] 🟢 SwayFX: blur behind notifications (`layer_effects`)
- **Maybe:** 🟡 choose the daemon (mako / swaync / dunst) · 🟡 position, timeout, max visible, width · 🟡 colors and
  font synced with the window colors · 🟡 per-app rules

### Apps & Autostart

- [ ] 🟢 Default terminal and launcher (fuzzel / wofi / rofi / tofi). Like `$mod`, `$term`/`$menu` only affect bindings
      we generate, so we generate the matching bindings
- [ ] 🟢 Default apps: browser, file manager, mail, image viewer, PDF, video, via our `sway-mimeapps.list`

- [ ] 🟡 Autostart as one hybrid list: our entries (editable, `exec` lines in our include), hand-written `exec` lines
      (read-only, file:line), XDG autostart entries (with "runner active?" status and an offer to enable
      `xdg-desktop-autostart.target` or `dex -a`; system entries disabled via our own `Hidden=true` override)
- **Maybe:** 🟡 launcher appearance (fuzzel INI, wofi CSS)

### Screenshots & Clipboard

- [ ] 🟢 Screenshot folder and filename pattern
- [ ] 🟢 Copy to clipboard and/or save; open an editor afterwards (swappy / satty)
- [ ] 🟡 Clipboard history setup (cliphist / clipman + launcher binding)
- [ ] 🟡 Screen recording shortcut (wf-recorder / wl-screenrec)

---

## System

### Power & Lock

- [ ] 🟡 Lock screen after N minutes (swayidle line in our include)
- [ ] 🟡 Turn screens off after N minutes (`output * power off`)
- [ ] 🟡 Suspend after N minutes, on battery vs. AC
- [ ] 🟡 Lock before sleep (`swayidle before-sleep`)
- [ ] 🟡 Lock screen appearance: color, image, indicator, ring colors (our swaylock config, `swaylock -C`)
- [ ] 🟡 Lid action when **no** external screen is connected: lock / suspend via `bindswitch` in our include. Must not
      double-trigger with logind's own lid handling; logind settings themselves are read-only
- [ ] 🟡 Power profile: power saver / balanced / performance (power-profiles-daemon D-Bus)
- [ ] 🟡 Battery info and health (UPower D-Bus)
- [ ] 🟡 Power button action: **read-only** (`logind.conf` is root-owned), with an explanation and a copyable drop-in

- [ ] 🟡 Logout / power menu config (wlogout / nwg-bar)

### Brightness & Night Light

- [ ] 🟡 Brightness slider (logind `SetBrightness` / brightnessctl; `ddcutil` for external monitors)
- [ ] 🟡 Night light: color temperature, schedule or location based (gammastep / wlsunset)

### Region & Time

- [ ] 🟡 Time zone, NTP on/off (timedated D-Bus, polkit)
- [ ] 🟡 System language and formats (localed; `LANG`/`LC_*` in our `environment.d` file)
- [ ] 🟢 Printers: launch `system-config-printer` (delegate)
- **Maybe:** 🟡 user avatar, full name, password (AccountsService)

### Accessibility

- [x] 🟢 Large cursor (→ Cursor & Tablet)
- [ ] 🟢 Larger text (→ Apps Look & Feel)
- [x] 🟢 Slow keys / repeat tuning (→ Keyboard)
- **Not possible on sway:** sticky keys, color inversion, zoom/magnifier

### Environment

- [ ] 🟡 Environment variables editor (our `~/.config/environment.d/50-sway-things.conf`)
- [ ] 🟢 Wayland toggles: Firefox, Qt, SDL, Java `_JAVA_AWT_WM_NONREPARENTING`
- [ ] 🟡 Electron/Chromium Wayland flags (create the flags files if missing, otherwise handover)
- [ ] 🟡 XDG portal check (xdg-desktop-portal-wlr / -gtk installed and configured for screen sharing and file pickers)
- [ ] 🟡 Screen sharing output chooser (xdg-desktop-portal-wlr config)

### About & Maintenance

- [x] 🟢 About: system details for bug reports (versions of Sway Things, sway / SwayFX, GTK, the distribution,
      NetworkManager and the kernel; whether the delegated tools are installed), copied as text and in
      `sway-things doctor`
- [ ] 🟢 About: GPU
- [ ] 🟢 About: outputs and inputs
- [ ] 🟢 Include-line status and conflict overview (same as `sway-things doctor`)
- [ ] 🟢 Validate the config (`sway --validate`) and show errors
- [ ] 🟢 Reload sway
- [ ] 🟢 History of our settings files with one-click restore (we never write user files, so they need no backups)
- [ ] 🟢 Export / import settings (`settings.toml` + host overlays)
- [ ] 🟡 Dependency check: missing optional tools, with install commands for zypper / dnf / apt / pacman
- [x] 🟢 "Show generated config": every file Sway Things writes, with its content
- [x] 🟢 "Sway Things" preferences section: accent source, safety-net timeout, editor command, show sway directives
