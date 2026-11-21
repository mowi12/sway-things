#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Makes the openSUSE Build Service run the source services of the package sway-things in home:mowi12:sway-things,
# which fetch the files of the latest published GitHub release. The release workflow runs it when a release is
# published. OBS_TOKEN is a token bound to that package that can do nothing but run its services
# (osc token --create --operation runservice home:mowi12:sway-things sway-things); it goes to curl on standard input,
# never on the command line.
#
# Usage: OBS_TOKEN=… scripts/trigger-obs.sh
#
# Needs curl.

set -euo pipefail

# shellcheck source=scripts/lib.sh
source "$(dirname "$0")/lib.sh"
no_arguments "$@"

if [[ -z ${OBS_TOKEN:-} ]]; then
  echo "error: needs the token in OBS_TOKEN" >&2
  exit 1
fi
require curl

printf 'Authorization: Token %s\n' "$OBS_TOKEN" |
  curl --silent --show-error --fail-with-body --retry 3 --header @- --request POST \
    https://build.opensuse.org/trigger/runservice
echo
echo "OBS runs the source services of home:mowi12:sway-things/sway-things"
