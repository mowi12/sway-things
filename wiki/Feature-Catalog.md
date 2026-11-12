# Feature Catalog

The complete planned scope, per page, **unordered**. Open a GitHub issue when you start or plan an item.

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

- 🟢 Pointer speed (`pointer_accel`, −1.0 … 1.0)
- 🟢 Acceleration profile: flat / adaptive (`accel_profile`)
- 🟢 Natural scrolling (`natural_scroll`)
- 🟢 Scroll speed (`scroll_factor`)
- 🟢 Left-handed mode (`left_handed`)
- 🟢 Middle-click emulation (`middle_emulation`)
- 🟢 Scroll method: two-finger / edge (touchpads) / on-button-down / none (`scroll_method`, `scroll_button`)
- 🟢 Touchpad: tap to click (`tap`), tap button map lrm / lmr (`tap_button_map`)
- 🟢 Touchpad: tap-and-drag, drag lock (`drag`, `drag_lock`; sticky drag lock needs sway ≥ 1.11)
- 🟢 Touchpad: disable while typing / while trackpointing (`dwt`, `dwtp`)
- 🟢 Touchpad: click method, button areas vs. clickfinger (`click_method`, `clickfinger_button_map`)
- 🟢 Disable touchpad while an external mouse is connected (`events disabled_on_external_mouse`)
- 🟢 Enable / disable individual devices (`events`), **risky → safety net**, the last pointing device is protected
- 🟢 Per-device overrides (device list from `GET_INPUTS`, hotplug via events)
- 🟢 Live test area for clicking and scrolling

### Keyboard

- 🟢 Layouts: add, remove, reorder (`xkb_layout`), **risky → safety net**
- 🟢 Variants, e.g. `nodeadkeys`, `dvorak`, `colemak` (`xkb_variant`)
- 🟢 Layout switching shortcut (`xkb_options grp:*`)
- 🟢 Common XKB options as toggles: Caps→Ctrl, Caps→Esc, swap Ctrl/Alt, Compose key, … (`xkb_options`)
- 🟢 Full XKB option browser (parsed from `/usr/share/X11/xkb/rules/evdev.xml`)
- 🟢 Keyboard model (`xkb_model`)
- 🟢 Repeat delay and rate (`repeat_delay`, `repeat_rate`)
- 🟢 NumLock / CapsLock state at startup (`xkb_numlock`, `xkb_capslock`)
- 🟢 Per-keyboard layouts (e.g. a different layout on an external keyboard)
- 🟢 Switch the active layout from the app (`xkb_switch_layout`)
- 🟡 Layout preview image (`xkbcli` render or bundled drawing)
- 🟢 "Try it" input field
- **Maybe:** 🟡 custom `.xkb` keymap file (`xkb_file`)

### Cursor & Tablet

- 🟢 Cursor theme and size (`seat * xcursor_theme` + gsettings / `XCURSOR_*` for apps)
- 🟢 Hide the cursor after inactivity / while typing (`seat * hide_cursor`)
- **Maybe:** 🟢 map a tablet or touchscreen to an output (`map_to_output`) · 🟢 map to a region (`map_to_region`,
  `map_from_region`) · 🟢 tool mode absolute / relative (`tool_mode`) · 🟡 touchscreen calibration wizard
  (`calibration_matrix`)

### Displays

The requirements below include the proven ideas from kanshi_gui. Display profile backend: kanshi (reference); without
kanshi, static `output` lines in our include (no profiles).

#### Layout editor

- 🟡 Drag-and-drop arrangement on a canvas, with smart snapping (guide lines, edge/corner/center alignment, a magnet
  that backs off)
- 🟢 Resolution and refresh rate (**risky → safety net**)
- 🟢 Scale and rotation per output
- 🟢 Enable / disable outputs (**risky**). The last enabled output is protected
- 🟢 Live apply over IPC, 15 s safety net for risky changes
- 🟢 Hotplug awareness (output events)
- 🟢 Identify displays (numbers flashed on each screen)
- 🟡 Mirror one output onto another (`wl-mirror`), with a scaling mode

#### Profiles (kanshi backend)

- 🟡 Create, rename, delete profiles; generated into our kanshi include

- 🟢 One-click profile switching
- 🟡 First run: detect the current layout and suggest a profile name
- 🟢 Flicker-free reload (`kanshictl reload` → `SIGHUP` → restart kanshi only as a last resort)
- 🟢 Generate `exec kanshi` / `exec_always kanshictl reload` into **our** sway include when kanshi isn't started yet

#### Workspace placement (opt-in)

- 🟡 Visual map of the screens with workspace numbers: drag a number to another screen
- 🟢 Presets: left-to-right, one block per screen, current placement, custom
- 🟡 Apply at profile switch via the CLI hook `sway-things hook workspaces --profile <name>`

#### Laptop lid (opt-in)

- 🟡 Lid closed with another screen active → turn off the built-in panel and move its workspaces; restore on open.
  `bindswitch` lines in our include call `sway-things hook lid closed|open`

**Maybe:** 🟡 adaptive sync / VRR per output · 🟡 sway-only output options (allow tearing, 10-bit, subpixel, scale
filter, ICC profile) · 🟡 custom modes / modelines · 🟡 match outputs by make/model/serial instead of connector (stable
across docks) · 🟡 show which profile is active and why · 🟡 per-profile custom `exec` commands · 🟡 shikane backend

### Sound

Native basics, delegate advanced cases. Backend: PulseAudio API via pipewire-pulse.

- 🟡 Output and input device selection
- 🟡 Master volume and mute, microphone level
- 🟡 Per-application volume
- 🟡 Device profiles (HDMI, A2DP/HFP for Bluetooth headsets)
- 🟡 Alert sound volume / event sounds
- 🟢 "Advanced…" → pavucontrol (or similar)

### Bluetooth & Network

- 🟡 Bluetooth on/off, scan, pair, connect, forget (BlueZ D-Bus). Device names are attacker-controlled data
- 🟡 Wi-Fi list and connect (NetworkManager D-Bus). SSIDs are attacker-controlled data
- 🟡 Wired / VPN status; "Advanced…" → `nm-connection-editor`
- **Not planned:** a full network editor (delegate)

---

## Appearance

### Windows

- 🟢 Focused window colors: border, background, text, indicator, child border (`client.focused`). The child border also
  drives the app accent
- 🟢 Colors for unfocused / focused-inactive / urgent windows and the focused tab title (`client.*`)
- 🟢 Border style: normal / pixel / none, plus width (`default_border`, `default_floating_border`)
- 🟢 Hide edge borders / smart borders (`hide_edge_borders`, `smart_borders`)
- 🟢 Gaps inner/outer, global and per workspace (`gaps`)
- 🟢 Smart gaps (`smart_gaps`)
- 🟢 Title bar font and size (`font`)
- 🟢 Title alignment and padding (`title_align`, `titlebar_padding`, `titlebar_border_thickness`)
- 🟢 Color presets (e.g. Catppuccin, Nord, Gruvbox) applied in one click
- 🟡 Match the accent color to the wallpaper (palette extraction)
- 🟢 Per-app window opacity (`for_window [...] opacity` for new windows, `[...] opacity` for open ones)
- 🟡 Live preview of focused and unfocused windows (colors, borders, gaps, title bars)

**Effects (SwayFX only)**, generated into `90-effects.conf`

- 🟢 Corner radius, smart corner radius
- 🟢 Blur on/off, passes, radius (and noise / brightness / contrast / saturation)
- 🟢 Shadows on/off, color, blur radius, offset; inactive shadow color
- 🟢 Dim inactive windows (amount, colors)
- 🟢 Animations (`animation_duration_ms`, SwayFX ≥ 0.6)
- 🟡 Effects in the live preview
- Directive names are taken from the man page of the oldest supported SwayFX release (0.5) and gated by version

### Wallpaper

- 🟢 Image per output or for all outputs (`output * bg`)
- 🟢 Fill mode: fill / fit / stretch / center / tile
- 🟢 Solid color background
- 🟢 Folder browser with thumbnails
- 🟢 "Managed externally" detection when a custom wallpaper script runs (handover)
- **Maybe:** 🟡 slideshow (with no daemon this needs a tool like swww plus a timer)

### Apps Look & Feel

- 🟢 GTK theme, icon theme, font (gsettings)
- 🟢 Dark / light preference (`color-scheme`)
- 🟡 Qt theming (`QT_QPA_PLATFORMTHEME`, qt5ct/qt6ct, Kvantum) via our `environment.d` file
- 🟢 Font rendering: antialiasing, hinting, subpixel order
- 🟢 Text scaling factor
- 🟡 Automatic dark mode by time of day (darkman backend)

### Bar

Waybar via **handover** (waybar's include lets the main file win, see [Architecture](Architecture.md#other-tools));
swaybar via our sway include.

- 🟡 Position, height, output(s), layer, margins
- 🟡 Module layout: drag modules between left / center / right
- 🟡 Module catalog: clock, battery, network, pulseaudio, tray, cpu, memory, …
- 🟡 Per-module settings form, driven by a schema per module
- 🟡 Custom script modules (`custom/<name>`)
- 🟡 Styling: colors, font, size, rounded corners (generated `style.css`)
- 🟢 Reload waybar (`SIGUSR2`)
- 🟡 "Import as starting point" from the user's existing waybar config (read-only)
- 🟢 swaybar alternative: `bar { position, colors, font, status_command }`
- 🟢 SwayFX: blur behind the bar (`layer_effects`)
- **Maybe:** 🟡 multiple bars

---

## Desktop

### Window Behavior

- 🟢 Focus follows mouse: yes / no / always (`focus_follows_mouse`)
- 🟢 Mouse warping (`mouse_warping`)
- 🟢 Focus wrapping (`focus_wrapping`)
- 🟢 Focus on window activation (`focus_on_window_activation`)
- 🟢 Default layout for new workspaces (`workspace_layout`)
- 🟢 Default split orientation (`default_orientation`)
- 🟢 Floating modifier and drag behavior (`floating_modifier`)
- 🟢 Floating min/max size (`floating_minimum_size`, `floating_maximum_size`)
- 🟢 Tiling drag and threshold (`tiling_drag`, `tiling_drag_threshold`)
- 🟢 Popups during fullscreen (`popup_during_fullscreen`)
- 🟢 Workspace back-and-forth (`workspace_auto_back_and_forth`)
- 🟢 XWayland on/off (`xwayland`)
- 🟢 Inhibit idle for fullscreen apps (`for_window [...] inhibit_idle fullscreen`)

### Workspaces

- 🟢 Workspace names / icons
- 🟢 Number of workspaces with generated default bindings
- Workspace-to-output placement: see [Displays](#displays)

### Keyboard Shortcuts

- 🟢 Searchable list of all bindings: hand-written ones (from the parser, with file:line) and ours
- 🟡 Add, edit, remove bindings with a "press shortcut" capture field. Changing a hand-written binding works through
  `unbindsym` + a new `bindsym` in our include (no edit of the user's file)
- 🟡 Conflict detection between bindings
- 🟡 Main modifier (`$mod`) for **our generated** bindings. Sway substitutes variables when a line is read, so this
  doesn't rebind hand-written lines; the page explains this
- 🟢 Media keys: volume, mute, brightness, play/pause/next (`wpctl`, `brightnessctl`, `playerctl`)
- 🟢 Screenshot shortcuts (grim + slurp / grimshot)
- 🟡 Touchpad gestures (`bindgesture`)
- 🟡 Binding modes (resize mode, custom modes)
- 🟢 Printable / exportable cheat sheet

### Notifications

- 🟢 Do Not Disturb toggle (`swaync-client -d`, `makoctl mode`)
- 🟢 SwayFX: blur behind notifications (`layer_effects`)
- **Maybe:** 🟡 choose the daemon (mako / swaync / dunst) · 🟡 position, timeout, max visible, width · 🟡 colors and
  font synced with the window colors · 🟡 per-app rules

### Apps & Autostart

- 🟢 Default terminal and launcher (fuzzel / wofi / rofi / tofi). Like `$mod`, `$term`/`$menu` only affect bindings we
  generate, so we generate the matching bindings
- 🟢 Default apps: browser, file manager, mail, image viewer, PDF, video, via our `sway-mimeapps.list`

- 🟡 Autostart as one hybrid list: our entries (editable, `exec` lines in our include), hand-written `exec` lines
  (read-only, file:line), XDG autostart entries (with "runner active?" status and an offer to enable
  `xdg-desktop-autostart.target` or `dex -a`; system entries disabled via our own `Hidden=true` override)
- **Maybe:** 🟡 launcher appearance (fuzzel INI, wofi CSS)

### Screenshots & Clipboard

- 🟢 Screenshot folder and filename pattern
- 🟢 Copy to clipboard and/or save; open an editor afterwards (swappy / satty)
- 🟡 Clipboard history setup (cliphist / clipman + launcher binding)
- 🟡 Screen recording shortcut (wf-recorder / wl-screenrec)

---

## System

### Power & Lock

- 🟡 Lock screen after N minutes (swayidle line in our include)
- 🟡 Turn screens off after N minutes (`output * power off`)
- 🟡 Suspend after N minutes, on battery vs. AC
- 🟡 Lock before sleep (`swayidle before-sleep`)
- 🟡 Lock screen appearance: color, image, indicator, ring colors (our swaylock config, `swaylock -C`)
- 🟡 Lid action when **no** external screen is connected: lock / suspend via `bindswitch` in our include. Must not
  double-trigger with logind's own lid handling; logind settings themselves are read-only
- 🟡 Power profile: power saver / balanced / performance (power-profiles-daemon D-Bus)
- 🟡 Battery info and health (UPower D-Bus)
- 🟡 Power button action: **read-only** (`logind.conf` is root-owned), with an explanation and a copyable drop-in

- 🟡 Logout / power menu config (wlogout / nwg-bar)

### Brightness & Night Light

- 🟡 Brightness slider (logind `SetBrightness` / brightnessctl; `ddcutil` for external monitors)
- 🟡 Night light: color temperature, schedule or location based (gammastep / wlsunset)

### Region & Time

- 🟡 Time zone, NTP on/off (timedated D-Bus, polkit)
- 🟡 System language and formats (localed; `LANG`/`LC_*` in our `environment.d` file)
- 🟢 Printers: launch `system-config-printer` (delegate)
- **Maybe:** 🟡 user avatar, full name, password (AccountsService)

### Accessibility

- 🟢 Large cursor (→ Cursor & Tablet)
- 🟢 Larger text (→ Apps Look & Feel)
- 🟢 Slow keys / repeat tuning (→ Keyboard)
- **Not possible on sway:** sticky keys, color inversion, zoom/magnifier

### Environment

- 🟡 Environment variables editor (our `~/.config/environment.d/50-sway-things.conf`)
- 🟢 Wayland toggles: Firefox, Qt, SDL, Java `_JAVA_AWT_WM_NONREPARENTING`
- 🟡 Electron/Chromium Wayland flags (create the flags files if missing, otherwise handover)
- 🟡 XDG portal check (xdg-desktop-portal-wlr / -gtk installed and configured for screen sharing and file pickers)
- 🟡 Screen sharing output chooser (xdg-desktop-portal-wlr config)

### About & Maintenance

- 🟢 About: sway / SwayFX and wlroots versions, GPU, distro, outputs, inputs
- 🟢 Include-line status and conflict overview (same as `sway-things doctor`)
- 🟢 Validate the config (`sway --validate`) and show errors
- 🟢 Reload sway
- 🟢 History of our settings files with one-click restore (we never write user files, so they need no backups)
- 🟢 Export / import settings (`settings.toml` + host overlays)
- 🟡 Dependency check: missing optional tools, with install commands for zypper / dnf / apt / pacman
- 🟢 "Show generated config": every file Sway Things writes, with its content
- 🟢 "Sway Things" preferences section: accent source, safety-net timeout, editor command, show sway directives
