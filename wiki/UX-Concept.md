# UX Concept

GNOME Settings and macOS System Settings are the model for **structure and behavior**, not for the look. Sway Things
looks like the user's other GTK4 apps and takes its accent color from sway.

## Window layout

```text
┌ Sway Things ──────────────────────────────────────────────────────────┐
│ 🔍 Search settings…        │  Mouse & Touchpad                         │
├────────────────────────────┤  Pointer speed, scrolling and touchpad    │
│ DEVICES                    │                                           │
│ ▸ Mouse & Touchpad       ◀ │  POINTER                                  │
│   Keyboard                 │  Pointer speed        ──────●────────     │
│   Cursor & Tablet          │  pointer_accel 0.30                       │
│   Displays                 │  Acceleration profile    [ Adaptive ▾ ]   │
│   Sound                    │  Natural scrolling                 [  ●]  │
│   Bluetooth & Network      │                                           │
│ APPEARANCE                 │  TOUCHPAD                                 │
│   Windows                  │  Tap to click                      [  ●]  │
│   Wallpaper                │  …                                        │
│   Apps Look & Feel         │                                           │
│   Bar                      │  TEST AREA                                │
│ DESKTOP                    │  ┌ Click, double-click and scroll here ┐  │
│   …                        │  └─────────────────────────────────────┘  │
│ SYSTEM                     │                                           │
│   …                        │  ~/.config/sway/sway-things/10-input.conf │
└────────────────────────────┴───────────────────────────────────────────┘
```

- **Sidebar** with grouped pages and the search field at the top. The **content pane** on the right.
- A **page** has a header (title + one-line description), sections with setting rows, and a footer showing the file that
  page writes to.
- A **setting row** has a title, optional subtitle, the control on the right, and for sway settings the **sway directive
  in monospace** as the subtitle (`pointer_accel 0.30`), so users learn the config language as they go.

### Input devices

Settings of input devices (Mouse & Touchpad, Keyboard) apply to all devices of a type or to one device:

- **Device selector:** the sections of one input type start with a _Settings for_ drop-down: _All mice_ (_All
  touchpads_, …) and each device of the type. Devices that aren't connected but have stored values are listed with "(not
  connected)", so their values stay reachable. Picking a device points every row of those sections at it.
- **One device:** a row shows the device's own value, or the value of all devices of the type with the note _Same as all
  mice_. A change stores a value for the device, on this machine by default. Removing a device's own value comes with
  the _Reset_ indicator.
- **No device:** while no device of a type is connected and nothing is stored for the type, its sections show their
  heading with a hint (_No touchpad is connected._) instead of the rows. Without a session the devices aren't known, so
  every section shows.
- **Hotplug:** devices appear and disappear live. When the device picked is gone from the list, the sections go back to
  all devices of the type.
- **Mouse & Touchpad test area:** below the sections, a pad that names the last click (_Left click_, _Double-click_,
  _Right click_, …) to try tapping, the button maps and left-handed mode, and a strip of lines to scroll to feel scroll
  speed and natural scrolling.

### Sidebar groups

| Group | Pages |
| --- | --- |
| **Devices** | Mouse & Touchpad · Keyboard · Cursor & Tablet · Displays · Sound · Bluetooth & Network |
| **Appearance** | Windows · Wallpaper · Apps Look & Feel · Bar |
| **Desktop** | Window Behavior · Workspaces · Keyboard Shortcuts · Notifications · Apps & Autostart · Screenshots & Clipboard |
| **System** | Power & Lock · Brightness & Night Light · Region & Time · Accessibility · Environment · About & Maintenance |

## Navigation and search

- **Search finds individual settings, not just pages.** Each registry entry has a title, keywords and synonyms ("dpi",
  "sensitivity" → _Pointer speed_; "caps lock" → _XKB options_). Choosing a result opens the page, scrolls to the
  setting and highlights it briefly.
- **Keyboard-first:** typing anywhere (or `Ctrl+F`) focuses search; arrow keys move through the sidebar without opening
  pages; `Enter` (or a click) opens one; `Esc` clears the search or goes back from the page to the sidebar. Every
  control is reachable with `Tab`.
- **Every page is listed** from the start. A page with settings in the registry shows them in rows, by section; a page
  that isn't built yet shows its header and a placeholder saying so.
- **Narrow mode:** sway often gives the window half a screen or less. Below a width threshold the app shows one pane at
  a time: sidebar, then the page with a back button. The threshold is decided while building the shell in v0.1, from
  real page widths.
- **Deep links:** `sway-things open displays`, `sway-things open keyboard/repeat-rate`. Waybar modules, bindings and
  launchers can jump straight to a page or setting.
- **Single instance:** starting the app again focuses the existing window and switches page.
- **Stable app_id** `io.github.mowi12.SwayThings`, so users can write rules for it, e.g.
  `for_window [app_id="io.github.mowi12.SwayThings"] floating enable`.

## Apply model

- **Instant.** No Apply buttons anywhere. Changes take effect live and are saved after a short debounce.
- **Safety net** for risky changes (display mode, disabling an output or input device, keyboard layouts, variants and
  XKB options):

  ```text
  ┌────────────────────────────────────────────────────────────────────────┐
  │ Keep the change to Mouse › Enabled? Reverting in 12 s  [Revert] [Keep] │
  └────────────────────────────────────────────────────────────────────────┘
  ```

  The banner sits above the page and names the setting by section and title, and the device for a change of one device
  (_Mouse › MX Master 3 › Enabled_). Nothing is saved until _Keep_; _Revert_, the end of the countdown (the safety-net
  timeout preference) or closing the window puts the old value back, and the row shows it again. While the change waits,
  its row shows the tried value. When the banner appears, _Revert_ gets the focus, so a stray key press undoes the
  change instead of keeping it, and screen readers announce the banner. One risky change at a time: another one is
  refused at its row until this one is kept or reverted. The last output, the last keyboard and the last pointing device
  can't be disabled; the row says why.
- **Badges** for settings that can't apply live: "Applies after re-login" (environment variables), "Applies after
  reload".

## Per-setting indicators

A row stays lean: status as small badges (icon and text) under the sway line, actions behind buttons next to the
control. Badges are labels, never buttons.

| Indicator | When | Shown as | Actions |
| --- | --- | --- | --- |
| **Reset** | the value is stored in Sway Things | ↺ button | removes the value (for one device: back to the value of all devices of the type); a risky setting goes through the safety net |
| **This machine only** | the value is stored | computer toggle, pressed for the host overlay; new values of device- and output-bound settings start there | moves the value between `settings.toml` and the host overlay |
| **Your config wins** | a line of the user's config after our include overrides the stored value | badge marked for attention (warning icon and border) and a ⋮ menu | the menu shows file:line and the line, then _Let my config win_, _Open config in editor_, _Copy line_ |
| **Overridden** | a line of the user's config before our include is dead while the value is stored | plain badge and a ⋮ menu | as above |
| **Requires SwayFX** / **Requires sway ≥ x.y** | the running compositor lacks the setting | badge with a lock; the control is disabled, with the reason as tooltip | none |
| **Requires _tool_** | backend tool not installed | page stays visible but disabled, with install commands for zypper / dnf / apt / pacman | (comes with the pages that need backends) |
| **Managed externally** | something else is in charge (e.g. a custom wallpaper script, file:line) | explanation + handover | (comes with the pages that need backends) |

- The ⋮ button appears only on a row with a conflict. _Let my config win_ isn't offered for risky settings; their reset
  goes through the safety net. _Open config in editor_ opens the file with the desktop's text editor; jumping to the
  line comes with the editor command preference.
- **Reset page…** in the page header removes every stored value of the page after asking. Values of risky settings stay:
  the question names them and says to reset them from their rows, so a page reset never needs the safety net. The button
  can't be pressed while there is nothing it would remove.
- Indicators follow the state, the user's config and the include status live.

## First run: instructions only

The app **never** adds the include line itself. On first run (or whenever the line is missing) it shows:

```text
┌ Welcome to Sway Things ──────────────────────────────────────────┐
│ Sway Things never edits your config. It writes its own files     │
│ and needs ONE line at the end of ~/.config/sway/config:          │
│                                                                  │
│   include ~/.config/sway/sway-things/*.conf          [Copy]      │
│                                                                  │
│ [Open config in editor]           Waiting for the line… ⟳        │
└──────────────────────────────────────────────────────────────────┘
```

- A file watcher and the parser detect the line, including one written by hand earlier or in an included file.
- If the line exists but isn't last, the app explains the consequence (some of the user's lines win) instead of
  demanding a change.
- The line must cover every file Sway Things generates. If the config includes only some of them, the app stays
  read-only, names the files that are missing and shows the full line.
- If the line isn't found but part of the config can't be followed (an include that runs a shell command), the app stays
  read-only, names that include and suggests adding the line to the main config.
- If there is no config of the user's own yet (sway reads `/etc/sway/config`), the instructions say to create one first.
- **Where it shows:** the welcome screen takes the place of the page in the content pane; the sidebar stays usable. It
  has a **Copy** button for the line, **Open config in editor** (the desktop's default application for text files; a
  terminal editor from `$EDITOR` can't start without a terminal) and _Waiting for the line…_. It closes by itself once
  the watcher finds the line, and opens the page that was open.
- **Read-only until enabled**: _Look around first_, or opening a page from the sidebar, dismisses the welcome screen,
  after which a persistent banner remains above every page, with **Show instructions** to bring the welcome screen back.
  It shows again at the next start while the line is still missing. All pages show current values (from IPC + parser),
  conflicts and the generated-config preview. Controls are disabled with "Add the include line to change settings".
- **Line not last:** once enabled, if some of the user's lines come after the include, a banner says how many and that
  they win where they set the same thing; it can be dismissed.

  ```text
  ┌ ⓘ Not enabled yet: add one line to your sway config to change settings. ┐
  │   [Show instructions]                                                   │
  └─────────────────────────────────────────────────────────────────────────┘
  Pointer speed        ──────●────   (disabled)
  pointer_accel 0.30 · from ~/.config/sway/config:120
  ```

## Handover flows for companion tools

When a tool is configured by a hand-written file that we may not write (e.g. `~/.config/waybar/config.jsonc`):

```text
Your waybar uses ~/.config/waybar/config.jsonc (not created by Sway Things).

To manage the bar here, Sway Things keeps its own copy:
  1. [Import as starting point]  (reads your file, changes nothing)
  2. Start waybar with:  waybar -c ~/.config/sway-things/waybar/config.jsonc \
                                -s ~/.config/sway-things/waybar/style.css   [Copy]
```

For tools with include support (sway, kanshi), the flow is the same as the first-run instructions: one line to paste.

## Theming

- Follows the user's **GTK theme**, **system font** and **dark/light preference**.
- **Light and dark are both required.** Every page, dialog, banner and indicator is designed and checked in both, and
  switches live when the preference changes. Our own CSS uses theme colors (named colors or the theme's tokens), never
  hard-coded light or dark values, so a custom GTK theme keeps working.
- **Accent = sway's focus color** (`client.focused` child border, default `#285577`). Changing it on the Windows page
  recolors the app live. An app preference switches back to the theme's accent.
- **App preferences** (accent source, safety-net timeout, editor command, show sway directives) are a "Sway Things"
  section at the bottom of About & Maintenance.
- Our own CSS only for structure (sidebar, rows, cards, banners).

## SwayFX

With SwayFX detected, the Windows page gets an **Effects** section (corner radius, smart corners, blur, shadows, dim
inactive) and the live window preview renders those effects. The Bar and Notifications pages get layer-effect toggles
(blur behind waybar or notifications). On vanilla sway these controls show "Requires SwayFX".

## Accessibility

- Every control has an accessible name (AT-SPI via GTK); icon-only buttons get labels.
- Full keyboard operation, visible focus, no information carried by color alone (indicators have icons and text).
- Text and indicator contrast meets WCAG AA (4.5:1) in light **and** dark, including on the sway accent color.
- Respects text scaling and the user's font.
