#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Checks the packaging files against the code, without building anything:
#
# - The version of [workspace.package] in Cargo.toml is the Version of both RPM specs, the version in the header of
#   the man page data/man/sway-things.1, and the newest <release> of the AppStream metainfo, once it has one.
# - The License of both specs names exactly the license expressions of the crates linked into the binaries (cargo
#   tree as Fedora's %cargo_license_summary runs it: no build, dev or proc-macro dependencies, every target) and
#   CC0-1.0 of the metainfo. The SourceLicense of the Fedora spec names exactly those of every crate in the tarball's
#   vendor/ and the licenses in LICENSES/. So a new dependency with a new license can't make them wrong unnoticed.
#
# In the specs, License and SourceLicense are written as %{shrink:…} with one expression per line, joined by AND; an
# expression with more than one license is put in parentheses.
#
# Usage: scripts/check-packaging.sh
#
# Needs cargo (with the network, unless the crates are already downloaded).

set -euo pipefail

cd "$(dirname "$0")/.."

if ! command -v cargo >/dev/null; then
  echo "error: needs cargo" >&2
  exit 1
fi

opensuse=packaging/rpm/opensuse/sway-things.spec
fedora=packaging/rpm/fedora/sway-things.spec
status=0

# Versions.
version=$(sed -n '/^\[workspace\.package\]/,/^\[/s/^version = "\(.*\)"$/\1/p' Cargo.toml)
check_version() {
  local file=$1 found=$2
  if [[ $found != "$version" ]]; then
    echo "error: $file has the version ${found:-(none)}, Cargo.toml has $version" >&2
    status=1
  fi
}
for spec in "$opensuse" "$fedora"; do
  check_version "$spec" "$(sed -n 's/^Version:[[:space:]]*//p' "$spec")"
done
check_version data/man/sway-things.1 \
  "$(sed -n 's/^\.TH .* "Sway Things \([^"]*\)" .*/\1/p' data/man/sway-things.1)"
metainfo=data/io.github.mowi12.SwayThings.metainfo.xml.in
if grep -q '<release ' "$metainfo"; then
  check_version "$metainfo" "$(grep -o -m 1 '<release [^>]*version="[^"]*"' "$metainfo" | sed 's/.*version="//; s/"$//')"
fi

# The license expressions cargo tree prints with these arguments, one per line, in the form of the specs.
licenses() {
  cargo tree --workspace --locked --target all --prefix none --format '{l}' "$@" |
    sed '/^$/d; s/ (\*)$//; s| / |/|g; s|/| OR |g; s/^\(.* .*\)$/(\1)/' | LC_ALL=C sort -u
}
# The expressions of the tag TAG in SPEC, one per line.
spec_licenses() {
  local spec=$1 tag=$2
  sed -n "/^$tag:[[:space:]]*%{shrink:\$/,/^}\$/p" "$spec" | sed '1d; $d; s/^[[:space:]]*//; s/[[:space:]]*AND$//' |
    LC_ALL=C sort -u
}
compare_licenses() {
  local spec=$1 tag=$2 expected=$3
  local found
  found=$(spec_licenses "$spec" "$tag")
  if [[ -z $found ]]; then
    echo "error: $spec has no $tag in the form %{shrink: … }" >&2
    status=1
  elif ! diff -u --label "$spec $tag" --label expected <(printf '%s\n' "$found") <(printf '%s\n' "$expected") >&2; then
    echo "error: the $tag of $spec differs from the licenses of the crates (- spec, + expected)" >&2
    status=1
  fi
}

linked=$( (licenses --edges no-build,no-dev,no-proc-macro && echo CC0-1.0) | LC_ALL=C sort -u)
for spec in "$opensuse" "$fedora"; do
  compare_licenses "$spec" License "$linked"
done
own=$(for text in LICENSES/*.txt; do basename "$text" .txt; done)
vendored=$( (licenses --edges normal,build,dev --all-features && printf '%s\n' "$own") | LC_ALL=C sort -u)
compare_licenses "$fedora" SourceLicense "$vendored"

exit "$status"
