#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Validates the packaging metadata: desktop entries with desktop-file-validate and AppStream metainfo with
# appstreamcli. Passes while there are no such files.
#
# Usage: scripts/check-metadata.sh
#
# Needs desktop-file-validate (desktop-file-utils) and appstreamcli (AppStream), each only once files of its kind exist.

set -euo pipefail

cd "$(dirname "$0")/.."

require() {
  if ! command -v "$1" >/dev/null; then
    echo "error: needs $1" >&2
    exit 1
  fi
}

# Tracked and new files, but none that git ignores.
mapfile -t desktop < <(git ls-files --cached --others --exclude-standard -- '*.desktop')
mapfile -t metainfo < <(git ls-files --cached --others --exclude-standard -- '*.metainfo.xml')

if ((${#desktop[@]} + ${#metainfo[@]} == 0)); then
  echo "No desktop entries or AppStream metainfo files yet."
  exit 0
fi

status=0
if ((${#desktop[@]})); then
  require desktop-file-validate
  desktop-file-validate "${desktop[@]}" || status=1
fi
if ((${#metainfo[@]})); then
  require appstreamcli
  appstreamcli validate --no-net "${metainfo[@]}" || status=1
fi
exit "$status"
