#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Takes fresh screenshots of the app (scripts/gui-screenshots.sh, on GTK's Broadway backend: no window opens and no
# compositor starts) and copies those of the released pages, listed in website/src/data/released-pages.json, in light
# and dark into website/src/assets/screenshots/. Part of a release pull request (wiki/Releasing.md).
#
# Usage: scripts/update-website-screenshots.sh
#
# Needs what scripts/gui-screenshots.sh needs (cargo, gtk4-broadwayd) and Node.js to read the page list.

set -euo pipefail

cd "$(dirname "$0")/.."

if ! command -v node >/dev/null; then
  echo "error: needs node" >&2
  exit 1
fi

shots=$(mktemp -d)
trap 'rm -rf "$shots"' EXIT

# The site is in American English: numbers and texts as an en-US user sees them.
LC_ALL=C.UTF-8 LANG=C.UTF-8 LANGUAGE=en_US scripts/gui-screenshots.sh "$shots"

mapfile -t ids < <(node -e '
  const pages = JSON.parse(require("fs").readFileSync(process.argv[1], "utf8"));
  for (const page of pages) console.log(page.id);
' website/src/data/released-pages.json)

target=website/src/assets/screenshots
rm -f "$target"/*.png
for id in "${ids[@]}"; do
  for scheme in light dark; do
    cp "$shots/$id--$scheme.png" "$target/$id--$scheme.png"
  done
done
echo "Screenshots of ${#ids[@]} pages in $target"
