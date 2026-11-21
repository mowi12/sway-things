# Command Line

> Part of the [architecture](Architecture.md). The `sway-things` binary: its commands, the doctor, and the hooks that
> replace a daemon.

## Commands

`crates/cli/` is the `sway-things` binary. `src/lib.rs` runs a command on a `Session` (an `Applier` and the
`IncludeStatus`), so the tests run it in-process; `src/main.rs` only parses the arguments (clap) and opens the user's
session with `Applier::open`, the same start as the app: settings and generated files in their usual places, the running
sway or, without one, the installed `sway`, and the main config sway loaded or would find. `doctor` starts on its own so
it works where that fails, and `open` needs none of it.

The man pages (`data/man/`, one per command plus `sway-things-gui.1`) and the shell completions for bash, zsh and fish
(`data/completions/`) are generated from these clap definitions by `crates/cli-docs` and committed, like the translation
template. `sway-things.1` adds the exit status, the environment variables, the files and related pages. The completions
are static: commands, options and the files of `show-generated` complete, setting keys don't.
`scripts/update-cli-docs.sh` regenerates them, and a test (`crates/cli-docs/tests/fresh.rs`) fails while they are out of
date. The page of the app comes from a small clap command that only describes the link argument; a test keeps it in line
with what the app reads.

| Command | What it does |
| --- | --- |
| `get [<key>] [--device <id> \| --collection <name>] [--json]` | what a row of the app shows (`SettingView`, see [Settings registry](Architecture.md#settings-registry), with `apply::in_effect` like the app: while kanshi sets up the displays, the value the connected displays share and no sway line): the value in effect and where it comes from (`settings.toml`, `settings.d/<host>.toml`, the user's config with file:line, or the default), and the user's line that wins over a stored value. Without a key, every stored value, those of each device, display and collection under it. |
| `set <key> <value> [--device <id> \| --collection <name>] [--host \| --shared] [--yes] [--overwrite]` | the change through the `Applier`, then `persist` at once. A new value goes to the [default file](Architecture.md#state) of the setting unless `--host` or `--shared` says otherwise (`Applier::move_to`). |
| `reset <key> [--device <id> \| --collection <name>] [--yes] [--overwrite]` | removes the stored value and sends the default. A risky setting is reset through the safety net like `set`, with the value from the user's config if it sets one. |
| `wallpaper next [--device <id>] [--due]` | the next picture of a [collection](Backends.md#wallpaper-collections), for one display by its identifier or in a session by its name, now; with `--due` (run by the timer) only when its time has come, and without a session it stops the timer. |
| `show-generated [<file>]` | the generated files as `persist` would write them, without writing; without a file, every file with content under a `# ==> <path> <==` line. |
| `open [<link>]` (also `sway-things` without a command) | starts the app, or brings the running window up, at a [deep link](Architecture.md#settings-registry): a page id or a setting key. It checks the link first and lists the valid page ids or the keys closest to an unknown key; then it starts `sway-things-gui [<link>]` in a process group of its own, with stdin closed, and returns at once. It reads neither the settings nor sway. The app is the one `SWAY_THINGS_GUI` names, the one next to `sway-things`, or the first in `PATH`. |
| `doctor [--fix [--overwrite]] [--json]` | why settings might not take effect, section by section ([Doctor](#doctor)); `--fix` writes the generated files that are missing or out of date, with `--overwrite` also those changed outside. |
| `hook <gsettings\|displays\|workspaces\|mirror\|new-display\|wallpaper>` | the glue that generated config runs at login, on reload and on hotplug, so nothing has to keep running ([No daemon](#no-daemon-the-cli-as-a-hook)). |

- **`--color auto|always|never`** (global, `colorchoice-clap`) decides colors; `auto` colors only a terminal and follows
  `NO_COLOR`, `CLICOLOR` and `CLICOLOR_FORCE` (anstream). Only `doctor` colors its report so far; the other commands
  take the flag and print as before.
- **Values** are typed as `Kind::parse` reads them and `Value`'s `Display` writes them: `true`/`false`, a number, a
  choice's id, `#rrggbb` or `#rrggbbaa`, any text, and a list as its items separated by commas (`us,de`, like
  `xkb_layout`; empty items at the end are dropped, those before others stay: `,nodeadkeys`). An empty text is an empty
  list. A value may start with `-`.
- **`--json`** gives an object per setting: `key`, `device`, `value` (a boolean, a number, a list of strings, or a
  string), `source` (`kind`: `shared`, `host`, `config` or `default`, with `file`, `line` and `for_type` where they
  apply), `line` (the sway line), `overridden_by` (`file`, `line`) and `unsupported`. Without a key, an array.
- **Risky settings:** in a terminal (stdin and stderr), `set` and `reset` try the value, ask `Keep? [y/N]` on stderr
  with the seconds left and keep it only on `y`; anything else, the end of input or the timeout reverts it. Without a
  terminal they refuse unless `--yes` is given ([safety net](Applying-Settings.md#risky-settings-safety-net)).
- **Read-only:** while the [include line](Reading-Sway.md#include-detection) is missing, `set` and `reset` print the
  explanation with the line to add and change nothing; `get`, `show-generated` and `doctor` work, and `doctor --fix`
  writes nothing.
- **Writing:** a generated file that was changed outside is left alone and reported; the settings are saved, and the
  same command with `--overwrite` replaces the file. When sway rejects a generated file, each rejected line is shown
  with the settings it comes from. Problems in the settings files are warnings on stderr.
- **Exit codes:**

  | Code | Meaning |
  | --- | --- |
  | 0 | done |
  | 1 | failure: sway rejected a command or a generated file, a file can't be read or written, no sway to ask, the app can't be found or started; for `doctor`, `--fix` couldn't write |
  | 2 | usage: wrong arguments, an unknown key or page, a value the setting doesn't take, `--device` for a setting that isn't per device |
  | 3 | read-only: the include line is missing |
  | 4 | refused: a risky setting without a terminal or `--yes`, a change that wasn't kept, a setting the compositor doesn't support, a lockout, a generated file changed outside and not overwritten |
  | 5 | problems: `doctor` ran and found at least one problem, also after `--fix` |

### Doctor

`crates/core/src/doctor/` checks everything between a stored value and sway, for `sway-things doctor` and later the
About & Maintenance page. `Checkup::diagnose` reads and validates, and writes nothing. It returns a `Diagnosis`: a list
of findings, each with a severity (`ok`, `warning`, `problem`), a stable code, a message, details, a hint how to fix it,
the file and line, the setting and device where they apply, and what `--fix` does about it. The CLI only renders it.

| Section | Codes | What is checked |
| --- | --- | --- |
| Compositor | `session`, `no_session`, `session_unusable`, `installed_unsupported`, `version_unknown`, `sway_missing` | the running sway or SwayFX (`GET_VERSION`), or without a session the installed one (`sway --version`) |
| Sway config | `main_config`, `no_config`, `include_active`, `include_not_last`, `include_partial`, `include_unknown`, `include_missing`, `config_diagnostic`, `kanshi_include_missing`, `kanshi_file_foreign`, `kanshi_profile_overlap`, `hook_program`, `swaybg_command` | the main config in use, the [include status](Reading-Sway.md#include-detection) (with the instructions while read-only, and up to ten of the user's lines after the include if it isn't last), and the parser's diagnostics: unreadable file, not a regular file, unresolved include, include not found, …; kanshi's config without the include of our profiles, a foreign file in their place, a profile of the user's that comes first for a combination we remember ([Outputs](Outputs.md)); whether sway and kanshi can run this `sway-things` for the hooks; and, while a wallpaper is stored, a `swaybg_command` of the user's that starts no program (`-`) or one that can't be found, so the wallpaper isn't drawn |
| Settings files | `settings_loaded`, `settings_unusable`, `settings_entry`, `setting_skipped`, `setting_not_copied` | files that can't be used (problem), entries that are ignored (warning), stored settings that are a comment in the generated file because the compositor doesn't support them, and stored values GSettings can't take ([GSettings](Backends.md#gsettings)) |
| Generated files | `file_current`, `file_not_needed`, `file_missing`, `file_out_of_date`, `file_edited`, `file_foreign`, `file_unreadable`, `file_unchecked`, `file_valid`, `file_invalid`, `validation_skipped` | each file against the manifest and what `generate` gives now, and each file of Sway Things on disk through `sway --validate` (the generators' `Validate`), with the settings behind a rejected line while the file is up to date |
| Conflicts | `no_conflicts`, `config_wins`, `config_line_dead` | for each stored value, the user's line that wins over it (warning) and those it makes dead (ok), with file:line |

- **System details:** the text report starts with a _System_ block and `--json` has a `system` object, read by
  `crates/core/src/system.rs` for the About section too: the versions of Sway Things, the compositor, GTK, the
  distribution (`PRETTY_NAME` of `/etc/os-release`, else `/usr/lib/os-release`), NetworkManager (its `Version` property,
  asked only while its bus name has an owner, so the question doesn't start it), the sound server (its name, which for
  pipewire-pulse carries PipeWire's version, from libpulse's server info; `not running` without one, answered at once)
  and the kernel (`/proc/sys/kernel/osrelease`), and whether nm-connection-editor, blueman-manager, pwvucontrol and
  pavucontrol are in `PATH`, without a version. The CLI doesn't link GTK, so it runs `sway-things-gui --version` (a
  `GApplication` option handled before the app registers on the bus, so it needs no display and reaches no running
  instance) and reads its `GTK` line; without the app the value is `unknown`. BlueZ (no version on D-Bus, `bluetoothd`
  in a different `libexec` per distribution) and wlroots (reported neither by IPC nor by `sway --version`) are left out:
  an unreliable value is worse than none.
- **Severity:** a problem means a stored setting doesn't take effect, nothing can be saved, or sway reports errors when
  it starts. A warning is worth knowing, but the settings work. A dead line of the user's is ok: that is what storing a
  value means.
- **Without a session:** the compositor is the installed one (`sway version 1.10.1`,
  `swayfx version 0.5.3 (based on sway 1.11.0)`), and the generated files are validated anyway, since `sway --validate`
  needs no session. That is the case of a sway that doesn't start, with the user at a TTY. If `sway` can't be run,
  validation is skipped (a warning, not a failure), and without a version the files can't be compared with what would be
  generated (`file_unchecked`).
- **`--fix`** writes the generated files with `generate::write`, as `persist` does: missing and out of date ones (such
  as `90-effects.conf` after [switching between SwayFX and sway](#no-daemon-the-cli-as-a-hook)), those changed outside
  only with `--overwrite`, those Sway Things didn't make never. Settings files and the user's config are never written.
  It needs the write permission of an active include status, so while read-only it writes nothing and says so. The
  report that follows shows the files as they are after the fix.
- **Exit code:** 0 without problems (warnings don't count), 5 with a problem left, 1 if `--fix` couldn't write.
- **On a terminal** (`--color`, see [Commands](#commands)), the text keeps its layout and words and only gains styles
  from the 16 standard ANSI colors, so the terminal's theme picks the shades: bold headings, the status word green,
  yellow or red (not its padding), paths, file:line, details and hints dim, commands cyan, and the summary bold in the
  color of what it counts. Meanwhile a braille spinner on stderr (`indicatif`, drawn only if stderr is a terminal) says
  what runs: loading the settings, asking sway, reading the config, each section, `sway --validate` per file, and
  writing with `--fix`. `Checkup::diagnose_with` reports each `Step` (a section starts, a file is validated) to a
  callback, so the core stays free of terminal code. The spinner is cleared before the report is printed. Piped,
  `--color never` or `NO_COLOR`, the text is exactly the plain one; `--json` is never colored and shows no spinner.
- **`--json`:** an object with `status` (the worst severity), the number of `problems` and `warnings`, `findings` and
  `fix`. Each finding has `section`, `code`, `status`, `message`, `details` (a list of lines), `hint`, `file`, `line`,
  `key`, `device` (each `null` where it doesn't apply) and `fix` (`regenerate`, `overwrite` or `null`). `fix` is `null`
  without `--fix`, else `result` (`done`, `nothing`, `read_only`, `failed`), `message` and `files`, each with `file`,
  `outcome` (`written`, `unchanged`, `not_created`, `edited_outside`, `foreign`, `invalid`) and sway's `errors`.

## No daemon: the CLI as a hook

Sway Things keeps nothing running. Behavior that has to happen without the window goes to existing tools, and where glue
is needed, generated config calls the CLI briefly:

- **`hook displays`**: switches an output on if none is on ([Outputs](Outputs.md)); the generated `20-output.conf` runs
  it with `exec_always` while Sway Things stores anything for an output. While kanshi runs, it first has kanshi apply
  its profile again (`kanshictl reload`, given five seconds), since sway just read its config, then makes sure a display
  is on and starts the mirrors again.
- **`hook workspaces`**: puts the workspaces on the displays by the rule of the Workspaces page
  ([Workspace placement](Outputs.md#workspace-placement)); `20-output.conf` and each kanshi profile run it.
- **`hook mirror`**: ends the mirrors Sway Things started and starts those of the displays connected now
  ([Mirroring](Outputs.md#mirroring)); kanshi runs it after setting up a profile.
- **`hook new-display`**: asks once whether a display plugged in to a remembered combination mirrors or extends, through
  the notification server, and remembers the answer ([A new display asks once](Outputs.md#a-new-display-asks-once));
  kanshi runs it from the profiles of our file with one display more than a remembered combination. It ends with the
  answer or after two minutes.

- **`hook wallpaper`**: shows the next picture of each [collection](Backends.md#wallpaper-collections) whose time has
  come and starts the timers of those that change daily or after an interval (`wallpaper::login`); `20-output.conf` runs
  it with `exec`, once when sway starts, while a display shows a collection, since a reload would move on a collection
  that changes at login.

The four of the displays are `outputs::hooks` in the core (`displays`, `workspaces`, `mirror`, `new_display`, with a
`HookEnv` of the settings files and kanshi's files). The CLI only connects (`hooks::connect`: `$SWAYSOCK`, or for a hook
kanshi runs without it, sway's newest socket) and prints, so the user's run and the in-process tests go through the same
code. A `HookEnv` without kanshi's files leaves kanshi out, also one that runs, so tests never reach the machine's.

```text
# ~/.config/kanshi/sway-things.conf (generated)
profile sway-things-1a2b3c4d {
    output "Dell Inc. DELL U2723QE <serial>" enable mode 3840x2160@60Hz position 0,0 scale 1.5 transform normal
    output "eDP-1" disable
    exec sway-things hook workspaces
}

profile sway-things-5e6f7a8b {
    output "eDP-1" enable
}

# ~/.config/sway/sway-things/80-hooks.conf (generated)
exec_always sway-things hook gsettings
```

`hook gsettings` (see [GSettings](Backends.md#gsettings)) runs from `80-hooks.conf`; lid hooks may follow there. It and
`hook wallpaper` run in the session like the other commands, with the settings loaded.

Our generated `20-output.conf` can also contain the `exec kanshi` line, and `hook displays` runs `kanshictl reload`.
They're our files, so that needs no edit of the user's config.

**SwayFX switching edge case:** `90-effects.conf` is only regenerated when the app or CLI runs. On most distros the
`swayfx` and `sway` packages conflict, so switching means reinstalling. After switching to vanilla sway, sway reports
unknown commands (non-fatal) until `sway-things` runs once. `sway-things doctor` reports the file as out of date and
rejected by `sway --validate`, also without a session, and `sway-things doctor --fix` writes it again.
