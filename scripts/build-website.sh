#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Builds the website (website/, Astro) into website/dist. The site reads README.md and the app icon, so it is built
# from the whole repository. scripts/install-from-obs.sh goes next to the pages as /install and /install.sh, so that
# `curl -fsSL https://sway-things.org/install | sh` runs the script on main.
#
# The imprint is fetched from IMPRINT_URL while building (website/src/lib/imprint.mjs); without it, the imprint page
# says that this copy has none, and with IMPRINT_REQUIRED set the build fails instead.
#
# Usage: scripts/build-website.sh
#
# Needs Node.js 22.12 or newer and the website's packages: `npm ci --prefix website`.

set -euo pipefail

# shellcheck source=scripts/lib.sh
source "$(dirname "$0")/lib.sh"
no_arguments "$@"

cd "$(dirname "$0")/../website"

if [[ ! -x node_modules/.bin/astro ]]; then
  echo "error: needs the website's packages (npm ci --prefix website)" >&2
  exit 1
fi

# Astro collects anonymous usage data unless told not to.
export ASTRO_TELEMETRY_DISABLED=1
node_modules/.bin/astro build

for name in install install.sh; do
  install -m 644 ../scripts/install-from-obs.sh "dist/$name"
done
