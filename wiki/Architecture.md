# Architecture

> Status: **design, no code yet.** Facts about sway, SwayFX, kanshi, waybar, mako and swaync were checked on 2026-11-07
> against their man pages, source and releases (sway 1.12, SwayFX 0.6).

## Overview

```mermaid
flowchart LR
    subgraph app[Sway Things]
        GUI["sway-things-gui<br/>(gtk4-rs)"]
        CLI["sway-things<br/>(CLI, hooks)"]
        subgraph core[crates/core]
            REG["Settings registry"]
            ST["State<br/>settings.toml + host overlay"]
            GEN["Generators"]
            PAR["Sway config parser<br/>+ conflict detection"]
            BE["Backends<br/>kanshi, waybar, swayidle, ..."]
        end
    end

    GUI --> core
    CLI --> core
    ST --> GEN
    GEN -->|"own files only"| FILES[("Generated files<br/>~/.config/sway/sway-things/*.conf<br/>~/.config/sway-things/...")]
    core -->|"IPC: run, get_*, subscribe"| SWAY["sway / SwayFX"]
    PAR -->|"read only"| USERCFG[("User's hand-written<br/>sway config")]
    BE -->|"D-Bus, polkit"| SYS["timedated, localed, NetworkManager,<br/>BlueZ, UPower, power-profiles-daemon, logind"]
    BE -->|"reload signals / ctl tools"| TOOLS["kanshi, waybar, swaync, mako, ..."]
    SWAY -->|include| FILES
```

The core idea: **apply live over IPC, persist to our own state file, generate our own include files, and only ever read
the user's files.**

## Cargo workspace

| Crate | Binary | Depends on GTK | Responsibility |
| --- | --- | --- | --- |
| `crates/core` | – | no | Settings registry, state (load, merge, save, history), generators, sway config parser, conflict detection, IPC client, backends, typed sway command builders |
| `crates/cli` | `sway-things` | no | `get` / `set` / `reset` / `open` / `doctor` / `show-generated` / `export` / `import` / `hook` |
| `crates/gui` | `sway-things-gui` | yes | GTK4 application: shell, sidebar, search, rows built from the registry, custom pages |

All crates share the package metadata and lints of the root `Cargo.toml`: edition 2024, MSRV 1.85, `unsafe_code`
forbidden, clippy pedantic, and no `unwrap`, `expect`, `panic!`, `todo!` or `dbg!` outside tests. The core also denies
`println!` and requires docs for its public API.

The CLI deliberately has no GTK dependency, so hooks that run at login or on hotplug start fast and stay small.
`sway-things` with no arguments, or `sway-things open <page>`, starts or activates the GUI.

## Settings registry

Every setting is **defined once** in `crates/core/src/registry/`. Everything else derives from that definition:

| Derived from the registry | From |
| --- | --- |
| GUI rows (toggle, slider, dropdown, color, entry) | the value type; auto-built for standard settings, on the setting's page |
| Search index | title, subtitle, keywords and synonyms (translatable source strings, also matched in English) |
| CLI keys | the key: `sway-things set input.touchpad.tap true` |
| TOML schema and validation | the key is the path in `settings.toml`; the value type has the range or the choices |
| Defaults and "Reset to default" | the default of the value type |
| Generator output | the render function builds the commands, the mapping names the generated file |
| Conflict detection | the sway directive and block context the setting maps to; the read function gives the value of a user's line |
| Version and fork gating | `min_sway(…)`, `swayfx_only(…)` |
| Scope | global / per-host / per-device / per-output (decides the host-overlay default) |
| Apply mode | live, reload, tool-reload, re-login, risky (safety net) |
| Requirements | `requires_tools(&["swayidle"])` → "Requires swayidle" indicator plus install hints |

```rust
// crates/core/src/registry/input.rs
Setting::toggle("input.touchpad.tap", Page::MouseTouchpad, false)
    .title("Tap to click")
    .subtitle("Tap the touchpad instead of pressing it")
    .keywords(&["touchpad", "trackpad"])
    .scope(Scope::PerDevice(InputType::Touchpad))
    .sway(
        Directive::input("tap"),
        ConfigFile::Input,
        |ctx| Ok(vec![ctx.input()?.tap(ctx.bool("input.touchpad.tap")?)]),
        |args| Ok(Value::Bool(read::boolean(read::first(args)?, true))),
    )
    .build()
```

- **Value types:** boolean, integer and decimal (each with a range), choice (ids with translatable labels), color, text
  and list of texts. `Setting::check` validates a `Value` against the type. What sway accepts inside a text or a list
  depends on the directive, so the command builders check that when the setting is rendered.
- **Defaults are sway's own:** the default is what sway, libinput or SwayFX does with nothing configured (tap off, no
  gaps, `client.focused` child border `#285577`). A setting that isn't stored therefore shows what is in effect without
  Sway Things, and a reset removes the key from the state. libinput defaults that depend on the device are the common
  ones.
- **Rendering:** `Registry::render(setting, values, device, target)` returns the `SwayCmd`s for one target. The render
  function reads values through a `Context`, which checks each stored value against its setting and falls back to the
  default. Over IPC a setting can need more commands than in the config: `windows.gaps.inner` renders `gaps inner 8` for
  both, and over IPC also `gaps inner all set 8` for the workspaces that are already open.
- **One setting per value:** a directive with several values is several settings. The five colors of `client.focused`
  are `windows.focused.border`, `….background`, `….text`, `….indicator` and `….child_border`. Settings with the same
  directive form a group (`Registry::group`) with one render function that reads all of them, so storing one color
  writes the whole line with the defaults of the others. An `input` option is a directive of its own per input type
  (`events` of keyboards and of touchpads), so a group also shares the scope.
- **Per-device settings** have `Scope::PerDevice(<input type>)`. Without a selected device they render for the type
  (`input type:touchpad tap enabled`), with one for that device's identifier.
- **Directive:** each sway setting names its directive, the block it stands in (top level or `input`) and, where one
  name covers independent values, the first argument (`gaps` + `inner`).
- **Reading:** each sway setting also has a read function, the reverse of rendering: `Setting::read` takes a directive
  of the user's config and returns the value sway reads from it. The helpers follow sway's own parsing: its words for
  true (`parse_boolean`, everything else is false), `strtol` and `atoi` for numbers, `parse_color` with or without `#`
  (an opaque alpha is the same color as none), the defaults of a `client.*` line for the colors it leaves out (the
  indicator of the class, the background as child border). A line sway refuses (`ReadError::Rejected`) is told apart
  from a value sway takes but the setting can't hold (`ReadError::Unsupported`, e.g. out of its range). Every setting of
  a group reads its own value from the same line.
- **Apply mode:** live by default. A setting built on a config-only command (`workspace_layout`) must use reload.
- **Preferences of Sway Things itself** (`app.safety_net_timeout`) are settings without a sway mapping.
- **Pages:** `Page::ALL` lists every page of the app in sidebar order, each with its id, sidebar group (`Group`), title
  and one-line description. The id is what deep links and the CLI use (`sway-things open mouse-touchpad`). Every setting
  names its page, and the registry lists the settings in page order.
- **Sections:** every setting names the section of its page it is shown in (`.section("Touchpad")`, translatable), and
  the settings of a section are next to each other.
- **Sliders:** a number setting can give a slider range and step narrower than what sway accepts
  (`.slider(100.0, 2000.0, 10.0)` for a repeat delay sway takes up to 65535 ms). Without one, the slider covers the
  whole range, in steps of 1 for whole numbers and a hundredth of the range for decimals.
- **Config line:** `Registry::config_line(setting, values, device)` is the line of the generated file for a value, as
  the app shows it under the title: an `input` option without its `input <selector>` (`pointer_accel 0.3`).
- **What a row shows** (`crates/core/src/view.rs`): `SettingView::of(setting, device, state, status, compositor)` gives
  the value in effect and its source: the stored value (for a device without one of its own, its type's), else what the
  user's config sets (the last line that decides it, with file and line), else sway's default. With one value of a group
  stored, Sway Things writes the whole line, so the others show its defaults. It also has the config line and why the
  setting is locked: no include line, or a compositor that doesn't support it.

Tests check the invariants over every entry: keys are unique and valid TOML paths, defaults fit their type, every
setting renders for the config and every live one for IPC, at the limits of its range and for every choice, config-only
commands force the reload mode, and the declared requirements equal those of the built commands (SwayFX settings go to
`90-effects.conf`, nothing else does).

Pages that don't fit the row model are hand-written GTK widgets: the Displays canvas, the bindings editor, the bar
layout editor and the window preview. They still read and write settings through the registry.

## State

```text
$XDG_CONFIG_HOME/sway-things/
├── settings.toml          # shared values: commit to dotfiles
└── settings.d/
    └── <hostname>.toml    # per-machine overrides (optional)

$XDG_STATE_HOME/sway-things/
├── manifest.toml          # every file we generated, with hash, for "changed outside Sway Things" detection
└── history/               # snapshots of the settings files (restore, undo)
    └── 20261110T142355Z/  # both files as they were before a save, named after the UTC time
        ├── settings.toml
        └── settings.d/<hostname>.toml
```

```toml
# settings.toml
version = 1

[input.touchpad]
tap = true

[windows.focused]
child_border = "#3b6e4f"

# settings.d/laptop.toml
version = 1

[device."1267:12377:ELAN1300:00_04F3:3059_Touchpad".input.touchpad]
tap = false
```

- **Sparse:** only settings the user changed are stored. A missing key means _not managed by Sway Things_: the user's
  own config or the sway default applies. A reset removes the key (and tables that are empty afterwards).
- **Keys are TOML paths:** the registry key `input.touchpad.tap` is `tap` in the table `[input.touchpad]`. Colors are
  strings (`#rrggbb`, `#rrggbbaa`), choices are their id, lists are arrays of strings, and a decimal setting also takes
  a whole number.
- **Host overlay:** `settings.d/<hostname>.toml` overrides `settings.toml` key by key. The host name is the kernel's
  (`uname -n`). New values go to the shared file, except those bound to this machine's hardware (a value for one device,
  per-host and per-output settings), which go to the overlay. A changed value stays in the file it is in. The UI shows a
  "this machine only" toggle per setting, which moves the value between the two files. A value written to the shared
  file removes the one in the overlay, which would hide it otherwise.
- **Per-device values** are stored below `device."<identifier>"` (the identifier from `GET_INPUTS`) at the same key as
  the value for all devices of the type. A device without a value of its own uses the value of its type.
- **Hand edits allowed:** a file watcher reports when either file may have changed, and the state reloads the files
  whose content differs from what it last loaded or wrote. So its own saves cause no reload. A reload returns the
  settings whose value in effect changed, for the GUI and the generators. The file on disk wins: a change to that file
  that wasn't saved yet is dropped and is among the changed settings. Whatever doesn't fit the registry is reported with
  file and key, never silently dropped, and the rest still loads: unknown keys, values of the wrong type or out of
  range, entries of `device` that aren't device identifiers, and settings in a device table that can't be set per
  device. The lines stay in the file. A save only changes the lines of the settings that changed; comments, order,
  formatting and unknown keys survive.
- **Schema version:** a top-level `version = 1` key, for migrations. A file without it loads as version 1, and the first
  save adds the line. A file with another version, a syntax error or one that can't be read is reported and left alone:
  nothing in it is in effect, and it is never written. The other file keeps working.
- **Saving needs the include line:** `State::save` takes the write permission that only an active
  [include status](#include-detection) hands out. Loading, changing values in memory and reading need none.
- **Atomic saves:** a changed file is written to a temporary file next to it and renamed over it. If the file is a
  symbolic link (into a dotfiles repository), the link stays and its target is replaced. File access is synchronous
  (`std::fs`): the files are small, and only loading and saving touch the disk.
- **App preferences** (accent source, safety-net timeout, editor command, …) live under `[app]` and are ordinary
  registry entries.
- **Export / import:** `settings.toml` (plus overlays) _is_ the export format.
- **History:** before a save replaces anything, both files as they are on disk go to `history/` as one snapshot, also if
  they were changed outside in the meantime. A snapshot equal to the newest one is skipped, and the newest 50 are kept.
  If the snapshot can't be written, nothing is replaced. Restoring a snapshot loads both files from it, comments and
  unknown keys included (a file that didn't exist then becomes empty), and is written by a normal save, which takes a
  snapshot in turn, so a restore can be undone. A snapshot of a file that can't be loaded can't be restored.
- **Backups:** we never write user files, so we never need to back them up. `history/` covers our own state.

### File watcher

`crates/core/src/watch.rs` watches a set of files for any part of the app: the settings files, and the sway config files
for the [include detection](#include-detection).

- It uses the `notify` crate (inotify on Linux, kqueue on the BSDs) and watches **directories**, not files: the
  directory of each file, or the nearest one above it that exists. That covers a file that is created later, deleted, or
  replaced by a rename (how editors and `git` save), and a directory that appears or goes away. For a file that is a
  symbolic link, the directory of its target is watched too.
- Events about other files in those directories and reads are ignored.
- It can also watch a **directory** for the files directly in it (created, changed, renamed, deleted), which is how a
  glob such as `include config.d/*` is followed.
- **Debounce:** a burst of events becomes one report once the watched files have been quiet for 200 ms (after 2 s at the
  latest), so a reader doesn't see a half-written file.
- A report only says that a file _may_ have changed; the reader decides. That keeps self-write suppression out of the
  watcher.
- The watcher debounces on its own thread and hands reports over through a channel. It needs no particular executor:
  await the next report on the glib main loop, or block on it in the CLI.

## Generated files and the file rule

**Rule: Sway Things only writes files it created**. A file we created carries a header and is recorded in
`manifest.toml`. If its hash no longer matches, someone edited it. The app shows a warning and asks before overwriting.

### sway

```text
$XDG_CONFIG_HOME/sway/sway-things/
├── 10-input.conf
├── 20-output.conf         # only when kanshi is not the display backend
├── 30-appearance.conf
├── 40-behavior.conf
├── 50-bindings.conf
├── 60-idle.conf           # swayidle exec line, if the user handed idle over
├── 70-autostart.conf
├── 80-hooks.conf          # bindswitch lid → sway-things hook lid …
└── 90-effects.conf        # SwayFX only; comments instead of directives on vanilla sway
```

```text
# Generated by Sway Things from ~/.config/sway-things/settings.toml. DO NOT EDIT.
# Change settings in the app or with `sway-things set`. Hand edits are overwritten.
input type:touchpad tap enabled
input type:touchpad pointer_accel 0.3
input "1267:12377:ELAN1300:00_04F3:3059_Touchpad" tap disabled
```

`crates/core/src/generate/` makes these files. v0.1 has `10-input.conf`, `30-appearance.conf`, `40-behavior.conf` and
`90-effects.conf`.

- **Content:** `generate` turns the stored settings into file contents without writing anything, which is also what
  "Show generated config" and `sway-things show-generated` display. Below the header, each line is one command exactly
  as the command builders render it for a config file; there are no blocks. The settings of one directive make one line.
  The order is the order of the registry, the lines for input types before those for single devices, so the same state
  gives the same bytes. The registry decides the file of each setting.
- **Unsupported settings:** a stored setting the compositor doesn't support becomes a comment instead of a command, e.g.
  `# effects.corner_radius: requires SwayFX 0.5 or newer`, so the file stays valid and says why a setting has no effect.
  The same happens to a value the command builders reject. The value stays stored. The compositor is the running one,
  asked over IPC; without a session it is the installed one, read from `sway --version`.
- **Validation:** a file whose content changed is staged next to its place, under a hidden name that `*.conf` doesn't
  match, and checked with `sway --validate --config <staged file>` as a main config of its own. sway runs through
  `async-process` with an argument array. Only a valid file replaces the old one, atomically; a symbolic link stays one.
  If sway rejects the file, the old one stays, and each error carries the line and the settings behind it. If sway can't
  be run, nothing is replaced.
- **Manifest:** `manifest.toml` records the SHA-256 hash of each generated file by absolute path (the value `sha256sum`
  prints). Before a file is replaced, its content is compared:

  | The file on disk… | Result |
  | --- | --- |
  | doesn't exist | created, unless it would hold only the header |
  | already holds the new content | left as it is, and recorded |
  | has the recorded hash | replaced |
  | is recorded with another hash, or isn't recorded but starts with the header | reported as changed outside; replaced only after the user agreed |
  | isn't recorded and doesn't start with the header | never written |

  A generated file that isn't recorded came with the dotfiles from another machine, or the manifest was lost; that is
  why the header counts. A manifest that is missing or broken is empty.
- A file that loses its last setting keeps existing with only the header.
- **Writing needs the include line:** like saving the state, `generate::write` takes the write permission of an active
  [include status](#include-detection). `generate` needs none, so the preview works while read-only.

The user adds **one line** to their own config, normally as the last line:

```text
include ~/.config/sway/sway-things/*.conf
```

The app never adds this line itself. It shows the line with a copy button and detects it via the parser, see
[Include detection](#include-detection).

### Other tools

| Tool | Include support | Strategy |
| --- | --- | --- |
| kanshi | yes (`include`, wordexp) | Our file `~/.config/kanshi/sway-things.conf`; the user adds `include` to their kanshi config. With no kanshi config at all, we create `~/.config/kanshi/config` ourselves |
| waybar | `include` exists, but **the first definition wins**, so included files can only add, never override | **Handover:** our complete config + CSS in `~/.config/sway-things/waybar/`; the user starts waybar with `-c`/`-s` (instructions), or we create `~/.config/waybar/config.jsonc` if none exists. "Import as starting point" reads their file into our state |
| swaylock | no | Our config `~/.config/sway-things/swaylock/config`; our generated swayidle line calls `swaylock -C <path>` |
| swayidle | n/a (started via `exec`) | Our generated `60-idle.conf` holds the `exec swayidle …` line. If the user's config already starts swayidle, the page shows "managed externally" plus a handover explanation |
| mako | yes (`include=`, absolute or `~/` path; later definitions win) | Our file `~/.config/mako/sway-things`; the user adds `include=` at the end of their mako config. With no mako config at all, we create it |
| swaync | no, but `-c` / `-s` flags | Handover: our `config.json` + `style.css` in `~/.config/sway-things/swaync/`; the user starts swaync with `-c`/`-s` |
| gammastep / wlsunset | gammastep has a config file; wlsunset uses arguments | Own config file or our generated `exec` line |
| environment | `environment.d` is a directory | Our own file `~/.config/environment.d/50-sway-things.conf` |
| default apps | XDG spec: desktop-specific list | Our own `~/.config/sway-mimeapps.list`, read before `mimeapps.list` under `XDG_CURRENT_DESKTOP=sway`; the user's `mimeapps.list` is never touched |
| GTK look, fonts, dark mode | dconf database | Written via gsettings/GIO (official API, allowed) |
| electron/chromium flags | text files | Create if missing, otherwise handover |
| XDG autostart | directory of `.desktop` files | Our entries are `exec` lines in `70-autostart.conf`. To disable a system entry we create **our own** override `~/.config/autostart/<name>.desktop` with `Hidden=true` (only if no user file of that name exists). The runner (`xdg-desktop-autostart.target` or `dex -a`) is started from our include |

**Handover** means we generate our own complete file and the user switches the tool to it once (an instruction with a
copy button). Alternatively the user deletes or renames their own file, and we create the standard one.

## Data flow

### Changing a normal setting

1. The user flips a toggle (or runs `sway-things set …`).
2. The core builds the sway command with the [**typed builder**](#sway-command-builders) and sends it over IPC
   (`RUN_COMMAND`) → instant effect.
3. The core updates the in-memory state and saves `settings.toml` / the overlay after a debounce (~500 ms).
4. The generators regenerate the affected files:
   - write to a staged file → `sway --validate --config <staged file>` for each staged file → atomic replace. Each file
     is validated as a main config of its own: sway ignores errors in included files (`cmd_include` in sway 1.10 –
     1.12), and `--validate` only reports the main file, so a wrapper config that includes the staged files would always
     pass. This works without a running session (checked with sway 1.12); errors are reported per line, so they can be
     mapped back to settings;
   - update the hash in `manifest.toml`.
5. Conflict detection re-runs for that setting and updates the indicators.

`crates/core/src/apply/` is this flow for the CLI and the GUI alike. An `Applier` owns the state and the IPC connection.

- **`set` and `reset`** check the change against the registry, send the commands and change the state in memory. Both
  need the write permission of an active [include status](#include-detection).
- **A failed command changes nothing:** the state is as before, and commands of the same directive that were already
  sent are undone with the previous values.
- **Reset** removes the stored value and sends sway's default (for one device: the value of its type). A value the
  user's own config sets is back after the next reload of sway. A reset of a setting that isn't stored sends nothing.
- **Refused:** risky settings (they only change through the [safety net](#risky-settings-safety-net)), and a `set` of a
  setting or value the compositor doesn't support, with what it needs. Nothing is stored then. A reset is always
  allowed, so such a value can be removed.
- **Without a session** (the CLI at login) the change is only stored, saved and generated, for the compositor the
  installed `sway` reports. The result says so.
- **`persist`** saves the settings files, generates every file and writes those that changed. The result has the outcome
  per file, so the caller can show sway's errors with the settings behind them and ask before it overwrites a file that
  was changed outside. The CLI calls it right after the change. The GUI asks the applier when saving is due, 500 ms
  after the last change, and waits for that with a timer of its main loop.
- **Changes on disk:** when the watcher reports the settings files, `reload` loads them, sends the changed settings
  (those of one directive once) and leaves the rest to `persist`. The file wins, so a command that fails undoes nothing
  and is reported. Risky settings aren't sent; they are in effect after the next reload of sway.
- The result of each step says how the change takes effect: live with the commands sent, without a session, after a
  reload of sway (`command::reload`, which the caller runs when it suits), after a tool reload or a re-login.

### Risky settings (safety net)

Display mode, disabling an output or input device, keyboard layout changes:

1. Snapshot the current value and apply live.
2. Show the _Keep / Revert_ banner with a 15 s countdown.
3. **Nothing is persisted until the user presses _Keep_.** Revert or timeout re-applies the snapshot over IPC.
4. Lockout protection: the last enabled keyboard and the last enabled pointing device can't be switched off (outputs
   come with the Displays page).

`crates/core/src/apply/risky.rs` is this flow in the core; the GUI banner and the CLI prompt build on it.

- **`Applier::try_risky`** checks the change like `set` does, sends it and returns a `Countdown` with the deadline from
  `app.safety_net_timeout` (15 s by default). Nothing is stored. The snapshot is the value in effect as a row shows it
  (`SettingView`): the stored value, else what the user's config sets, else sway's default. sway reports keyboard
  layouts only by their names (`English (US)`), not as `xkb_layout` takes them, so the core's value is what is restored.
- **`keep`** stores the value like any other change (saved with `persist`); nothing more is sent. **`revert`** sends the
  snapshot; the caller runs it when the user says so and when `Countdown::elapsed` returns. The countdown waits with
  `async_io`, so it works under the GTK main loop and `block_on` alike.
- **One at a time:** while a change waits, another risky change is refused until it is kept or reverted.
- **Only in a session:** without a running sway nothing can be tried and reverted, so `try_risky` refuses.
  `set_risky_unchecked` (for `sway-things set --yes`) sends the value if there is a session and stores it at once.
- **Lockout protection:** a change of `events` asks sway for the devices and is refused if it would leave no usable
  keyboard or pointing device ([Input devices](#input-devices)), for one device as for a whole type. A session that has
  none to begin with (a headless sway) has nothing to lose. Without a session the devices aren't known, so switching off
  a whole type of keyboards or pointing devices is refused, one device is allowed.
- **If the app or the command line ends** while a change waits, nothing was stored: the session keeps the tried value
  until the next reload of sway (or login), which brings back the old one.

**From the CLI**: in an interactive terminal, `sway-things set` applies live, asks `Keep? [y/N]` with the same
countdown, and saves only on `y`. Without a TTY it refuses unless `--yes` is given, so scripts and hooks never hang.

### Apply modes

| Mode | Example | Mechanism |
| --- | --- | --- |
| live | gaps, colors, input options | IPC command + persist |
| reload | settings sway only honors at load | persist + `swaymsg reload` (with confirmation if disruptive) |
| tool reload | kanshi, waybar, swaync | `kanshictl reload`, `SIGUSR2` to waybar, `swaync-client -R`, … |
| re-login | environment.d | persist + "takes effect after re-login" badge |
| risky | see above | live + safety net, persist on _Keep_ |

## Sway command builders

Every sway command, sent over IPC or written into a generated file, is a `SwayCmd` from `crates/core/src/command/`. A
command is a list of typed parts: keywords (static strings from our code, only ASCII letters, digits and `_ . : * + -`),
integers, finite decimals, booleans in the directive's spelling (`enabled`, `yes`, `on`, …), colors (`#rrggbb`,
`#rrggbbaa`), text and criteria. External strings only enter as `SwayText` or as criteria values, never as a raw string.

sway parses IPC commands and config lines differently (`execute_command` and `config_command` in `sway/commands.c`,
helpers in `common/stringop.c`; unchanged from 1.10 to 1.12):

| | IPC (`RUN_COMMAND`) | Config line |
| --- | --- | --- |
| Command separators | `;` and `,` outside quotes | none, one command per line |
| `$variables` | replaced in each argument, after unquoting | replaced in the whole line, before unquoting |
| Quotes | stripped from arguments that start with one (not for `exec`, `mode`) | the same (also not for `bindsym`, `set`, `for_window`, …) |
| Backslashes | kept, but still escape the next quote | unescaped (`\"`, `\\`, `\n`, …) |

So `SwayCmd::render` has two targets:

- **Text over IPC:** quoted with `"`, or with `'` around a run that contains `"`; every `$` is doubled.
  `Dell "U2415" $1` becomes `'Dell "U2415" $$1'`.
- **Text in a config line:** quoted with `"`, inner `"` become `\"`, every `$` is doubled: `"Dell \"U2415\" $$1"`.
- **What text can't contain:** `SwayText` rejects empty text, control characters (they end a config line or change how
  sway splits it) and backslashes. sway's IPC splitter keeps its escape flag across `\'` and then misreads the next
  quote, so a backslash next to a quote can't be represented reliably.
- **Criteria** match exactly. Each value becomes an anchored PCRE2 regex `^…\z` in which every character except ASCII
  letters, digits and `_` is written as `\x{hex}`, so criteria never contain a quote, `$`, `]` or whitespace. Config
  lines double the backslashes, because sway unescapes them.
- **`for_window`:** sway stores the inner command and parses it again, like an IPC command, when a window appears. So
  its text is quoted twice over IPC, and its `$` doubled twice in a config line. The inner command can't contain
  criteria or another `for_window`.
- **Leading criteria:** `SwayCmd::with_criteria` builds `[app_id="…"] opacity 0.8`, which applies a command to open
  windows. It only works over IPC, and sway reports a failure if no window matches. Per-app settings use it for the live
  change and `for_window` for windows that open later.
- **Availability:** some commands work only in config files (`xwayland`, `xkb_numlock`, `workspace_layout`,
  `default_orientation`), so settings built on them apply with a reload instead of live. Others work only over IPC.
  While sway reads the config it defers commands such as `xkb_switch_layout`, `focus` and `workspace <name>` and later
  runs them through the IPC parser, so their builders are runtime-only. Rendering for the wrong target is an error.
- **Minimum version:** a command records the oldest sway that understands it (`SwayCmd::min_sway`, by default sway
  1.10), and for SwayFX directives the oldest SwayFX (`SwayCmd::min_swayfx`, `None` if vanilla sway understands it).
  Rendering doesn't check either; the registry compares them with the running compositor (SwayFX by its base sway
  version for `min_sway`).

On top of `SwayCmd`, typed builders cover the directives of each page, with an enum per set of values and range checks
on numbers taken from sway's and SwayFX's own checks:

- `command::input` builds `input <identifier> …` for every device (`*`), a device type (`type:touchpad`) or one device
  by its `GET_INPUTS` identifier. `drag_lock enabled_sticky` needs sway 1.11, and a sway built against a libinput that
  supports it; otherwise sway reads it as `disabled` without an error.
- `command::window` builds colors (`client.*`; sway ignores `client.background` and `client.placeholder`, so they have
  no builder), borders, gaps, the title bar and `opacity`. `gaps <which> <px>` only changes the default for new
  workspaces, and `gaps <which> all|current set <px>` (runtime-only) only open workspaces, including those with gaps of
  their own from `workspace <name> gaps`; the registry combines them. sway rejects a title bar padding below the border
  thickness and a thickness above the vertical padding, so the order of those two commands matters.
- `command::effects` builds the SwayFX effects with the names and ranges of SwayFX 0.5; `animation_duration_ms` needs
  SwayFX 0.6. `corner_radius` raises the horizontal title bar padding to at least the radius.
- `command::behavior` builds focus, layout, floating, dragging, popup, Xwayland and idle inhibition settings.
  `floating_minimum_size` and `floating_maximum_size` take each dimension as no limit (`-1`), sway's automatic value
  (`0`) or pixels. `inhibit_idle`, like `opacity`, is wrapped in `for_window` or leading criteria for per-app rules.

A port of sway's parsing functions (`command/model.rs`, tests only) checks with property tests that every accepted value
arrives unchanged in both targets. Fixture files generated from the builders (`ci/integration/*.conf` and `*-ipc.txt`)
go to real sway 1.10, sway latest and SwayFX: every generated config file must pass `sway --validate` on its own, every
IPC command must succeed, and the escaping fixtures read every value back. Fixtures marked `# Needs SwayFX.` only run on
SwayFX.

## Reading the current state

- **IPC:** `GET_VERSION`, `GET_INPUTS`, `GET_OUTPUTS`, `GET_SEATS`, `GET_BINDING_MODES`, `GET_BINDING_STATE`,
  `GET_TREE`, `GET_CONFIG`. Event subscriptions for `input`, `output`, `mode`, `workspace`, `shutdown`, `tick`, so
  hotplugged devices and outputs appear immediately. See [IPC client](#ipc-client).
- **`GET_CONFIG` only returns the main config text**, not included files. Many values (all `client.*` colors, `gaps`,
  `font`, bindings) aren't exposed over IPC at all. So the core has its own **read-only parser**.

### IPC client

`crates/core/src/ipc/` talks to the compositor over the socket in `$SWAYSOCK` (`man 7 sway-ipc`).

- **Transport:** the wire format (the magic string `i3-ipc`, payload length, message type, JSON payload) is implemented
  in the core on async-io's Unix socket, so the client runs on any executor. Replies and events are read into the types
  of the `swayipc-types` crate (`ipc::types`). Those types are strict: a value they don't know makes that reply an
  error, not a guess.
- **Without a session:** an unset `SWAYSOCK` ("not in a sway session") and a socket nobody answers on ("sway is not
  running") are different errors, so the CLI and the read-only parts can say which it is and carry on.
- **Compositor detection:** connecting sends `GET_VERSION`. SwayFX is the compositor that sends `sway_original_version`,
  the sway it is based on; its own version is in the version numbers then. A compositor below sway 1.10 or SwayFX 0.5 is
  refused, with what was detected in the error.
- **Commands:** `Client::run` takes a `SwayCmd`, renders it for IPC and sends it in a `RUN_COMMAND` message of its own,
  so the result belongs to that command. A rejected command returns sway's error message and whether it was a parse
  error. A command that needs a newer sway or SwayFX than the running one isn't sent at all: sway doesn't reject every
  value it doesn't know (`drag_lock enabled_sticky` on sway 1.10 means `disabled`).
- **Events:** `Client::subscribe` opens a second connection, so replies and events don't mix, and `Events::next` returns
  them in order. An event that can't be read is an error for that event only, an unknown event type is skipped, and the
  events end when sway closes the connection (after a `shutdown` event, if subscribed).
- **Connecting under load:** sway queues only a few connections before it accepts them, and a connect while that queue
  is full ends unconnected. The client checks the connection and tries again for up to two seconds.
- After an I/O or protocol error a connection is out of step and has to be replaced.

### Input devices

`crates/core/src/devices/` is the list of input devices for the device selectors of the pages, per-device settings and
the lockout protection.

- **From sway:** each device has the identifier sway selects it by, the name to show, the type (`keyboard`, `pointer`,
  `touchpad`, …; a type this build doesn't know is listed without one), whether it is switched off (libinput's
  `send_events`), and for keyboards the layouts and the one in use. The name is an external string: it is shown and
  never part of a command.
- **Hotplug without polling:** `DeviceWatch` subscribes to sway's `input` events on a connection of its own and reads
  `GET_INPUTS` again after each one. It reports what changed (added, removed, changed) and skips events that changed
  nothing. Devices of the same model share an identifier, and with it every setting, so they are one entry that stays
  until the last of them is unplugged; an event alone doesn't say which one went.
- **Stored but not connected:** a device with values in the settings files stays listed as disconnected, with the type
  its settings are for and the name part of its identifier, so its values can still be seen and removed.
- **Last keyboard, last pointing device:** the list says whether switching a device off would leave no keyboard or no
  pointing device (mouse, trackpoint or touchpad). Only connected devices that aren't switched off count, and only those
  with a vendor ID: sway lists the power button and the video bus (`0:1:Power_Button`, `0:6:Video_Bus`) as keyboards,
  and virtual devices of other programs have vendor 0 too. The extra keys some mice register as a keyboard can't be told
  apart and count. A touchpad that is off while an external mouse is connected counts, because it works again once the
  mouse is gone. `Devices::lockout(type, device, sending)` says what switching the devices of a type (or one of them) to
  a sending mode would leave the user without.

### Sway config parser

`crates/core/src/sway_config/` reads the user's config the way sway's `read_config` does, and only reads.

- **Which file:** `parse` takes the path of the main config. In a session, `GET_VERSION` names the loaded one; otherwise
  `default_path` follows sway's search order: `~/.sway/config`, `$XDG_CONFIG_HOME/sway/config`, `~/.i3/config`,
  `$XDG_CONFIG_HOME/i3/config`, `/etc/sway/config`, `/etc/i3/config`.
- **Lines:** a line starting with `#` is a comment (there are no comments at the end of a line), a backslash at the end
  joins the next line, and a `{` on a line of its own belongs to the line before it.
- **Blocks:** like sway, the parser puts the line that opened a block in front of every line inside it. A directive
  carries its block as words: `["input", "type:touchpad"]`, `["bar", "colors"]`, `["mode", "resize"]`. `input`, `output`
  and `seat` settings written on one line (`input type:touchpad tap enabled`) get the same block, so both spellings read
  the same.
- **Variables:** `set $var` is applied **when each line is read** (see `config_command` in sway's `commands.c`), so a
  later `set` doesn't change earlier lines. Longer names are tried first, `$$` is a literal `$`, and the command itself
  can come from a variable. Variables carry over into included files.
- **Arguments** are what the command gets: variables replaced, quotes removed and escapes resolved. `exec`,
  `exec_always`, `bindsym`, `bindcode`, `bindswitch`, `bindgesture`, `mode`, `set` and `for_window` keep their quotes,
  as in sway.
- **Includes** are read in place, relative to the directory of the including file, each file at most once (which also
  ends cycles). sway expands the path with `wordexp`; the parser does the same itself: words split at blanks, quotes
  removed, `~` and `$VAR` / `${VAR}` replaced from the environment, globs matched with the rules of `glob(3)` (sorted,
  no hidden files unless the pattern starts with a dot, a pattern that matches nothing stands for itself).
- **Never executes anything.** An include with `$(…)` or backticks, `~user`, a `${VAR…}` operator or something `wordexp`
  rejects is reported as an unresolved include; what such a file would set is unknown.
- **Output:** the directives in the order sway runs them, each with file, first line, block, command, resolved arguments
  and the text as written; every `include` with the paths it stood for and the files read because of it; and diagnostics
  (unreadable file, unresolved include, include not found, file already included, unbalanced braces, nesting or size
  beyond the limits). No diagnostic stops the parser.
- **Best-effort by design:** the parser doesn't know which commands exist or what they do; that is for conflict
  detection and the registry. It reads through a `Files` trait, with the real file system or files in memory.
- **Robustness:** every step is linear in the length of the input, include depth, directive count and file size are
  bounded, and property tests feed it arbitrary bytes and compare its line splitting with the port of sway's parser that
  the command builders are tested against. It's built to be **fuzzed**.

### Include detection

`crates/core/src/include/` answers whether the user's config includes the generated files, and where.

- **How:** `IncludeStatus::detect` parses the user's config through a view of the file system in which every file Sway
  Things can generate exists and holds one marker directive. Where the markers turn up is where sway reads the files. So
  the line counts in any spelling sway resolves to them (`~`, `$HOME`, an absolute or relative path, a glob, through
  symbolic links), in the main config or an included file, and also before a file was ever written.
- **The files on disk decide,** not what the running sway loaded: the line counts as soon as it is saved. Live changes
  go over IPC anyway, and the next reload reads the generated files.
- **Status:**

  | Status | Meaning | Settings |
  | --- | --- | --- |
  | active | every generated file is included | can be changed |
  | partial | only some files are included (one file by name, a narrower glob); the status names the others | read-only |
  | unknown | the line wasn't found, but part of the config can't be followed: an include that needs a shell command, an unreadable file, includes nested too deeply | read-only |
  | missing | no include of the generated files, or no config | read-only |

- **Position:** for each generated file the status knows how many of the user's directives sway runs before it. The
  user's lines after the first generated file are listed: they win where they set the same thing. With the line at the
  end of the main config there are none. The status also holds the user's config without the generated files, for
  conflict detection.
- **Write permission:** an active status hands out a `WriteAccess`, which `State::save` and `generate::write` require.
  So neither the CLI nor the GUI nor a hook can persist anything without having checked, and the refusal carries the
  text to show ("Add the include line to change settings", the explanation and the instructions). The permission says
  the line was there when the config was last read; callers get a new one for each change.
- **Instructions:** the line to copy and the file it goes into come from the core, so the GUI and the CLI show the same.
  The line is written with `~` when the generated files are in the home directory, escaped where the path needs it, and
  checked by parsing it back. If sway reads the config of the system (`/etc/sway/config`) or none, the instructions say
  to create `$XDG_CONFIG_HOME/sway/config` first.
- **Watching:** `IncludeWatch` keeps the status current. It watches the main config, every included file, include paths
  that don't exist yet and the directories the includes point into, reads the config again after each report, and
  reports only when the status differs. Without a session it looks up sway's default config again on every change, so a
  config created later is found. The writes of Sway Things to its own files change nothing and aren't reported.

### Conflict detection

Sway evaluates config top to bottom, and for most settings the last definition wins. The
[include detection](#include-detection) knows where each generated file sits in the user's config.

| User line is… | Effect | Indicator |
| --- | --- | --- |
| before our include | our value wins, the user's line is dead | "Overridden: your config line N is no longer in effect" |
| after our include | the user's value wins, ours is dead | "Your config line N overrides this setting" |
| our include missing, partial or unknown | nothing we generate is reliably active | app-wide **read-only** mode with the include instructions |

`crates/core/src/conflict/` finds these lines. `Report::check(status, setting, device, devices)` lists every user line
that sets a setting, in the order sway runs them, each with file, line, text, the value read from it, and whether it
runs before or after the generated file of the setting. The search is linear in the length of the config, so it runs
again for one setting after it changed, and for all of them (`check_all`, also for every known device) after the config
or the settings files changed on disk.

- **Top-level directives** match by name and, where the registry names one, first argument (`gaps inner`). Names compare
  without regard to case, as in sway.
- **`input` options** merge per device. For one device the last line wins among those for `*`, for its type and for its
  identifier, in config order: sway's `store_input_config` merges each kind onto the others as it reads them.
  Identifiers and types compare exactly (`strcmp`). In a report for a whole input type, a line for one device is listed
  with that device and doesn't decide the type's value; with the device list, lines for devices of another type are left
  out.
- **One line, several settings:** a `client.*` line sets all five colors, so every setting of the group reports it, and
  storing one color makes the user's line dead for all of them.
- **What counts:** a line sway rejects sets nothing and never wins; a line with a value the setting can't hold still
  wins. The **user's value** is that of the last line that decides the setting: pages show it while Sway Things stores
  nothing ("from ~/.config/sway/config:120").
- **Not yet:** `exec` (accumulates) and `bindsym` (a duplicate overrides with a warning) get their semantics with their
  first settings, on the Autostart and Keyboard Shortcuts pages.

Actions on a conflict: **Open in editor at line** (`$VISUAL`/`$EDITOR`, otherwise the default text editor), **Copy
line**, **Let my config win**. `Applier::let_config_win` removes the stored values of the setting's directive (every
setting of a group, since the user's line sets all of them) and sends the user's value from the report, so nothing
reloads; without a line of the user's, sway's default. If a value of the group can't be read from the line, nothing is
sent and the caller reloads sway, which reads the line itself.

Bindings defined in the user's config can still be changed from the app: our include comes later, so it can `unbindsym`
and re-bind without touching their file.

## Backends

Each area that has competing tools defines a backend trait:

```rust
trait Backend {
    fn detect(&self) -> Availability;   // installed? running? version? managed externally by file:line?
    fn read(&self) -> Result<Values>;
    fn apply(&self, change: &Change) -> Result<()>;
    fn generate(&self, state: &State) -> Vec<GeneratedFile>;
}
```

| Area | Backends (reference first) |
| --- | --- |
| Display profiles | kanshi; fallback without kanshi: static `output` lines in `20-output.conf` (no profiles); shikane later |
| Bar | waybar, swaybar (`bar {}` block) |
| Idle / lock | swayidle + swaylock |
| Notifications | swaync, mako, dunst (Do Not Disturb first) |
| Night light | gammastep, wlsunset |
| Wallpaper | swaybg via `output * bg`; swww later |
| Launcher | fuzzel, wofi, rofi(-wayland), tofi |
| Screenshots | grim + slurp (grimshot), swappy / satty |
| Clipboard history | cliphist, clipman |
| Auto dark mode | darkman |
| Audio | PulseAudio API via pipewire-pulse (`libpulse-binding` + `libpulse-glib-binding`) |
| Autostart | our `exec` lines + hand-written `exec` (read-only) + XDG autostart with runner detection |
| Bluetooth | BlueZ (D-Bus) |
| Network | NetworkManager (D-Bus) |
| Power | power-profiles-daemon / tuned-ppd, UPower (D-Bus) |
| Brightness | logind `Session.SetBrightness` (no root needed) / brightnessctl; ddcutil for external monitors |
| Time / locale | systemd-timedated, systemd-localed (D-Bus, polkit) |

If an unknown tool or a custom script is in charge (e.g. an `exec ~/.local/bin/my-wallpaper.sh` line), `detect()`
reports **managed externally by file:line**. The page explains this and offers a handover. It never fights the script.

## No daemon: the CLI as a hook

Sway Things keeps nothing running. Behavior that has to happen without the window goes to existing tools, and where glue
is needed, generated config calls the CLI briefly:

```text
# ~/.config/kanshi/sway-things.conf (generated)
profile office {
    output eDP-1 disable
    output "Dell Inc. DELL U2723QE <serial>" mode 3840x2160@60Hz scale 1.5
    exec sway-things hook workspaces --profile office
}

# ~/.config/sway/sway-things/80-hooks.conf (generated)
bindswitch --reload --locked lid:on  exec sway-things hook lid closed
bindswitch --reload --locked lid:off exec sway-things hook lid open
```

Our generated files can also contain the `exec kanshi` / `exec_always kanshictl reload` lines. They're our files, so
that needs no edit of the user's config.

**SwayFX switching edge case:** `90-effects.conf` is only regenerated when the app or CLI runs. On most distros the
`swayfx` and `sway` packages conflict, so switching means reinstalling. After switching to vanilla sway, sway reports
unknown commands (non-fatal) until `sway-things` runs once. `sway-things doctor` detects and fixes this.

## Version and fork gating

- `GET_VERSION` tells sway from SwayFX and gives the version; SwayFX also reports the sway it is based on
  (`sway_original_version`). Minimum: **sway 1.10** (Debian 13 ships 1.10.1) and **SwayFX 0.5**. SwayFX bases: 0.5 →
  sway 1.10.1, 0.5.2/0.5.3 → 1.11, 0.6 → 1.12 (adds animations).
- Registry entries carry `min_sway(...)` / `swayfx_only()`, and every built command its minimum sway version (some
  values need a newer sway than their directive) and, for SwayFX directives, its minimum SwayFX version. Controls the
  running compositor doesn't support show as disabled with a reason, instead of disappearing.
- SwayFX settings go to `90-effects.conf`. While vanilla sway runs, the generator writes a comment per stored effect
  instead of the directive.

## Security model

**Runs as the user. No privileged code, no daemon, no network**.

| Threat | Mitigation |
| --- | --- |
| Command injection via external strings (device names, **SSIDs**, **Bluetooth names**, file names, workspace names, window titles) | Typed sway command builders with escaping and quoting; processes started with argument arrays, never `sh -c` |
| Malicious or broken `settings.toml` from shared dotfiles | Schema validation; `exec`/`bindsym … exec` lines shown verbatim in "Show generated config"; generated sway config validated with `sway --validate` |
| Corrupting the user's config | We never write user files; atomic writes; manifest hashes detect foreign edits to our files |
| Locking the user out (no display, no keyboard) | Safety net with auto-revert; last-output and last-keyboard protection |
| Privilege escalation | Only existing polkit-guarded D-Bus services (timedated, localed, NetworkManager, BlueZ, power-profiles-daemon); root-owned files such as `logind.conf` are **read-only**, with a copyable command |
| Parser exploits | `unsafe_code` forbidden in every crate, fuzzing, never executes during parsing |
| Supply chain | `cargo-deny` (advisories, licenses, sources), lockfile committed, vendored release tarballs, GitHub Actions pinned to commit SHAs with least-privilege permissions, CI tools and dprint plugins pinned with checksums |

## Platform floor

| Component | Minimum | Reason |
| --- | --- | --- |
| sway | 1.10 | Debian 13 (trixie) ships 1.10.1 |
| GTK | 4.18 | Debian 13 ships 4.18.6 |
| Rust (MSRV) | 1.85 | Debian 13 ships 1.85.1. Raised only on demand, at most to trixie-backports (currently 1.96) |

## UI architecture

- Plain **gtk4-rs**. No libadwaita, because it overrides the user's GTK theme. No Relm4, because standard rows are
  generated from the registry anyway, and custom widgets need plain gtk-rs.
- `GtkApplication` with app_id **`io.github.mowi12.SwayThings`**. It handles single instance and D-Bus activation;
  `sway-things open <page>[/<setting>]` activates the running window.
- **Rows from the registry:** a `SettingRow` per setting, with title, subtitle, the config line in monospace, where a
  value from the user's config comes from, an error under the title when a change failed, and the control for its value
  type: a switch (on/off), a slider over the setting's slider range plus a number field for every value it accepts
  (whole and decimal numbers), a drop-down (choice), a color button (color) and an entry, applied on Enter or when it
  loses the focus (text). Lists get their own widget with the Keyboard page. A locked setting is insensitive, with the
  reason as tooltip and accessible description.
- **Pages from the registry:** `PageView::for_settings(model, page, device)` builds a page from the registry: a heading
  and a bordered list per section, and the generated files the page writes to at the bottom. `device` selects an input
  device for the per-device settings. Every page with settings is built this way; custom pages can mix these rows with
  their own widgets.
- **Model** (`crates/gui/src/model.rs`): owns the core's `Applier`, the include status and the watchers of the user's
  config and the settings files. Rows read from a copy of the state, so they never wait for a change in progress. A
  change is queued and sent one at a time; a setting that changes again before its turn sends only its newest value (a
  slider being dragged). The files are saved `SAVE_DELAY` after the last change. The model reports `changed` (rows read
  again; a row with a newer value on its way keeps it), `failed` (per setting) and `problem` (shown in a banner above
  the page: a generated file sway rejected or that was changed outside).
- Custom pages are GObject subclasses.
- **Components:** every building block is a widget in `crates/gui/src/widgets/`, a GObject subclass written in Rust (no
  `.ui` files) with its own CSS node name (`st-sidebar`, `st-page-header`, …). The shell is made of them: `Sidebar` (the
  pages of `Page::ALL` under their group headings), and per page a `PageView` (header, body, scrolled, at most 760 px
  wide through `Clamp`); a page without settings that isn't built yet shows a `Placeholder`.
- Our own CSS (`crates/gui/src/style.css`) covers structure only. Colors come from the GTK theme, and the accent comes
  from the sway `client.focused` child border color (live). Secondary text uses `.st-dim` (opacity 0.75) instead of the
  theme's `.dim-label` (0.55), which stays below 4.5:1 in GTK's own theme.
- **Concurrency**: one event loop. Futures run on the GTK main context (`glib::spawn_future_local`); I/O uses
  executor-agnostic, async-io based crates (zbus, swayipc-async, async-process); the CLI uses `async_io::block_on`. No
  tokio. `crates/core` must not depend on a specific executor.

## Internationalization

gettext from day one. Source strings (msgids) are American English; en_GB and other languages are translations. Registry
titles, subtitles and keywords are translatable, and search matches both the translated and the English keywords.

## Testing

| Level | What |
| --- | --- |
| Unit | the sway config parser (sample configs, the generated files, arbitrary bytes), the IPC client against a stand-in for sway (protocol errors, events), the device list (replies to `GET_INPUTS` as sway sends them, hotplug event sequences against the stand-in, stored devices, what switching devices off would leave), registry invariants (every setting reads back each value it renders), reading values as sway reads them, conflict detection (sample configs for each kind of directive, both positions, includes in included files, per-device lines), state merge (shared + overlay), reload and history, the file watcher against real files, the include detection (every spelling of the line, each status, symbolic links) and its watcher, applying a change against the stand-in for sway and a fake validator (each apply mode, failures, changes on disk, letting the user's config win, the safety net: keep, revert, one change at a time, no session, the lockout cases), command builders and escaping (property tests against a port of sway's parser) |
| Golden files | every generator: state in → exact file out (`ci/integration/settings/` → `ci/integration/generated/`), for the oldest supported sway and SwayFX |
| Fuzzing | sway config parser (`cargo-fuzz`, scheduled CI) |
| Integration | headless compositor (`WLR_BACKENDS=headless`, `WLR_LIBINPUT_NO_DEVICES=1`) in CI on **sway 1.10, sway latest and SwayFX latest** (containers from Debian 13, Arch Linux and Void Linux in `ci/containers/`; SwayFX renders in software on a virtual DRM node): apply over IPC, read back, validate generated files (incl. `90-effects.conf` on SwayFX); the generators' golden files (each passes `sway --validate` on its own and is included by the sample config) and their write path with the real validator (`crates/core/tests/generate_sway.rs`); the config parser against the modes sway reports (`crates/core/tests/sway_config_sway.rs`); a setting changed end to end (`crates/core/tests/apply_sway.rs`): sent, read back, saved, generated into a directory the sample config includes, and still in effect after a `reload`; the IPC client's tests (`crates/core/tests/ipc.rs`; all built in the containers, skipped by a plain `cargo test`): compositor detection, a command read back, sway's message for a rejected command, version gating, every typed reply, tick and workspace events, the device watch on a session without devices; fixtures generated from the command builders: each generated config file passes `sway --validate` on its own, every IPC command succeeds, and hostile strings set as bar IDs from a config and as seat names over IPC come back unchanged |
| GUI | `crates/gui/tests/gui.rs`, run by `scripts/check-integration.sh` on the headless sway of the sway containers (skipped by a plain `cargo test`; SwayFX draws no frames in a container): the window with each page open, drawn in GTK's own theme in light and dark and scrolled through a window height at a time, must reach WCAG AA contrast (4.5:1, large text 3:1). The check reads what GTK draws: the render nodes say where each text is and which colors are painted under it (after opacity; the worst stop of a gradient), and the rendered pixels give the text color as the user sees it. Keyboard: arrow keys move through the sidebar without opening pages, Enter opens one, Esc goes back from the content. Rows: every page shows a row per setting in registry order, each control takes the focus, a change reaches the settings, a SwayFX setting on sway is locked and says why, and a change without the include line shows its error. The window works on settings files in a temporary directory, a sway config in memory and no session, and never saves, so nothing runs sway. `scripts/gui-screenshots.sh` runs the same tests on a private Broadway display and keeps a PNG of the window with each page, light and dark. Logic stays in `core`, so the GUI stays thin |

## Packaging

- **openSUSE Build Service** builds openSUSE, Fedora, Debian and Ubuntu packages from one project (one `.spec`, one
  `debian/` dir, vendored crates, offline build). **AUR**: release and `-git` packages.
- Shipped: both binaries, `io.github.mowi12.SwayThings.desktop`, AppStream metainfo, icon, man pages (`clap_mangen`),
  shell completions (`clap_complete`), translations.
- No Flatpak. Aim for official distro repositories after 1.0.
- License: `GPL-3.0-or-later`.
