#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Smoke-tests a headless sway: validates the sample config in ci/integration/ with `sway --validate`, starts sway with
# it and talks to it over IPC. SwayFX gets the SwayFX sample config. The real integration tests will plug in here.
#
# Without options, tests the sway (or SwayFX) on PATH, in a private session that doesn't touch a running desktop. With
# --container NAME, builds ci/containers/NAME/Dockerfile and runs this script inside it; NAME=all runs every container.
# The host's DRM render nodes are passed into the containers, because SwayFX needs one even to validate a config (CI
# loads the vgem module for a virtual one).
#
# Usage: scripts/check-integration.sh [--container NAME|all]
#
# Needs sway and swaymsg; for --container, docker or podman (CONTAINER_ENGINE overrides the choice).

set -euo pipefail

usage() {
  echo "usage: $0 [--container NAME|all]" >&2
  exit 2
}

container=""
case "${1-}" in
  "") ;;
  --container) container=${2:-$(usage)} ;;
  *) usage ;;
esac

cd "$(dirname "$0")/.."

if [[ -n $container ]]; then
  engine=${CONTAINER_ENGINE:-$(command -v docker || command -v podman || true)}
  if [[ -z $engine ]]; then
    echo "error: needs docker or podman" >&2
    exit 1
  fi
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
    tar -c "ci/containers/$name/Dockerfile" ci/integration scripts/check-integration.sh | tar -x -C "$context"
    "$engine" build --quiet --tag "sway-things-ci:$name" --file "$context/ci/containers/$name/Dockerfile" "$context" \
      >/dev/null
    rm -rf "$context"
    "$engine" run --rm "${devices[@]}" "sway-things-ci:$name"
  done
  exit
fi

for tool in sway swaymsg; do
  if ! command -v "$tool" >/dev/null; then
    echo "error: needs $tool" >&2
    exit 1
  fi
done

sway --version
config=ci/integration/sway.conf
if sway --version | grep -qi swayfx; then
  config=ci/integration/swayfx.conf
fi

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
# sway quits before it answers, so swaymsg reports an error; the exit status of sway itself counts.
swaymsg --socket "$socket" exit >/dev/null 2>&1 || true
wait "$pid"
pid=""
echo "The smoke test passed."
