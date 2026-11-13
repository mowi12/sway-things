#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Extracts the translatable strings, those marked with msg!("…"), from the Rust sources into the gettext template
# po/sway-things.pot. Leaves the template alone when only its creation date would change.
#
# Usage: scripts/update-pot.sh [OUTPUT]   (OUTPUT defaults to po/sway-things.pot)
#
# Needs xgettext 0.24 or newer (GNU gettext), the first version that reads Rust.

set -euo pipefail

cd "$(dirname "$0")/.."

output=${1:-po/sway-things.pot}

if ! command -v xgettext >/dev/null; then
  echo "error: needs xgettext 0.24 or newer (GNU gettext)" >&2
  exit 1
fi
version=$(xgettext --version | head -n 1 | awk '{print $NF}')
if [[ $(printf '%s\n' 0.24 "$version" | sort -V | head -n 1) != 0.24 ]]; then
  echo "error: needs xgettext 0.24 or newer (GNU gettext) to read Rust, found $version" >&2
  exit 1
fi

# The sources of every crate, in a stable order. No git: it refuses a checkout another user owns (as in CI's
# container), and a failure inside the process substitution wouldn't stop the script.
mapfile -t sources < <(find crates/*/src -name '*.rs' -type f | LC_ALL=C sort)
if [[ ${#sources[@]} -eq 0 ]]; then
  echo "error: no Rust sources found under crates/*/src" >&2
  exit 1
fi

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# Only msg!("…") marks a string; --keyword= drops the default keywords (gettext, …). Locations name the file without
# the line, so the template only changes when the strings do.
xgettext --language=Rust --from-code=UTF-8 --keyword= --keyword='msg!' --add-comments=TRANSLATORS: \
  --add-location=file --package-name=sway-things --copyright-holder='Sway Things contributors' \
  --msgid-bugs-address=https://github.com/mowi12/sway-things/issues --output="$work/new.pot" "${sources[@]}"

without_date() {
  grep -v '^"POT-Creation-Date: ' "$1"
}

if [[ -f $output ]] && cmp -s <(without_date "$output") <(without_date "$work/new.pot"); then
  exit 0
fi
mkdir -p "$(dirname "$output")"
mv "$work/new.pot" "$output"
