# Reading Sway

> Part of the [architecture](Architecture.md). How Sway Things learns what sway runs and what the user's config says,
> without ever writing it: IPC, input devices, themes, XKB rules, the config parser, include and conflict detection, the
> key bindings (and the file of those Sway Things writes), and opening the config in an editor. The outputs have a page
> of their own, [Outputs](Outputs.md).

- **IPC:** `GET_VERSION`, `GET_INPUTS`, `GET_OUTPUTS`, `GET_SEATS`, `GET_BINDING_MODES`, `GET_BINDING_STATE`,
  `GET_TREE`, `GET_CONFIG`. Event subscriptions for `input`, `output`, `mode`, `workspace`, `shutdown`, `tick`, so
  hotplugged devices and outputs appear immediately. See [IPC client](#ipc-client).
- **`GET_CONFIG` only returns the main config text**, not included files. Many values (all `client.*` colors, `gaps`,
  `font`, bindings) aren't exposed over IPC at all. So the core has its own **read-only parser**.

## IPC client

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
  value it doesn't know (`drag_lock enabled_sticky` on sway 1.10 means `disabled`). `Client::run_together` sends several
  commands as one message, which sway runs with nothing of another client's in between: moving a workspace is "focus it,
  then move the focused one", and two placers (a kanshi hook and the app) would otherwise move each other's.
- **Events:** `Client::subscribe` opens a second connection, so replies and events don't mix, and `Events::next` returns
  them in order. An event that can't be read is an error for that event only, an unknown event type is skipped, and the
  events end when sway closes the connection (after a `shutdown` event, if subscribed).
- **Connecting under load:** sway queues only a few connections before it accepts them, and a connect while that queue
  is full ends unconnected. The client checks the connection and tries again for up to two seconds.
- After an I/O or protocol error a connection is out of step and has to be replaced.

## Input devices

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
- **Switching the layout:** `devices::switch_layout(client, keyboard, switch)` switches the active layout of every
  keyboard, or of the keyboards a selector picks (one by its identifier), to the next, the previous or the one at an
  index (`xkb_switch_layout` over IPC). It is an action, not a setting: nothing is stored, and sway starts with the
  first layout again.

## Cursor themes

`crates/core/src/cursor/` lists the installed cursor themes for the Cursor & Tablet page. It only reads.

- **Where:** the directories libXcursor and wlroots search: `$XCURSOR_PATH` if set, otherwise the `icons` directories of
  `$XDG_DATA_HOME` (or `~/.local/share`) and of `$XDG_DATA_DIRS`, with `~/.icons` after the first, then
  `/usr/share/pixmaps`. A theme is a directory with a `cursors/` subdirectory, counted where its name is found first.
- **What:** its directory name (what `seat * xcursor_theme` and GSettings take), its title from the `Name=` of the
  `[Icon Theme]` group in `index.theme`, and the sizes its arrow (`left_ptr`, `default` or `arrow`) ships. The size
  drop-down offers these sizes, since libXcursor picks the closest one and doesn't scale.
- **Xcursor files** are external input: each is read up to 16 MiB, every offset and size is checked against the file,
  and a table of contents or image larger than libXcursor allows is refused. The reader returns the image of the size
  closest to the one asked for, which the theme list shows as a preview.

## GTK and icon themes

`crates/core/src/look/` lists the installed GTK themes and icon themes for the Apps Look & Feel page. It only reads.

- **Where:** the `themes` and `icons` directories of `$XDG_DATA_HOME` (or `~/.local/share`) and of `$XDG_DATA_DIRS`,
  with `~/.themes` or `~/.icons` after the first. A theme counts where its name is found first. A title two themes share
  gets the directory name in parentheses, so the list tells them apart.
- **GTK themes:** a directory with `gtk-3.0/gtk.css` or `gtk-4.0/gtk.css`; one with only `gtk-keys.css` (the Emacs key
  bindings) is no look. Adwaita and HighContrast are built into GTK 3, so they are always offered.
- **Icon themes:** a directory whose `index.theme` lists icon `Directories=`, which leaves out cursor-only themes;
  hicolor, the fallback of every theme, isn't offered. The drop-down draws a folder, a document, settings and the trash
  from each theme with its own `GtkIconTheme`.

## XKB rules

`crates/core/src/xkb/` lists the keyboard models, layouts, variants and XKB options of the system for the Keyboard page,
from the rules registry of xkeyboard-config. It only reads.

- **Which files:** `rules/evdev.xml` in the XKB config root (`$XKB_CONFIG_ROOT`, otherwise `/usr/share/X11/xkb`); sway
  uses the `evdev` rules. `rules/evdev.extras.xml` is merged in if it exists: its layouts, its variants of layouts the
  base file has and its options of known groups are added, each marked as extra, like the entries the registry marks as
  exotic. Pages show extra entries only on request.
- **What is kept:** for each entry its name and English description, and where the registry has them the short
  description, the vendor of a model, and the languages (ISO 639) and countries (ISO 3166) of a layout or variant, so
  search can match them. Option groups know whether more than one of their options can be set. Lookups give the entry
  for a model, layout, variant or option name, and the options of a group (`grp` for the layout switching shortcut).
- **Errors:** a missing or broken base file is reported and leaves the lists empty; the pages still take names typed by
  hand. A missing extras file is normal; a broken one is reported and the base file's entries stay.
- **External input:** each file is read up to 4 MiB (the registry has about 250 KiB) and at most a million XML nodes. A
  file that declares entities is refused; the DTD the registry names is never loaded, so nothing is fetched or run.
  Unknown elements, translated descriptions (`xml:lang`) and entries without a name are skipped, and a name that turns
  up twice keeps its first entry. Property tests feed the parser arbitrary bytes.
- **Translations** of the descriptions (the `xkeyboard-config` gettext domain) are not read yet.

## Sway config parser

`crates/core/src/sway_config/` reads the user's config the way sway's `read_config` does, and only reads.

- **Which file:** `parse` takes the path of the main config. In a session, `GET_VERSION` names the loaded one; otherwise
  `default_path` follows sway's search order: `~/.sway/config`, `$XDG_CONFIG_HOME/sway/config`, `~/.i3/config`,
  `$XDG_CONFIG_HOME/i3/config`, `/etc/sway/config`, `/etc/i3/config`.
- **Lines:** a line starting with `#` is a comment (there are no comments at the end of a line), a backslash at the end
  joins the next line, and a `{` on a line of its own belongs to the line before it.
- **Blocks:** like sway, the parser puts the line that opened a block in front of every line inside it. A directive
  carries its block as words: `["input", "type:touchpad"]`, `["bar", "colors"]`, `["mode", "resize"]`. `input`, `output`
  and `seat` settings written on one line (`input type:touchpad tap enabled`) get the same block, so both spellings read
  the same. Blocks nest at most 32 deep (each line repeats every line that opened its blocks, so deeper nesting would
  take quadratic time); a deeper block is skipped up to its `}` with everything in it.
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
  (unreadable file, not a regular file, unresolved include, include not found, file already included, unbalanced braces,
  blocks or includes nested too deeply, a config too big to read). Only a config too big to read stops the parser; it
  reads on after the others.
- **Best-effort by design:** the parser doesn't know which commands exist or what they do; that is for conflict
  detection and the registry. It reads through a `Files` trait, with the real file system or files in memory.
- **Robustness:** the work is bounded by limits on what the config expands to, not only on its size, because short lines
  can stand for long text: include depth (32), block depth (32), directives (100,000), file size (8 MiB, for the main
  config and every included file; a file is read no further than that, so a file that never ends can't fill the memory),
  a line once its variables are replaced (8 MiB; a variable holds a line, so lines that each use it twice would double
  it every time), and the text of all lines with their block prefixes and variables replaced plus the paths of their
  includes (64 MiB; otherwise each line could copy a long variable or block line). Too many directives, a line too long
  or too much text end the config; a file too big and includes or blocks too deep are skipped; each with a diagnostic.
  Every step within these limits is linear in the length of its input. Property tests feed the parser arbitrary bytes
  and compare its line splitting with the port of sway's parser that the command builders are tested against. A
  cargo-fuzz target feeds it configs with their includes in memory and checks that it ends within its limits without a
  panic (see [Testing](Testing.md)).
- **Only regular files:** the main config and every included file are read only if they are regular files once symbolic
  links are followed (sway follows them too). A directory, device (`/dev/zero`), FIFO or socket isn't even opened and
  gets a diagnostic, because reading it might never end and opening a FIFO waits for a writer; sway itself would read
  it. A path that changes between that check and the read can't block either: the file is opened with `O_NONBLOCK`, and
  what was opened is checked again before anything is read.

## Include detection

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
  | unknown | the line wasn't found, but part of the config can't be followed: an include that needs a shell command, an unreadable file or one that isn't a regular file, includes nested too deeply | read-only |
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

## Conflict detection

Sway evaluates config top to bottom, and for most settings the last definition wins. The
[include detection](#include-detection) knows where each generated file sits in the user's config.

| User line is… | Effect | Indicator |
| --- | --- | --- |
| before our include | our value wins, the user's line is dead | "Overridden: your config line N is no longer in effect" |
| after our include | the user's value wins, ours is dead | "Your config line N overrides this setting" |
| our include missing, partial or unknown | nothing we generate is reliably active | app-wide **read-only** mode with the include instructions |

`crates/core/src/conflict/` finds these lines. `Report::check(status, setting, device, known)` lists every user line
that sets a setting, in the order sway runs them, each with file, line, text, the value read from it, and whether it
runs before or after the generated file of the setting. The search is linear in the length of the config, so it runs
again for one setting after it changed, and for all of them (`check_all`, also for every known device and output) after
the config or the settings files changed on disk.

- **Top-level directives** match by name and, where the registry names them, leading arguments (`gaps inner`). Names
  compare without regard to case, as in sway.
- **`input` options** merge per device. For one device the last line wins among those for `*`, for its type and for its
  identifier, in config order: sway's `store_input_config` merges each kind onto the others as it reads them.
  Identifiers and types compare exactly (`strcmp`). In a report for a whole input type, a line for one device is listed
  with that device and doesn't decide the type's value; with the device list, lines for devices of another type are left
  out.
- **`seat` options** count for `seat *` and the default seat, `seat0`: Sway Things sets every seat with `seat *`, and
  sway takes the cursor of the default seat for `XCURSOR_THEME`. Lines for other seats are left out.
- **`output` settings** merge per output the same way: for one output, the last line wins among those for `*`, for its
  name and for its identifier (`Known::outputs` matches a line by name with the output stored by its identifier; without
  the outputs only lines for `*` and the identifier count). sway's own merging depends on whether the output is
  connected while it reads the line (`store_output_config` in `sway/config/output.c`): at a reload it is, and the last
  line wins as reported; at a login it usually isn't yet, and a line by the identifier (as Sway Things writes it) wins
  over one by the name wherever it stands. The report follows the reload, which is when a change made in the app meets
  the user's lines.
- **One line, several settings:** a `client.*` line sets all five colors, so every setting of the group reports it, and
  storing one color makes the user's line dead for all of them.
- **What counts:** a line sway rejects sets nothing and never wins; a line with a value the setting can't hold still
  wins. The **user's value** is that of the last line that decides the setting: pages show it while Sway Things stores
  nothing ("from ~/.config/sway/config:120").
- **`exec`** accumulates: every line runs, so there is nothing to win or lose. The Apps & Autostart page lists the
  user's `exec` lines next to Sway Things' own, see [Apps and autostart](Backends.md#apps-and-autostart). Bindings have
  their own conflict detection, see [Key bindings](#key-bindings).

Actions on a conflict: **Open in editor at line** (see [Editor](#editor)), **Copy line**, **Let my config win**.
`Applier::let_config_win` removes the stored values of the setting's directive (every setting of a group, since the
user's line sets all of them) and sends the user's value from the report, so nothing reloads; without a line of the
user's, sway's default. If a value of the group can't be read from the line, nothing is sent and the caller reloads
sway, which reads the line itself.

Bindings defined in the user's config can still be changed from the app: our include comes later, so it can `unbindsym`
and re-bind without touching their file.

## Key bindings

`crates/core/src/bindings/` reads every binding the way sway does and writes those of the Keyboard Shortcuts page; a
single line, its keys and its command are read in `crates/core/src/sway_config/bindings/` (`keys`, `Line`,
`BindCommand`, `item`), in the sway layer. The facts behind it are in the
[Sway Reference](Sway-Reference.md#key-bindings).

- **Keys** (`keys`): a combination is split at `+` (empty parts dropped); parts that name a modifier (`Shift`, `Lock`,
  `Control`/`Ctrl`, `Mod1`/`Alt`, `Mod2`, `Mod3`, `Mod4`/`Super`, `Mod5`, without regard to case) make a bit mask, the
  others are keys, sorted. A keysym compares in lowercase with its aliases resolved (`E` is `e`, `Page_Up` is `Prior`,
  `quoteleft` is `grave`), as xkbcommon's case-insensitive lookup prefers the lowercase keysym; a numeric name (`0x60`,
  `U0060`) compares as the printable ASCII keysym it stands for. `leaves_typing` says whether a combination leaves its
  keys usable for typing: with Super, Ctrl, Alt or `Mod3`, or on keys nobody types with (F1 to F35, `XF86…`, Print,
  Pause, Scroll Lock, Menu); Shift and AltGr (`Mod5`) alone type other characters. `button1` to `button9` and `BTN_…`
  names, and `bindcode` numbers that are button event codes, make a mouse binding, as do `--whole-window`, `--border`
  and `--exclude-titlebar`; `Group1` to `Group4` (and `Mode_switch`) limit a binding to a layout. Labels are what people
  read: modifiers first (Super, Ctrl, Alt, Shift), letters upper case, `Enter`, `Volume Up`, `Keypad 1`.
- **Lines** (`Line`): `bindsym`, `bindcode`, `unbindsym` and `unbindcode` with their options (read until a word isn't
  one, like sway), keys and command; at the top, in a `mode` block (with `--pango_markup`) or as
  `mode <name> bindsym …`. `$mod` and other variables are already replaced by the [parser](#sway-config-parser) as sway
  does when it reads the line, so a line that uses `$mod` before its `set` is rejected, like in sway. The **identity**
  is what sway compares (`binding_key_compare`): mode, input device, kind of keys (keysym, keycode, mouse; a `--to-code`
  binding counts as keycodes when a keymap translates its keys), `--release`, `--locked`, `--inhibited`, the window
  region of a mouse binding, layout group, modifiers and keys. `--no-repeat` and `--no-warn` aren't part of it.
- **Table** (`Table`): the lines in the order sway reads them; a binding with the identity of one before takes its place
  (`Replaced`), an unbind removes one (`Removed`) or fails (sway reports an error).
- **Listing** (`Listing`): the user's lines before the generated files, ours, the user's after them, as rows for the
  page, each with what it is (_hand-written_, _added_, _changed_, _removed_, a media key group), whether sway has it,
  and its **clashes**: replaces or is replaced by another line (both rows), removed by an unbind of the user's, a keysym
  and a keycode binding of one key (sway looks at keycode bindings first, so the keycode one wins; needs the keymap,
  which the app takes from GDK: the keycode whose first level in the first layout gives the keysym, as sway translates
  `--to-code`), and a `--locked` binding next to one without it with another command (sway runs the one without unless
  the screen is locked). `users_of` lists what a new binding would clash with; the Workspaces page can use it for its
  keys. A row can be changed here only in the default mode, read before the generated files, with keys and command that
  can be written back.
- **Commands** (`BindCommand`): sway keeps the rest of the line after the keys as one string and runs it like an IPC
  message when the keys are pressed (split at `;` and `,`, variables replaced again, quotes removed except for `exec`).
  So a command is kept exactly as sway keeps it (one space between words outside quotes) and rendered so sway keeps
  exactly it: in a config line with every `$` doubled, over IPC as one quoted argument (`SwayText`'s IPC quoting). A
  command can't have a control character, a backslash (escapes are read once more and can't be written back reliably),
  an unclosed quote or `[`, or end with a brace. A property test runs both renderings through the port of sway's parser.
  `Action` is what the editor offers instead of free text: `exec` with the command line as one quoted argument (sway
  runs a single quoted argument of `exec` without its quotes, so the shell gets the line as typed), `focus`/`move` in a
  direction, `workspace number N`, `move container to workspace number N`, `kill`, `fullscreen toggle`,
  `floating toggle`, or a sway command whose first word must be a command sway 1.12 knows. `describe` and `Category` say
  what common commands do, for the rows: sway's own (focus, move, workspaces, layouts, resizing a window in a mode) and
  the programs people bind most, by their command line (volume and mute with `wpctl` or `pactl`, brightness with
  `brightnessctl`, `playerctl`, a lock screen, `grim` with or without `slurp`, `swaynag` before `swaymsg exit`); any
  other `exec` is _Run …_.
- **Settings** (`registry/shortcuts.rs`, `bindings::ours`, `sway_config::bindings::item`): `shortcuts.bindings` is a
  list of lines in sway's syntax, as people write them (`bindsym $mod+Return exec foot`, `unbindsym Mod4+d`), each
  checked by `ours::parse` (keys and device without quotes, a command that can be written back, no `mode` prefix). A
  command whose first word sway (or SwayFX) doesn't know passes that check, so one typo in `settings.toml` doesn't drop
  every binding, but `parse_strict` refuses it for the generated file (its item becomes a comment that names it, which
  `doctor` reports), for `sway-things set` and in the app. `$mod` stands for `shortcuts.modifier` (`super` → `Mod4`,
  `alt` → `Mod1`); it is never written as a variable, since the user's `$mod` may mean something else. A binding of the
  user's is removed with an `unbindsym` item, and changed with a change item: the unbind, `=>` and the binding that
  takes its place (`unbindsym Mod4+d => bindsym $mod+p exec menu`; an unbind has no command, so the separator can't be
  part of one). Only a change item makes a row _Changed here_; an unbind next to an unrelated binding stays a removal
  and an addition. `shortcuts.media.volume`, `….brightness` and `….player` bind fixed keys with `--locked` (`wpctl`,
  `brightnessctl`, `playerctl`; each switch needs its program). `….volume_osd`, `….brightness_osd` and `….player_osd`
  bind the same keys to `swayosd-client` instead (`Group::osd_keys`: `--output-volume raise`, `--brightness lower`,
  `--playerctl play-pause`, …), which changes the value and shows it in SwayOSD's on-screen display; they only matter
  while their group is on (`depends_on`). `crate::swayosd` tells whether `swayosd-client` is installed and whether
  `swayosd-server` runs in this session (a process of that name in `/proc`, as for kanshi) or what starts it (an `exec`
  of the user's config, an enabled systemd user unit `swayosd…`); Sway Things never starts it. None of these settings
  has a sway directive. The keys and the check of the items are defined in the registry and re-exported by
  `bindings::ours`; the check reads an item with `sway_config::bindings::item`, in the sway layer below the registry.
- **`50-bindings.conf`** (`ours::file_lines`): first every unbind, each after a placeholder of the same identity
  (`bindsym --no-warn Mod4+d nop`, then `unbindsym Mod4+d`). sway rejects an unbind of a binding it doesn't have, and
  the file is validated on its own (with nothing bound), so a bare unbind would never pass `sway --validate`, and would
  turn into an error at login once the user's line is gone; the placeholder quietly replaces the user's binding if it is
  there and makes the unbind always succeed. Then the media keys with `--no-warn`, as switching them on means taking
  those keys; then the bindings of the items, without `--no-warn`, so sway still warns about a duplicate nobody asked
  for. An item that can't be written becomes a comment. `SwayCmd::binding` renders a line for both targets.
- **Applied live** (`bindings::live`, `Applier::apply_bindings`): the table of the whole config is worked out with the
  bindings before and after the change, and only the difference in the default mode is sent: a binding that is gone is
  unbound (after a placeholder), a new or changed one is bound with `--no-warn`. So undoing a change binds the user's
  line again with its command from their config. The applier knows the user's config through `Applier::set_user_config`
  (the app before each change, the command line once); without it, or for a binding that can't be sent exactly (a user's
  command with a backslash), the change waits for a reload. A command sway rejects (an unknown key) undoes what was sent
  and keeps nothing. A binding whose options changed is bound again (every option counts but `--no-warn`); one whose
  `--to-code` changed is unbound first, since sway may keep it in another list. The commands follow the order of the
  table, so where several bindings change at once, a binding sent later replaces an earlier one of the same keys, as in
  the file; the list sway ends up with is the same, though its order (which decides between two bindings that clash
  without being the same) can differ from that after a reload.
- **Editing** (`bindings::edit`): the page's changes as new lists of items: `add` (a binding of the user's on the same
  keys becomes a change item; one of Sway Things' is replaced, a change on the same keys keeps only its unbind),
  `change`, `remove` (a change keeps its unbind) and `restore`, which takes only the item of its row.
- **Keys of other pages** (`capture::shortcuts`): the keys of Screenshots & Clipboard are settings of that page (the
  keys as text, `$mod` allowed, empty for none; checked to need no quotes and to leave typing alone) bound while the
  switch of their part is on. `ours::file_lines` writes them after the media keys, with `--no-warn` like those and
  `--no-repeat` (a key held down would take screenshots in a row, or start and stop a recording), each
  `exec "sway-things <command>"`; their origin is `Origin::Feature(<key>)` and their row `RowKind::Feature`, which the
  Keyboard Shortcuts page lists with what they take the place of and can't change, and whose keys its dialog refuses. A
  change of one is sent like any binding change (`Applier::apply_bindings`).
- **The terminal and the launcher** of the Apps & Autostart page are bindings of this file too (`Origin::App`, after the
  media keys): `bindsym --no-warn <keys> exec <command>`, since choosing one means taking its keys. Their rows on
  Keyboard Shortcuts say where they are set and can't be changed there; the editor refuses their keys like a media key
  group's.

## Editor

`editor::resolve` (core) turns the preference `app.editor_command`, a file and maybe a line into a program with
arguments (`editor::Run`) or `Launch::DefaultApp`. It reads the environment through a function it is given, so tests
pass their own.

- `editor::split` splits the command into words like a POSIX shell (single quotes, double quotes with `\"`, `\\`, `\$`
  and ``\` ``, a backslash outside quotes) and expands nothing: `$`, `~`, globs, `;` and `|` stay plain characters. A
  quote that isn't closed or a backslash at the end is an error, which is also the setting's check
  (`ValueError::NotACommand`), so such a value is never stored.
- `{file}` and `{line}` are filled into the words (the file as an `OsString`, so it needn't be UTF-8); a word with
  `{line}` is dropped when there is no line, and the file is appended if no word names it.
- An empty command takes the words of `$VISUAL`, else `$EDITOR`, adds `+LINE` and the file, and puts a terminal in
  front: `xdg-terminal-exec` if an executable of that name is in `PATH`, else the words of `$TERMINAL` and `-e`. A
  variable that is empty, not UTF-8 or can't be split counts as unset. With no editor or no terminal, the result is the
  default application.
- `Run::spawn` starts the program with `async-process` (argument array, standard input closed). The GUI (`editor::open`)
  waits for it on the main loop, so it doesn't stay behind as a zombie, and shows an error if it can't start or ends
  with a failure, until an open works; the default application opens through `gtk::FileLauncher`.
