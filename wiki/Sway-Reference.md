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

## PipeWire and PulseAudio

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

## Distributions

| Fact | Source |
| --- | --- |
| Debian 13 ships sway 1.10.1, GTK 4.18.6 and rustc 1.85.1; trixie-backports has newer rustc (1.96). These set the minimum versions. | sources.debian.org |
| gtk4-rs is packaged in Debian (0.9.6 in trixie); Relm4 isn't. | sources.debian.org |
| sway in the distributions built on OBS: Fedora 43 and 44 ship 1.11, Fedora Rawhide 1.12, Debian unstable 1.12, Ubuntu 26.04 1.11, openSUSE Tumbleweed 1.12. | Fedora mdapi, sources.debian.org, Launchpad, a Tumbleweed system (2026-11-14) |
