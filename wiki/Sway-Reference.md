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
| waybar's `include`: the first definition of a key wins. | `man 5 waybar` (0.15) |
| mako supports `include=` with an absolute or `~/` path. | mako `doc/mako.5.scd` |
| swaync has no include, but takes its config and style with `-c` and `-s`. | `swaync --help` (0.12.6) |

## Distributions

| Fact | Source |
| --- | --- |
| Debian 13 ships sway 1.10.1, GTK 4.18.6 and rustc 1.85.1; trixie-backports has newer rustc (1.96). These set the minimum versions. | sources.debian.org |
| gtk4-rs is packaged in Debian (0.9.6 in trixie); Relm4 isn't. | sources.debian.org |
| sway in the distributions built on OBS: Fedora 43 and 44 ship 1.11, Fedora Rawhide 1.12, Debian unstable 1.12, Ubuntu 26.04 1.11, openSUSE Tumbleweed 1.12. | Fedora mdapi, sources.debian.org, Launchpad, a Tumbleweed system (2026-11-14) |
