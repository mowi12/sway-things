# shellcheck shell=bash
# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Helpers for the scripts in this directory. A script sources it right after `set -euo pipefail`:
#
#   # shellcheck source=scripts/lib.sh
#   source "$(dirname "$0")/lib.sh"
#
# The scripts that run inside a container (check-integration.sh, build-packages.sh) are copied there with it.

# Prints the header comment of the running script, the lines after the SPDX header up to the first line that isn't a
# comment, and exits. For -h and --help.
print_help() {
  sed -n '5,/^[^#]/{/^#/s/^# \{0,1\}//p}' "$0"
  exit 0
}

# help_option "$@": prints the help if the first argument is -h or --help. A script with arguments calls it before
# parsing them.
help_option() {
  case "${1-}" in
    -h | --help) print_help ;;
  esac
}

# no_arguments "$@": for a script without arguments, -h and --help print the help, anything else the usage.
no_arguments() {
  help_option "$@"
  if (($#)); then
    echo "usage: $0" >&2
    exit 2
  fi
}

# require TOOL…: fails unless every TOOL is on PATH.
require() {
  local tool
  for tool in "$@"; do
    if ! command -v "$tool" >/dev/null; then
      echo "error: needs $tool" >&2
      exit 1
    fi
  done
}

# require_hint TOOL HINT: fails unless TOOL is on PATH, and says HINT about where to get it.
require_hint() {
  if ! command -v "$1" >/dev/null; then
    echo "error: needs $1 ($2)" >&2
    exit 1
  fi
}

# Prints the container engine to use: the CONTAINER_ENGINE variable if set, else docker or podman, whichever is
# installed. Fails if there is none.
container_engine() {
  local engine=${CONTAINER_ENGINE:-$(command -v docker || command -v podman || true)}
  if [[ -z $engine ]]; then
    echo "error: needs docker or podman" >&2
    exit 1
  fi
  printf '%s\n' "$engine"
}

# repo_files PATHSPEC…: the tracked and new files matching PATHSPEC, but none that git ignores.
repo_files() {
  git ls-files --cached --others --exclude-standard -- "$@"
}
