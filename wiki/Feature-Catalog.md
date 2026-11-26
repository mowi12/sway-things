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
- [x] 🟡 A picture that went comes back by itself, without a daemon: when the app opens and when displays come and go,
      in kanshi's hooks and at each start and reload of sway; `sway-things hook mirror` for a key binding
- [x] 🟡 A display never seen before, plugged in to a remembered combination, asks once (a notification with _Mirror_
      and _Extend_) and the answer is remembered for the new combination
- [ ] 🟡 Recognize a projector again by make and model where its serial is generic (`0x00000001`, `0x01010101`) or it
      comes through another port; needs checking with real projectors and adapters (see Sway-Reference)

#### Profiles (kanshi backend)

- [x] 🟡 Each combination of displays remembered on its own as a kanshi profile, without naming or picking profiles;
      generated into our kanshi include (`~/.config/kanshi/sway-things.conf`)
- [x] 🟢 A profile that switches on any one display alone, so no combination stays dark
- [x] 🟢 `kanshictl reload` after saving
- [x] 🟢 Our include goes above the user's own kanshi profiles, so the arrangement made here comes first; a warning with
      the command to move it while one of theirs wins for the displays connected
- [x] 🟢 Generate `exec kanshi` into **our** sway include when the user's config doesn't start kanshi
- [x] 🟡 The user's own profiles keep setting up their displays while Sway Things places the workspaces and mirrors on
      them: a copy of each in our include, kept current, without their lines that place workspaces

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
      structs to libpulse (see [Architecture → Backends](Backends.md))
- [x] 🟢 "Advanced…" → pwvucontrol, else pavucontrol
- [x] 🟢 Volume above 100 % (up to 150 %) as an app preference

### Bluetooth

- [x] 🟡 Bluetooth on/off, scan, pair, connect, forget (BlueZ D-Bus). Device names are attacker-controlled data
- [x] 🟡 Pairing dialogs through our own agent: compare a code, type a code or PIN, authorize
- [x] 🟢 Cancel a connection or pairing while it runs
- [x] 🟢 Lift an rfkill soft block when switching on; explain a hard block
- [x] 🟢 Several adapters: pick the one the page shows
- [x] 🟢 Battery level of connected devices, and whether they charge (UPower or the kernel)
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
- [x] 🟡 Ask for VPN passwords through the VPN plugin's dialog
- [x] 🟡 Sign in to OpenConnect VPNs through our own dialog (`openconnect --authenticate`), also with a TOTP or a web
      sign-in
- **Not planned:** a full network editor (delegate)

### Printers & Scanners

CUPS and SANE own the state: nothing is stored in `settings.toml` and nothing is generated. Printer and scanner names,
models and job titles are attacker-controlled data. The page was _Printers_ before scanners joined it;
`sway-things open printers` and `scanners` still open it.

Printers:

- [x] 🟢 One line per printer with its state in words, a _Default_ badge and small ink or toner bars; a click opens its
      details (GNOME Settings' layout)

- [x] 🟡 Printers with their state (Ready, Printing, Paused, Out of paper, Toner low, Offline, …), model and location
      (IPP to the local CUPS)
- [x] 🟡 Ink and toner levels as bars in their colors (`marker-*`)
- [x] 🟢 Default printer: marked, and set (`CUPS-Set-Default`); the user's own default from `lpoptions -d` shown
- [x] 🟢 Pause and resume a printer
- [x] 🟡 The print queue: title, owner, state and time of each job; cancel a job, cancel all jobs of a printer
- [x] 🔴 Add a driverless printer found by Avahi (network and IPP over USB) or typed as an address, with the
      `everywhere` model
- [x] 🟢 Remove a printer, asking first
- [x] 🟢 Print a test page
- [x] 🟢 Change the description and location, share on the network
- [x] 🟡 Changes as a member of CUPS's administrator group, else through cups-pk-helper (polkit); without either, say
      what to install or which group to join
- [x] 🟢 "Advanced…" → `system-config-printer`, else CUPS's web interface (printers that need a driver)
- [ ] 🟡 Default options: paper size, two-sided, color or grayscale, from what the printer supports
- [ ] 🟢 Live updates through a CUPS subscription instead of reading every 2 s
- **Maybe:** 🟡 choosing drivers natively (system-config-printer's D-Bus service), classes, CUPS's server settings
- **Not planned:** a driver editor (delegate)

Scanners (scanning is basic, see [Vision and Scope](Vision-and-Scope.md#scope-by-sidebar-group)):

- [x] 🟡 The scanners SANE finds (`scanimage -f`), with how they are connected (driverless on the network or on USB,
      USB, network), read when the page comes on screen and with _Search Again_
- [x] 🟡 Driverless scanners Avahi finds (`_uscan._tcp`, `_uscans._tcp`) that SANE doesn't list, marked
- [x] 🟡 Hints for what is missing: scanimage (with the distribution's package), sane-airscan, ipp-usb for an
      IPP-over-USB device, the access to a USB scanner (with the group); `sway-things doctor` reports the same
- [x] 🔴 Scan into a file: source, colors, resolution and paper size from `scanimage -A`, a preview, PNG, JPEG or TIFF
      as scanimage writes them, or PDF; progress and _Cancel_; errors in words with scanimage's output under _Details_;
      a file chooser in Documents with a dated name; _Open_ and _Show in Folder_
- [x] 🟡 Multi-page PDF: every sheet of the document feeder, and _Scan Another Page_; a PDF writer of our own (JPEG
      pages as they are, black and white run-length encoded), so no img2pdf, ImageMagick or `scanimage --format=pdf`
- [x] 🟢 _Open Document Scanner_ (or Skanpage, XSane) for the rest
- [ ] 🟡 Choose the scan area on the preview
- [ ] 🟡 Two-sided scans from a simplex feeder (turn the stack over), page order and rotation in the PDF
- **Not planned:** OCR and image editing (scan apps); sharing scanners (saned)

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
- [x] 🟢 Preview of the displays as laid out, one or all edited at a time, in the same arrangement and look as on the
      Displays page
- [x] 🟢 "Managed externally" detection when a custom wallpaper script runs (handover)
- [x] 🟢 Back to the default: the user's own wallpaper as the first tile, _Use Default_ per display or for all, and
      _Reset page…_
- [x] 🟢 Warning for a picture that can't be read (no gdk-pixbuf loader for WebP and others)
- [x] 🟡 Collections: named sets of pictures and folders whose pictures take turns at login, daily or after an interval,
      shuffled or in order (systemd user timer, no daemon)

### Apps Look & Feel

- [x] 🟢 GTK theme, icon theme, font (gsettings): the installed themes as drop-downs, the icon themes with a few of
      their icons, the interface, document and monospace fonts with GTK's font dialog
- [x] 🟢 Dark / light preference (`color-scheme`)
- [ ] 🟡 Qt theming (`QT_QPA_PLATFORMTHEME`, qt5ct/qt6ct, Kvantum) via our `environment.d` file
- [x] 🟢 Font rendering: antialiasing, hinting, subpixel order
- [x] 🟢 Text scaling factor
- [ ] 🟡 Automatic dark mode by time of day (darkman backend)

### Bar

Waybar via **handover** (waybar's include lets the main file win, see [Bar](Backends.md#bar)); swaybar via our sway
include.

- [x] 🟡 Position, height, output(s), layer, margins, reserved space (exclusive zone)
- [x] 🟡 Module layout: drag modules between left / center / right, or move them with Alt and the arrow keys
- [x] 🟡 Module catalog: clock, battery, network, pulseaudio, tray, cpu, memory, …, with what each needs here
- [x] 🟡 Per-module settings form, driven by a schema per module; unknown options kept as JSON
- [x] 🟡 Custom script modules (`custom/<name>`), and templates (pomodoro, camera, updates, battery warning, VPN, system
      load, power menu, notification center)
- [x] 🟡 Styling: colors (yours or sway's window colors), font, padding, rounded corners, transparency, a pill per
      module (generated `style.css`), presets, your own stylesheet read after ours
- [x] 🟡 Looks style groups as one pill with the drawer's items inside, the item under the pointer inset in the pill's
      shape, empty modules without room, the states of modules (warning, critical, ok) in the look's colors, tabular
      figures, and no theme's frames or fills
- [x] 🟢 Reload waybar (`SIGUSR2`, only to the waybar that runs our config)
- [x] 🟡 "Import as starting point" from the user's existing waybar config (read-only, JSONC with `include`s)
- [x] 🟢 swaybar alternative: `bar { position, colors, font, status_command }` as one-line `bar sway-things …` commands
- [x] 🟢 SwayFX: blur behind the bar (`layer_effects`)
- [x] 🟡 Preview of the bar in its colors, sizes and fonts with sample values, clickable items, and the bar in words
- [x] 🟡 Preview drawn with Waybar's own stylesheet and the user's (confined to the preview), Waybar's widget names,
      groups and drawers as Waybar shows them, over the wallpaper
- [x] 🟡 Items in Left / Center / Right lanes: drag, Alt and the arrow keys, or a menu; Undo for removals, moves and
      looks (Ctrl+Z)
- [x] 🟡 Item picker with search, categories, a sample per item and badges (not available here, needs a program, uses
      the network)
- [x] 🟡 Basic settings per item in plain words (clock formats as examples, battery warning levels, …), every option
      under Advanced
- [x] 🟡 Your own command as a program and arguments, with a test run (no shell, time limit) and what it costs
- [x] 🟢 Looks as cards (Simple, GNOME-like, Floating islands, Minimal, Large & high contrast, Like your windows,
      Classic Waybar, Glass on SwayFX), style only, marked when in use
- [x] 🟢 Size (height and text size together), contrast of the text on the background
- [x] 🟢 Show the bar always or while a key is held (swaybar `mode hide`, waybar over sway's IPC)
- [x] 🟢 Bar program in one card, a dialog that says what the other program can't show before switching
- [x] 🟡 Use Sway Things' bar in place of the user's `bar { … }` block without editing their config
      (`bar <id> swaybar_command sway-things`), kept within 15 s or switched back by itself, and _Switch Back to My Own
      Bar_
- [x] 🟢 The files Sway Things writes for Waybar, read-only, with their paths
- [ ] 🟡 Per-monitor bars: all screens as the base, overrides per display matched by make, model and serial
- [ ] 🟡 Keep-or-revert after a change of the bar (15 s), and a history of the last applied bars
- [ ] 🟡 Export and import of the bar as one file, also from the command line
- [ ] 🟡 Thresholds with colored zones for CPU, memory, temperature and disk; a state simulator under the preview
- [ ] 🟡 More items: microphone, media player filter, notifications (swaync, mako), power menu, privacy indicators
- [ ] 🟢 Lanes named Start / End and mirrored for right-to-left languages
- [ ] 🟢 Warning when an icon font (Nerd Font, Font Awesome) is missing
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
- [x] 🟡 Also where the user's own kanshi profiles set up the displays: a copy of each runs the placement in place of
      their own `exec swaymsg workspace` lines (see [Profiles](#profiles-kanshi-backend))
- [x] 🟢 Layout, split and back-and-forth of new workspaces (moved from Window Behavior)
- [ ] 🟡 Keys for workspaces 11–20 and to move a workspace to another display, with conflict detection for the user's
      own bindings (→ shares the binding parser with Keyboard Shortcuts)
- [ ] 🟢 Workspace names / icons

### Keyboard Shortcuts

- [x] 🟢 Searchable list of all bindings: hand-written ones (from the parser, with file:line) and ours
- [x] 🟡 Add, edit, remove bindings with a "press shortcut" capture field. Changing a hand-written binding works through
      `unbindsym` + a new `bindsym` in our include (no edit of the user's file)
- [x] 🟡 Conflict detection between bindings
- [x] 🟡 Main modifier (`$mod`) for **our generated** bindings. Sway substitutes variables when a line is read, so this
      doesn't rebind hand-written lines; the page explains this
- [x] 🟢 Media keys: volume, mute, brightness, play/pause/next (`wpctl`, `brightnessctl`, `playerctl`)
- [x] 🟢 On-screen display for the media keys with SwayOSD (`swayosd-client`), per group, with a note while
      `swayosd-server` doesn't run
- [x] 🟢 Screenshot shortcuts (grim + slurp; the keys of Screenshots & Clipboard, listed here with their clashes)
- [ ] 🟡 Touchpad gestures (`bindgesture`)
- [ ] 🟡 Binding modes (resize mode, custom modes)
- [ ] 🟢 Printable / exportable cheat sheet

### Notifications

- [x] 🟢 Do Not Disturb toggle (`swaync-client -d`, `makoctl mode`, `dunstctl set-paused`), also as
      `sway-things notifications dnd`
- [x] 🟢 SwayFX: blur behind notifications (`layer_effects`)
- [x] 🟡 Which daemon runs and what starts it; start one from our include; two daemons reported; a test notification
- [x] 🟡 Choose the daemon (mako / swaync / dunst) to start with sway
- [x] 🟡 Position, timeout, max visible, width (mako and swaync; dunst not yet)
- [x] 🟡 Colors synced with the window colors, or custom; font (mako)
- [x] 🟡 swaync's panel: widgets, button grid, a frosted style preset; import of an existing swaync config
- [x] 🟡 Clicking a notification shows its app
- **Maybe:** 🟡 per-app rules · 🟡 dunst's look through a `dunstrc.d` drop-in

### Apps & Autostart

- [x] 🟢 Default terminal and launcher (fuzzel / wofi / rofi / tofi). Like `$mod`, `$term`/`$menu` only affect bindings
      we generate, so we generate the matching bindings
- [x] 🟢 Default apps: browser, file manager, mail, image viewer, PDF, video, via our `sway-mimeapps.list`

- [x] 🟡 Autostart as one hybrid list: our entries (editable, `exec` lines in our include), hand-written `exec` lines
      (read-only, file:line), XDG autostart entries (with "runner active?" status and an offer to enable
      `xdg-desktop-autostart.target` or `dex -a`; system entries disabled via our own `Hidden=true` override)
- **Maybe:** 🟡 launcher appearance (fuzzel INI, wofi CSS)

### Screenshots & Clipboard

- [x] 🟢 Screenshot folder and filename pattern
- [x] 🟢 Copy to clipboard and/or save; open an editor afterwards (swappy / satty)
- [x] 🟡 Clipboard history setup (cliphist + launcher binding; clipman, which keeps only text, isn't offered)
- [x] 🟡 Screen recording shortcut (wf-recorder / wl-screenrec)

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

- [x] 🟡 Brightness slider (logind `SetBrightness`, from 1 %; `ddcutil` for external monitors; the keyboard light;
      `sway-things brightness`)
- [x] 🟡 Night light: color temperature, schedule or location based (wlsunset / gammastep; sunset to sunrise from the
      time zone's place or a location entered by hand, fixed times, always on; `sway-things night-light`)

### Region & Time

- [x] 🟡 Time zone, NTP on/off, the clock by hand while NTP is off (timedated D-Bus, polkit)
- [x] 🟡 Automatic time zone from geoclue's location, off by default (at login through a hook, and while the page is
      open)
- [x] 🟡 Language and formats for the user (`LANG`/`LC_*` in our `environment.d` file, from the installed locales, with
      a preview), and the system's through localed
- ~~🟢 Printers: launch `system-config-printer` (delegate)~~: the Printers page does it, under Devices
- **Maybe:** 🟡 user avatar, full name, password (AccountsService)

### Accessibility

- [x] 🟢 Large cursor (→ Cursor & Tablet)
- [x] 🟢 Larger text (→ Apps Look & Feel)
- [x] 🟢 Slow keys / repeat tuning (→ Keyboard)
- **Not possible on sway:** sticky keys, color inversion, zoom/magnifier

### Environment

- [x] 🟡 Environment variables editor (our `~/.config/environment.d/50-sway-things.conf`), in environment.d's syntax
      with a preview of the expanded value, guarded against the session's own variables and a `PATH` that loses the
      system's programs; the variables of the other environment.d files read only, with which line wins
- [x] 🟢 Wayland for older apps, each with a fallback to X11: Qt 5 (`QT_QPA_PLATFORM=wayland;xcb`), SDL 2
      (`SDL_VIDEODRIVER=wayland,x11`), Java (`_JAVA_AWT_WM_NONREPARENTING=1`), Electron 28 to 37
      (`ELECTRON_OZONE_PLATFORM_HINT=auto`), in our `environment.d` file, with a notice when the session doesn't read it
- ~~🟢 Wayland toggle for Firefox (`MOZ_ENABLE_WAYLAND`)~~: Firefox uses Wayland by itself since 121
- ~~🟡 Electron/Chromium Wayland flags (create the flags files if missing, otherwise handover)~~: Chromium 140 and
  Electron 38 use Wayland by default, Electron 28 to 37 takes the variable above, and only Arch's wrappers read the
  flags files
- [x] 🟡 XDG portal check (xdg-desktop-portal-wlr / -gtk installed and configured for screen sharing and file pickers),
      with the session's variables in systemd and a portal running; fixes only in our own files: our `sway-portals.conf`
      and a switch that passes the session's variables to systemd and D-Bus
- [x] 🟡 Screen sharing output chooser (xdg-desktop-portal-wlr config): xdpw's default, slurp, a menu of the first
      launcher installed, or always one display; our own `~/.config/xdg-desktop-portal-wlr/sway`, only where the user
      has no file there

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
