#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Validates the packaging metadata as it is installed: merges the translations into the desktop entry and the AppStream
# metainfo (scripts/merge-metadata.sh), then checks the desktop entry with desktop-file-validate and the metainfo with
# appstreamcli.
#
# Check level: lint (see scripts/run-checks.sh)
#
# Usage: scripts/check-metadata.sh
#
# Needs msgfmt (GNU gettext), desktop-file-validate (desktop-file-utils) and appstreamcli (AppStream).

set -euo pipefail

# shellcheck source=scripts/lib.sh
source "$(dirname "$0")/lib.sh"
no_arguments "$@"

cd "$(dirname "$0")/.."

require msgfmt desktop-file-validate appstreamcli

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

scripts/merge-metadata.sh "$work"

status=0
for desktop in "$work"/*.desktop; do
  desktop-file-validate "$desktop" || status=1
done
for metainfo in "$work"/*.metainfo.xml; do
  appstreamcli validate --no-net --explain "$metainfo" || status=1
done
exit "$status"
