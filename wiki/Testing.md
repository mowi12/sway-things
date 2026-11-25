# Testing

> Part of the [architecture](Architecture.md). Which tests exist and where they run.

`just test` (`cargo test --workspace`) runs the unit, golden-file and in-process tests on any machine; it skips the
tests that need a running sway, a display or the stand-ins for D-Bus services, the sound server and CUPS. Those run with
`scripts/check-integration.sh`, in the containers of `ci/containers/`, never on the host. `just check` and
`just check-all` run every check ([CONTRIBUTING](../CONTRIBUTING.md#checks)).

## Unit tests

- The sway config parser (sample configs, the generated files, arbitrary bytes)
- The IPC client against a stand-in for sway (protocol errors, events)
- The device list (replies to `GET_INPUTS` as sway sends them, hotplug event sequences against the stand-in, stored
  devices, what switching devices off would leave)
- The XKB rules registry (a sample registry and extras file, missing, broken, oversized and entity-declaring files,
  arbitrary bytes, and the registry of the system if it has one)
- Registry invariants (every setting reads back each value it renders)
- Search (matching in English and in a translation, ranking)
- Reading values as sway reads them
- Conflict detection (sample configs for each kind of directive, both positions, includes in included files, per-device
  lines)
- State merge (shared + overlay)
- Reload and history
- The file watcher against real files: each change is reported, and a burst of writes once. A busy machine can delay or
  split a change's events across the debounce time, and a second report of the same file is right then, so the tests
  take the reports until they settle and check each one; the burst test passes when one of three bursts is one report,
  which writes reported one by one never are. A failure names the step it came from
- The include detection (every spelling of the line, each status, symbolic links) and its watcher
- Applying a change against the stand-in for sway and a fake validator (each apply mode, failures, changes on disk, the
  changes that wait for a reload or a re-login and the reload of sway, letting the user's config win, the safety net:
  keep, revert, one change at a time, no session, the lockout cases, for a change and for a reset)
- Command builders and escaping (property tests against a port of sway's parser)
- The GTK and icon themes in sample directories (the stylesheet a GTK theme needs, the built-in ones, icon directories,
  hicolor, unreadable files)
- The notification daemons' files (mako's from the stored values with the Do Not Disturb section, a missing mako config
  created with the include alone and the user's never written again, a file of someone else's in our place left alone,
  swaync's config being its default while nothing is stored and the stored settings on top of it, its style with the
  colors and the preset, a hand-made swaync config imported and given back), what starts a daemon (`exec` words, D-Bus
  service files), the include of ours among mako's global options only, finding the window of a clicked notification's
  app (Signal under Xwayland, Zulip and KeePassXC with reverse-DNS names, a Chrome app, Firefox's `firefox-esr`), the
  `con_id` criteria and `seat - cursor set`
- GSettings (a choice to an enum key, a decimal to a double key, what GSettings has shown without a stored value, a
  reset, the hook)
- The doctor against sample setups for each finding (a working setup, the include line missing, partial or unknown,
  lines after it, conflicts per setting and device, problems of the settings files, files missing, out of date after a
  switch from SwayFX to sway, changed outside, foreign or rejected, no sway to validate with, compositors it can't use,
  a stored value GSettings can't take) and its fixes
- The command line in-process against the stand-in for sway and a fake validator (`crates/cli/tests/run.rs`; the
  stand-in comes from the core's `test-support` feature), and the binary in a temporary home with the stand-in in
  `$SWAYSOCK` for the commands that write nothing (`crates/cli/tests/binary.rs`)
- Bluetooth: the snapshot from BlueZ's objects (adapters, devices, battery, names cleaned of control characters), error
  mapping, the agent's checks of PINs and passkeys, rfkill events from a socket and the unblock event, the delegates'
  `PATH` lookup and the UUIDs nm-connection-editor gets
- Network: the snapshot from NetworkManager's objects (devices, speed, addresses and DNS, access points merged into
  networks by SSID, saved networks out of range, VPNs), security from an access point's flags, signal levels and bands,
  SSIDs cleaned of control characters, the checks of SSIDs and passwords, the settings of a new connection (for every
  user, the password kept by NetworkManager, hidden), error mapping, the change stream's noise, which questions the
  secret agent answers; the VPN plugins' `.name` files (service and aliases, a missing or relative auth dialog, files
  others may write), the auth dialog's arguments and input, a VPN's request from its settings, and the key file of
  external UI mode; openconnect's arguments from a profile, its stderr split into lines and prompts, secret fields and
  choices, progress lines, and its result read as nmcli reads it
- Printers: the IPP encoding (requests as RFC 8010 shows them, several values and groups, collections, text with a
  language, broken messages, nesting too deep; round trips and arbitrary or cut-off bytes as property tests), HTTP
  answers (a length, chunked, interim, cut off, garbage), where CUPS is found (local only; its socket while it exists,
  else `localhost:631`), a socket without access told apart from a CUPS that is off, printers and jobs from their
  attributes (reasons, supplies, colors, names cleaned), state in words, queue names made from a model, addresses typed
  by the user, Avahi's TXT records and service URIs, the test page's cross-reference table, the group to suggest when
  CUPS refuses a change (the first of `lpadmin`, `wheel` and `sys` that exists), and the user's default from
  `~/.cups/lpoptions`
- Scanners: the lines of `scanimage -f` and `-L` (sane-airscan eSCL and WSD, escl through ipp-usb, USB and network
  backends, a webcam, none, a backend's noise, a broken line, a device twice, names with control characters), how a
  scanner is connected and its title, the options of `-A` (SANE's test backend, sane-airscan with a feeder, a vendor
  backend with `Binary`, inactive and broken options), the paper sizes that fit and the locale's paper, the formats of
  `--help`, the problems from exit statuses and messages, the PDF writer (JPEG frame headers, PNM bitmaps, gray and
  broken files, run-length round trips, the cross-reference table), the private scan directory and saving next to the
  file, which scanners Avahi finds SANE lists, the hints against fake installations (nothing, scanimage without
  sane-airscan, sane-airscan in a multiarch directory, an IPP-over-USB device with and without ipp-usb, a USB scanner in
  the hwdb without access, udev rules and hwdb lines, `os-release` families) and `doctor`'s _Scanners_ findings
- The XDG desktop portals against fake installations in temporary directories: backends read from their `.portal` files
  (the first directory wins, those meant for the desktop sorted first), a component installed only with its D-Bus
  service and program, the configuration searched like xdg-desktop-portal (per directory, the first desktop's file
  before `portals.conf`), the backend of each interface (an interface's line, `default`, `*`, `none`, the UseIn and gtk
  fallbacks), Sway Things' `sway-portals.conf` wanted only where the user has no file there it would hide, the package
  manager by `os-release`, and `doctor`'s _Environment_ findings; the output chooser's lines for each choice (none
  without its tool or display, no output name that isn't a plain word), the first launcher installed, the file
  xdg-desktop-portal-wlr reads in its order, and its file wanted only where the user has none

## Golden files

Every generator: state in → exact file out (`ci/integration/settings/` → `ci/integration/generated/`), for the oldest
supported sway and SwayFX.

The file for kanshi: a user's own kanshi config in (`ci/integration/kanshi/config`, modeled on a real one with a profile
per combination and its own `exec swaymsg workspace` lines) → `ci/integration/generated/kanshi/sway-things.conf` with
nothing set up on the Displays page and the workspaces placed round robin (`kanshi_fixtures.rs`).

## Fuzzing

The sway config parser with `cargo-fuzz` (`fuzz/`, `scripts/fuzz.sh`), on the stable toolchain without a sanitizer (the
parser has no unsafe code): an input is a main config, and a line `#=> PATH` in it starts a file it can include and a
line `#=\| PATH` puts something there that isn't a regular file, all in `MemoryFiles`, so the fuzzer never touches the
file system. The target checks that the parser ends without a panic, in less than 10 s and 2 GB, and within its limits
(directives, text), that no file is read twice, that nothing but a regular file is read and that every directive and
diagnostic belongs to a file that was read. It starts from the seeds in `fuzz/seeds/` (includes, globs, cycles,
variables, blocks, hostile includes) and the sample configs of `ci/integration/` bundled with their includes, with a
dictionary of the words the parser treats specially. CI lints the fuzz crate and fuzzes each change for 60 s from the
seeds; `.github/workflows/fuzz.yml` fuzzes daily for 15 minutes with the corpus kept in the Actions cache and minimized,
uploads what it finds, and opens a Bug issue per crash signature (where it panicked, or timeout or out of memory) with
the minimized input as base64 and hex, or comments on the open issue with that title. The report job alone may write
issues, and the input reaches the issue only as base64 and hex.

## Install

`scripts/check-install.sh`, in the Debian 13 container: for the default `PREFIX=/usr/local` and for `/usr` as packages
use it, builds and installs into a temporary `DESTDIR`, compares the installed files with the expected list, validates
desktop entry and metainfo together (`appstreamcli validate-tree`), the man pages (`groff` without warnings) and the
completions (`bash -n`, `zsh -n`, `fish --no-execute`), then uninstalls and finds no file left, and checks that an
install for another `PREFIX` than the build's is refused. The man pages and completions themselves are compared with the
generator's output by `crates/cli-docs/tests/fresh.rs`.

## Integration

Headless compositor (`WLR_BACKENDS=headless`, `WLR_LIBINPUT_NO_DEVICES=1`) in CI on **sway 1.10, sway latest and SwayFX
latest** (containers from Debian 13, Arch Linux and Void Linux in `ci/containers/`; SwayFX renders in software on a
virtual DRM node): apply over IPC, read back, validate generated files (incl. `90-effects.conf` on SwayFX):

- The generators' golden files (each passes `sway --validate` on its own and is included by the sample config) and their
  write path with the real validator (`crates/core/tests/generate_sway.rs`)
- The config parser against the modes sway reports (`crates/core/tests/sway_config_sway.rs`)
- A setting changed end to end (`crates/core/tests/apply_sway.rs`): sent, read back, saved, generated into a directory
  the sample config includes, and still in effect after a `reload`
- The `sway-things` binary setting, reading back, showing and resetting settings, with the real `sway --validate` and a
  risky setting set and reset with `--yes`, and `doctor` validating the generated files and replacing a
  `90-effects.conf` with a SwayFX directive (which only SwayFX accepts) with `--fix --overwrite`
  (`crates/cli/tests/cli_sway.rs`, with `XDG_CONFIG_HOME` in another directory the sample config includes)
- The workspaces placed while the user's own kanshi profile sets up the displays (`crates/cli/tests/kanshi_sway.rs`, in
  a headless sway of its own with two displays and a real kanshi): `set workspaces.placement` copies the profile into
  the file of ours without its `exec swaymsg workspace` lines, kanshi applies the user's display setup, the workspaces
  stand round robin, also after a `reload` and after the user changed their profile (that step needs `kanshictl`); not
  on SwayFX, which can't apply a display setup without a render node
- The IPC client's tests (`crates/core/tests/ipc.rs`; all built in the containers, skipped by a plain `cargo test`):
  compositor detection, a command read back, sway's message for a rejected command, version gating, every typed reply,
  tick and workspace events, the device watch on a session without devices
- Fixtures generated from the command builders: each generated config file passes `sway --validate` on its own, every
  IPC command succeeds, and hostile strings set as bar IDs from a config and as seat names over IPC come back unchanged
- The BlueZ backend against python-dbusmock's `bluez5` template on a private bus
  (`crates/core/tests/bluetooth_bluez.rs`, stand-in in `bluetooth::stand_in`, `test-support`): snapshot, BlueZ stopping
  and starting, power, discovery, pair, connect, rename, forget, the change stream without noise, the battery, and the
  agent's flows (confirm, reject, passkey, PIN, display, cancel) with a `Pair` that calls the agent from BlueZ's own
  connection, and a stranger's call rejected
- The NetworkManager backend against python-dbusmock's `networkmanager` template (`crates/core/tests/network_nm.rs`,
  stand-in in `network::stand_in`, extended by an agent manager, a device's `Disconnect`, `ActivateConnection` with "/",
  a `DeactivateConnection` that only touches its own devices and an `AddAndActivateConnection` that keeps the settings
  it was sent): snapshot, NetworkManager stopping and starting (a new owner), Wi-Fi and networking on and off, scans and
  `LastScan` moving on after one, a new network with its password, a saved one connected, disconnected and forgotten, a
  hidden one, a failed connection's reason, a VPN without its password (also one that ends before its activation
  returns), wired and VPN connections, addresses, the change stream without noise, and the secret agent asking again,
  being canceled, leaving other questions (802.1X, WireGuard) to others and rejecting a stranger; with a fake VPN plugin
  (`stand_in::fake_vpn_plugin`, a shell script as auth dialog) it runs the plugin's own window and sends `QUIT`, asks
  for the fields of external UI mode, cancels a canceled dialog and ends one NetworkManager stopped waiting for, and
  reports a plugin without a dialog; with a fake openconnect (`stand_in::fake_openconnect`) it signs in through
  username, password and a TOTP and through a choice, reports a refused login with the server's message, refuses an
  unverified certificate, cancels when the sign-in is dropped, and starts none without openconnect. A test that connects
  a VPN asks the agent for its secrets only once `ActivateConnection` was called, as NetworkManager does
- The notification daemons against stand-ins for mako, swaync and dunst on a private bus
  (`crates/core/tests/notifications_bus.rs`, stand-in in `notifications::stand_in`, `test-support`): the daemon found by
  its name without starting one, Do Not Disturb read and switched for each (mako's mode not added twice) with the change
  reported, reload, a daemon without Do Not Disturb that still takes a test notification, a daemon appearing and going
  away, mako's click (default action), closing swaync's panel
- systemd-timedated against a stand-in on a private bus (`crates/core/tests/region_timedate.rs`, stand-in in
  `region::stand_in`, `test-support`): the clock and the zones read, the time zone, automatic time and the clock set
  with each change reported (also one made elsewhere), a zone it doesn't know, setting the clock while automatic time is
  on, polkit's refusal, no NTP service, and timedated missing
- The automatic time zone against stand-ins for geoclue and timedated (`crates/core/tests/region_geoclue.rs`): the
  location asked at city level under Sway Things' app id, the zone of the nearest city set only when it differs,
  geoclue's refusal (or its wait for an agent) with the drop-in to create, polkit's with the command, and geoclue
  missing
- systemd-localed against a stand-in on the same private bus (`crates/core/tests/region_localed.rs`): the system's
  language and formats read and set, polkit's refusal with the `localectl` command, a variable localed doesn't take, and
  localed missing
- The sound backend against a private PipeWire with WirePlumber (its monitors of ALSA, Bluetooth and cameras off, so it
  never opens the machine's sound cards) and pipewire-pulse, with null sinks and sources and `pacat` as apps
  (`crates/core/tests/sound_pulse.rs`, stand-in in `sound::stand_in`, `test-support`): the devices and defaults,
  switching them, volume with its balance and mute, the apps and their volume, the level meter (also on an input nothing
  records from), and the server going away and coming back
- The printers backend against the distribution's `cupsd` run as the user with a configuration in a temporary directory
  (its own socket, no port, the default policies of CUPS, `SystemGroup` the user's group or `root`'s, printers seeded
  through `printers.conf`), a fake IPP Everywhere printer on the loopback address (`FakePrinter`, answering
  `Get-Printer-Attributes` and taking jobs) and stand-ins for Avahi and cups-pk-helper made with python-dbusmock on a
  private bus (`crates/core/tests/printers_cups.rs`, stand-ins in `printers::stand_in`, `test-support`): an empty CUPS
  and its version, a printer's state, reasons and supplies, adding a driverless printer with `everywhere`, making it the
  default, renaming, moving and sharing it, a test page waiting in a paused queue and canceled, one reaching the printer
  after resuming, removing it, a printer that isn't there failing without leaving a queue, a user canceling their own
  job but not resuming a printer, cups-pk-helper making the changes CUPS refuses (and polkit's refusal, and its
  absence), CUPS not running, and Avahi's printers found, made secure and gone
- The scanners backend against the distribution's scanimage with SANE's `test` backend (a `dll.conf` of its own through
  `SANE_CONFIG_DIR`) and a stand-in for Avahi on a private bus (`crates/core/tests/scanners_sane.rs`, `test-support`):
  the two test devices and their options, scanimage missing, a page as PNG, the ten sheets of the feeder and a page of
  the glass in one PDF, a canceled scan, a fake scanimage that hangs after `Batch terminated` (after a page, and after a
  jam) and is ended, and Avahi's driverless scanners. Each scan of the feeder test has a limit of its own (30 s), so a
  hang says which scan it was; scanimage can stall between two pages of the test backend's feeder
  ([Sway Reference](Sway-Reference.md#sane-scanimage-and-the-driverless-scanners)), so a scan that stalls is killed and
  tried once more, and a second stall fails

## GUI

`crates/gui/tests/gui/` (`main.rs` and a module per area, sharing the fixtures of `common.rs`), run by
`scripts/check-integration.sh` on the headless sway of the sway containers (skipped by a plain `cargo test`; SwayFX
draws no frames in a container): the window with each page open (config lines shown, groups expanded), drawn in GTK's
own theme in light and dark and scrolled through a window height at a time, must reach WCAG AA contrast (4.5:1, large
text 3:1). The check reads what GTK draws: the render nodes say where each text is and which colors are painted under it
(after opacity; the worst stop of a gradient), and the rendered pixels give the text color as the user sees it.

- Keyboard: arrow keys move through the sidebar without opening pages, Enter opens one, Esc goes back from the content.
- Narrow mode: each of the four v0.1 pages, groups expanded, fits next to the sidebar at the threshold with every row's
  controls beside its text (measured from each row's width beside, since a tiled window can't be resized) and the window
  can get narrower; every page fits 544 px (a third of a laptop screen) and scrolls sideways when narrower; a slider row
  shrinks its slider first, then moves its controls below its text and gets higher; narrow (the tests move the
  threshold, since a tiled window can't be resized), the sidebar comes first, opening a page shows it with the back
  button and the focus in it, Esc and the back button show the sidebar at the open page, switching keeps the page, the
  pane with the focus and the focus, a search on the page shows the sidebar with the results and the focused field, a
  result and a deep link show their page and focus their row, and Esc goes back to the result.
- Deep links: a link opens its page, a link to a setting tints its row, focuses its control and the tint goes again, one
  to a member of a collapsed group expands the group first, and one to a row hidden for want of devices only opens the
  page.
- Welcome: without the include line the window opens on the welcome screen with the line, _Look around first_ and
  opening a page lead to the pages with the banner, whose button leads back, its spinner turns again after coming back
  (compared on the drawn pixels, with animations on: a spinner's animation doesn't restart by itself, so the screen puts
  in a new one each time it is shown), the welcome screen and the banner go away once the line is there, and a banner
  tells about lines after the include until dismissed; the welcome screen and a read-only page are checked for contrast
  in light and dark. Text inside insensitive widgets is left out of the contrast check: WCAG exempts inactive controls,
  and GTK draws their text faint on purpose. Animations are off, so screens are checked as they end up.
- Sections: a header collapses its section with a click or Enter and expands it again, `ui.toml` follows, a section
  collapsed in the file starts collapsed, with the preference off none does and nothing is written, and a deep link into
  a collapsed section expands it without storing that.
- Rows: every page shows a row per setting in registry order, each control takes the focus, a change reaches the
  settings, a slider dragged near its default lands exactly on it while one outside the zone and a key step don't, a
  slider keeps its place when the reset buttons appear, a SwayFX setting on sway is locked and says why, a setting that
  applies after a reload has that badge and one that applies after a re-login its own, a change without the include line
  shows its error, and the config line shows only while `app.show_directives` is on, in rows of every page, and follows
  a switch at once.
- Groups: the colors of `client.focused` are the members of one collapsed group row, which shows a swatch per color and
  the line once (not in the members), follows a change, and expands and collapses with `Enter` and `Space`.
- Safety net: switching pointers off shows the banner with _Revert_ focused and the tried value in the row, _Keep_
  stores it, _Revert_, the countdown and closing the window put the old value back, the only pointer can't be switched
  off, and the page with the banner is checked for contrast in light and dark.
- Indicators: a stored value has a reset button and a machine toggle (which moves it to the host overlay; the reset
  removes it), a line before the include shows _Overridden_ and one after it _Your config wins_, both with the ⋮ menu
  showing file:line and the line, _Let my config win_ hands the setting back to the config, a risky reset shows the
  safety-net banner and is removed on Keep, a device's own value resets to the value of all mice, a page reset removes
  all but risky values, an unsupported setting shows its requirement as a badge, and the page with the indicators is
  checked for contrast in light and dark.
- Keyboard page: a layout moved with its variant is one change with one banner and Keep stores both, a layout added is
  gone after Revert, the last layout can't be removed, the swap switch and an option of the browser try the options
  through the safety net, the model is a drop-down, and _Active layout_ lists sway's layouts and switches them.
- Keyboard Shortcuts (`shortcuts.rs`): every binding of the config and of Sway Things is listed and found by keys (also
  typed as _super+shift+q_) and command; a click on a row opens its dialog, a click on the field listens, holding Super
  lists the keys taken with it, Super+P is free and saved in place of the user's binding, Esc stops listening and then
  closes the dialog, _Remove_ stores an unbind and the row's ↶ button takes it back, _Add Shortcut…_ listens at once; a
  line after the include line has a lock with its reason and opens read only, and a key of the volume keys links to
  their switch. The dialog adds a binding and changes one of the user's through the items of `shortcuts.bindings`,
  refuses a bare letter and a sway command sway doesn't know, warns of keys in use and refuses keys of a line after the
  include line or of the volume keys. The page (with a realistic config of over 30 bindings, clashes, a mode, the volume
  keys, a removed and a changed binding and a line after the include line) and the dialog (keys in use, a clash with its
  details, listening with Super held, free keys, read only) are checked for contrast in light and dark.
- Keeping the place (`place.rs`): with the realistic config scrolled down, removing a binding in its dialog, putting it
  back with its ↶ button and changing its keys leave the scroll position as it was and the focus on its row; removing a
  binding of Sway Things moves the focus to the row after it; switching off the volume keys and changing the main
  modifier leave the focused control where it was on screen. The Workspaces map keeps the focus on the number clicked,
  on its new card.
- SwayOSD (`swayosd.rs`): the switch of the volume keys' display shows only with a stand-in `swayosd-client` in the
  model's search path, or while it is stored; switched on, the volume keys run `swayosd-client` and their rows say _with
  an on-screen display_. Its note says how to start `swayosd-server` while nothing does, where the config starts it with
  an `exec` line, and goes away while a stand-in process named `swayosd-server` runs.
- Apps & Autostart (`apps_autostart.rs`): Sway Things' entries, the user's `exec` and `exec_always` lines and the XDG
  autostart entries are one list, a line each with where it comes from; a switch stores the entry with `#` in front, a
  line of the config has a lock instead, and the lists of entries have no rows of their own. A click on a line opens its
  details: a config line's file and line with _Open in Editor_, Sway Things' entry with _Start Now_ and _Remove_, which
  removes it. _Add App…_ lists the apps with a search, and its first item asks for a command, which is stored. The
  terminal shows the config's binding of its keys without a choice, its keys are a button of keys, and a terminal chosen
  takes the place of the config's binding and is bound. The page with entries from every source (with the entries that
  don't start unfolded) and its dialogs are checked for contrast in light and dark; with the XDG directories of a
  desktop (`XDG_DATA_HOME`, `XDG_CONFIG_HOME`, `XDG_CONFIG_DIRS`) they show its apps and entries.
- Environment (`environment.rs`): the four switches have rows and the variables have none, but a list, which shows a
  stored variable; switching SDL 2 on puts its variable before the user's in the file. _Add Variable…_ opens a dialog
  whose _Save_ stays insensitive for `HOME` (with the reason) and for a `PATH` without `/usr/bin`, and stores a `PATH`
  that keeps `$PATH`, which the list then shows. The portal configuration has no row but the _Portals_ row, whose line
  says what the check found and whose _Details_ dialog shows the four steps with the backend of each interface. The
  output chooser has its four choices, and _Display to share_ lists the connected displays after _None chosen_ and
  becomes sensitive once the chooser shares one display.
- Devices: the selectors list the devices of their type (one with stored values only as "not connected"), picking a
  mouse points the rows at it and a change is stored for it in the host overlay, a type without devices and without
  stored values shows its hint (checked for contrast in light and dark), a value stored for the type brings the rows
  back, a touchpad that is plugged in and a mouse that is unplugged (the stand-in sends `input` events) change the page
  live, and the test area names a click and scrolls.
- Displays (against a stand-in that reports outputs): the canvas shows the outputs that are on at their places and the
  one that is off, the picker lists them all with the one with the focus picked, a tile clicked picks its output for the
  rows and the mode row offers its modes, _All displays_ stays picked through a change, an output moved onto another is
  settled next to it with every position stored and sent and Ctrl+Z puts it back, a gap is reported and _Arrange Side by
  Side_ closes it, a monitor plugged in shows up, the last output that is on can't be switched off, without a session
  the canvas gives way to a hint, _Identify_ (the stand-in's output named like the headless sway's own) shows one window
  with the number on the overlay layer and takes it away after three seconds, and the page with the canvas is checked
  for contrast in light and dark.
- Wallpaper: the preview shows the session's display, the pictures of the folder and its subfolders (as chips) are
  offered, and one picked for one display is stored, sent and selected; a picture the user's config sets outside the
  folder (a PNG, and a WebP that can't be read, which gets its warning) is the selected _Default_ with where it comes
  from, and after another picture is picked, the _Default_ tile, _Use Default_ and a page reset each bring it back and
  send the user's line; a wallpaper tool the config starts gets its notice. The preview places the displays as the
  Displays canvas does, side by side with one that is off (not shown), and one above another, turned and at other
  scales: the same places and sizes relative to each other on both pages.
- Notifications (against a stand-in swaync on a private bus, `notifications.rs`): the Do Not Disturb switch becomes
  usable, switches swaync's Do Not Disturb and follows a change made elsewhere, _Send Test Notification_ reaches the
  daemon, and only swaync's sections show.
- Sound: the page against the stand-in sound server shows the outputs, the input and the apps, choosing an output makes
  it the default, the volume slider and mute reach the server, an app's volume too, the server's own changes move the
  slider, the slider goes to 150 % only with `app.sound_above_100`, a server that goes away gives way to the placeholder
  and comes back, and without one the page says how to start it.
- Region & Time (against the stand-in for timedated, `region.rs`): the time zone shows as city and region, the clock
  can't be set while automatic time is on, turning it off reaches timedated and frees the clock, polkit's refusal shows
  the `timedatectl` command and puts the switch back, a change made elsewhere shows, and without timedated the
  placeholder takes the section's place. Language & Formats (against the stand-in for localed): the stored language in
  its row, the preview of the C locale, polkit's refusal of the system's locale with the `localectl` command, and
  without localed _System Default…_ insensitive with the reason.
- About: About & Maintenance shows the system details (Sway Things, the compositor with SwayFX's sway base, GTK, the
  distribution, NetworkManager and CUPS as not running, whether cups-pk-helper is installed, the kernel and the
  delegated tools), _Copy System Details_ copies them as the block `doctor` starts with and says _Copied_ for a moment,
  and the links go to the website, the repository and its issues.
- Generated config: a footer's _Show_ opens the window at its file without writing it, a second one switches the same
  window, it follows a change, About & Maintenance opens it, it closes with the main window; a file changed outside has
  its banner, its content on disk on request and is replaced by Overwrite; a foreign file and, without the include line,
  an edited one can't be overwritten; the window with a banner is checked for contrast in light and dark.
- Reload: a change of a setting for the config only shows the reload banner with the count, a change for the next login
  doesn't add to it, and the reload saves the generated file, sends `reload` and hides the banner; the page with the
  banner is checked for contrast in light and dark.
- Theme: the accent stylesheet carries the stored focus color, follows a change of it and is empty once
  `app.accent_source` asks for the theme's accent. Apps Look & Feel, with GSettings in memory: a theme set with another
  tool shows in its row as GSettings', _Dark_ goes to `color-scheme` as `prefer-dark` and a reset resets the key, and a
  font row shows the stored font in a button.
- Editor: _Open config in editor_ in the ⋮ menu runs the editor command with the file and the line filled in (a quoted
  argument with a space stays one), the welcome screen's runs it with the config, a command with a quote that isn't
  closed isn't stored and its row says why, and an editor that ends with an error shows it, under the row and on the
  welcome screen, until an open works.
- Bluetooth: the page against the BlueZ stand-in shows the paired and the other devices with their state and battery,
  the adapter choice for two adapters, looks for devices while shown, cancels a slow connection and a slow pairing
  (_Cancel_ on the row, `Disconnect` or `CancelPairing` sent, no error, the row back as it was), connects, pairs through
  the agent's dialog and connects, shows no _Search_ button, with searches only on request no discovery runs until
  _Search_, which reads _Stop_ and ends it, and switching back to automatic hides the button and looks for devices
  again, and it stops looking once the radio is off, which, turned off while the headset connects slowly, cancels the
  connection first and keeps the knob off until BlueZ answers; test windows reach no Bluetooth but the stand-in.
- Network: the page against the NetworkManager stand-in shows the known networks (also one out of range) and the other
  networks with their state and security, the wired device and the VPN, asks for scans with the spinner and registers
  the agent while shown, shows no _Search_ button, with scans only on request none runs until _Search_ and one does
  after it, which ends with the new list, and switching back to automatic hides the button and scans again, connects to
  a saved network, asks for the password of a new one (refusing a short one) and sends it, answers the agent's "Wrong
  password" question and the fields of a fake VPN plugin's dialog, walks through a fake openconnect's sign-in (username,
  password, the server's TOTP question) in one dialog, switches the cable and the VPN on, shows an unplugged cable
  without a switch, shows the captive portal note with its button, and stops scanning once Wi-Fi is off; test windows
  reach no NetworkManager but the stand-in.
- Printers: without rights and without cups-pk-helper, the error names cups-pk-helper and the group; _Add Printer…_
  checks the address and adds the fake printer by it, and closes by its button, Escape and the window's close request
  and opens again with its content; without CUPS the page shows its placeholder.
- Printers & Scanners: the lines of the printers (state with the jobs, the default's badge, the ink as a tooltip,
  _Resume_ where it belongs); the details with the job, the supplies and the right buttons; _Cancel_, _Resume_, _Pause_,
  _Print Test Page_ and _Set as Default_ in the details reach CUPS, and a description changed there is saved when they
  close; the details close by _Close_, Escape and the close request. The scanners against scanimage with SANE's test
  backend and Avahi's stand-in: the two test devices and a driverless scanner SANE doesn't list, with the sane-airscan
  hint; none found; scanimage missing, with Debian's package. The scan dialog against the test backend: the options it
  offers, a preview, a scan into PNG, a feeder into a PDF of ten pages plus one from the glass, a cancel mid-scan, a
  scanner that is gone, and the files written (only the one picked). Screenshots of the page, the details, _Add
  Printer…_ and the scan dialog, light and dark.

The window works on settings files in a temporary directory and a sway config in memory, without a session or in the
session of the stand-in for sway (the safety net, the devices, the reload). It never saves by itself, and the files of a
reload are validated with `true`, so nothing runs sway. `scripts/gui-screenshots.sh` runs the same tests on a private
Broadway display and keeps a PNG of the window with each page, light and dark.

`crates/gui/tests/gui_pseudo.rs` opens every page, the welcome screen and the live pages against their stand-ins in a
pseudo-translation and fails on an English word that isn't data, so a string that misses the catalog shows; it needs a
display too and runs with the GUI tests ([Internationalization](UI-Architecture.md#internationalization)).
