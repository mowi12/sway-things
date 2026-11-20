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
| `kanshictl reload` has kanshi read its config again; `exec` lines of a profile run (through a shell) when it was applied. | `man 1 kanshictl`, `man 5 kanshi` of kanshi 1.9.0 |
| sway's `workspace N output X` adds to the list of outputs of workspace N and never clears it; a new workspace goes to the first connected output of the list. A reload of sway clears the lists. | sway `commands/workspace.c`, `tree/workspace.c` (`workspace_get_initial_output`) |
| `workspace N output A B C` lists several outputs: the first one available is used, and a workspace placed on an output further down moves to a higher one once that one is there again. | `man 5 sway` (`workspace <name> output <outputs...>`), 1.12 |
| `wl-mirror --fullscreen-output O --scaling fit\|cover\|exact --title T <source>` shows `<source>` fullscreen on `O`. | `wl-mirror --help` |
| waybar's `include`: the first definition of a key wins. | `man 5 waybar` (0.15) |
| mako supports `include=` with an absolute or `~/` path. | mako `doc/mako.5.scd` |
| swaync has no include, but takes its config and style with `-c` and `-s`. | `swaync --help` (0.12.6) |

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
| WPA passwords take 8 to 63 bytes or 64 hexadecimal digits; SAE (WPA3) takes any non-empty password. | `nm_utils_wpa_psk_valid()` in `nm-utils.c`, `nm-setting-wireless-security.c` |
| Device state reasons: 7 no secrets, 8 supplicant disconnected, 53 SSID not found; active connection state reasons: 9 no secrets, 10 login failed. | `libnm-core-public/nm-dbus-interface.h` |
| GNOME Settings asks for a Wi-Fi scan every 15 s while its Wi-Fi panel is shown. | `panels/network/net-device-wifi.c` (`PERIODIC_WIFI_SCAN_TIMEOUT`) |

## Distributions

| Fact | Source |
| --- | --- |
| Debian 13 ships sway 1.10.1, GTK 4.18.6 and rustc 1.85.1; trixie-backports has newer rustc (1.96). These set the minimum versions. | sources.debian.org |
| gtk4-rs is packaged in Debian (0.9.6 in trixie); Relm4 isn't. | sources.debian.org |
| sway in the distributions built on OBS: Fedora 43 and 44 ship 1.11, Fedora Rawhide 1.12, Debian unstable 1.12, Ubuntu 26.04 1.11, openSUSE Tumbleweed 1.12. | Fedora mdapi, sources.debian.org, Launchpad, a Tumbleweed system (2026-11-14) |
