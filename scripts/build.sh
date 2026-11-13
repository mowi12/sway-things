#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Builds what scripts/install.sh installs: the two binaries (cargo), the desktop entry and the AppStream metainfo with
# their translations merged in (scripts/merge-metadata.sh), and the translations of the languages in po/LINGUAS. The
# man pages and completions are committed in data/ and need no build.
#
# The binaries look for the translations in PREFIX/share/locale, so install.sh refuses a different PREFIX.
#
# Usage: scripts/build.sh [--data-only] [-- CARGO_BUILD_ARGUMENTS…]
#   --data-only   build everything but the binaries, e.g. after a packaging macro ran cargo with the same PROFILE
#   PREFIX            where the files will be installed (default /usr/local)
#   PROFILE           the cargo profile (default release)
#   CARGO_TARGET_DIR  cargo's target directory (default target)
#
# Needs cargo (unless --data-only) and msgfmt (GNU gettext).

set -euo pipefail

cd "$(dirname "$0")/.."

usage() {
  echo "usage: scripts/build.sh [--data-only] [-- CARGO_BUILD_ARGUMENTS…]" >&2
  exit 2
}

data_only=false
while [[ $# -gt 0 ]]; do
  case $1 in
    --data-only) data_only=true ;;
    --)
      shift
      break
      ;;
    *) usage ;;
  esac
  shift
done

prefix=${PREFIX:-/usr/local}
profile=${PROFILE:-release}
if [[ $prefix != /* ]]; then
  echo "error: PREFIX must be an absolute path, not $prefix" >&2
  exit 1
fi
case $profile in
  release) profile_dir=release ;;
  dev) profile_dir=debug ;;
  *) profile_dir=$profile ;;
esac
out=${CARGO_TARGET_DIR:-target}/$profile_dir
localedir=$prefix/share/locale

if $data_only; then
  # The binaries were built without this script: they look for the translations where SWAY_THINGS_LOCALEDIR said at
  # build time, else in /usr/share/locale.
  if [[ ${SWAY_THINGS_LOCALEDIR:-/usr/share/locale} != "$localedir" ]]; then
    echo "error: the binaries look for translations in ${SWAY_THINGS_LOCALEDIR:-/usr/share/locale}, not in" \
      "$localedir; build them with SWAY_THINGS_LOCALEDIR=$localedir" >&2
    exit 1
  fi
else
  if ! command -v cargo >/dev/null; then
    echo "error: needs cargo" >&2
    exit 1
  fi
  SWAY_THINGS_LOCALEDIR=$localedir cargo build --locked --profile "$profile" "$@"
fi

for binary in sway-things sway-things-gui; do
  if [[ ! -f $out/$binary ]]; then
    echo "error: $out/$binary is missing; build it with cargo --profile $profile" >&2
    exit 1
  fi
done

if ! command -v msgfmt >/dev/null; then
  echo "error: needs msgfmt (GNU gettext)" >&2
  exit 1
fi

rm -rf "$out/data"
scripts/merge-metadata.sh "$out/data"
# po/LINGUAS: language codes separated by white space, comments start with #.
mapfile -t languages < <(sed 's/#.*//' po/LINGUAS | tr -s '[:space:]' '\n' | sed '/^$/d')
for language in "${languages[@]}"; do
  mkdir -p "$out/data/locale/$language/LC_MESSAGES"
  msgfmt --check -o "$out/data/locale/$language/LC_MESSAGES/sway-things.mo" "po/$language.po"
done
# The prefix the binaries were built for, which install.sh checks.
printf '%s\n' "$prefix" >"$out/data/prefix"
echo "Built for $prefix in $out"
