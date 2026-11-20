#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Builds the website and runs Lighthouse on every page, served locally, as a phone and as a desktop
# (scripts/lighthouse.mjs): performance, accessibility, best practices and SEO each must reach their threshold.
#
# Check level: host (see scripts/run-checks.sh)
#
# Usage: scripts/check-website-performance.sh
#
# Needs Node.js 22.12 or newer, the website's packages (`npm ci --prefix website`) and Chrome or Chromium (set
# CHROME_PATH if it isn't found).

set -euo pipefail

# shellcheck source=scripts/lib.sh
source "$(dirname "$0")/lib.sh"
no_arguments "$@"

cd "$(dirname "$0")/.."

scripts/build-website.sh

node scripts/lighthouse.mjs
