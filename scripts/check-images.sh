#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Checks the container images pinned to a digest: every image of a workflow's `container:` is in ci/images/Dockerfile,
# where Dependabot updates it, and an image:tag carries the same digest in every Dockerfile of ci/ and every workflow.
# So when Dependabot updates one place, the check names the others.
#
# Check level: lint (see scripts/run-checks.sh)
#
# Usage: scripts/check-images.sh

set -euo pipefail

# shellcheck source=scripts/lib.sh
source "$(dirname "$0")/lib.sh"
no_arguments "$@"

cd "$(dirname "$0")/.."

status=0

# FILE IMAGE DIGEST, one line per pinned image.
pins=$(grep -EHo '[a-z0-9./_-]+(:[A-Za-z0-9._-]+)?@sha256:[0-9a-f]{64}' ci/*/*/Dockerfile ci/images/Dockerfile \
  .github/workflows/*.yml | sed 's/^\([^:]*\):\(.*\)@sha256:\(.*\)$/\1 \2 \3/')

# The same image:tag with more than one digest.
while read -r image; do
  echo "error: $image has more than one digest:" >&2
  awk -v image="$image" '$2 == image {print "  " $1 ": " $3}' <<<"$pins" >&2
  status=1
done < <(awk '{print $2, $3}' <<<"$pins" | sort -u | awk '{print $1}' | uniq -d)

# Images of workflows that Dependabot doesn't see.
while read -r file image; do
  if ! awk -v image="$image" '$1 == "ci/images/Dockerfile" && $2 == image {found=1} END {exit !found}' <<<"$pins"; then
    echo "error: $file uses $image, which is not in ci/images/Dockerfile" >&2
    status=1
  fi
done < <(awk '$1 ~ /^\.github\/workflows\// {print $1, $2}' <<<"$pins" | sort -u)

exit "$status"
