#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Sets the version to VERSION everywhere it is written down, for the release pull request:
#
# - [workspace.package] version in Cargo.toml, and the workspace crates in Cargo.lock and fuzz/Cargo.lock;
# - Version of both RPM specs in packaging/rpm/;
# - a <release version="VERSION" date="DATE"/> at the top of <releases> in the AppStream metainfo (DATE defaults to
#   today, UTC);
# - the man pages, whose headers name the version (scripts/update-cli-docs.sh).
#
# The changelog is written separately: by hand for the first release, with git-cliff for every later one (see
# wiki/Architecture.md → Releases). scripts/check-packaging.sh checks afterwards that all versions agree.
#
# Usage: scripts/bump-version.sh VERSION [DATE]
#
# Needs cargo (offline is enough), GNU sed and date.

set -euo pipefail

if [[ $# -lt 1 || $# -gt 2 || ! $1 =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "usage: scripts/bump-version.sh X.Y.Z [YYYY-MM-DD]" >&2
  exit 2
fi
version=$1
date=${2:-$(date -u +%F)}
if [[ ! $date =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}$ ]]; then
  echo "error: not a date: $date" >&2
  exit 2
fi

cd "$(dirname "$0")/.."

if ! command -v cargo >/dev/null; then
  echo "error: needs cargo" >&2
  exit 1
fi

metainfo=data/io.github.mowi12.SwayThings.metainfo.xml.in
if grep -q "<release version=\"$version\"" "$metainfo"; then
  echo "error: $metainfo already has a release $version" >&2
  exit 1
fi

sed -i "/^\[workspace\.package\]/,/^\[/s/^version = \".*\"$/version = \"$version\"/" Cargo.toml
cargo update --workspace --offline --quiet
cargo update --manifest-path fuzz/Cargo.toml --package sway-things-core --offline --quiet
sed -i "s/^\(Version:[[:space:]]*\).*/\1$version/" packaging/rpm/opensuse/sway-things.spec \
  packaging/rpm/fedora/sway-things.spec

release="<release version=\"$version\" date=\"$date\"/>"
if grep -q '<releases>' "$metainfo"; then
  sed -i "s|^\([[:space:]]*\)<releases>$|&\n\1  $release|" "$metainfo"
else
  sed -i "s|^\([[:space:]]*\)<content_rating |\1<releases>\n\1  $release\n\1</releases>\n&|" "$metainfo"
fi

scripts/update-cli-docs.sh

echo "Set the version to $version. Next: the changelog section \"## $version ($date)\", then scripts/check-packaging.sh."
