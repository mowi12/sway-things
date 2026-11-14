#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Prints the release notes of TAG: the section "## X.Y.Z (…)" of CHANGELOG.md without its heading, the same text the
# changelog has. For a pre-release (vX.Y.Z-rc.N), which only tests the release workflow, a fixed note.
#
# CHANGELOG.md is wrapped at 120 columns, but GitHub shows every line break of a release description. So the wrapped
# lines of each paragraph and list item are joined into one line. Headings, blank lines, the start of a list item (with
# its indentation), block quotes, tables, code blocks and lines after a hard line break stay as they are.
#
# Usage: scripts/release-notes.sh TAG
#
# Needs awk.

set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: scripts/release-notes.sh TAG" >&2
  exit 2
fi
tag=$1

cd "$(dirname "$0")/.."

if [[ $tag =~ ^v[0-9]+\.[0-9]+\.[0-9]+-rc\.[0-9]+$ ]]; then
  echo "A pre-release that tests the release workflow. It is no release; don't install it."
  exit
fi
if [[ ! $tag =~ ^v([0-9]+\.[0-9]+\.[0-9]+)$ ]]; then
  echo "error: $tag is no version tag" >&2
  exit 1
fi
version=${BASH_REMATCH[1]}

notes=$(awk -v heading="## $version (" '
  index($0, heading) == 1 { found = 1; next }
  found && /^## / { exit }
  found { print }
' CHANGELOG.md)
# Without the empty lines around the section.
notes=$(printf '%s\n' "$notes" | sed -e '/./,$!d' | tac | sed -e '/./,$!d' | tac)
# One line per paragraph and list item.
notes=$(printf '%s\n' "$notes" | awk '
  function flush() {
    if (pending != "") print pending
    pending = ""
  }
  # A fenced code block stays as it is.
  fence != "" {
    print
    if ($0 ~ "^[ \t]*" fence) fence = ""
    next
  }
  /^[ \t]*(```|~~~)/ {
    flush()
    match($0, /(```|~~~)/)
    fence = substr($0, RSTART, RLENGTH)
    print
    next
  }
  # Lines that never continue another line or get continued.
  /^[ \t]*$/ || /^#/ || /^[ \t]*>/ || /^[ \t]*\|/ {
    flush()
    print
    next
  }
  # The start of a list item, which a paragraph or an item before it does not continue.
  /^[ \t]*([-*+]|[0-9]+[.)])([ \t]|$)/ {
    flush()
    pending = $0
    next
  }
  # A continuation line, unless the line before ends with a hard line break (two spaces or a backslash).
  pending != "" && pending !~ /(  |\\)$/ {
    sub(/^[ \t]+/, "")
    pending = pending " " $0
    next
  }
  {
    flush()
    pending = $0
  }
  END { flush() }
')
if [[ -z $notes ]]; then
  echo "error: CHANGELOG.md has no section \"## $version (…)\"" >&2
  exit 1
fi
printf '%s\n' "$notes"
