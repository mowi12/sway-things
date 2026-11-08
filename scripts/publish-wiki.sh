#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Publishes wiki/ to the GitHub wiki: replaces the content of WIKI_CHECKOUT (a clone of the wiki repository) with
# wiki/, rewrites links for the wiki, then commits and pushes if anything changed. With --dry-run, only shows what
# would change (WIKI_CHECKOUT is still modified, but nothing is committed or pushed).
#
# Links are rewritten because the wiki serves pages without the .md extension and has no access to the repository's
# other files: `Architecture.md#x` becomes `Architecture#x`, and `../README.md` becomes a link to the file on GitHub.
#
# Usage: scripts/publish-wiki.sh [--dry-run] WIKI_CHECKOUT
#
# Needs git, and push access to the wiki repository unless --dry-run.

set -euo pipefail

dry_run=false
if [[ ${1-} == --dry-run ]]; then
  dry_run=true
  shift
fi
if (($# != 1)) || [[ ! -d $1/.git ]]; then
  echo "usage: $0 [--dry-run] WIKI_CHECKOUT" >&2
  exit 2
fi
wiki=$(cd "$1" && pwd)

cd "$(dirname "$0")/.."

readonly blob="https://github.com/${GITHUB_REPOSITORY:-mowi12/sway-things}/blob/main"

find "$wiki" -mindepth 1 -maxdepth 1 ! -name .git -exec rm -rf {} +
cp -R wiki/. "$wiki/"
find "$wiki" -name '*.md' -not -path '*/.git/*' -exec sed -E -i \
  -e 's/\]\(([A-Za-z0-9_-]+)\.md(#[^)]*)?\)/](\1\2)/g' \
  -e "s#\]\(\.\./([^)]*)\)#]($blob/\1)#g" {} +

git -C "$wiki" add --all
if git -C "$wiki" diff --cached --quiet; then
  echo "The wiki is up to date."
  exit 0
fi

if "$dry_run"; then
  git -C "$wiki" diff --cached --stat
  exit 0
fi

git -C "$wiki" -c user.name="github-actions[bot]" \
  -c user.email="41898282+github-actions[bot]@users.noreply.github.com" \
  commit --quiet --message "Update from $(git rev-parse --short HEAD)"
git -C "$wiki" push --quiet
echo "Published the wiki."
