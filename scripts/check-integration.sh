#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Tests a headless sway: validates the sample config in ci/integration/ with `sway --validate`, starts sway with it and
# talks to it over IPC. SwayFX gets the SwayFX sample config. The fixtures that crates/core/tests/*_fixtures.rs generate
# from the command builders plug in here: their .conf files are included from the sample config, every command in a
# *-ipc.txt file must succeed, and the escaping fixtures check that hostile strings reach sway unchanged, from the
# config and over IPC. Then the tests that need a real sway run against it: the IPC client (crates/core/tests/ipc.rs),
# the write path of the generators (crates/core/tests/generate_sway.rs), the config parser, which must find the modes of
# ci/integration/parser.conf that sway reports (crates/core/tests/sway_config_sway.rs), and a setting that is changed
# end to end and must survive a reload (crates/core/tests/apply_sway.rs), and the `sway-things` binary changing settings
# (crates/cli/tests/cli_sway.rs) and placing the workspaces while the user's own kanshi profile sets up the displays, in
# a headless sway of its own with two displays and kanshi, not on SwayFX (crates/cli/tests/kanshi_sway.rs). The BlueZ
# and NetworkManager backends run against python-dbusmock on a private bus (crates/core/tests/bluetooth_bluez.rs,
# crates/core/tests/network_nm.rs), the notification daemons against stand-ins on a private bus
# (crates/core/tests/notifications_bus.rs), systemd-timedated and geoclue against stand-ins on a private bus
# (crates/core/tests/region_timedate.rs, crates/core/tests/region_geoclue.rs), systemd-localed against one
# (crates/core/tests/region_localed.rs), and the sound backend against a private PipeWire with pipewire-pulse and
# virtual devices (crates/core/tests/sound_pulse.rs), and the printers backend against the distribution's cupsd run as
# the user with its own socket, a fake driverless printer on the loopback address, and Avahi and cups-pk-helper on a
# private bus (crates/core/tests/printers_cups.rs), and the scanners backend against the distribution's scanimage with
# SANE's test backend and Avahi on a private bus (crates/core/tests/scanners_sane.rs). The files generated from the
# sample settings (ci/integration/generated/) must pass `sway --validate` too, and the sample configs include them.
# Last, the GUI tests (crates/gui/tests/gui/: contrast in light and dark, keyboard navigation) open their windows on
# the headless sway (not on SwayFX, see below).
#
# Without options, runs every container: --container NAME builds ci/containers/NAME/Dockerfile and runs this script
# inside it, NAME=all (the default) every one. --host tests the sway (or SwayFX) on PATH instead, in a private session;
# the containers run it so, and it is never the default, because it starts sway on the machine it runs on.
# The host's DRM render nodes are passed into the containers, because SwayFX needs one even to validate a config (CI
# loads the vgem module for a virtual one). The images hold a Rust toolchain and build the tests that need sway with
# --build when the image is built, so the tests are built on the system they run on.
#
# Check level: container (see scripts/run-checks.sh)
#
# Usage: scripts/check-integration.sh [--container NAME|all | --host | --build]
#
# Needs sway, swaymsg, cargo, dbus-daemon, python-dbusmock, pipewire, wireplumber, pipewire-pulse, pactl, pacat, cupsd
# with its ipp backend and filters, and scanimage with SANE's test backend; for --container, docker or podman
# (CONTAINER_ENGINE overrides the choice).

set -euo pipefail

# shellcheck source=scripts/lib.sh
source "$(dirname "$0")/lib.sh"
help_option "$@"

usage() {
  echo "usage: $0 [--container NAME|all | --host | --build]" >&2
  exit 2
}

# The tests that need a running sway, so a plain `cargo test` skips them.
sway_tests=(cargo test --locked --package sway-things-core --test ipc --test generate_sway --test sway_config_sway
  --test apply_sway)
cli_tests=(cargo test --locked --package sway-things-cli --test cli_sway --test kanshi_sway)
# The tests that need stand-ins for services, but no sway: dbus-daemon and python-dbusmock (BlueZ, NetworkManager,
# Avahi and cups-pk-helper), dbus-daemon (the notification daemons), PipeWire, WirePlumber and pipewire-pulse with pactl
# and pacat (the sound server), cupsd (CUPS) and scanimage (SANE).
dbus_tests=(cargo test --locked --package sway-things-core --features test-support --test bluetooth_bluez
  --test network_nm --test sound_pulse --test notifications_dbus --test printers_cups
  --test scanners_sane --test notifications_bus --test region_timedate
  --test region_geoclue --test region_localed)
# The tests that need a display.
gui_tests=(cargo test --locked --package sway-things-gui --test gui --test gui_pseudo)

container=all
build=false
case "${1-}" in
  "") ;;
  --container)
    [[ -n ${2-} ]] || usage
    container=$2
    shift
    ;;
  --host) container="" ;;
  --build) build=true ;;
  *) usage ;;
esac
[[ $# -le 1 ]] || usage

cd "$(dirname "$0")/.."

if $build; then
  "${sway_tests[@]}" --no-run
  "${cli_tests[@]}" --no-run
  "${dbus_tests[@]}" --no-run
  "${gui_tests[@]}" --no-run
  exit
fi

if [[ -n $container ]]; then
  engine=$(container_engine)
  if [[ $container == all ]]; then
    names=(ci/containers/*/)
    names=("${names[@]#ci/containers/}")
    names=("${names[@]%/}")
  elif [[ -f ci/containers/$container/Dockerfile ]]; then
    names=("$container")
  else
    echo "error: unknown container: $container" >&2
    exit 1
  fi
  devices=()
  for node in /dev/dri/renderD*; do
    if [[ -e $node ]]; then
      devices+=(--device "$node" --group-add "$(stat -c %g "$node")")
    fi
  done
  for name in "${names[@]}"; do
    echo "==> $name"
    # Only the files the image needs go into the build context.
    context=$(mktemp -d)
    tar -c "ci/containers/$name/Dockerfile" ci/integration scripts/check-integration.sh scripts/lib.sh Cargo.toml \
      Cargo.lock crates po/sway-things.pot | tar -x -C "$context"
    "$engine" build --quiet --tag "sway-things-ci:$name" --file "$context/ci/containers/$name/Dockerfile" "$context" \
      >/dev/null
    rm -rf "$context"
    "$engine" run --rm "${devices[@]}" "sway-things-ci:$name"
  done
  exit
fi

require sway swaymsg cargo

sway --version
# An absolute path: sway 1.10 resolves a relative include against the directory of a relative config path twice and
# silently skips the file.
config=$PWD/ci/integration/sway.conf
swayfx=false
if sway --version | grep -qi swayfx; then
  config=$PWD/ci/integration/swayfx.conf
  swayfx=true
fi

# Whether a fixture applies to the running compositor: fixtures marked "# Needs SwayFX." only run on SwayFX.
applies() {
  $swayfx || ! grep -q '^# Needs SwayFX\.' "$1"
}

runtime=$(mktemp -d)
pid=""
cleanup() {
  if [[ -n $pid ]]; then
    kill "$pid" 2>/dev/null || true
    wait "$pid" 2>/dev/null || true
  fi
  rm -rf "$runtime"
}
trap cleanup EXIT

# A private session: own runtime directory, headless backend, no input devices, nothing from a running desktop.
unset WAYLAND_DISPLAY DISPLAY SWAYSOCK I3SOCK
export XDG_RUNTIME_DIR=$runtime WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1
export WLR_RENDERER=${WLR_RENDERER:-pixman}

echo "Validating $config"
sway --validate --config "$config"
# sway ignores errors in included files (cmd_include: "We don't care if the included config(s) fails to load"), and
# `sway --validate` only reports the main file. So every generated fixture is validated as a main config of its own.
for fixture in ci/integration/*.conf; do
  if grep -q '^# Generated by crates/core/tests/' "$fixture" && applies "$fixture"; then
    echo "Validating $fixture"
    sway --validate --config "$PWD/$fixture"
  fi
done

# The files the generators make from the sample settings: those for sway everywhere, those for SwayFX only there.
generated=(ci/integration/generated/sway/*.conf)
if $swayfx; then
  generated+=(ci/integration/generated/swayfx/*.conf)
fi
for file in "${generated[@]}"; do
  echo "Validating $file"
  sway --validate --config "$PWD/$file"
done

# `sway --validate` leaves its Wayland sockets behind; without them the live sway's socket is the only one.
rm -f "$runtime"/wayland-*
echo "Starting sway headless"
sway --config "$config" >"$runtime/sway.log" 2>&1 &
pid=$!

socket=""
for _ in $(seq 100); do
  socket=$(compgen -G "$runtime/sway-ipc.*.sock" | head -n 1 || true)
  if [[ -n $socket ]] || ! kill -0 "$pid" 2>/dev/null; then
    break
  fi
  sleep 0.1
done
if [[ -z $socket ]]; then
  echo "error: sway didn't start:" >&2
  cat "$runtime/sway.log" >&2
  exit 1
fi

swaymsg --socket "$socket" --type get_version

# sway's Wayland socket, for the GUI tests. Taken now: the tests that run `sway --validate` add sockets of their own.
display=$(compgen -G "$runtime/wayland-[0-9]" | head -n 1 || true)
if [[ -z $display ]]; then
  echo "error: sway has no Wayland socket in $runtime" >&2
  exit 1
fi
display=${display##*/}

# Escaping round trip: escaping.conf set one bar ID per hostile name, the IPC commands set one seat name per hostile
# name, and every name must come back unchanged.
echo "Looking up the bar IDs from ci/integration/escaping-bars.txt"
while IFS= read -r id; do
  if [[ -n $id && $id != "#"* ]]; then
    if ! swaymsg --socket "$socket" --type get_bar_config -- "$id" >/dev/null; then
      echo "error: no bar with the ID: $id" >&2
      echo "sway's bar IDs:" >&2
      swaymsg --socket "$socket" --type get_bar_config >&2
      echo "sway's log:" >&2
      cat "$runtime/sway.log" >&2
      exit 1
    fi
  fi
done <ci/integration/escaping-bars.txt

for commands in ci/integration/*-ipc.txt; do
  if ! applies "$commands"; then
    continue
  fi
  echo "Sending the IPC commands from $commands"
  while IFS= read -r command; do
    if [[ -n $command && $command != "#"* ]]; then
      if ! swaymsg --socket "$socket" -- "$command" >/dev/null; then
        echo "error: command failed: $command" >&2
        exit 1
      fi
    fi
  done <"$commands"
done

echo "Comparing the seat names with ci/integration/escaping-seats.txt"
expected=$(grep -v -e '^#' -e '^$' ci/integration/escaping-seats.txt | LC_ALL=C sort)
actual=$(swaymsg --socket "$socket" --type get_seats --pretty | while IFS= read -r line; do
  if [[ $line == "Seat: "* ]]; then
    printf '%s\n' "${line#Seat: }"
  fi
done | LC_ALL=C sort)
if [[ $actual != "$expected" ]]; then
  echo "error: the seat names differ" >&2
  printf 'expected:\n%s\nactual:\n%s\n' "$expected" "$actual" >&2
  exit 1
fi

echo "Running the tests that need sway"
SWAYSOCK=$socket SWAY_THINGS_SWAY_VERSION=$(sway --version) "${sway_tests[@]}" -- --ignored
SWAYSOCK=$socket "${cli_tests[@]}" -- --ignored
"${dbus_tests[@]}" -- --ignored

# GTK draws with the CPU (cairo), so no GPU is needed. SwayFX gets no frames drawn in a container: its renderer needs
# a render node, and Mesa's software rendering can only allocate buffers on a primary node, so no window would ever
# paint. The tests check the app, not the compositor, so sway covers them.
if $swayfx; then
  echo "Skipping the GUI tests: SwayFX can't draw frames here"
else
  echo "Running the GUI tests on $display"
  if ! WAYLAND_DISPLAY=$display GDK_BACKEND=wayland GSK_RENDERER=cairo "${gui_tests[@]}" -- --ignored; then
    echo "sway's log:" >&2
    cat "$runtime/sway.log" >&2
    exit 1
  fi
fi

# sway quits before it answers, so swaymsg reports an error; the exit status of sway itself counts.
swaymsg --socket "$socket" exit >/dev/null 2>&1 || true
wait "$pid"
pid=""
echo "The integration test passed."
