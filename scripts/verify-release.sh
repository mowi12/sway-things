#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Checks that TAG may become a release, before the release workflow builds anything:
#
# - TAG is vX.Y.Z, or vX.Y.Z-rc.N for a pre-release (the dry run of the workflow), and X.Y.Z is [workspace.package]
#   version in Cargo.toml at the tagged commit.
# - TAG is an annotated tag, and GitHub shows its signature as verified: the signing key is on the account of whoever
#   made it.
# - The tagged commit is on origin/main.
# - For a release (not a pre-release): CHANGELOG.md has a section "## X.Y.Z (…)" and the newest <release> of the
#   AppStream metainfo is X.Y.Z. The other version numbers (specs, man pages) are checked by scripts/check-packaging.sh
#   on every pull request.
#
# Not named check-*: it needs a tag, and `just check` runs every check script without arguments.
#
# Usage: scripts/verify-release.sh TAG
#
# Needs git with the tag and origin/main fetched, and gh (GH_TOKEN in CI) for the signature.

set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: scripts/verify-release.sh TAG" >&2
  exit 2
fi
tag=$1

cd "$(dirname "$0")/.."

for tool in git gh; do
  if ! command -v "$tool" >/dev/null; then
    echo "error: needs $tool" >&2
    exit 1
  fi
done

status=0
fail() {
  echo "error: $*" >&2
  status=1
}

if [[ ! $tag =~ ^v([0-9]+\.[0-9]+\.[0-9]+)(-rc\.[0-9]+)?$ ]]; then
  echo "error: $tag is no version tag: vX.Y.Z, or vX.Y.Z-rc.N for a pre-release" >&2
  exit 1
fi
version=${BASH_REMATCH[1]}
prerelease=false
if [[ -n ${BASH_REMATCH[2]} ]]; then
  prerelease=true
fi

if [[ $(git cat-file -t "refs/tags/$tag" 2>/dev/null) != tag ]]; then
  echo "error: $tag is no annotated tag (git tag -s)" >&2
  exit 1
fi
commit=$(git rev-parse "refs/tags/$tag^{commit}")

cargo_version=$(git show "$commit:Cargo.toml" | sed -n '/^\[workspace\.package\]/,/^\[/s/^version = "\(.*\)"$/\1/p')
if [[ $cargo_version != "$version" ]]; then
  fail "$tag doesn't match the version $cargo_version in Cargo.toml"
fi

repository=${GITHUB_REPOSITORY:-$(gh repo view --json nameWithOwner --jq .nameWithOwner)}
verified=$(gh api "repos/$repository/git/tags/$(git rev-parse "refs/tags/$tag")" --jq .verification.verified)
if [[ $verified != true ]]; then
  fail "GitHub doesn't show the signature of $tag as verified; sign it with a key on your account (git tag -s)"
fi

if ! git merge-base --is-ancestor "$commit" origin/main; then
  fail "the commit of $tag isn't on origin/main"
fi

if ! $prerelease; then
  if ! git show "$commit:CHANGELOG.md" 2>/dev/null | grep -q "^## ${version//./\\.} ("; then
    fail "CHANGELOG.md has no section \"## $version (…)\""
  fi
  newest=$(git show "$commit:data/io.github.mowi12.SwayThings.metainfo.xml.in" |
    grep -o -m 1 '<release [^>]*version="[^"]*"' | sed 's/.*version="//; s/"$//') || true
  if [[ $newest != "$version" ]]; then
    fail "the newest <release> of the metainfo is ${newest:-(none)}, not $version"
  fi
fi

if ((status == 0)); then
  if $prerelease; then
    echo "$tag may become a pre-release"
  else
    echo "$tag may become a release"
  fi
fi
exit "$status"
