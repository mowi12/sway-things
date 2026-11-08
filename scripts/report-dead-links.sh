#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Reports dead external links from a `scripts/check-links.sh --external` report: opens a documentation issue laid out
# like the Docs form, or comments on the one that's still open, so the same links aren't reported in a new issue every
# week.
#
# Usage: scripts/report-dead-links.sh REPORT
#
# Needs gh (logged in, or GH_TOKEN set). gh targets the repository of the current checkout, or GH_REPO=owner/name if set.

set -euo pipefail

if (($# != 1)); then
  echo "usage: $0 REPORT" >&2
  exit 2
fi

readonly title="[Docs]: Dead external links"

# lychee's report: a summary table, then "### Errors in <file>" with one bullet per dead link. Keep the files and
# bullets, as bold lines so they don't clash with the form's headings.
errors=$(sed -n '/^## Errors per input/,$p' "$1" | sed -e '1d' -e 's/^### Errors in \(.*\)/**\1**/' | sed '/./,$!d')
files=$(sed -n 's/^### Errors in //p' "$1" | paste -sd , - | sed 's/,/, /g')
if [[ -z $errors || -z $files ]]; then
  echo "error: $1 lists no dead links" >&2
  exit 1
fi

existing=$(gh issue list --state open --label kind:docs --limit 100 --json number,title \
  --jq ".[] | select(.title == \"$title\") | .number" | head -n 1)

if [[ -n $existing ]]; then
  gh issue comment "$existing" --body-file - <<BODY
The weekly link check still finds dead links:

$errors
BODY
  exit
fi

gh issue create --title "$title" --label kind:docs --body-file - <<BODY
### Is there an existing issue for this?

- [X] I have searched the existing issues

### Which documentation is affected?

$files

### What is unclear, wrong or missing?

The weekly link check found external links that don't work:

$errors

### What should be changed?

Update or remove the dead links. If a link works in a browser, exclude it in \`lychee.toml\`.

### Anything else?

Opened by the scheduled link check workflow.
BODY
