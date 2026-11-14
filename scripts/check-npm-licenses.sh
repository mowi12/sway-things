#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Checks the licenses of the npm packages of the website (website/) and of the check tools (tools/) with
# license-checker-rseidelsohn, against the licenses deny.toml allows for the Rust crates plus the permissive ones only npm
# packages use. cargo-deny doesn't read npm packages. A few build-only packages are exempt below, each with its reason;
# none of them ends up in the published site.
#
# Usage: scripts/check-npm-licenses.sh
#
# Needs the installed packages (`npm ci --prefix tools` and `npm ci --prefix website`); license-checker-rseidelsohn is
# one of the check tools.

set -euo pipefail

cd "$(dirname "$0")/.."

checker=tools/node_modules/.bin/license-checker-rseidelsohn
if [[ ! -x $checker ]]; then
  echo "error: needs license-checker-rseidelsohn (npm ci --prefix tools)" >&2
  exit 1
fi
for directory in tools website; do
  if [[ ! -d $directory/node_modules ]]; then
    echo "error: needs the packages of $directory/ (npm ci --prefix $directory)" >&2
    exit 1
  fi
done

# The allow list of deny.toml, so both ecosystems follow one policy.
mapfile -t allowed < <(sed -n '/^allow = \[/,/^\]/p' deny.toml | grep -o '"[^"]*"' | tr -d '"')
if ((${#allowed[@]} == 0)); then
  echo "error: found no allow list in deny.toml" >&2
  exit 1
fi
# Permissive licenses that only npm packages use, and the license of the fonts the site serves: a font is a separate
# work next to the pages, not linked into code.
allowed+=("BlueOak-1.0.0" "PSF-2.0" "Python-2.0" "OFL-1.1")

exempt=(
  # Lighthouse's error reporting, off unless --enable-error-reporting; only scripts/lighthouse.mjs runs Lighthouse.
  "sentry"
  # The SPDX exception list (CC-BY-3.0) that license-checker-rseidelsohn reads; data of this check tool.
  "spdx-exceptions"
)

allow_list=$(IFS=';' && echo "${allowed[*]}")
exempt_list=$(IFS=';' && echo "${exempt[*]}")
status=0
for directory in tools website; do
  echo "==> $directory"
  "$checker" --start "$directory" --excludePrivatePackages --summary \
    --onlyAllow "$allow_list" --excludePackages "$exempt_list" || status=1
done
exit "$status"
