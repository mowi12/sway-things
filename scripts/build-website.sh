#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Builds the website (website/, Astro) into website/dist. The site reads README.md and the app icon, so it is built
# from the whole repository.
#
# Usage: scripts/build-website.sh
#
# Needs Node.js 22.12 or newer and the website's packages: `npm ci --prefix website`.

set -euo pipefail

cd "$(dirname "$0")/../website"

if [[ ! -x node_modules/.bin/astro ]]; then
  echo "error: needs the website's packages (npm ci --prefix website)" >&2
  exit 1
fi

# Astro collects anonymous usage data unless told not to.
export ASTRO_TELEMETRY_DISABLED=1
node_modules/.bin/astro build
