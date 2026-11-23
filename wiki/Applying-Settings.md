# Applying Settings

> Part of the [architecture](Architecture.md). How a change travels from a row or `sway-things set` to sway and the
> files: the data flow, the safety net for risky settings, the apply modes and the typed sway command builders.

## Data flow

### Changing a normal setting

1. The user flips a toggle (or runs `sway-things set …`).
2. The core builds the sway command with the [**typed builder**](#sway-command-builders) and sends it over IPC
   (`RUN_COMMAND`) → instant effect.
3. The core updates the in-memory state and saves `settings.toml` / the overlay: the CLI at once, the GUI 500 ms after
   the last change (see `persist` below).
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
  need the write permission of an active [include status](Reading-Sway.md#include-detection).
- **A failed command changes nothing:** the state is as before, and commands of the same directive that were already
  sent are undone with the previous values.
- **Reset** of one setting (`Applier::reset_to_config`, for the row's ↺ and `sway-things reset`) works like a page
  reset: where a line of the user's config sets the value, it wins as _Let my config win_ makes it (all settings of the
  directive at once, the user's value sent at once), under the same conditions; for one device or output also only while
  nothing is stored for all of them, whose value applies then. Else it removes the stored value and sends sway's default
  (for one device: the value of its type). A reload of sway wouldn't do: `gaps inner 8` in the config sets only the gaps
  of new workspaces. A reset of a setting that isn't stored sends nothing.
- **A page reset** (`Applier::reset_page`) resets every stored value of the page, as `PageReset::plan` lists them,
  except the risky ones, which only the safety net changes, and the wallpaper collections, which the user made rather
  than set. Where a line of the user's config sets a value, the page reset lets it win as _Let my config win_ does (all
  settings of the directive at once, the user's value sent at once), unless a setting of the directive is risky or on
  another page, or kanshi sets up the display; else it resets the value, which sends sway's default. While kanshi sets
  up the displays, a reset of Displays also forgets the profile of the displays connected. The app's confirmation counts
  from the same plan.
- **Refused:** risky settings (they change and reset only through the [safety net](#risky-settings-safety-net)), and a
  `set` of a setting or value the compositor doesn't support, with what it needs. Nothing is stored then. A reset is
  always allowed, so such a value can be removed.
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
- **Waiting changes:** in a session, the applier notes each change that waits for a reload of sway or a re-login
  (`waiting`), with the stored value from before, which is what sway still runs with. A change set back to that value no
  longer waits. Risky values from changes on disk aren't noted: a reload would apply them without the safety net.
- **`reload_sway`** persists, then sends `reload`. It refuses while a risky change waits (the reload would put the
  stored value back), without a session, and when a generated file with a waiting change couldn't be written. Afterwards
  the changes that waited for a reload are in effect; those for a re-login still wait. `reloaded` notes a reload from
  elsewhere, which sway reports as a `workspace` event with the change `reload` (`ipc::Reloads`); it only counts for
  changes that were in the generated files by then.

### Risky settings (safety net)

Display mode, disabling an output or input device, changes of the keyboard layouts, variants and XKB options:

1. Snapshot the current value and apply live.
2. Show the _Keep / Revert_ banner with a 15 s countdown.
3. **Nothing is persisted until the user presses _Keep_.** Revert or timeout re-applies the snapshot over IPC.
4. Lockout protection: the last enabled keyboard, the last enabled pointing device and the last output that is on can't
   be switched off.

`crates/core/src/apply/risky.rs` is this flow in the core; the GUI banner and the CLI prompt build on it.

- **`Applier::try_risky`** checks the change like `set` does, sends it and returns a `Countdown` with the deadline from
  `app.safety_net_timeout` (15 s by default). Nothing is stored. The snapshot is the value in effect as a row shows it
  (`SettingView`): the stored value, else what the user's config sets, else sway's default. sway reports keyboard
  layouts only by their names (`English (US)`), not as `xkb_layout` takes them, so the core's value is what is restored.
- **`Applier::try_risky_reset`** tries removing the stored value the same way. It sends the value that is in effect
  without it, as a row would show it then: what the user's config sets, else sway's default (for one device: the value
  of its type). That is the value sway has after the next reload, so that is what the safety net tests. A setting
  without a stored value sends nothing.
- **`Applier::try_risky_all`** tries several settings of one device as one change, kept or reverted as a whole: the
  Keyboard page adds, removes and reorders a layout together with its variant, and resets both together. Each setting
  gets a new value or is removed as in `try_risky_reset`. The directives are sent in registry order. A setting can name
  another that is sent at its default first when both are in the change (`clears_first`: the layouts clear the
  variants), so no command in between pairs a layout with a variant it doesn't have; the revert does the same.
- **`keep`** stores the value like any other change, or for a reset removes it (saved with `persist`); nothing more is
  sent. **`revert`** sends the snapshot; the caller runs it when the user says so and when `Countdown::elapsed` returns.
  The countdown waits with `async_io`, so it works under the GTK main loop and `block_on` alike.
- **One at a time:** while a change waits, another risky change is refused until it is kept or reverted.
- **Only in a session:** without a running sway nothing can be tried and reverted, so `try_risky` refuses.
  `set_risky_unchecked` and `reset_risky_unchecked` (for `sway-things set --yes` and `reset --yes`) send the value if
  there is a session and store or remove it at once.
- **Lockout protection:** a change of `events` asks sway for the devices and is refused if it would leave no usable
  keyboard or pointing device ([Input devices](Reading-Sway.md#input-devices)), for one device as for a whole type.
  Switching off an output asks sway for the outputs and is refused if none would stay on ([Outputs](Outputs.md)). A
  session that has none to begin with (a headless sway) has nothing to lose. Without a session the devices aren't known,
  so switching off a whole type of keyboards or pointing devices, or every output, is refused; one device or output is
  allowed.
- **Another value while one waits:** `try_risky_all` for the same settings of the same device while a change waits tries
  the new value in its place and keeps the first one's undo, so a revert goes back to what was before the first; the
  same value again returns the waiting trial and sends nothing; a value the output runs with already (the one from
  before) is still tried rather than stored at once. A change that sends nothing (a mode back to what sway chooses)
  reverts the waiting one and is stored for the next reload, as without one. The refresh rates of the mode row lock with
  the row. Any other risky change still gets `PendingChange`.
- **What a trial returns:** `Tried::Waits` with the `Trial` while the change waits to be kept or reverted,
  `Tried::Stored` for a change that needed no safety net and was stored at once, and `Tried::Unchanged` for a reset of a
  setting without a stored value.
- **Outputs:** where nothing sets an output's value, the snapshot is what the output runs with (`GET_OUTPUTS`, which
  also matches the user's lines that name the output by its connector), so a revert of a new mode goes back to the mode
  sway chose. A change that sends nothing (an empty mode, or removing a mode sway is to choose again; what the output
  runs with doesn't count for that, sway chooses anew) needs no safety net: it is stored at once (`Tried::Stored`) and
  waits for a reload of sway. So does a change that nothing could revert, such as the mode of an output that is off,
  which sway doesn't report. Keeping a change drops what waited for a reload for the same setting. Where an output runs
  with something else than what is stored (an output stored as off that `hook displays` switched on, a mode set with
  `swaymsg`), a revert brings back what it runs with (`as_running`), and a value it already runs with is stored at once
  without a trial.
- **If the app or the command line ends** while a change waits, nothing was stored: the session keeps the tried value
  until the next reload of sway (or login), which brings back the old one.

**In the app** (`crates/gui/src/model.rs`, `window.rs`): a change of a risky row goes through `try_risky` instead of
`set`, and the model keeps the `Pending` change for the rows (they show its value) and the window (the banner). A glib
timer at the countdown's deadline queues the revert; _Keep_ and _Revert_ queue `keep` and `revert`. Refusals reach the
row through the `failed` signal. Closing the window while a change waits reverts it, then closes.

**From the CLI**: in an interactive terminal, `sway-things set` and `reset` apply live, ask `Keep? [y/N]` with the same
countdown, and save only on `y`. Without a TTY it refuses unless `--yes` is given, so scripts and hooks never hang.

### Apply modes

| Mode | Example | Mechanism |
| --- | --- | --- |
| live | gaps, colors, input options | IPC command + persist |
| reload | settings sway only honors at load | persist + `reload` after the user confirmed (reload banner) |
| tool reload | the notification daemon's settings ([Notifications](Backends.md#notifications)); planned: waybar (kanshi reloads through the [display profiles](Outputs.md)) | mako's `Reload`, swaync's `ReloadConfig` and `ReloadCss` over D-Bus; planned: `SIGUSR2` to waybar |
| re-login | `xwayland` (sway reads it only at startup); planned: environment.d | persist + "takes effect after re-login" badge |
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
  sway splits it) and backslashes. An empty value is a part of its own, `SwayCmd::empty_text`, rendered `""` in both
  targets, which sway unquotes to an empty argument; only builders whose directive gives the empty value a meaning use
  it (`xkb_variant`, `xkb_options`, `xkb_model`). sway's IPC splitter keeps its escape flag across `\'` and then
  misreads the next quote, so a backslash next to a quote can't be represented reliably.
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
  supports it; otherwise sway reads it as `disabled` without an error. The XKB lists are joined with commas, so an item
  can't contain one; `xkb_layout` needs at least one layout, the variants, options and model can be empty.
- `command::output` builds `output <name> …` for every output (`*`) or one by its name or identifier (text, never `*`,
  `-` or `--`, which sway reads as every output or the focused one): `mode` (`2560x1440@59.951Hz`, the refresh rate in
  hertz with up to three decimals since sway counts millihertz and takes the mode nearest to it), `pos`, `scale` (0.25
  to 10; sway takes any scale above 0), `transform` and `enable`/`disable`. A mode is a word the builder makes from
  numbers, so it isn't quoted.
- `command::window` builds colors (`client.*`; sway ignores `client.background` and `client.placeholder`, so they have
  no builder), borders, gaps, the title bar and `opacity`. `gaps <which> <px>` only changes the default for new
  workspaces, and `gaps <which> all|current set <px>` (runtime-only) only open workspaces, including those with gaps of
  their own from `workspace <name> gaps`; the registry combines them. sway rejects a title bar padding below the border
  thickness and a thickness above the vertical padding, so the order of those two commands matters.
- `command::effects` builds the SwayFX effects with the names and ranges of SwayFX 0.5; `animation_duration_ms` needs
  SwayFX 0.6. `corner_radius` raises the horizontal title bar padding to at least the radius.
- `SwayCmd::binding` renders a binding line of [Key bindings](Reading-Sway.md#key-bindings): keys and device as plain
  words, the command as the binding keeps it (`$` doubled in a config line, quoted as one argument over IPC).
- `command::behavior` builds focus, layout, floating, dragging, popup, Xwayland and idle inhibition settings.
  `floating_minimum_size` and `floating_maximum_size` take each dimension as no limit (`-1`), sway's automatic value
  (`0`) or pixels. `inhibit_idle`, like `opacity`, is wrapped in `for_window` or leading criteria for per-app rules.
- `command::output` also builds `output <selector> bg …` for every output or one: an image with a fill mode and a
  fallback color, or a color alone (`solid_color`), without alpha. A fit stored without an image, color or collection
  (for the output, or for all of them) makes no line, so the user's own `bg` line stays. sway passes the image path to
  `wordexp(3)`, which runs `$(…)` and expands variables, so the path is quoted in single quotes for `wordexp` (an inner
  `'` as `"'"`) before it is quoted as text for the target; only a leading `~/` stays outside, for `wordexp` to replace
  with the home folder. Relative paths and two spaces in a row (which sway escapes before `wordexp`) are refused. The
  fallback color is always written: sway 1.11 and later show it for an image they can't find.

A port of sway's parsing functions (`command/model.rs`, tests only) checks with property tests that every accepted value
arrives unchanged in both targets. Fixture files generated from the builders (`ci/integration/*.conf` and `*-ipc.txt`)
go to real sway 1.10, sway latest and SwayFX: every generated config file must pass `sway --validate` on its own, every
IPC command must succeed, and the escaping fixtures read every value back. An IPC test sends images with hostile file
names; sway 1.10 only accepts an image it finds, so the name must reach `wordexp` unchanged. Fixtures marked
`# Needs SwayFX.` only run on SwayFX.
