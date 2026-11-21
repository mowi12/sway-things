#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Writes the man pages (data/man/) and shell completions (data/completions/) from the clap definitions of the command
# line and the app, and removes those no command needs any more. A test of crates/cli-docs fails while they are out of
# date.
#
# Usage: scripts/update-cli-docs.sh
#
# Needs cargo.

set -euo pipefail

# shellcheck source=scripts/lib.sh
source "$(dirname "$0")/lib.sh"
no_arguments "$@"

cd "$(dirname "$0")/.."

cargo run --quiet --locked -p sway-things-cli-docs -- data
