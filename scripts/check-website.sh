#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Builds the website and checks the result: valid HTML (html-validate, configured in website/.htmlvalidate.json) and
# working relative links and anchors (lychee, offline). The build itself fails when README.md no longer has the
# sections the site reads.
#
# Usage: scripts/check-website.sh
#
# Needs Node.js 22.12 or newer, the website's packages (`npm ci --prefix website`), html-validate (`npm ci --prefix
# tools` installs the pinned version, which this script finds on its own) and lychee (https://lychee.cli.rs/).

set -euo pipefail

cd "$(dirname "$0")/.."

PATH="$PWD/tools/node_modules/.bin:$PATH"
for tool in html-validate lychee; do
  if ! command -v "$tool" >/dev/null; then
    echo "error: needs $tool (html-validate: npm ci --prefix tools)" >&2
    exit 1
  fi
done

scripts/build-website.sh

mapfile -t pages < <(find website/dist -name '*.html' | sort)
html-validate --config website/.htmlvalidate.json "${pages[@]}"
lychee --offline --root-dir "$PWD/website/dist" --index-files index.html "${pages[@]}"
