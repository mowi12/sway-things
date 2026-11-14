#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Makes the release tarball: the source tree of REF (git archive, so uncommitted files never get in) with the crates
# it depends on in vendor/ (cargo vendor) and a .cargo/config.toml that points cargo at them, so the tarball builds
# offline. The archive is reproducible: two runs on the same REF give the same bytes. Next to it go a .sha256 file and
# the inputs of the openSUSE Build Service, under names that don't change with the version:
#
#   sway-things.spec                       the Fedora spec (packaging/rpm/fedora/), used by every repository without a
#                                          spec of its own
#   sway-things-openSUSE_Tumbleweed.spec   the openSUSE spec (packaging/rpm/opensuse/), which OBS picks for the
#                                          repository openSUSE_Tumbleweed because of its name
#
# The version is X.Y.Z for a tag vX.Y.Z, which must match [workspace.package] version and the specs. For any other REF
# it is a snapshot version, <version>+git<commit count>.g<short hash>, which sorts after <version> and before the next
# release; it replaces the version in the specs.
#
# Usage: scripts/make-tarball.sh [--ref REF] [--out DIR] [--check]
#   --ref REF   the commit, branch or tag to package (default HEAD)
#   --out DIR   where to put the files (default target/dist in the repository)
#   --check     make everything twice and fail unless both runs give the same bytes
#
# Needs git, cargo (and the network, to download the crates), GNU tar, xz and sha256sum.

set -euo pipefail

usage() {
  echo "usage: scripts/make-tarball.sh [--ref REF] [--out DIR] [--check]" >&2
  exit 2
}

ref=HEAD
out=""
check=false
while [[ $# -gt 0 ]]; do
  case $1 in
    --ref)
      ref=${2:-$(usage)}
      shift
      ;;
    --out)
      out=$(realpath -m -- "${2:-$(usage)}")
      shift
      ;;
    --check) check=true ;;
    *) usage ;;
  esac
  shift
done

cd "$(dirname "$0")/.."
out=${out:-$PWD/target/dist}

if $check; then
  runs=$(mktemp -d)
  trap 'rm -rf "$runs"' EXIT
  scripts/make-tarball.sh --ref "$ref" --out "$runs/1"
  scripts/make-tarball.sh --ref "$ref" --out "$runs/2"
  if ! diff -r "$runs/1" "$runs/2"; then
    echo "error: two runs on $ref gave different files: the tarball isn't reproducible" >&2
    exit 1
  fi
  echo "Both runs gave the same files"
  mkdir -p "$out"
  cp "$runs/1"/* "$out/"
  exit
fi

for tool in git cargo tar xz sha256sum; do
  if ! command -v "$tool" >/dev/null; then
    echo "error: needs $tool" >&2
    exit 1
  fi
done

commit=$(git rev-parse --verify --quiet "$ref^{commit}") || {
  echo "error: not a commit: $ref" >&2
  exit 1
}

# The version in [workspace.package] of the root Cargo.toml at REF.
cargo_version=$(git show "$commit:Cargo.toml" | sed -n '/^\[workspace\.package\]/,/^\[/s/^version = "\(.*\)"$/\1/p')
if [[ -z $cargo_version ]]; then
  echo "error: no version in [workspace.package] of Cargo.toml at $ref" >&2
  exit 1
fi
if [[ $ref =~ ^(refs/tags/)?v([0-9]+\.[0-9]+\.[0-9]+)$ ]] && git rev-parse --verify --quiet "refs/tags/${ref#refs/tags/}" \
  >/dev/null; then
  version=${BASH_REMATCH[2]}
  release=true
  if [[ $version != "$cargo_version" ]]; then
    echo "error: the tag $ref doesn't match the version $cargo_version in Cargo.toml" >&2
    exit 1
  fi
else
  version=$cargo_version+git$(git rev-list --count "$commit").g$(git rev-parse --short=7 "$commit")
  release=false
fi
name=sway-things-$version
# The commit time is the time of every file in the archive.
epoch=$(git log -1 --format=%ct "$commit")

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

echo "Packaging $commit as $name"
git archive --format=tar --prefix="$name/" "$commit" | tar -x -C "$work"
tree=$work/$name

# cargo vendor prints the configuration that replaces crates.io with vendor/ (--quiet would suppress it too).
mkdir -p "$tree/.cargo"
{
  echo "# Written by scripts/make-tarball.sh: build with the crates in vendor/, without the network."
  (cd "$tree" && cargo vendor --locked --versioned-dirs vendor)
} >"$work/config.toml"
mv "$work/config.toml" "$tree/.cargo/config.toml"
if ! git show "$commit:Cargo.lock" | cmp -s - "$tree/Cargo.lock"; then
  echo "error: cargo vendor changed Cargo.lock" >&2
  exit 1
fi

# The specs with the version of this tarball.
mkdir -p "$out"
spec() {
  local from=$1 to=$2
  local spec_version
  spec_version=$(sed -n 's/^Version:[[:space:]]*//p' "$tree/$from")
  if $release; then
    if [[ $spec_version != "$version" ]]; then
      echo "error: $from has the version $spec_version, not $version" >&2
      exit 1
    fi
    cp "$tree/$from" "$out/$to"
  else
    sed "s/^\(Version:[[:space:]]*\).*/\1$version/" "$tree/$from" >"$out/$to"
  fi
}
spec packaging/rpm/fedora/sway-things.spec sway-things.spec
spec packaging/rpm/opensuse/sway-things.spec sway-things-openSUSE_Tumbleweed.spec

# Reproducible: sorted names, the commit time, no owner, no access or change times, modes independent of the umask, and
# a single xz thread, so the bytes don't depend on the machine.
tar --create --format=pax --sort=name --mtime="@$epoch" --owner=0 --group=0 --numeric-owner \
  --mode='a+rX,u+w,go-w' --pax-option=exthdr.name=%d/PaxHeaders/%f,delete=atime,delete=ctime \
  -C "$work" "$name" | xz -T1 -9 >"$out/$name.tar.xz"
(cd "$out" && sha256sum "$name.tar.xz" >"$name.tar.xz.sha256")

echo "Wrote $out/$name.tar.xz, its .sha256 and the specs"
