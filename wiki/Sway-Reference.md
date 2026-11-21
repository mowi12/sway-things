# Sway Reference

Facts about sway, SwayFX and the companion tools that Sway Things relies on, each with where it was checked. The code
and the other wiki pages state the consequences; this page keeps the evidence, so that a new sway or SwayFX version can
be checked against it.

Sources are man pages, source files at the named version (`sway/…` is the sway repository, `WillPower3309/swayfx` the
SwayFX one), and tests in the CI containers (`ci/containers/`), which run sway 1.10, the latest sway and the latest
SwayFX. When a new version comes out, re-check the facts whose source names the version it replaces.

## Config and IPC

| Fact | Source |
| --- | --- |
| `sway --validate -c <file>` works without a running session and reports errors with file and line. | Tested with sway 1.12. |
| `sway --validate` ignores errors in included files: the exit status reflects only the main file. So each generated file is validated on its own. | `sway/commands/include.c` ("We don't care if the included config(s) fails to load"), `sway/main.c` (`return valid ? 0 : 1`), 1.10 and 1.12; reproduced in the containers. |
| Vanilla sway rejects SwayFX directives such as `corner_radius`. | Same test. |
| `include` takes globs and wordexp; `unbindsym` and `unbindswitch` exist. | `man 5 sway` (1.12) |
| sway 1.10 skips a relative `include` when the config path itself is relative (it joins the parent directory after changing into it); 1.12 fixed it. | `sway/config.c` 1.10 vs 1.12; the containers. |
| `GET_CONFIG` returns only the text of the main config, not of included files. | `man 7 sway-ipc` |
| Variables are substituted line by line while the config is read. | `sway/commands.c`, `config_command` (`do_var_replacement`) |
| A command over IPC and the same line in the config are parsed differently: command separators, the order of `$` replacement, quote stripping, and backslashes (kept over IPC, unescaped in the config). Unchanged from 1.10 to 1.12. | `sway/commands.c` (`execute_command`, `config_command`), `common/stringop.c`, 1.12; compared with 1.10. |
| `argsep` keeps its escape flag across `\'`, so a quote right after it is misread. That is why the command builders reject backslashes in sway text. | `common/stringop.c` (`argsep`); reproduced in a test of `crates/core/src/command/model.rs`. |
| Some commands are deferred while the config is read and run later through `execute_command`: `workspace <name>`, `focus`, `mode`, `seat cursor`, `xkb_switch_layout`, `exec_always`. | `CMD_DEFER` in `sway/commands/*.c`, `run_deferred_commands` |
| `default_orientation`, `workspace_layout` and `xwayland` are config-only; over IPC they are unknown commands. | `config_handlers` in `sway/commands.c`, 1.10 and 1.12; confirmed in the containers. |
| `seat <name> …` in the config doesn't create a seat that has no devices; over IPC it does. | The containers. |
| One-line `bar <id> …` commands reuse the current bar while the config is read; only a `bar {` block starts a new one. | `sway/commands/bar.c`, `read_config` |

## Key bindings

| Fact | Source |
| --- | --- |
| A binding takes the place of an earlier one with the same mode, input device, kind of keys, `--release`, `--locked`, `--inhibited`, window region, layout group, modifiers and set of keys; `--no-repeat` and `--no-warn` don't count. It logs "Overwriting binding" and shows a swaynag warning unless the new one has `--no-warn`. | `binding_key_compare`, `binding_upsert`, `binding_add` in `sway/commands/bind.c`, 1.10 and 1.12; `Super+F` after `$mod+f` reproduced with sway 1.12 and SwayFX 0.6. |
| Modifier names are compared without regard to case: `Shift`, `Lock`, `Control`, `Ctrl`, `Mod1`, `Alt`, `Mod2`, `Mod3`, `Mod4`, `Super`, `Mod5`. The keys are sorted, so their order doesn't matter. | `get_modifier_mask_by_name` in `sway/input/keyboard.c`; `list_qsort` in `bind.c`, 1.12. |
| A keysym is looked up with `XKB_KEYSYM_CASE_INSENSITIVE`, which prefers the lowercase keysym: `Mod4+E` and `Mod4+e` are one binding, as are `Prior` and `Page_Up`. An unknown name is an error ("Unknown key or button"). | `identify_key` in `bind.c`; `sway --validate -d` with sway 1.12. |
| An unbind of a binding sway doesn't have fails: "Could not find binding … for the given flags", which `sway --validate` reports as an error. | `binding_remove` in `bind.c`; `sway --validate` with sway 1.12. |
| `--to-code` translates the keysyms to the keycodes of the first layout of the keysym translation keymap; a keysym on no key or on several stays a keysym binding. `--to-code` is ignored for `bindcode`. | `translate_binding`, `get_keycode_for_keysym` in `bind.c`, 1.12. |
| On a key press sway looks at keycode bindings, then raw keysyms, then translated keysyms; of two matching bindings with the same input, lock and inhibit state the first found wins, so a keycode binding wins over a keysym binding of the same key. Without a lock, a binding without `--locked` wins over one with it. | `get_active_binding` and `handle_key_event` in `sway/input/keyboard.c`, 1.12. |
| The rest of a binding line after the keys is kept as one string (`join_args`) and run through `execute_command` when the keys are pressed: split at `;` and `,`, quotes removed except for `exec`, variables replaced again. Over IPC, `bindsym`'s arguments are unquoted (only `exec`, `exec_always` and `mode` keep quotes). | `cmd_bindsym_or_bindcode`, `seat_execute_command` in `bind.c`; `execute_command` in `sway/commands.c`, 1.12. |
| `exec` with a single quoted argument runs it without the quotes; otherwise it joins the arguments. The command runs with `sh -c`. | `cmd_exec_process` in `sway/commands/exec_always.c`, 1.10 and 1.12. |
| `mode <name> bindsym …` on one line binds in that mode; `mode` strips the quotes of the name itself. | `cmd_mode` in `sway/commands/mode.c`, 1.12. |
| `grave` and `quoteleft`, `apostrophe` and `quoteright` are one keysym; `U0060`, `u0060` and `0x60` are `grave`, `0x0027` is `apostrophe`. `Tab` and `ISO_Left_Tab`, `U20AC` and `EuroSign` are different keysyms. | `sway --validate -d` ("Overwriting binding") with SwayFX 0.6 (sway 1.12). |
| `$$` in a config line becomes `$` when the line is read, and again when a binding's command runs: `exec "echo $$HOME $$$$$$$$"` in the generated file, and `exec "echo $HOME $$$$"` quoted as one argument over IPC, both make the shell see `echo $HOME $$`, so it prints the home directory and its own process number. | A headless SwayFX 0.6 (sway 1.12), the binding pressed with `wtype`, from the generated file after a reload and sent live. |
| GTK 4 reports no modifier mask for `Mod3` or `Mod5`; a key pressed with AltGr shows as its third shift level. | `GdkModifierType` and `gdk_key_event_get_level` in GTK 4.18. |
| Bindings sent over IPC take effect at once, and keys of a virtual keyboard (`wtype`) run them. | A headless SwayFX 0.6 (sway 1.12) with `swaymsg -t subscribe '["binding"]'`. |

## Input

| Fact | Source |
| --- | --- |
| Input identifiers are `<vendor>:<product>:<name>`, decimal, with spaces and non-printable bytes in the name replaced by `_`. | `input_device_get_identifier` in `sway/input/input-manager.c`, 1.10 and 1.12 |
| `pointer_accel` must be within −1 … 1 and `scroll_factor` ≥ 0 (both `parse_float`); `repeat_delay` and `repeat_rate` are read with `atoi` and must be ≥ 0. | `sway/commands/input/*.c` 1.10 |
| `drag_lock enabled_sticky` needs sway 1.11 and a libinput with `LIBINPUT_CONFIG_DRAG_LOCK_ENABLED_STICKY`; otherwise `parse_boolean` reads it as disabled. | `sway-input.5.scd` 1.10 vs 1.11, `sway/commands/input/drag_lock.c`, `meson.build` 1.11 |
| `xkb_numlock` and `xkb_capslock` are config-only. | `input_config_handlers` in `sway/commands/input.c` 1.10 |
| `map_to_output` and `map_to_region` set the same mapping of a device; the last one wins. `map_to_output *` maps to the whole desktop and also ends a mapping to a region. | `sway/commands/input/map_to_output.c`, `map_to_region.c`, `seat_apply_input_mapping` in `sway/input/seat.c` 1.10 |
| Without a mapping, sway maps a built-in touchscreen or tablet to the built-in output and leaves other devices unmapped, but it never undoes a mapping by itself: once mapped over IPC, a device keeps it until another `map_to_output`. | `seat_apply_input_mapping` (`MAPPED_TO_DEFAULT`) in `sway/input/seat.c` 1.10 |
| `tool_mode * <mode>` sets the pen, eraser, brush, pencil and airbrush; mouse and lens tools are always relative. | `sway/commands/input/tool_mode.c`, `sway-input.5.scd` 1.10 |
| `seat * xcursor_theme <theme> [<size>]` (size 24 unless given) applies live; sway sets `XCURSOR_THEME` and `XCURSOR_SIZE` from the default seat's theme, which `seat *` reaches, for the programs it starts afterwards. Without a theme, wlroots loads the one called `default`. | `sway/commands/seat/xcursor_theme.c`, `seat_configure_xcursor` in `sway/input/seat.c` 1.10 |
| `hide_cursor <ms>` raises a timeout between 1 and 99 ms to 100; 0 never hides. `hide_cursor when-typing enable\|disable` is a separate option. | `sway/commands/seat/hide_cursor.c` 1.10 |
| GTK apps take the cursor theme and size from GSettings (`org.gnome.desktop.interface` `cursor-theme`, `cursor-size`), not from sway. | The `translations` table in `gdk/wayland/gdkdisplay-wayland.c` (GTK 4), which maps these keys to `gtk-cursor-theme-name` and `gtk-cursor-theme-size`. |
| GTK 4 on Wayland reads its theme, icon theme, font, text scaling and font rendering from the same schema (`gtk-theme`, `icon-theme`, `font-name`, `text-scaling-factor`, `font-antialiasing`, `font-hinting`, `font-rgba-order`), through the Settings portal if one runs, else from GSettings. The light or dark preference it reads only from the portal (`org.freedesktop.appearance` `color-scheme`), which xdg-desktop-portal-gtk fills from `org.gnome.desktop.interface` `color-scheme`; xdg-desktop-portal-wlr has no Settings backend, so without -gtk (or -gnome) apps don't learn the style. | `translations` in `gdk/wayland/gdksettings-wayland.c` (GTK 4, main); `get_color_scheme` in `src/settings.c` of xdg-desktop-portal-gtk (main), checked 2026-11-19 |
| `gsettings-desktop-schemas` 48 (Debian 13) already has `font-antialiasing`, `font-hinting` and `font-rgba-order` in `org.gnome.desktop.interface` (they were in `org.gnome.settings-daemon.plugins.xsettings` before), and `color-scheme` with `default`, `prefer-dark` and `prefer-light`. | `org.gnome.desktop.interface.gschema.xml` of the Debian 13 package, checked 2026-11-19 |

## Outputs

| Fact | Source |
| --- | --- |
| `output` reads its subcommands one after the other, each taking the words it needs (`mode` one or two, `pos` one or two, `transform` one plus an optional `clockwise`/`anticlockwise`, `enable`/`disable`/`toggle` none, `bg` up to its mode word and an optional `#fallback`, `modeline` the remaining eleven). An unknown subcommand or an invalid value rejects the whole line. Aliases: `res`/`resolution` for `mode`, `position` for `pos`, `background` for `bg`. | `sway/commands/output.c` and `sway/commands/output/*.c` in SwayFX 0.6 (sway 1.12) |
| An output's identifier is `<make> <model> <serial>` with `Unknown` for what wlroots doesn't know, written into a 128-byte buffer (longer ones are cut off). `GET_OUTPUTS` reports make, model and serial the same way. | `output_get_identifier` in `sway/config/output.c`, `ipc_json_describe_wlr_output` in `sway/ipc-json.c` 1.12 |
| For one output, sway merges the stored configs for `*`, then its name, then its identifier. Storing a config for a name supersedes the identifier's values only while that output is connected; one for `*` supersedes all others. | `find_output_config_from_list`, `store_output_config` in `sway/config/output.c` 1.12 |
| `mode` takes the mode of that size with the refresh rate nearest to `roundf(rate * 1000)` millihertz; without a rate the highest. There is no command back to the preferred mode, the automatic position or the automatic scale: only a config without them does that. | `set_mode` in `sway/config/output.c`, `sway/commands/output/mode.c` 1.12 |
| sway has no protection against having no output on: an `output X disable` from the config applies also when X is the only output connected. `exec_always` runs at the start and at every reload, after the outputs of the backend are there. | `sway/config/output.c`, `run_deferred_commands` in `sway/config.c` 1.12; checked with SwayFX 0.6 headless |
| A relative `transform … clockwise` is rejected in a config file. `scale` takes any number; one of 0 or less means "pick one". | `sway/commands/output/transform.c`, `apply_output_config` 1.12 |
| `GET_OUTPUTS` lists disabled outputs with `active` false, no scale or transform and a zero rect, and non-desktop outputs (VR headsets) with `non_desktop` true. The rect is in logical pixels: the mode turned by the transform and divided by the scale, rounded down (`wlr_output_effective_resolution`). | `sway/ipc-json.c`, `sway/ipc-server.c` 1.12 |
| A headless sway (`WLR_BACKENDS=headless`) has one output, `HEADLESS-1`, with make, model and serial `Unknown` and no modes; `create_output` adds another. Commands for outputs that aren't connected succeed: sway stores them. | Checked with SwayFX 0.6 (sway 1.12) |
| Without a `bg` for an output, sway starts no swaybg, and the output stays black. `output … bg` makes sway start swaybg (`swaybg_command`) with the backgrounds of every output; `swaybg_command -` starts none, so no background is drawn. | `spawn_swaybg` in `sway/config/output.c` 1.10; `swaybg_command` in sway(5) |
| `bg <color> solid_color` and the fallback color of `bg <file> <mode> [<color>]` only take `#rrggbb`. The modes are `stretch`, `center`, `fill`, `fit` and `tile`, without regard to case; the file is the words up to the first mode, and what follows the optional color is the next subcommand of the line. `background` is another name of `bg`. | `sway/commands/output/background.c` 1.10 and 1.12, `sway/commands/output.c` |
| The file goes through `wordexp(3)` with no flags, so `$(…)` and backticks run commands and variables expand, after sway put a backslash between two spaces in a row. A relative path is relative to the config file while the config is read. | `expand_path` in `common/stringop.c`, `background.c` 1.10 and 1.12 |
| sway 1.10 rejects `bg` with a file it can't find, also with a fallback color, and `sway --validate` fails; sway 1.11 and later use the fallback color then and reject only a line without one. | `background.c` 1.10 vs 1.11 and 1.12; `sway -d --validate` logs "Cannot access file" with 1.12. |
| swaybg reads pictures with gdk-pixbuf when built with it, else only PNG (cairo). gdk-pixbuf reads WebP only with the loader of `webp-pixbuf-loader`, a package of its own (Debian, Ubuntu, Fedora, Arch; not in openSUSE's main repository); without it, swaybg can't show a `.webp` and the screen keeps the color. | `load_background_image` in swaybg `background-image.c` (master); packages.debian.org, packages.ubuntu.com, packages.fedoraproject.org, archlinux.org |
| `output … bg` over IPC makes sway start swaybg again, which reads the file again, also when the line names the same file as before. | `cmd_output` in `sway/commands/output.c` 1.10 (calls `spawn_swaybg` unless the config is being read); `_spawn_swaybg` in `sway/config/output.c` destroys the old client without comparing commands |

## Windows, borders, gaps and title bars

| Fact | Source |
| --- | --- |
| Defaults: `default_border` and `default_floating_border` normal 2, `titlebar_padding` 5 4, `titlebar_border_thickness` 1, `title_align` left, `smart_gaps` off, gaps 0, `hide_edge_borders` none. | `config_defaults` in `sway/config.c` 1.10 |
| Default client colors (border, background, text, indicator, child border): focused_inactive `#333333 #5f676a #ffffff #484e50 #5f676a`, unfocused `#333333 #222222 #888888 #292d2e #222222`, urgent `#2f343a #900000 #ffffff #900000 #900000`. | `config_defaults` in `sway/config.c` 1.10 |
| `client.background` and `client.placeholder` are accepted but do nothing. | `sway/commands/client.c` 1.10, `sway.5.scd` |
| `client.focused_tab_title` takes 3–5 colors but uses 3, ignoring indicator and child border. While it is unset, such a container is drawn as focused_inactive (or unfocused). | `sway/tree/container.c`, `sway/commands/client.c` 1.10 |
| `default_border <style> [px]` reads the width (`atoi`) only with exactly two arguments, otherwise keeps it. `hide_edge_borders smart` and `smart_no_gaps` set the same flag as `smart_borders` (and edges none). | `sway/commands/default_border.c`, `hide_edge_borders.c` 1.10 |
| `titlebar_padding h [v]` must be at least the current border thickness, and `titlebar_border_thickness` at most the current vertical padding, so the order of the two commands matters. | `sway/commands/titlebar_*.c` 1.10 |
| `gaps <type> <px>` only sets the default for new workspaces. `gaps <type> all\|current set <px>` is runtime-only (rejected while the config is read) and changes the open workspaces, including those with `workspace <name> gaps`. | `sway/commands/gaps.c` 1.10 and 1.12 |
| `workspace <name> gaps …` looks for the keywords `output` and `gaps` after unquoting, so a workspace named `output` or `gaps` breaks the command. | `sway/commands/workspace.c` 1.10 |
| `opacity` takes 0 … 1 and needs a container: it fails while the config is read and when no window exists. | `sway/commands/opacity.c` |

## Floating windows and behavior

| Fact | Source |
| --- | --- |
| Floating size limits, per dimension: −1 means none, 0 automatic (at least 75 × 50, at most the output layout), more than 0 pixels. | `floating_calculate_constraints` in `sway/tree/container.c`, 1.10 and 1.12 |
| `floating_modifier` takes one modifier, case-insensitive: Shift, Lock, Control or Ctrl, Mod1 or Alt, Mod2, Mod3, Mod4 or Super, Mod5. | `get_modifier_mask_by_name` in `sway/input/keyboard.c` 1.10 |

## SwayFX

| Fact | Source |
| --- | --- |
| SwayFX 0.5 is based on sway 1.10.1, 0.5.2 and 0.5.3 on 1.11, 0.6 on 1.12 (and adds animations). | GitHub releases of WillPower3309/swayfx |
| Ranges: `corner_radius` 0–99, `blur_passes` and `blur_radius` 0–10, `blur_noise` 0–1, `blur_brightness`, `blur_contrast` and `blur_saturation` 0–2, `shadow_blur_radius` 0–99, `shadow_offset` ±99.9, `default_dim_inactive` 0–1, `animation_duration_ms` 0–5000 (since 0.6). All are in the general handler table, so they work in the config and over IPC. `corner_radius` raises the title bar padding. No effect changed from 0.5 to 0.5.3. | `sway/commands/*.c` at 0.5 and 0.6, `sway.5.scd` |
| Defaults: `corner_radius` 0, `smart_corner_radius` on, blur off, shadows off, `shadow_blur_radius` 20, `shadow_offset` 0 0, shadow and inactive shadow color `#0000007f`, `default_dim_inactive` 0, dim colors `#000000` and `#900000`, `animation_duration_ms` 0 (0.6). | `config_defaults` in `sway/config.c` at 0.5 and 0.6 |
| Blur defaults: passes 3, radius 5, noise 0.02, brightness 0.9, contrast 0.9, saturation 1.1. | scenefx `blur_data_get_default`, 0.2.1 (used by SwayFX 0.5) and 0.4.1 |
| `shadow_color` also sets the inactive shadow color while both are equal; `animation_duration_ms` reads its second argument when given several. | `sway/commands/shadow_color.c` 0.5, `animation_duration_ms.c` 0.6 |

## Companion tools

| Fact | Source |
| --- | --- |
| kanshi supports `include` with wordexp. | `man 5 kanshi` |
| kanshi applies the first profile whose displays are all connected and that covers every connected display; `output *` matches one display, `...output *` any number. A directive it doesn't know makes it reject the whole config. | `man 5 kanshi` of kanshi 1.9.0 (checked); older versions not checked |
| While its profile still matches, kanshi keeps it when displays are plugged in or out: with `...output * enable` active, plugging in a display of a remembered combination doesn't switch to that profile; without such a profile it does. | Tried on headless sway with kanshi 1.9.0 (`swaymsg create_output`, `output … unplug`) and seen on a docked laptop |
| kanshi reads its config with scfg: a word is quoted with `"…"` (with `\` escapes) or `'…'` (as it is), `\` escapes a character outside quotes, and a quoted string next to another word is a word of its own. A `#` starts a comment only as the first thing on a line, and a block's directives start on a line after its `{`. | Tried with `kanshi -c` of kanshi 1.9.0 |
| An alias (`output … { alias $name }`) or a default defined after the `include` line can be used in the included file: kanshi reads the whole config before it applies a profile. | Tried on headless sway with kanshi 1.9.0 |
| `kanshictl reload` has kanshi read its config again; `exec` lines of a profile run (through a shell) when it was applied. | `man 1 kanshictl`, `man 5 kanshi` of kanshi 1.9.0 |
| A profile with the displays of another one plus `output *` (and no directives for them) matches exactly one display more and changes nothing of their setup (a display that is off stays off; mode, position and scale stay); kanshi applies it in place of a profile of the same displays plus that display that comes after it (a profile of the user's after our include), and never for two displays more or a display alone. Two displays plugged in within one `swaymsg` command match no such profile. `kanshictl reload` applies the first profile that matches again, also while another one still matches: a profile for the combination written before it then wins. | Tried on headless sway with kanshi 1.9.0 (`swaymsg create_output`, `output … unplug`) |
| kanshi doesn't wait for a profile's `exec`: the command runs in a process of its own (not kanshi's child), and kanshi applies the next profile while it still runs. | Tried on headless sway with kanshi 1.9.0 (`exec … sleep 20`, displays unplugged meanwhile) |
| kanshi matches a display by its connector, by `make model serial` (`Unknown` for a missing field) with `fnmatch` patterns (`"Epson PJ *"`, since kanshi 1.8), or by `*`. | `man 5 kanshi` of kanshi 1.9.0, kanshi's `main.c` |
| wlroots (since 0.17, through libdisplay-info) takes an output's model from the EDID's product name, else the product code (`0x1234`), and its serial from the serial string, else the 32-bit number as `0x%08X`, else none (`Unknown` in sway). Some projectors report only a number that is the same for every unit of a model (`0x00000001`); several monitors report `0x01010101`. Adapters with DP++ or LSPCON pass the display's own EDID through; some adapters and AV switchers present an EDID of their own. | wlroots `backend/drm/util.c`, libdisplay-info `info.c`; EDIDs in github.com/linuxhw/EDID (BenQ BNQ0203, Panasonic MEI0030); not checked with real projectors |
| A notification server takes `Notify` with actions as key and label pairs; the key `default` is what a click on the notification invokes, and `ActionInvoked`/`NotificationClosed` say what happened. mako (1.11) shows no buttons: a left click invokes `default` (`on-button-left=invoke-default-action`), other actions only through `makoctl menu` or `makoctl invoke`. | Desktop Notifications Specification 1.2; `man 5 mako`; `makoctl invoke` tried with mako 1.11 on headless sway |
| sway gives an output a new `id` in `GET_OUTPUTS` each time it is plugged in: wlroots makes a DRM output anew at each hotplug. | Tried with headless outputs (`create_output`, `output … unplug`); not checked with a real display |
| sway's `workspace N output X` adds to the list of outputs of workspace N and never clears it; a new workspace goes to the first connected output of the list. A reload of sway clears the lists. | sway `commands/workspace.c`, `tree/workspace.c` (`workspace_get_initial_output`) |
| `workspace N output A B C` lists several outputs: the first one available is used, and a workspace placed on an output further down moves to a higher one once that one is there again. | `man 5 sway` (`workspace <name> output <outputs...>`), 1.12 |
| `wl-mirror --fullscreen-output O --scaling fit\|cover\|exact --title T <source>` shows `<source>` fullscreen on `O`. | `wl-mirror --help` |
| waybar's `include`: the first definition of a key wins. | `man 5 waybar` (0.15) |
| mako supports `include=` with an absolute or `~/` path. | mako `doc/mako.5.scd` |
| swaync has no include, but takes its config and style with `-c` and `-s`. | `swaync --help` (0.12.6) |
| `swayosd-client --output-volume raise\|lower\|mute-toggle\|mute\|unmute\|±N` changes the default sink (a number is a step: negative lowers, any other raises), `--input-volume` (same values) the default source, `--brightness raise\|lower\|+N\|-N\|N` the backlight (a number without a sign sets it), `--playerctl play-pause\|play\|pause\|stop\|next\|prev\|previous\|shuffle` the MPRIS player; `--device`, `--player`, `--monitor`, `--max-volume`, `--min-brightness` narrow it. `--flag=value` works too (clap). | SwayOSD 0.3.2: `src/args.rs`, `src/client/main.rs`, README |
| `swayosd-client` does nothing without `swayosd-server`: it asks the server over the session bus (`org.erikreider.swayosd-server`, `/org/erikreider/swayosd`) to do the action and show it, and exits with 1 ("Could not connect to SwayOSD Server") when no one owns the name. The server is started by the user (`exec swayosd-server`, the README's sway example); upstream ships no user unit for it, only the system service of `swayosd-libinput-backend`. | SwayOSD 0.3.2: `src/client/main.rs`, `src/config.rs`, `data/services/`, README |
| The server changes the volume through libpulse (so with pipewire-pulse too) by 5 % per step, up to `max_volume` of its `config.toml` (100 % unless set; the shipped config has 150 commented out); the brightness through `brightnessctl` (falling back to blight) by 5 %, not below `min_brightness` (5 in the shipped config); the player through MPRIS, without `playerctl`. A keyboard backlight changed by the hardware shows by itself through UPower (`keyboard_backlight = true`, the default); there is no client action for it. | SwayOSD 0.3.2: `src/server/utils.rs` (`VOLUME_CHANGE_DELTA`, `BRIGHTNESS_CHANGE_DELTA`), `src/server/application.rs`, `src/actions/`, `data/config/config.toml` |

## Notification daemons

Checked against mako 1.11.0, swaync 0.12.6 and dunst at 1.13.2 (sources at those tags, and the installed mako 1.11 and
swaync 0.12.6 in a headless sway); the CI containers' distributions ship mako 1.10.0 and swaync 0.11.0 (Debian 13), mako
1.11.0 and swaync 0.12.6 (Arch, Void), dunst 1.12.2 (Debian 13).

| Fact | Source |
| --- | --- |
| mako reads `~/.mako/config` if it exists, else `$XDG_CONFIG_HOME/mako/config`. | `get_default_config_path` in mako's `config.c` (1.11) |
| mako takes `include=` only exactly so: blanks around `=` make the key unknown, and a path that starts with neither `/` nor `~/` is refused ("Config path must start with / or ~/"); no variables are expanded. Either fails the whole config. | `load_config_file`, `expand_config_path` in `config.c` (1.11) |
| mako keeps its modes across `Reload`: they live in its state, not in its config. | `handle_reload` in `dbus/mako.c`, `mode.c` (1.11) |
| mako's `include=` takes an absolute or `~/` path and loads the file as if its lines stood there, starting in the global section; `include=` is only taken among the global options (in a section it is "Failed to parse option"), and a file it can't open fails the whole config. | `apply_config_option`, `load_config_file` in `config.c` (1.11) |
| mako matches criteria sections in the order they are defined; the last option that matches wins. Options before the first section are global. | `mako(5)` (1.11), "CRITERIA" |
| `[mode=do-not-disturb] invisible=1` hides notifications while the mode is set; `makoctl mode -a/-r/-t` read the modes (`ListModes`) and set the new list (`SetModes` on `fr.emersion.Mako`, `/fr/emersion/Mako`); `-a` adds a mode a second time if it is there. The list starts as `default`. | `mako(5)` "MODES", `run_mode` in `makoctl.c`, `dbus/mako.c` (1.11) |
| mako 1.10 and newer emit `PropertiesChanged` for `Modes`; 1.9 has no property. `Reload` re-reads the config (`fr.emersion.Mako.InvalidConfig` if it can't). `InvokeAction(u id, s action)` doesn't dismiss; `DismissNotifications(a{sv})` (1.10+) does, 1.9 has `DismissNotification(ub)`. | `dbus/mako.c` at 1.9.0, 1.10.0 and 1.11.0 |
| mako's `exec` bindings run `sh -c` with `id="$1"` set to the notification's id; `on-button-left` is one action, by default `invoke-default-action`, which invokes the action and dismisses the notification. | `notification_execute_binding`, `try_invoke_action` in `notification.c` (1.11) |
| mako ends when it can't own `org.freedesktop.Notifications` ("Is a notification daemon already running?"). | `init_dbus` in `dbus/dbus.c` (1.11) |
| mako's `GetServerInformation` says `mako`, `emersion`, `0.0.0`; swaync's `SwayNotificationCenter`, `ErikReider`, its version; dunst's `dunst`, `knopwob`, its version. | `dbus/xdg.c` (mako), `notiDaemon.vala` (swaync), `src/dbus.c` (dunst) |
| mako and swaync install a D-Bus service file for `org.freedesktop.Notifications` (swaync with `SystemdService=swaync.service`), and both have a systemd user unit (`Type=dbus`). With both installed, a call to the name while nobody owns it starts one of them. | `/usr/share/dbus-1/services/*`, `/usr/lib/systemd/user/*` of the Debian packages (mako 1.11, swaync 0.12.6) |
| swaync takes `-c`/`-s`; a path that doesn't exist falls back to `~/.config/swaync/` and then `$XDG_CONFIG_DIRS`. It always loads its packaged style sheet first (unless `--skip-system-css`) and the user's on top. It ends when it can't own `org.freedesktop.Notifications`. | `main.vala`, `get_style_path`, `load_css` in `functions.vala` (0.12.6) |
| swaync's D-Bus interface `org.erikreider.swaync.cc` at `/org/erikreider/swaync/cc`: `GetDnd`, `SetDnd`, `ToggleDnd`, `ReloadConfig`, `ReloadCss`, `SetVisibility`, and the signal `SubscribeV2(count, dnd, cc_open, inhibited)` on each change (what `swaync-client -D/-dn/-df/-R/-rs/-cp/-s` use). The same in 0.10.1 and 0.11.0. `swaync-client` waits for swaync to start unless given `-sw`. | `swayncDaemon.vala`, `client.vala` (0.10.1, 0.11.0, 0.12.6) |
| swaync ignores members of `config.json` it doesn't know (a `$comment` first member works). | Tried with swaync 0.12.6 |
| swaync runs a script of `scripts` with `run-on: action` when an action of a notification is clicked, beside the action itself, with `SWAYNC_APP_NAME`, `SWAYNC_DESKTOP_ENTRY`, `SWAYNC_SUMMARY`, … in its environment, through `/bin/sh -c`. A toggle button of `buttons-grid` runs its command with `SWAYNC_TOGGLE_STATE` and shows what `update-command` prints (`true`/`false`). `swaync-client -a N` invokes the N-th alternative action of the newest popup. | `configModel.vala`, `notification.vala` (`action_clicked`), `toggleButton.vala` (0.12.6); tried with a headless sway |
| swaync 0.12 (GTK 4) styles with CSS variables (`--noti-bg` as `r, g, b` with `--noti-bg-alpha`, `--text-color`, `--noti-border-color`, `--bg-selected`, `--border-radius`); 0.11 (GTK 3) with `@define-color` (`noti-bg`, `text-color`, `noti-border-color`, `bg-selected`) and a compiled radius. | `data/style/style.scss` at 0.11.0 and 0.12.6 |
| GTK 3 (swaync 0.11) rejects a whole user style sheet over a CSS variable or `:root` ("Load user CSS Error"); GTK 4 (swaync 0.12) still reads `@define-color`. | Tried with swaync 0.11.0 (Debian 13) and 0.12.6 |
| swaync's shipped `config.json` differs from its schema's defaults (`cssPriority` `user`, `control-center-height` 600, `control-center-layer` `top`). | `src/config.json.in`, `src/configSchema.json` (0.12.6) |
| dunst reads `dunstrc` from the first of `$XDG_CONFIG_HOME/dunst` and `$XDG_CONFIG_DIRS/dunst`, then the `*.conf` drop-ins of the `dunstrc.d` next to that file in alphabetical order; without a `dunstrc` of the user's, `~/.config/dunst/dunstrc.d/` isn't read. | `get_conf_files`, `config_files_add_drop_ins` in `src/settings.c` (1.13.2) |
| dunst's `org.dunstproject.cmd0` at `/org/freedesktop/Notifications` has the read-write property `paused` (what `dunstctl set-paused`/`is-paused` use) and `ConfigReload`. | `src/dbus.c` (1.13.2) |
| The layer-shell namespace of mako and dunst is `notifications`, of swaync's popups `swaync-notification-window` and of its panel `swaync-control-center`. | `wayland.c` (mako), `wl.c` (dunst), `notificationWindow.vala`, `controlCenter.vala` (swaync) |
| SwayFX's `layer_effects <namespace> <effects>` joins the arguments after the namespace into its effects (`blur enable`), in the config and over IPC; `sway --validate` passes any effect and only logs `Invalid layer_effects effect!` for one it doesn't know. | `sway/commands/layer_effects.c`, `sway-fx(5)` at 0.5 and 0.6.1; tried with SwayFX 0.6 |

## Apps and autostart

| Fact | Source |
| --- | --- |
| `exec` and `exec_always` are deferred while sway reads its config (and under `--validate`, which runs none of them): the line is queued as read, before its variables are replaced, and run later through `execute_command`, which replaces `$$` once and splits at `;` and `,` outside quotes. `exec` lines are skipped at a reload. | `cmd_exec_validate` in `sway/commands/exec_always.c` and `cmd_exec` in `exec.c`; `read_config` (`list_add(config->cmd_queue, strdup(expanded))`) and `run_deferred_commands` in `sway/config.c`, 1.10; the generated `70-autostart.conf` started a headless sway 1.12 with every entry run once. |
| sway doesn't set `XDG_CURRENT_DESKTOP`. Its session file has `DesktopNames=sway;wlroots` since 1.11 (so a login manager sets `sway:wlroots`); 1.10's has none. | `sway/main.c` (only `WAYLAND_DISPLAY`, `DISPLAY` are set), `sway.desktop` at 1.10, 1.11 and master. |
| sway doesn't start XDG autostart entries. | No reader of `autostart` in the sway repository (1.12). |
| systemd's `xdg-desktop-autostart.target` has `RefuseManualStart=yes`: only a unit that wants it can start it (`sway-session.target` of sway-systemd, uwsm, GNOME, Plasma). A manual stop is allowed, and it has `StopWhenUnneeded=yes`; stopping it or a unit that wants it stops none of the entries' services, since `Wants=` carries no stop. | `units/user/xdg-desktop-autostart.target` (unchanged since it came in systemd 246). |
| `systemd-xdg-autostart-generator` makes a service per entry: none for `Hidden=true`, `X-systemd-skip=true`, a `Type` other than `Application`, no `Exec`, a `TryExec` or `Exec` program that isn't found; `OnlyShowIn`/`NotShowIn` become an `ExecCondition` that checks `XDG_CURRENT_DESKTOP` of the user manager, the first desktop named in either list deciding; `X-GNOME-Autostart-Phase` adds `NotShowIn=GNOME`. `X-GNOME-Autostart-enabled` isn't read. The services are `After=` and `PartOf=graphical-session.target`. | `src/xdg-autostart-generator/xdg-autostart-service.c` and `xdg-autostart-condition.c` (systemd main, November 2026). |
| `dex --autostart` reads `$XDG_CONFIG_HOME/autostart` and the `autostart` directories of `$XDG_CONFIG_DIRS`, the first file of a name winning; it skips `Hidden=true`, entries whose `OnlyShowIn` lacks the environment, those whose `NotShowIn` has it, and a missing `TryExec`. The environment is `--environment`, else the whole of `XDG_CURRENT_DESKTOP` compared as one string. `X-GNOME-Autostart-enabled` isn't read. | `dex` 0.10.1 (`get_autostart_directories`, `get_autostart_files`, `--environment`); a headless sway 1.12 with dex 0.10.1 started the entries the page says start. |
| An override with `Hidden=true` and no `Exec` is skipped quietly: systemd's generator checks `Hidden` before anything else (a debug message only), dex reads an entry without requiring `Exec` and checks `Hidden` before it runs one. | `xdg_autostart_service_generate_unit` (systemd main); `Application.__init__` and `get_autostart_files` in `dex` 0.10.1. |
| GNOME Console (`kgx`) takes everything after `-e` or `--` as the command and its arguments; Tilix's `-e` takes the command as one string. | `kgx_filter_arguments` in `src/kgx-utils.c` (Console main); `cmdparams.d` (Tilix). |
| The MIME apps spec reads, in `$XDG_CONFIG_HOME`, `$XDG_CONFIG_DIRS` and then `applications/` of `$XDG_DATA_HOME` and `$XDG_DATA_DIRS`, first `<desktop>-mimeapps.list` for each desktop of `XDG_CURRENT_DESKTOP` (lowercase, in order), then `mimeapps.list`. GLib does the same. | MIME Applications Associations spec 1.0.1; `desktop_file_dir_unindexed_read_mimeapps_lists` in GLib's `gio/gdesktopappinfo.c` (main). |
| GLib starts apps with `Terminal=true` through `xdg-terminal-exec` before any other known terminal, since 2.76. `xdg-terminal-exec` reads `<desktop>-xdg-terminals.list` then `xdg-terminals.list` in the XDG config directories, one desktop file ID per line, `#` starting a comment. | `known_terminals` in `gio/gdesktopappinfo.c` (commit "gio: add xdg-terminal-exec as a known terminal", October 2022); the README of xdg-terminal-exec. |
| A desktop file ID is the path below `applications/` with `/` as `-`; the first data directory with an ID wins, and `Hidden=true` there means the entry is deleted. Localized keys are looked up as `lang_COUNTRY@MODIFIER`, `lang_COUNTRY`, `lang@MODIFIER`, `lang`. `Exec` field codes `%f %F %u %U` take files, `%i` the icon (`--icon`), `%c` the name, `%k` the file; `%d %D %n %N %v %m` are deprecated. | Desktop Entry Specification 1.5. |

| Fact | Source |
| --- | --- |
| pipewire-pulse serves the PulseAudio protocol on `$XDG_RUNTIME_DIR/pulse/native`; its server name carries PipeWire's version (`PulseAudio (on PipeWire 1.6.9)`), its protocol version is 15.0.0. | `pactl info` against pipewire-pulse 1.6.9 |
| When the default sink changes, the streams that follow the default move along (WirePlumber; PulseAudio since 15.0); a stream moved explicitly (`move-sink-input`) gets a target and stays there. | checked with PipeWire 1.6.9 and WirePlumber 0.5.19; PulseAudio 15.0 release notes |
| `PA_CONTEXT_NOFAIL` only waits for the first connection; a context whose server goes away fails and stays failed. | checked with libpulse 17.0 against a restarted pipewire-pulse |
| A peak-detect recording stream (`PA_STREAM_PEAK_DETECT`, float32, one channel, 25 Hz, `fragsize` of one sample) gives a level meter. With `PA_STREAM_DONT_INHIBIT_AUTO_SUSPEND` it doesn't keep the device awake, but pipewire-pulse makes such a stream a passive node, which gets no data from an input no app records from (tested on PipeWire 1.6.9), so the meter leaves the flag out. | pavucontrol `src/mainwindow.cc` (`createMonitorStreamForSource`); tested against pipewire-pulse 1.6.9 and the stand-in |
| `libpulse-binding` 2.30's `StreamRestore::write` passes its `&[&Info]` to `pa_ext_stream_restore_write` as an array of C structs, so the server answers "Invalid argument"; its subscription callback is freed with the `StreamRestore` value. | `libpulse-binding` 2.30.1 `src/context/ext_stream_restore.rs` |
| pipewire-pulse's `module-null-sink` with `media.class=Audio/Source/Virtual` makes a virtual source; WirePlumber 0.5 leaves the machine's sound cards alone with `monitor.alsa = disabled` (and the Bluetooth and camera monitors) in `wireplumber.profiles`. | PipeWire's `pipewire-pulse-modules`, WirePlumber 0.5 configuration docs |
| pavucontrol's volume sliders go to `PA_VOLUME_UI_MAX` (about 153 %); GNOME offers going above 100 % as a setting (`org.gnome.desktop.sound allow-volume-above-100-percent`). | pulse `volume.h`, gsettings-desktop-schemas `org.gnome.desktop.sound` |

## BlueZ and rfkill

| Fact | Source |
| --- | --- |
| BlueZ uses the agent of the program that called `Pair`; the default agent only for requests nobody started. | `doc/org.bluez.AgentManager.rst`, `doc/org.bluez.Device.rst` (BlueZ 5.7x) |
| An agent answers with `org.bluez.Error.Rejected` or `Canceled`; `Cancel()` ends an open request. | `doc/org.bluez.Agent.rst` |
| Setting `Alias` to an empty string gives a device its own name back. | `doc/org.bluez.Device.rst` |
| `/dev/rfkill` speaks 8-byte events (`idx`, `type`, `op`, `soft`, `hard`); `RFKILL_OP_CHANGE_ALL` sets every radio of a type. | `include/uapi/linux/rfkill.h` |
| systemd's udev rules tag `/dev/rfkill` with `uaccess`, so the user of the active session may write it. | systemd `src/login/70-uaccess.rules.in` |

## NetworkManager

| Fact | Source |
| --- | --- |
| NetworkManager's object manager is at `/org/freedesktop`; `GetManagedObjects` there lists the manager, devices, access points, connections and IP configurations. | `src/core/nm-dbus-manager.c` (`OBJECT_MANAGER_SERVER_BASE_PATH`) |
| `AddAndActivateConnection` uses the access point given to complete the new connection; `nmcli device wifi connect` sends only the password (`psk`) of the security settings, without `permissions` (every user) and with the default `psk-flags` 0 (kept by NetworkManager). | `introspection/org.freedesktop.NetworkManager.xml`, `src/nmcli/devices.c` |
| `ActivateConnection` with the connection "/" activates the best connection for the device; a VPN needs its connection path. | `introspection/org.freedesktop.NetworkManager.xml` |
| A secret agent lives at `/org/freedesktop/NetworkManager/SecretAgent`; `GetSecrets` flags: `0x1` may ask the user, `0x2` the old secret failed; it answers errors of `org.freedesktop.NetworkManager.SecretAgent` (`PermissionDenied`, `UserCanceled`, `NoSecrets`). | `introspection/org.freedesktop.NetworkManager.SecretAgent.xml`, `libnm-core-public/nm-dbus-interface.h`, `nm-errors.h` |
| An agent registers with `RegisterWithCapabilities`; `NM_SECRET_AGENT_CAPABILITY_VPN_HINTS` is `0x1`. | `introspection/org.freedesktop.NetworkManager.AgentManager.xml`, `libnm-core-public/nm-dbus-interface.h` |
| libnm reads VPN plugins' `.name` files from `NM_VPN_PLUGIN_DIR`, then `$libdir/NetworkManager/VPN`, then `/etc/NetworkManager/VPN`; a file must end in `.name`, not start with a dot, be a regular file owned by root or the user, and not be group- or world-writable or setuid. A plugin matches its `service` or one of its `aliases`; a relative `[GNOME]` `auth-dialog` is its basename in `LIBEXECDIR`; `supports-hints` is read as true/yes/on/1. | `src/libnm-core-impl/nm-vpn-plugin-info.c`, `_nm_utils_check_file()` in `nm-utils.c` |
| GNOME Shell starts a VPN's auth dialog with `-u` UUID, `-n` name, `-s` service type, `--external-ui-mode` (when `[GNOME]` `supports-external-ui-mode` is true/yes/on/1), `-i` (may ask), `-r` (request new) and `-t` per hint (when it supports hints); writes `DATA_KEY=`/`DATA_VAL=` and `SECRET_KEY=`/`SECRET_VAL=` pairs, each followed by an empty line, then `DONE`; reads key and value lines up to two empty lines and answers `QUIT`. In external UI mode it reads a key file (`[VPN Plugin UI]` with `Version=2`, `Title`, `Description`; a group per secret with `Value`, `Label`, `IsSecret`, `ShouldAsk`), asks for the groups with `ShouldAsk` and passes the others' non-empty values on; no output means the secrets are stored. | `js/ui/components/networkAgent.js` |
| nmcli signs in to OpenConnect VPNs itself: it runs `openconnect --authenticate <gateway>` (found in `/sbin`, `/usr/sbin`, `/usr/local/sbin`, then the `bin` directories) with `--protocol`, `--useragent`, `--os`, `--certificate`, `--cafile`, `--sslkey`, `--key-password`, `--proxy`, the MCA options, `--key-password-from-fsid`, `--csd-wrapper` and `--token-mode`/`--token-secret` from the profile's data, and sets the `cookie`, `gateway` (`CONNECT_URL`, else `HOST`), `gwcert` (`FINGERPRINT`) and `resolve` (`RESOLVE`) secrets from its output. | `src/libnmc-base/nm-vpn-helpers.c` (`nm_vpn_openconnect_authenticate_helper`) |
| openconnect prompts on stderr without a line break and reads a line from stdin (also from a pipe); a choice is printed as `LABEL [a\|b]:`; with `--authenticate` all progress goes to stderr and only the result to stdout; a certificate that fails verification asks "Enter 'yes' to accept, 'no' to abort; anything else to view:". openconnect 9.21 also prints `CONNECT_URL` and `RESOLVE`, and has `--external-browser`. | openconnect `main.c` (`prompt_for_input`, `prompt_opt_select`, `write_progress`, `validate_peer_cert`), `openconnect --help` |
| WPA passwords take 8 to 63 bytes or 64 hexadecimal digits; SAE (WPA3) takes any non-empty password. | `nm_utils_wpa_psk_valid()` in `nm-utils.c`, `nm-setting-wireless-security.c` |
| Device state reasons: 7 no secrets, 8 supplicant disconnected, 53 SSID not found; active connection state reasons: 9 no secrets, 10 login failed. | `libnm-core-public/nm-dbus-interface.h` |
| GNOME Settings asks for a Wi-Fi scan every 15 s while its Wi-Fi panel is shown. | `panels/network/net-device-wifi.c` (`PERIODIC_WIFI_SCAN_TIMEOUT`) |

## Screenshots, clipboard and recording

| Fact | Source |
| --- | --- |
| grim takes a picture of a rectangle of sway's layout (`-g "x,y wxh"`), an output (`-o <name>`) or all outputs, writes it to standard output with `-`, and takes it at the largest scale of the outputs it covers unless `-s` says otherwise; `-t png\|ppm\|jpeg` sets the format. A grim built without libjpeg refuses `-t jpeg` ("jpeg support disabled", seen with grim 1.5.0 of Debian). | `grim -h`; tested with grim 1.5.0 in a headless sway with outputs of scale 1 and 2 |
| slurp prints the selection as `x,y wxh`; it reads boxes to choose from from standard input when that isn't a terminal (one `x,y wxh` per line, a click picks a box); Escape or another button prints "selection cancelled" and exits with a failure. | slurp `main.c` (`isatty(STDIN_FILENO)`, `handle_selection_cancelled`) |
| In `GET_TREE`, a window's `rect` is its geometry with the borders and without the title bar; `visible` says whether it is on screen. | `man 7 sway-ipc`; swayipc-types `Node` |
| `wl-copy` reads standard input, forks and serves the clipboard in the background until something else is copied; without `--type` it guesses the type of the data. | wl-copy(1) |
| `wl-paste --watch <command>` runs the command with the clipboard on its standard input at each change, with `CLIPBOARD_STATE` set (`data`, `nil`, `clear`, `sensitive`), since wl-clipboard 2.2.0; it sets `sensitive` when the copying app offers `x-kde-passwordManagerHint` (as KeePassXC does) since 2.3.0 (2026-03-22). | wl-paste(1) of wl-clipboard 2.3.0; release notes of v2.2.0 and v2.3.0 |
| cliphist's recommended setup is `wl-paste --watch cliphist store`, once per session, for text and images; `-max-items` (default 750) limits the history (since 0.4.0); `list` prints `<id>\t<preview>`, newest first; `decode` reads such a line from standard input and prints the entry; `wipe` removes all. `store` stores nothing for `CLIPBOARD_STATE=sensitive` and removes the last entry for `clear` (since 0.5.0). | cliphist `readme.md`, `cliphist.go`; commit 46c6661 (cliphist honors `CLIPBOARD_STATE`), release v0.5.0 |
| fuzzel `--dmenu`, wofi `--dmenu`, rofi `-dmenu`, tofi and bemenu read a list from standard input and print the line picked; closed without a pick they print nothing or exit with a failure. | their man pages; cliphist `readme.md` (`cliphist list \| fuzzel --dmenu \| cliphist decode \| wl-copy`) |
| swappy opens a file with `-f <file>` (`-` for standard input) and saves to `-o <file>`; it copies with wl-copy. satty takes `--filename` (`-` for standard input), `--output-filename` (with `strftime`-like `%` conversions) and `--copy-command`. | swappy `README.md`; satty `README.md` |
| wf-recorder and wl-screenrec take `-g "x,y wxh"` or `-o <output>`, `-f <file>` (the extension decides the container) and `--audio` (the default input); both stop and finish the file on SIGINT, as on Ctrl+C. wf-recorder asks before it overwrites a file unless `-y`; wl-screenrec needs VA-API encoding, with H.264 by default (HEVC and AV1 by choice), so neither records into WebM (VP8 or VP9) without other options. | wf-recorder(1); wl-screenrec `README.md` |
| `org.freedesktop.Notifications`: `Notify` takes app name, id to replace, icon, summary, body, actions (id and label pairs), hints (`image-path`, `desktop-entry`) and a timeout (0 never expires); `ActionInvoked` and `NotificationClosed` tell what became of it; `GetCapabilities` lists `actions` if the server shows buttons. | Desktop Notifications Specification 1.2 |

## CUPS, cups-pk-helper and Avahi

| Fact | Source |
| --- | --- |
| CUPS listens on `/run/cups/cups.sock` (`cups.socket`); libcups uses its default socket when it can read it, else `localhost`. | `cups.socket` on Arch (CUPS 2.4.20) and Debian 13 (2.4.10); `cups/usersys.c` (`CUPS_DEFAULT_DOMAINSOCKET`), 2.4.10 |
| `SystemGroup` is `root lpadmin` on Debian 13 and `sys root wheel` on Arch; `cups-files.conf` is readable by everyone on Debian, by root and the group `cups` on Arch. `WebInterface` is on in both. | `/etc/cups/cups-files.conf` and `cupsd.conf` in the containers |
| Over the domain socket, cupsd answers an administrative request without credentials with `401` and `WWW-Authenticate: Basic realm="CUPS", PeerCred, Local trc="y"`; sent again with `Authorization: PeerCred <user>`, it checks the socket's peer and lets members of `SystemGroup` change printers, without a password. It needs a `<Location>` that applies to the request. | `scheduler/client.c` (`, PeerCred`), `cups/auth.c` (`cups_local_auth`), 2.4.10; cupsd's log in the containers |
| `CUPS-Add-Modify-Printer` with `ppd-name=everywhere` makes cupsd build the driver in a background thread from the printer's `Get-Printer-Attributes` before it answers; a device URI with `._tcp` in it is resolved through DNS-SD first. | `scheduler/ipp.c` (`add_printer`, `create_local_bg_thread`), 2.4.10 |
| When that fails, CUPS 2.4.10 keeps the new queue; 2.4.20 removes it. | `crates/core/tests/printers_cups.rs` in the containers |
| A queue without a driver reports the model `Local Raw Printer`, whatever `printers.conf` says. `marker-*` attributes and reasons kept in `printers.conf` come back from `CUPS-Get-Printers`. | The containers |
| `cupsd` is installed for root only (mode 0700) on Arch and Void, for everyone on Debian. `ippeveprinter` (Arch's CUPS 2.4.20) aborts without avahi-daemon, so the tests use a printer of their own. | The containers |
| `CUPS-Get-Printers` and `CUPS-Get-Default` answer `client-error-not-found` when there is no printer or no default. | The containers |
| cups-pk-helper's methods answer an error text (empty when it worked): `PrinterAdd(name, uri, ppd, info, location)`, `PrinterDelete`, `PrinterSetDefault`, `PrinterSetEnabled(name, enabled)`, `PrinterSetAcceptJobs(name, enabled, reason)`, `PrinterSetInfo`, `PrinterSetLocation`, `PrinterSetShared`, `JobCancelPurge(id, purge)`. | `src/cups-pk-helper-mechanism.xml` (gitlab.freedesktop.org/cups-pk-helper) |
| Avahi's `ServiceBrowserNew(interface, protocol, type, domain, flags)` creates a browser that reports `ItemNew` and `ItemRemove(interface, protocol, name, type, domain, flags)` right away, so a client subscribes to the signals first (`ServiceBrowserPrepare` of the `Server2` interface avoids the race, but only on newer Avahi); `ResolveService(…, aprotocol, flags)` with `AVAHI_LOOKUP_NO_ADDRESS` (8) skips the address lookup and gives the TXT record; the result flag 8 (`AVAHI_LOOKUP_RESULT_LOCAL`) marks a service announced by this computer. | `avahi-daemon/org.freedesktop.Avahi.Server.xml`, `avahi-common/defs.h` (avahi master, 2026-11-20) |

## Distributions

| Fact | Source |
| --- | --- |
| Debian 13 ships sway 1.10.1, GTK 4.18.6 and rustc 1.85.1; trixie-backports has newer rustc (1.96). These set the minimum versions. | sources.debian.org |
| gtk4-rs is packaged in Debian (0.9.6 in trixie); Relm4 isn't. | sources.debian.org |
| sway in the distributions built on OBS: Fedora 43 and 44 ship 1.11, Fedora Rawhide 1.12, Debian unstable 1.12, Ubuntu 26.04 1.11, openSUSE Tumbleweed 1.12. | Fedora mdapi, sources.debian.org, Launchpad, a Tumbleweed system (2026-11-14) |
