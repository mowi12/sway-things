# Testing

> Part of the [architecture](Architecture.md). Which tests exist and where they run.

`just test` (`cargo test --workspace`) runs the unit, golden-file and in-process tests on any machine; it skips the
tests that need a running sway, a display or the stand-ins for D-Bus services and the sound server. Those run with
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
- The file watcher against real files
- The include detection (every spelling of the line, each status, symbolic links) and its watcher
- Applying a change against the stand-in for sway and a fake validator (each apply mode, failures, changes on disk, the
  changes that wait for a reload or a re-login and the reload of sway, letting the user's config win, the safety net:
  keep, revert, one change at a time, no session, the lockout cases, for a change and for a reset)
- Command builders and escaping (property tests against a port of sway's parser)
- The GTK and icon themes in sample directories (the stylesheet a GTK theme needs, the built-in ones, icon directories,
  hicolor, unreadable files)
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
  hidden one, a failed connection's reason, a VPN without its password, wired and VPN connections, addresses, the change
  stream without noise, and the secret agent asking again, being canceled, leaving other questions (802.1X, WireGuard)
  to others and rejecting a stranger; with a fake VPN plugin (`stand_in::fake_vpn_plugin`, a shell script as auth
  dialog) it runs the plugin's own window and sends `QUIT`, asks for the fields of external UI mode, cancels a canceled
  dialog and ends one NetworkManager stopped waiting for, and reports a plugin without a dialog; with a fake openconnect
  (`stand_in::fake_openconnect`) it signs in through username, password and a TOTP and through a choice, reports a
  refused login with the server's message, refuses an unverified certificate, cancels when the sign-in is dropped, and
  starts none without openconnect
- The sound backend against a private PipeWire with WirePlumber (its monitors of ALSA, Bluetooth and cameras off, so it
  never opens the machine's sound cards) and pipewire-pulse, with null sinks and sources and `pacat` as apps
  (`crates/core/tests/sound_pulse.rs`, stand-in in `sound::stand_in`, `test-support`): the devices and defaults,
  switching them, volume with its balance and mute, the apps and their volume, the level meter (also on an input nothing
  records from), and the server going away and coming back

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
  send the user's line; a wallpaper tool the config starts gets its notice.
- Sound: the page against the stand-in sound server shows the outputs, the input and the apps, choosing an output makes
  it the default, the volume slider and mute reach the server, an app's volume too, the server's own changes move the
  slider, the slider goes to 150 % only with `app.sound_above_100`, a server that goes away gives way to the placeholder
  and comes back, and without one the page says how to start it.
- About: About & Maintenance shows the system details (Sway Things, the compositor with SwayFX's sway base, GTK, the
  distribution, NetworkManager as not running, the kernel and the delegated tools), _Copy System Details_ copies them as
  the block `doctor` starts with and says _Copied_ for a moment, and the links go to the website, the repository and its
  issues.
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
  the adapter choice for two adapters, looks for devices while shown, connects, pairs through the agent's dialog and
  connects, shows no _Search_ button, with searches only on request no discovery runs until _Search_, which reads _Stop_
  and ends it, and switching back to automatic hides the button and looks for devices again, and it stops looking once
  the radio is off; test windows reach no Bluetooth but the stand-in.
- Network: the page against the NetworkManager stand-in shows the known networks (also one out of range) and the other
  networks with their state and security, the wired device and the VPN, asks for scans with the spinner and registers
  the agent while shown, shows no _Search_ button, with scans only on request none runs until _Search_ and one does
  after it, which ends with the new list, and switching back to automatic hides the button and scans again, connects to
  a saved network, asks for the password of a new one (refusing a short one) and sends it, answers the agent's "Wrong
  password" question and the fields of a fake VPN plugin's dialog, walks through a fake openconnect's sign-in (username,
  password, the server's TOTP question) in one dialog, switches the cable and the VPN on, shows an unplugged cable
  without a switch, shows the captive portal note with its button, and stops scanning once Wi-Fi is off; test windows
  reach no NetworkManager but the stand-in.

The window works on settings files in a temporary directory and a sway config in memory, without a session or in the
session of the stand-in for sway (the safety net, the devices, the reload). It never saves by itself, and the files of a
reload are validated with `true`, so nothing runs sway. `scripts/gui-screenshots.sh` runs the same tests on a private
Broadway display and keeps a PNG of the window with each page, light and dark.

`crates/gui/tests/gui_pseudo.rs` opens every page, the welcome screen and the live pages against their stand-ins in a
pseudo-translation and fails on an English word that isn't data, so a string that misses the catalog shows; it needs a
display too and runs with the GUI tests ([Internationalization](UI-Architecture.md#internationalization)).
