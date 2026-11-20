#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Runs the checks of a level, each scripts/check-*.sh once, and lists the ones that failed at the end. Every check
# script names its level in a `# Check level:` line of its header:
#
# - lint: fast and offline; builds the code, but runs no tests.
# - host: offline and safe on any machine: the tests (without the ones that need sway), the installation, the packaging
#   files and the website. Never starts sway, a container or a session.
# - network: needs the network (the advisory database of cargo-deny).
# - container: needs docker or podman; runs sway only inside the containers.
#
# LEVEL lint runs the lint checks, host the lint and host checks, all every check (what CI runs). --list prints every
# check with its level.
#
# Usage: scripts/run-checks.sh lint|host|all|--list
#
# Needs what the checks of the level need; each check says so in its header.

set -euo pipefail

usage() {
  echo "usage: $0 lint|host|all|--list" >&2
  exit 2
}

case "${1-}" in
  lint) wanted=(lint) ;;
  host) wanted=(lint host) ;;
  all) wanted=(lint host network container) ;;
  --list) wanted=() ;;
  -h | --help)
    sed -n '5,/^$/s/^# \{0,1\}//p' "$0"
    exit 0
    ;;
  *) usage ;;
esac
[[ $# -le 1 ]] || usage

cd "$(dirname "$0")/.."

# The level of every check script, in the order of the levels.
checks=()
for level in lint host network container; do
  for script in scripts/check-*.sh; do
    found=$(sed -n 's/^# Check level: \([a-z]*\).*/\1/p' "$script")
    if [[ -z $found ]]; then
      echo "error: $script names no check level" >&2
      exit 1
    fi
    if [[ $found == "$level" ]]; then
      checks+=("$level $script")
    fi
  done
done

if [[ ${1-} == --list ]]; then
  printf '%s\n' "${checks[@]}" | column -t
  exit 0
fi

failed=()
for entry in "${checks[@]}"; do
  level=${entry%% *}
  script=${entry#* }
  for allowed in "${wanted[@]}"; do
    if [[ $level == "$allowed" ]]; then
      echo "==> $script"
      "$script" || failed+=("$script")
    fi
  done
done
if ((${#failed[@]})); then
  printf 'failed: %s\n' "${failed[@]}" >&2
  exit 1
fi
