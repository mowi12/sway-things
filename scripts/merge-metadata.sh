#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Merges the translations of the languages in po/LINGUAS into the desktop entry and the AppStream metainfo, from their
# templates data/*.desktop.in and data/*.metainfo.xml.in. A helper of scripts/build.sh and scripts/check-metadata.sh.
#
# Usage: scripts/merge-metadata.sh OUT_DIR
#
# Needs msgfmt (GNU gettext) with the ITS rules for AppStream metainfo, its own or those of the appstream package.

set -euo pipefail

# shellcheck source=scripts/lib.sh
source "$(dirname "$0")/lib.sh"
help_option "$@"

cd "$(dirname "$0")/.."

if [[ $# -ne 1 ]]; then
  echo "usage: $0 OUT_DIR" >&2
  exit 2
fi
out=$1

require_hint msgfmt "GNU gettext"

mkdir -p "$out"
for template in data/*.desktop.in; do
  msgfmt --desktop --template="$template" -d po -o "$out/$(basename "$template" .in)"
done
for template in data/*.metainfo.xml.in; do
  msgfmt --xml --template="$template" -d po -o "$out/$(basename "$template" .in)"
done
