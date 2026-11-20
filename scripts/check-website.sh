#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Runs the website's unit tests (website/src/lib/*.test.mjs, with node --test), builds the website and checks the
# result: valid HTML (html-validate, configured in website/.htmlvalidate.json), working relative links and anchors
# (lychee, offline), and the install script served next to the pages. The build itself fails when README.md no longer
# has the sections the site reads.
#
# Check level: host (see scripts/run-checks.sh)
#
# Usage: scripts/check-website.sh
#
# Needs Node.js 22.12 or newer, the website's packages (`npm ci --prefix website`), html-validate (`npm ci --prefix
# tools` installs the pinned version, which this script finds on its own) and lychee (https://lychee.cli.rs/).

set -euo pipefail

# shellcheck source=scripts/lib.sh
source "$(dirname "$0")/lib.sh"
no_arguments "$@"

cd "$(dirname "$0")/.."

PATH="$PWD/tools/node_modules/.bin:$PATH"
require_hint html-validate "npm ci --prefix tools"
require lychee

node --test website/src/lib/*.test.mjs

scripts/build-website.sh

for name in install install.sh; do
  if ! cmp -s scripts/install-from-obs.sh "website/dist/$name"; then
    echo "error: website/dist/$name isn't scripts/install-from-obs.sh" >&2
    exit 1
  fi
done

mapfile -t pages < <(find website/dist -name '*.html' | sort)
html-validate --config website/.htmlvalidate.json "${pages[@]}"
lychee --offline --root-dir "$PWD/website/dist" --index-files index.html "${pages[@]}"
