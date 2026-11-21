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
│   Bluetooth                │                                           │
│   Network                  │                                           │
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
- **Lines that grow with scrolling:** below the search field, a horizontal line like the separator between the panes
  marks the edge once the page list or the results scroll. At the top it is hidden; over the first 50 px of scrolling it
  grows from the center to the full width, in proportion to the offset, and shrinks again on the way back. It follows
  the scroll position directly, without an animation, and always takes its 1 px, so nothing below it moves.
- A **page** has a header (title + one-line description), sections with setting rows, and a footer showing the files
  that page writes to, each with a _Show_ button that opens it in _Generated config_.
- **Collapsible sections:** every section header has a chevron; a click, `Enter` or `Space` collapses or expands the
  section. Sections start expanded. Which ones are collapsed is remembered per machine across restarts while the
  preference _Remember collapsed sections_ is on (the default); off, all sections are expanded again at the next start.
  Opening a setting through search or a deep link expands its section for the moment without storing that.
- **Generated config** is a window of its own, so it can be tiled next to the app and watched while settings change. A
  tab per generated file shows what Sway Things writes to it for the current settings (also while read-only, since
  showing writes nothing), with its path, and a copy button in the corner of the text, as on a code snippet. A file
  changed outside Sway Things gets a banner with _Show file on disk_ and _Overwrite…_, which asks first and needs the
  include line; a file Sway Things didn't make is never written. The window reads the files again whenever it gets the
  focus. About & Maintenance opens it from its "Generated config" section.
- **About** is the first section of About & Maintenance, part of the page (and so of search and deep links) instead of a
  dialog: a row per line of the system details, with the value selectable for bug reports: the versions of Sway Things,
  the running compositor (for SwayFX with the sway it is based on), GTK (the runtime version), the distribution
  (`PRETTY_NAME`), NetworkManager (or _Not running_), the sound server (or _Not running_) and the kernel, then
  _Installed_ or _Not installed_ for nm-connection-editor, blueman-manager, pwvucontrol and pavucontrol. Below the rows,
  a centered _Copy System Details_ button copies them as a plain-text block in the same order, the one
  `sway-things doctor` starts with, like GNOME Settings' System Details, and then says _Copied_ for two seconds. They
  are read again each time the section shows. Below the button, a centered line of links with a symbolic icon each:
  _Website_ (a globe), _Source Code (GitHub)_ (code brackets) and _Report a Bug_ (a bug). They open in the default
  browser; the app itself makes no network request.
- A **setting row** has a title, optional plain-language subtitle and the control on the right. For sway settings an app
  preference (_Show sway directives_, off by default) adds the **sway directive in monospace** as a line below the
  subtitle (`pointer_accel 0.30`), so users who want to learn the config language see it as they go and the others get a
  quieter page. Switching it changes every row at once.
- **Narrow rows:** a row keeps its controls beside the text while the text keeps 160 px next to them; a slider shrinks
  from 200 to 120 px first. On a narrower row the controls move below the text, aligned to the end, so the text never
  gets squeezed. Each row decides for itself: a switch stays beside its text where a slider has already moved below.
- **Sliders** mark the setting's default on their track and snap to it while dragged with the pointer (within 2 % of the
  range), so the default is easy to hit. Arrow keys, Page Up/Down and the number field stay exact. A display's scale is
  sent only once the pointer is let go and the control stood still for 0.6 s, so steps in a row are one change: the
  display (often the one the app is on) changes size under the pointer with each one, and a change while the button is
  down would move what the pointer is on.
- **Grouped rows:** settings that are values of one directive (the five colors of `client.focused`) share one row with
  the group's title (_Focused window_), a compact summary of their values (a swatch per color, else the values), the
  directive once in monospace (while _Show sway directives_ is on) and a chevron. It starts collapsed; a click, `Enter`
  or `Space` expands it to the members' rows, indented below it, which don't repeat the directive. A link to a member
  expands its group first.

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
- **Keyboard layouts:** below the _Layouts_ row, a row per layout: a drag handle, its name, a drop-down of its variants
  and ✕. Drag a layout by its handle, or press Alt+↑ / Alt+↓ on its row to move it and Delete to remove it. The last
  layout can't be removed. _Add layout…_ opens a search over the layouts and their variants (name, language, country);
  picking a variant adds the layout with it. A layout and its variant change together, as one change through the safety
  net. _Active layout_ switches the layout in use now (only in a session), and _Try it_ is a field to type in.
- **XKB options:** below the _XKB options_ row, the common ones as controls: _Caps Lock key_ (Caps Lock, Ctrl, Escape,
  Swap with Escape, Off), _Swap left Ctrl and Alt_, _Compose key_ and _Switch layouts with_. _All XKB options_ opens
  every option of the XKB rules by group, with a search, inline on the page, so the safety-net banner stays reachable.
- **Mouse & Touchpad test area:** below the sections, a pad that names the last click (_Left click_, _Double-click_,
  _Right click_, …) to try tapping, the button maps and left-handed mode, and a strip of lines to scroll to feel scroll
  speed and natural scrolling.

### Displays

The Displays page arranges the outputs and sets each one up. Every change is live, except that going back to what sway
chooses (a mode, a position or a scale no longer stored) waits for a reload of sway, since sway has no command for it; a
new mode, a rotation and switching an output off go through the safety net.

- **Arrangement:** a canvas shows the outputs that are on as tiles at their places, scaled to fit with room around them
  for half of the smallest output on every side, each with its number, a short name (_Built-in_ for a laptop's panel,
  else the brand, like _Samsung_) and its mode (with the scale and turn when they aren't the plain ones); a tile shows
  only the lines that fit it (the picked one, too small for all, its scale instead of its name, so a change of it
  shows), and the whole name in its tooltip. Outputs that are off stand right of them with a thin grey outline and no
  fill (picked: the outline in the accent) and say _Off_; picked, only _Use this display_ shows below, since nothing
  else applies to a display that is off. Switched on, a display goes to the place stored for it, else right of the
  others in the same trial, never at 0,0 over another one. Below it a hint: _Drag the displays to where they stand on
  your desk. Pick one to change its settings below._ Without a session the canvas gives way to a hint.
- **Dragging:** a tile follows the pointer. Near an edge or the center of another output it snaps, and a guide line in
  the accent runs along the outputs it lines up with; pulled out of a snap twice, that direction stops snapping for the
  rest of the drag. With its center near the edge two outputs share, an accent bar marks that edge instead (only where
  the tile fits in): dropped there, it goes in between, and the outputs behind it in that row, with whatever stands on
  them, make room; taken from that same row, it is a reorder, and what stands below doesn't move. While the bar shows,
  the other tiles slide aside to where the drop puts them, and back once it goes; they move only when the bar comes,
  goes or moves to another edge, never with each motion of the pointer, and the bar holds a little beyond its zone, so
  nothing flickers. Dropped on another output or apart from all of them, it moves to the nearest place next to an edge,
  sharing at least a quarter of it, so the pointer can always cross. Moved away from where it stood (not just nudged),
  the gap it leaves closes: what stood behind it in its row (or column) moves up, it stays next to the output it was
  dropped at, and other gaps stay as they are. After a drop the tiles slide to their places (unless animations are off).
  The whole layout then starts at 0,0 and every output that is on gets its position stored, so it comes back as
  arranged. Ctrl+Z goes back to the arrangement before.
- **Identify:** _Identify_ next to the hint shows each display's number and make and model big in the middle of that
  display for three seconds, and outlines the tiles meanwhile, so it is clear which tile is which screen. A display
  plugged in or out meanwhile takes the numbers away, since they may be wrong then.
- **Scaling:** the picked tile has a grip in its bottom right corner. Dragging it along the tile's diagonal scales the
  output, the tile keeping its proportions and showing the scale; it catches at 100, 125, 150, 175 and 200 % and else
  moves in steps of 5 %, from 50 to 300 %. While it is dragged, the other tiles already stand where the new size puts
  them; released, the scale is stored, and once sway reports the new size the layout flows along as for a scale set in
  the row.
- **While a risky change waits** (a new mode, a rotation, a display switched off), another value of the same setting for
  the same display takes its place (still with the safety net, even if it is the value from before), and Revert still
  goes back to what was before the first. _Automatic_ meanwhile puts back what was before and is stored for the next
  reload of sway, as it is when nothing waits, since sway can't try it live. Meanwhile the tiles can be picked but not
  moved or scaled, and the settings of the other displays and every display's _Show_ wait (their controls say why), so
  Revert puts everything back. A new mode or rotation that changes the display's size moves the others along already
  during the trial, and a display switched off has the gap it leaves closed, so nothing overlaps and nothing floats
  apart while the user decides; Revert puts them back, Keep stores them with the change.
- **Keyboard:** Tab reaches the tiles, Space picks one, the arrow keys move it by 10 logical pixels (100 with Shift),
  settled the same way.
- **Overlaps and gaps:** when outputs overlap or don't touch (a config written by hand, a scale changed elsewhere), a
  warning says the pointer can't reach every display and offers _Arrange Side by Side_.
- **Sizes:** when the scale, rotation or mode of outputs changes through Sway Things (one output, or the scale of all of
  them), the layout flows so that what touched still touches, without overlaps and gaps. A change from elsewhere
  (`swaymsg`) is left as it is.
- **Positions are absolute:** a display plugged in later with other displays (another dock) comes back where it stood;
  if it doesn't touch the others then, the warning and _Arrange Side by Side_ help. Arrangements per set of displays
  come with display profiles.
- **The output's settings:** the _Display_ section starts with _Settings for_: _All displays_ and each output, numbered
  as on the canvas, with "(off)" or "(not connected)". A click on a tile picks it there, and the other way round; at
  first the output with the focus is picked. The rows: _Use this display_, _Resolution and refresh rate_ (two choices:
  the resolution, _Automatic_ and then the output's sizes from the largest down, and beside it the refresh rates of that
  size, like _59.951 Hz_, exactly as sway reports them; a new resolution takes its highest rate), _Scale_, _Rotation_
  and _Position_. Where nothing sets the mode, the scale or the position, the row shows what sway chose with the note
  _Chosen by sway_. _All displays_ offers only the scale: a mode, a rotation, a position or switching off is for one
  display at a time. A new mode, a rotation and switching a display off are tried with the safety net. _Use this
  display_ says what happens to a display that is off when sway starts: if no other is on then, Sway Things switches it
  on again (also at a reload, so `$mod+Shift+c` brings a display back after undocking). A display that is on although it
  is stored as off says _On for now: no other display was on_.
- **Hotplug:** outputs appear and disappear live; the output picked stays picked while it is connected.
- **Remembered per combination:** with kanshi installed, every combination of displays is remembered on its own: what is
  set on the page when it saves is kanshi's profile for that combination, and kanshi brings it back whenever those
  displays are connected together, also without the app. Nothing is named or picked by hand. When the combination
  changes, the page shows what kanshi set up for it, so the rows and the canvas always show what runs; the rows then
  have no reset button or machine toggle. The first time, kanshi is started at once. A note above the canvas says so
  (and that kanshi starts with the next login, if it can't be started yet). Without the include line in an existing
  kanshi config, the note says to put the line at the top, above the user's profiles, and shows the line with _Copy
  Line_ (which says _Copied_ for a moment); without kanshi, it says that installing kanshi makes this happen. The line
  belongs above the user's own profiles: kanshi applies the first profile that matches, so the arrangement made here
  comes first and the user's profiles stay for the combinations never arranged here. While anything of the user's comes
  before the line, the note is a warning with the line to move up, to copy; it names the profile that sets up the
  displays connected now, since every change on the page is undone by that profile at once. The page follows kanshi's
  config, so the warning goes as soon as the line is in place. _Reset page_ forgets what was set here for the displays
  connected (their profile with its pictures and placement, and their stored values): what set them up before, a kanshi
  profile of the user's own or sway, applies again. Our file for kanshi shows per combination where each display stands
  and, in comment lines, which workspaces go on it. Any one display alone always comes on, so no combination stays dark.
- **The user's own kanshi profiles:** displays that a profile of the user's sets up stay theirs, and the workspaces are
  still placed (and pictures mirrored) on them. While a rule places workspaces or a display mirrors another, our file
  holds a copy of each of the user's profiles, kept current with their config: their display lines as they wrote them,
  without the lines that place workspaces, and with the hook that places them as the Workspaces page says. The note
  above the canvas says so ("These displays are set up by your kanshi profile Desk, and Sway Things places the
  workspaces on them. Change something here to arrange them here instead."). A workspace moved on the map is remembered
  for the combination without taking over its displays. `sway-things doctor` warns of a line in one of their profiles
  that may move workspaces too (a script of theirs), and of profiles in a file they include after ours, which can't be
  copied.
- **Workspaces:** which workspace goes on which display is set on the [Workspaces](#workspaces) page.
- **Mirroring (with kanshi and wl-mirror):** the last row of a display's settings, _Show_, says what it shows: _Its own
  content_ or _Picture of …_ another display, and for a picture how it fits (_Fit_, _Cover_, _Exact Multiples_). It is
  there for a display that is on while another one is. The picture starts at once as a fullscreen `wl-mirror` window on
  that display, without taking the focus, and comes back whenever kanshi brings the combination back (no daemon: kanshi
  runs the hook). Meanwhile the display stands apart from the others, 2000 logical pixels right of them, which the
  pointer doesn't cross, and its tile is parked beside the others like one that is off, saying _Picture of …_ (its
  position isn't offered). The picture has a workspace of its own named after the display, so no workspace number leads
  there. The workspaces with windows that stood on the display move to the one shown, as does one that lands there later
  (at the next start, reload or kanshi profile), and go back (the first of them shown again, the display at its place
  again) when it shows its own content again; a display that mirrors takes no workspaces, also not from the placement.
  Without kanshi or wl-mirror the row says what it needs, and a remembered picture that can't start (wl-mirror gone)
  leaves its display and its workspaces as they are. A picture that went (its window closed, `wl-mirror` crashed) comes
  back by itself where Sway Things runs anyway: when the app opens and three seconds after displays came or went, when
  kanshi applies a profile, and at each start and reload of sway. A key can bring it back at once:
  `bindsym $mod+Shift+p exec sway-things hook mirror` in the user's config. While it starts, the focus stays where the
  user works.
- **A new display asks once (with kanshi and wl-mirror):** when exactly one display Sway Things has never seen is
  plugged in to a combination set up on the page (the projector or the TV at a meeting), a notification asks: _New
  display: …_ with _Mirror_ (also a click on the notification, as mako has no buttons) and _Extend_. _Mirror_ shows the
  laptop's own panel on it (else the display with the focus, else the leftmost one), with _Fit_; either answer is
  remembered for the new combination, so the next time it comes back as answered without a question. A dismissed or
  ignored notification (it goes after two minutes) changes nothing: the display extends the desktop, as it would anyway,
  and is asked about again the next time it is plugged in, not before. No question for several displays at once (a dock
  with two monitors, also when they come one after the other within a moment), for a display alone (a laptop with its
  lid closed), for a combination a profile of the user's own may set up, or without a notification server that shows
  actions. A display seen before in another combination shows again the picture it showed there, if that display is
  connected; one that extended the desktop there does so again. A display is known by its make, model and serial, so a
  second unit of a model already seen that reports the same serial (some projectors report one for every unit) counts as
  seen and isn't asked about; plugged in again while its question waits, it isn't asked a second time. Without a
  notification server, or one without buttons, kanshi's log says why nothing was asked.

### Workspaces

The numbered workspaces you reach with `$mod+1` … `$mod+0`, and where they are.

- **Place workspaces:** a choice for the whole page.
  - _Off (sway decides)_, the default: sway's own behavior, a new workspace on the display with the focus. Nothing is
    placed, and the map is hidden.
  - _Round robin, left to right_: 1 on the first display, 2 on the second, … (1 3 5 · 2 4 6 with two).
  - _A block per display_: 1–5 on the first of two displays, 6–10 on the second.
  - _All on one display (the laptop's, or the leftmost)_.
  - _Fixed, as you arrange them_: remembered for each combination of displays; one seen for the first time starts round
    robin.

  A rule is worked out again for whatever displays are connected, so it fits at home, at the office and on the laptop
  alone without any setup per combination. A display that shows another one's picture takes no workspaces: while a rule
  places them, starting or ending a picture places them anew and reloads sway at once, without asking (sway keeps the
  displays it was told for each workspace until it reloads, and a number that still led onto the picture would hide it),
  so no number leads onto the picture and every number is back on its display when the picture ends. After placing,
  every display shows one of its own workspaces, so the number of another display's workspace never leads there.
- **Workspaces placed:** how many, from 1 (default 10, up to 20). sway's own keys reach 1 to 10; keys for more can be
  added on Keyboard Shortcuts.
- **The map:** below the rows, a card per display from left to right (two displays of one brand with their connector)
  with the numbers it holds. A click on a number sends it to the next display, dragging puts it on another card; either
  makes the placement _Fixed_, starting from what the rule gave. With one display there is nothing to arrange, and the
  map says so.
- **Applied:** open workspaces move at once. sway only adds to the displays it knows for a workspace, so the ones that
  aren't open follow with the next reload: the change waits for it like other settings, with the reload banner.
- **When displays come and go:** every workspace gets the other displays after its own, from left to right
  (`workspace 3 output DP-1 eDP-1`). Unplugging a display moves its workspaces to the next one; plugging it back in
  moves them back. With kanshi, each combination places the workspaces again as the rule says; every start and reload of
  sway does too. Without kanshi, a combination seen for the first time is placed at the next start or reload.
- **New workspaces:** _Layout of new workspaces_, _Split of new workspaces_ and _Back and forth_, which were on Window
  Behavior.

### Keyboard Shortcuts

GNOME Settings' _Keyboard Shortcuts_ is the model: a quiet list to find a key and what it does, with everything else one
click away in the binding's dialog.

```text
 SHORTCUTS                                                             ⌄
 ┌ 🔍 Search by action or keys ──────────────────┐ ┌ + Add Shortcut… ┐

 Windows
 ┌──────────────────────────────────────────────────────────────────┐
 │ Close the window                          [Super] + [Shift] + [Q] │
 │ Split horizontally            Removed  ↶        [Super] + [B]     │
 │ Toggle fullscreen                       ⚠     [Super] + [F]       │
 └──────────────────────────────────────────────────────────────────┘
 Media
 ┌──────────────────────────────────────────────────────────────────┐
 │ Raise the volume                         🔒       [Volume Up]     │
 └──────────────────────────────────────────────────────────────────┘
```

- **One list of every binding, one line each:** the user's and those of Sway Things, in few groups by what they do
  (_Windows_, _Workspaces_, _Apps_, _Media_, _System and Other_) and then one per mode (_Mode “resize”_); empty groups
  are left out. A row says what the binding does in words on the left (_Close the window_, _Raise the volume_, _Lock the
  screen_, _Take a screenshot of an area_; a command without words shows as it is, in monospace, cut off with its whole
  text as the tooltip) and its keys as keycaps on the right, right-aligned so they form a column, the way people read
  them (`Super` + `Shift` + `E`, `Enter` for `Return`, `Volume Up`, `−` for `minus`, a keycode by the key the keyboard
  gives it). Nothing else is on the row unless there is something to say, and then only as a small sign before the keys,
  with the reason as its tooltip (and its accessible description):
  - a **warning sign** when the binding isn't in effect, or only on the lock screen: a later line on the same keys takes
    its place, an unbind of the user's removes it, a keycode binding of the same key wins (the keyboard's keymap tells),
    or a `--locked` binding runs only on the lock screen next to one without it;
  - a **lock** when it can't be changed here: a line read after Sway Things' file, a binding of a mode, keys that need
    quotes, a key of a media key group (_Part of the volume keys, switched on below._);
  - _Removed_ and the keys struck through for a binding of the user's that Sway Things removes;
  - a round **↶ button** (_Back to Your Config's Binding_) on a binding of the user's that Sway Things changed or
    removes, which puts it back at once, like GNOME's reset of a changed shortcut.
- **Search** sits above the list, as wide as it: it matches what a binding does (in the user's language and in English),
  its command, its mode and its keys, as written and as shown, so _super enter_, _super+shift+q_ and _Super + Shift + Q_
  all find theirs; every word must match. Without a match the list gives way to _No Shortcuts Found_ with a hint (_No
  Shortcuts Yet_ when nothing binds any key).
- **A click on a row opens its dialog** (Enter and Space too: the rows are activatable). _Add Shortcut…_ opens the same
  dialog empty. It shows, from top to bottom:
  - the **keys** as large keycaps in a field; a click (or, for a new shortcut, opening the dialog) makes it listen:
    outlined in the accent, _Press the new shortcut…_ with _Esc to cancel · Backspace to clear_. Sway passes every key
    to it meanwhile. While modifiers are held, a box below the field lists the **keys taken** with exactly those
    modifiers, as small keycaps with what each does as its tooltip (_Taken with Super:_ `1` `2` `D` `Space` …), so a
    free key is easy to pick; without a modifier held it says to hold one;
  - once keys are pressed, a line says whether they are **free** (a check, _These keys are free._), **in use** (a
    warning sign, _In use by “Close the window” (file:line). Saving puts this in its place._) or **taken** by a binding
    that can't be replaced here (an error sign; Save stays off). The binding's own keys say nothing;
  - the binding's **clashes** in words, each with a warning sign (_Takes the place of fullscreen at file:line_);
  - _Action_: what the binding does (open an app, run a command, focus or move a window, go to or move to a workspace,
    close, fullscreen, floating, or a sway command, which must start with a command sway knows), _Also on the lock
    screen_ and _The same key in every keyboard layout_. A key that types (a letter, Enter) needs Super, Ctrl or Alt;
    F1, Print and the media keys work alone. AltGr counts as `Mod5`. Why it can't be saved shows in the line below the
    keys, with an error sign;
  - _Details_, collapsed: the command (monospace, selectable), its options in words (_also on the lock screen_, _the
    same key in every layout_, _by keycode_, _on release_), _Defined in_ with the file and line and _Open in Editor_,
    and what Sway Things made of it (_Added here_, _Changed here; your config binds …_, _Removed here_). For a binding
    of the user's, a note says their config stays as it is;
  - the buttons: _Remove_ and _Back to Your Config's Binding_ at the start, _Cancel_ and _Save_ at the end. Enter saves,
    Esc closes (while the field listens, Esc only stops it). The change applies at once, without a reload.
- **A binding that can't be changed here** opens the same dialog to look at: its title is what it does, the keys show
  without listening, a lock says why (_Read after Sway Things' file, so it can only be changed in your config._), the
  details are expanded with _Open in Editor_ as the main button, and _Close_. A binding of another setting links to it:
  a key of a media key group has _Show Switch_, which closes the dialog and points to its switch; a key that a setting
  of another page owns gets _Open_ and the name of that page, which opens it at the setting.
- **Never in the user's file:** a binding of the config is removed with `unbindsym` in Sway Things' file and, for a
  change, bound again there; _Back to Your Config's Binding_ undoes it.
- **Main modifier:** _Super_ or _Alt_ for what `$mod` means in the bindings Sway Things writes. The user's own lines
  keep the `$mod` they were read with, since sway replaces a variable when it reads a line; the row says so.
- **Media keys:** a switch each for the volume (`wpctl`), brightness (`brightnessctl`) and media player keys
  (`playerctl`), shown only while the program is installed (or the switch is on, so it can be switched off). Below a
  switch, what its keys replace of the user's config or what wins over them.
- **Later:** editing bindings of modes, touchpad gestures (`bindgesture`), a cheat sheet to print.

### Sound

The Sound page shows the sound server's live state (pipewire-pulse or PulseAudio); nothing of it is stored by Sway
Things, so it has no reset, no indicators and no generated file. Every change applies at once: the server remembers it.

- **Output:** _Output device_, a drop-down of the devices sound can play on, sorted by name, the default chosen (a
  default the page doesn't list, such as one not known yet, shows as _Another device_, and choosing a listed one makes
  it the default); _Connector_ (speakers, headphones, line out) where the device has more than one that is plugged in;
  _Volume_ with the mute button, the slider and the level in percent.
- **Input:** _Input device_ (the monitors of the outputs aren't offered), _Connector_, _Volume_ with mute, and _Input
  level_, a meter that moves with the default input while the page is on screen: not on a hidden workspace, where the
  window is suspended, so it doesn't keep the microphone in use ("Speak to test the microphone"). While the meter runs,
  a privacy indicator may show the microphone in use, since the meter does read it; a Bluetooth headset stays in its
  music profile (A2DP), because WirePlumber doesn't switch it for a meter. A monitor of an output as the default input
  shows as _Another device_.
- **Apps:** a row per app that plays or records: its icon, its name, what it plays (or _Recording_), _on_ the device
  when it isn't the default, and its volume with mute. A new row goes where it sorts; the rows already there are never
  moved or rebuilt, so a slider being dragged or focused keeps its row. Screen readers hear each control with its app
  ("Volume of Firefox", "Mute Firefox"). Without any: "No app is playing or recording sound."
- **Sound cards:** for each card that offers a choice, its available profiles (HDMI, a headset's A2DP or HFP), under
  "Connectors and sound quality". _Pro Audio_ is left out unless it is in use: its raw channels don't route HDMI or
  headphones the usual way, so it is a trap for anyone who isn't setting up a studio (Advanced… still offers it).
- **Sliders:** a move is sent at once, and what the server reports while the slider moves, while a request is on its way
  and a moment after doesn't move it back. A change the server refuses (a profile the headset can't run) shows its
  error, and the drop-down goes back to what the server has. Sliders go to 100 %; the app preference _Allow volume above
  100 %_ lets them go to 150 % with a mark at 100 %. A volume set above the range elsewhere still shows, with the slider
  reaching it.
- **Long names:** a drop-down is as wide as its names need, up to 40 characters, and shrinks to 12 on a narrow page; a
  longer name is shortened with an ellipsis, whole in its list and its tooltip, so the page fits a third of a laptop
  screen.
- **Advanced…:** in the header, while pwvucontrol or pavucontrol is installed (pwvucontrol first): balance, latency
  offsets and the routing of every stream stay there.
- **Not available:** without a sound server, a placeholder says how to start PipeWire's
  (`systemctl --user start pipewire-pulse`); the page looks again every three seconds and comes back by itself, also
  after the server restarts.
- The volume of alert sounds isn't offered: see [Architecture → Backends](Backends.md).

### Bluetooth

The Bluetooth page shows BlueZ's live state; nothing of it is stored by Sway Things, so it has no reset, no indicators
and no generated file.

- **Bluetooth section:** a switch for the radio, its subtitle the name other devices see and the address, or _Off_. With
  two or more adapters an _Adapter_ drop-down comes first (not remembered; the first adapter after a restart). A hard
  block (a switch or key) locks the switch. Without access to `/dev/rfkill`, the page shows `rfkill unblock bluetooth`
  with _Copy_. A ⋮ menu next to the switch has _Rename…_, the device dialog for the adapter's name (empty: the adapter's
  own name). Below, _Visible to other devices_ lets other devices find the computer; its subtitle says how long it stays
  visible (BlueZ's `DiscoverableTimeout`) and the switch follows BlueZ when the time is up. Both act on the selected
  adapter and are insensitive while the radio is off.
- **Off:** while the radio is off, a placeholder _Bluetooth is off_ (icon, title, text) replaces the device sections
  (_My devices_, _Other devices_) and points to the switch. The Bluetooth section stays, with _Visible to other devices_
  and the ⋮ menu insensitive. A soft block (airplane mode, `rfkill`) says so and that the switch lifts it; a hard block
  says how to turn the radio back on. The devices come back as soon as the radio is on.
- **My devices:** the paired devices, connected first: icon, name, _Connected_ or _Not connected_ with the battery
  level, _Connect_ or _Disconnect_, and a ⋮ menu with _Rename…_ and _Forget…_ (which asks first).
- **Other devices:** the devices around, as BlueZ knows them. _Connect_ pairs, trusts and connects in one step.
- **Search:** see [Searching for devices and networks](#searching-for-devices-and-networks).
- **Pairing dialogs:** compare a code (_Pair_ / _Cancel_), type a shown code on a keyboard (the digits typed so far are
  underlined), enter a PIN or a code, or allow a device without a code. A dialog closes when the device cancels.
- **Advanced…:** in the header, while blueman is installed: starts blueman-manager for what the page doesn't do.
- **Not available:** without BlueZ or without an adapter, the page shows a placeholder and updates by itself once either
  appears.

### Searching for devices and networks

Bluetooth and Network search the same way. The app preferences _Search for Bluetooth devices_ and _Search for Wi-Fi
networks_ choose per page between two modes:

- **Automatic** (the default): Bluetooth discovers the whole time the page is on screen and the radio is on; Wi-Fi asks
  for a scan every 15 s. A spinner with _Searching_ next to _Other devices_ and _Other networks_ says so. There is no
  _Search_ button.
- **Manual:** opening the page shows what BlueZ and NetworkManager already know, and nothing searches by itself. A flat
  _Search_ button with a refresh icon at the right of the heading starts one search: a Bluetooth discovery of 30 s, or
  one Wi-Fi scan that ends when NetworkManager reports the new list. While it runs, a spinner takes the icon's place and
  the button reads _Stop_. The button is insensitive while the radio is off.
- Either way a search stops when the page goes off screen, the radio is turned off or another adapter is chosen.
  Switching the preference shows or hides the button at once.

### Network

The Network page shows NetworkManager's live state; like Bluetooth it has no reset, no indicators and no generated file.

- **Captive portal:** while NetworkManager reports one, a note at the top says the network wants a sign-in, with _Open
  Login Page_ (NetworkManager's connectivity check address in the browser) when NetworkManager has that address.
- **Wi-Fi section:** a switch, its subtitle the network it is connected to or _Off_. With two or more adapters an
  _Adapter_ drop-down comes first (not remembered). A hardware block locks the switch. Without a Wi-Fi adapter the
  section is a hint, and the wired and VPN sections stay usable.
- **Wi-Fi off:** a placeholder _Wi-Fi is off_ (icon, title, text) replaces the known and other networks and points to
  the switch; the wired and VPN sections stay. A hardware block says how to turn Wi-Fi back on. The networks come back
  as soon as Wi-Fi is on.
- **Known networks:** every saved network, the connected one first: a signal icon (four bars), the name, the state and
  security as text (_Connected · WPA2_), _Connect_ or _Disconnect_, and a ⋮ menu with _Details…_ (while connected),
  _Edit…_ (nm-connection-editor) and _Forget…_ (which asks first). Saved networks that aren't around say _Not in range_.
- **Other networks:** the networks around, by signal level and name; for the search see
  [Searching for devices and networks](#searching-for-devices-and-networks). _Connect_ on a secured network asks for the
  password: a field with the eye, checked as NetworkManager checks it, and the note _Saved for all users of this
  computer._ 802.1X and WEP networks offer _Set Up…_, which opens nm-connection-editor. The last row is _Connect to
  Hidden Network…_: name, security (none, WPA & WPA2 Personal, WPA3 Personal) and password.
- **Wrong password:** while the page is on screen, and until a connection it started has finished, NetworkManager asks
  the page when a password is missing or wrong: _Wrong password for “Home”_ with a password field. Canceling gives up
  the connection.
- **Details:** a dialog with selectable values: signal, band, security, link speed, addresses, gateway, DNS, hardware
  address.
- **Wired:** a row per device with its state, speed and address and a switch. While the cable is unplugged the row only
  says _Cable unplugged_, without a switch.
- **VPN:** shown when there are VPN or WireGuard connections: each with its type and state and a switch. A VPN that asks
  for a password or code when it connects gets it through its plugin's auth dialog: the plugin's own window, or, when
  the plugin supports it, a dialog of the page, _Sign in to VPN “Office”_ (_… again_ after a failed attempt), with the
  plugin's text and its fields (a password field for a secret, a text field with the current value otherwise) and
  _Cancel_ / _Connect_. Canceling gives up the connection. Without the plugin's dialog the switch fails with an error
  that names the plugin and says its GNOME package provides the dialog. An OpenConnect VPN signs in through one dialog
  of the page that walks through the server's steps: _Sign in to VPN “Uni”_ with the server's message and the field it
  asks for (username, password, a code; a drop-down for a choice) and _Cancel_ / _Continue_, then _Signing in…_ with a
  spinner until the next field, or _Continue the sign-in in your browser…_ for a web sign-in; it closes once signed in.
  A failed sign-in shows the server's message in the switch's error; a server certificate that fails verification is
  refused.
- **Advanced…:** in the header, while nm-connection-editor is installed.
- **Not available:** a placeholder while NetworkManager isn't running or there is no adapter and no VPN at all, and
  _Networking is turned off_ with _Turn On_; the page updates by itself.

### Sidebar groups

| Group | Pages |
| --- | --- |
| **Devices** | Mouse & Touchpad · Keyboard · Cursor & Tablet · Displays · Sound · Bluetooth · Network |
| **Appearance** | Windows · Wallpaper · Apps Look & Feel · Bar |
| **Desktop** | Window Behavior · Workspaces · Keyboard Shortcuts · Notifications · Apps & Autostart · Screenshots & Clipboard |
| **System** | Power & Lock · Brightness & Night Light · Region & Time · Accessibility · Environment · About & Maintenance |

A thin line, inset to the rows' text, separates the groups: above every group heading but the first, none between the
pages of a group and none in the search results. Like the headings, the lines take no focus, so the arrow keys skip
them.

## Navigation and search

- **Search finds individual settings, not just pages.** Each registry entry has a title, keywords and synonyms ("dpi",
  "sensitivity" → _Pointer speed_; "caps lock" → _XKB options_). Choosing a result opens the page, scrolls to the
  setting and highlights it briefly.
- **Results replace the page list** while the search field has text. Each shows its title and where it is (_Mouse &
  Touchpad › Touchpad_, or the group of a page); text too long for the sidebar ends in an ellipsis, and the sidebar
  keeps its width. Titles rank before keywords, subtitles and sway directives, and every page is found by its name, with
  settings or without. The results stay after one is opened, so the next one is a click away. `Enter` in the search
  field opens the best result, `Down` moves into the results (or the pages), `Up` from the first one goes back to the
  field. From the page, `Esc` goes back to the result, and once more clears the search.
- **Keyboard-first:** typing anywhere (or `Ctrl+F`) focuses search; arrow keys move through the sidebar without opening
  pages; `Enter` (or a click) opens one; `Esc` clears the search or goes back from the page to the sidebar. Every
  control is reachable with `Tab`.
- **Every page is listed** from the start. A page with settings in the registry shows them in rows, by section; a page
  that isn't built yet shows its header and a placeholder saying so.
- **Narrow mode:** sway often gives the window half a screen or less. Below **960 px** the app shows one pane at a time:
  the sidebar, then the page with a back button (an arrow) above it. With a larger font the threshold grows to what the
  sidebar and the page need side by side.
  - The threshold is the sidebar (240 px plus a 1 px separator) next to the widest page with every row's controls beside
    its text, with the default fonts of Debian (DejaVu Sans) and Arch Linux, which are wider than most: Window Behavior
    needs 705 px with DejaVu Sans (712 px on Arch), for the drop-down of the floating modifier's buttons; Windows 682
    px, Mouse & Touchpad and Keyboard 634 px.
  - Every page fits a tile a third of a laptop screen wide (544 px: 1646 px at 175 %, less the borders) without
    scrolling sideways; the rows move their controls below the text there. With DejaVu Sans the pages need 436 px (Mouse
    & Touchpad, Keyboard) to 517 px (Window Behavior). The page's margins shrink from 32 px to 12 px below 544 px. A
    window narrower than the open page scrolls it sideways instead of cutting it off.
  - Opening a page (`Enter`, a click, a search result, a deep link) shows it and moves the focus to its first control.
    `Esc` or the back button shows the sidebar again, with the focus on the open page or the opened result.
  - Below the back button, the same line as below the search field grows as the page scrolls. It exists only in narrow
    mode, together with the back button.
  - Typing or `Ctrl+F` on the page shows the sidebar with the search field.
  - The window switches live while it is resized. The open page stays, and so does the pane with the keyboard focus (the
    page if neither has it); the other pane can't be reached with `Tab` while hidden.
- **Deep links:** `sway-things open displays`, `sway-things open input.keyboard.repeat_rate`: a page id or the key of a
  setting. Waybar modules, bindings and launchers can jump straight to a page or setting; a setting's row is scrolled
  into view (its group expanded), focused and highlighted briefly. A setting of an input type without devices opens just
  the page.
- **Single instance:** starting the app again focuses the existing window and switches page.
- **Stable app_id** `io.github.mowi12.SwayThings`, so users can write rules for it, e.g.
  `for_window [app_id="io.github.mowi12.SwayThings"] floating enable`.

## Apply model

- **Instant.** No Apply buttons anywhere. Changes take effect live and are saved after a short debounce, as in
  mainstream settings apps; the safety net keeps that from locking anyone out.
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
- **Badges** for settings that can't apply live: "Applies after re-login" (environment variables, Xwayland), "Applies
  after reload".
- **Reload banner:** once a change that applies after a reload is saved in a running session, a banner above every page
  counts the waiting changes:

  ```text
  ┌────────────────────────────────────────────────────────────────────────┐
  │ ⟳ 2 changes apply after sway reloads its config      [Reload sway…]   │
  └────────────────────────────────────────────────────────────────────────┘
  ```

  _Reload sway…_ always asks first: a reload starts the bars again and runs the `exec_always` lines again. If a setting
  sway only reads at login changed (Xwayland), the question also says that sway reports its line as an error after the
  reload until the next login. The button can't be pressed while a risky change waits for _Keep_ or _Revert_. A change
  set back to the value sway runs with no longer counts, and the banner goes after any reload of sway, also one from a
  key binding or `swaymsg reload`. Without a session nothing waits: sway reads the files when it starts.

## Per-setting indicators

A row stays lean: status as small badges (icon and text) under the sway line, actions behind buttons next to the
control. Badges are labels, never buttons. The reset and _This machine only_ buttons keep their room while hidden, and
so does the ⋮ menu in rows whose setting the user's config sets too, so a control never moves when they appear: a slider
being dragged stays under the pointer when its first change stores a value.

| Indicator | When | Shown as | Actions |
| --- | --- | --- | --- |
| **Reset** | the value is stored in Sway Things | ↺ button | removes the value (for one device: back to the value of all devices of the type); a risky setting goes through the safety net |
| **This machine only** | the value is stored | computer toggle, pressed for the host overlay; new values of device- and output-bound settings start there | moves the value between `settings.toml` and the host overlay |
| **Your config wins** | a line of the user's config after our include overrides the stored value | badge marked for attention (warning icon and border) and a ⋮ menu | the menu shows file:line and the line, then _Let my config win_, _Open config in editor_, _Copy line_ |
| **Overridden** | a line of the user's config before our include is dead while the value is stored | plain badge and a ⋮ menu | as above |
| **Needs another setting** | the setting only matters while another one is set (blur passes while blur is on) | the control is disabled, the reason under the line (_Turn on Blur first_) | none; the stored value stays |
| **Requires SwayFX** / **Requires sway ≥ x.y** | the running compositor lacks the setting | badge with a lock; the control is disabled, with the reason as tooltip | none |
| **Requires _tool_** | backend tool not installed | page stays visible but disabled, with install commands for zypper / dnf / apt / pacman | (comes with the pages that need backends) |
| **Managed externally** | something else is in charge (e.g. a custom wallpaper script, file:line) | explanation + handover | Wallpaper: a notice per `exec` line that starts a wallpaper tool, with _Open config in editor_; other pages with their backends |

- The ⋮ button appears only on a row with a conflict. _Let my config win_ isn't offered for risky settings; their reset
  goes through the safety net. _Open config in editor_ opens the file at the line in the
  [user's editor](#opening-the-config-in-an-editor).
- **Reset page…** in the page header removes every stored value of the page after asking. Values of risky settings stay:
  the question names them and says to reset them from their rows, so a page reset never needs the safety net. The button
  can't be pressed while there is nothing it would remove. What the user's config sets comes back at once, as with _Let
  my config win_; the wallpaper collections stay.
- **Back to the default** on the Wallpaper page: the default (the user's own `bg` line, else none; for one display, the
  wallpaper of all displays) is always the first tile of the pictures, wherever its file lies, and _Use Default_ beside
  what the edited display shows goes back to it. Trying a picture never loses the way back. A fit or color chosen while
  the user's own picture shows changes how that picture is drawn; it never replaces the picture with a color.
- Indicators follow the state, the user's config and the include status live.
- **Notices** say something about a page as a whole rather than one row: the captive portal (Network), kanshi and
  displays that overlap (Displays), apps that can't read the settings (Apps Look & Feel). Every page draws them alike,
  as a framed box like a section with an icon (information, or a warning when something keeps the page from working),
  the text and, where there is something to do, one button.

## First run: instructions only

The app **never** adds the include line itself: not touching a hand-written file is the core promise, and one copied
line is little to ask for it. On first run (or whenever the line is missing) it shows:

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
  has a **Copy** button for the line, **Open config in editor** (the [user's editor](#opening-the-config-in-an-editor))
  and _Waiting for the line…_. It closes by itself once the watcher finds the line, and opens the page that was open.
- **Read-only until enabled**: _Look around first_, or opening a page from the sidebar, dismisses the welcome screen,
  after which a persistent banner remains above every page, with **Show instructions** to bring the welcome screen back.
  It shows again at the next start while the line is still missing. All pages show current values (from IPC + parser),
  conflicts and the generated-config preview. Controls are disabled with "Add the include line to change settings". So
  users see what the app would manage before they enable it, and it works as a viewer of their config.
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

Planned with the Bar, Notifications and Power & Lock pages. When a tool is configured by a hand-written file that we may
not write (e.g. `~/.config/waybar/config.jsonc`):

```text
Your waybar uses ~/.config/waybar/config.jsonc (not created by Sway Things).

To manage the bar here, Sway Things keeps its own copy:
  1. [Import as starting point]  (reads your file, changes nothing)
  2. Start waybar with:  waybar -c ~/.config/sway-things/waybar/config.jsonc \
                                -s ~/.config/sway-things/waybar/style.css   [Copy]
```

For tools with include support (sway, kanshi; both built), the flow is the same as the first-run instructions: one line
to paste.

## Opening the config in an editor

The app doesn't fix a conflict itself, since it never edits the user's config; opening the file at the line keeps the
fix one click away. _Open config in editor_ (welcome screen, ⋮ menu of a conflict) follows the _Editor command_
preference:

- **A command** is a program with its arguments, split like a shell splits words (quotes and backslashes group them) but
  never run by a shell, so nothing in it is expanded. `{file}` and `{line}` are filled in (`foot nvim +{line} {file}`,
  `code -g {file}:{line}`); without `{file}` the file comes last, and without a known line (the welcome screen) an
  argument with `{line}` is left out. A command with a quote that isn't closed is refused with the reason on the row.
  Some terminals need `-e` before the editor (Ghostty, Alacritty: `ghostty -e nvim -c {line} {file}`), while foot takes
  the editor directly. Ghostty swallows `+LINE` arguments as its own, so with Ghostty (also as `$TERMINAL`) set the
  command in the field and use `-c {line}` for vim and nvim.
- **Empty** (the default) means `$VISUAL`, else `$EDITOR`, with `+LINE` and the file. Those are terminal editors as a
  rule, so they run in a terminal: `xdg-terminal-exec` if it is installed, else `$TERMINAL -e`. Without an editor
  variable or a terminal, the desktop's default application for text files opens the file.
- An editor that can't start, or ends with an error, says so where the button is, until an open works.

## Theming

- Follows the user's **GTK theme**, **system font** and **dark/light preference**.
- **Light and dark are both required.** Every page, dialog, banner and indicator is designed and checked in both, and
  switches live when the preference changes. Our own CSS uses theme colors (named colors or the theme's tokens), never
  hard-coded light or dark values, so a custom GTK theme keeps working.
- **Accent = sway's focus color** (`client.focused` child border, default `#285577`). Changing it on the Windows page
  recolors the app live, so the accent is personal to each setup. An app preference switches back to the theme's accent.
- **App preferences** (accent source, safety-net timeout, editor command, show sway directives, remember collapsed
  sections, volume above 100 % on the Sound page, and whether Bluetooth and Wi-Fi search automatically or only with
  _Search_) are a "Sway Things" section at the bottom of About & Maintenance. There search, the CLI and reset work for
  them like for any setting, without a separate preferences window in a tiling window manager.
- Our own CSS only for structure (sidebar, rows, cards, banners).

## SwayFX

With SwayFX detected, the Windows page gets the effect sections **Corners**, **Blur**, **Shadows**, **Dim inactive** and
**Animations** (SwayFX ≥ 0.6). The Bar and Notifications pages (planned) get layer-effect toggles (blur behind waybar or
notifications). On vanilla sway these controls show "Requires SwayFX".

## Accessibility

- Every control has an accessible name (AT-SPI via GTK); icon-only buttons get labels.
- Full keyboard operation, visible focus, no information carried by color alone (indicators have icons and text).
- Text and indicator contrast meets WCAG AA (4.5:1) in light **and** dark, including on the sway accent color.
- Respects text scaling and the user's font.
