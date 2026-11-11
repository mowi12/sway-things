#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Runs the GUI tests (crates/gui/tests/gui.rs) on a private display and keeps a PNG of the window with every page, in
# light and dark. The display is GTK's Broadway backend, so no window opens on the desktop and no compositor is
# started. CI runs the same tests against a headless sway in scripts/check-integration.sh.
#
# Usage: scripts/gui-screenshots.sh [DIR]   (default: target/screenshots)
#
# Needs cargo and gtk4-broadwayd (part of GTK 4).

set -euo pipefail

cd "$(dirname "$0")/.."

directory=$(realpath -m "${1:-target/screenshots}")

if ! command -v gtk4-broadwayd >/dev/null; then
  echo "error: needs gtk4-broadwayd" >&2
  exit 1
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

# A display number that is free: Broadway listens on port 8080 + the number.
display=""
for number in $(seq 40 60); do
  if [[ ! -e $XDG_RUNTIME_DIR/broadway$number.socket ]]; then
    display=:$number
    break
  fi
done
if [[ -z $display ]]; then
  echo "error: no free Broadway display" >&2
  exit 1
fi

gtk4-broadwayd "$display" >"$runtime/broadway.log" 2>&1 &
pid=$!
sleep 1
if ! kill -0 "$pid" 2>/dev/null; then
  echo "error: gtk4-broadwayd didn't start:" >&2
  cat "$runtime/broadway.log" >&2
  exit 1
fi

rm -rf "$directory"
GDK_BACKEND=broadway BROADWAY_DISPLAY=$display GSK_RENDERER=cairo SWAY_THINGS_SCREENSHOTS=$directory \
  cargo test --locked --package sway-things-gui --test gui -- --ignored
echo "Screenshots in $directory"
