#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Checks the packaging files against the code, without building anything:
#
# - The version of [workspace.package] in Cargo.toml is the Version of both RPM specs, the version in the header of
#   the man page data/man/sway-things.1, the newest section of CHANGELOG.md and the newest <release> of the AppStream
#   metainfo, once it has one.
# - The MSRV, rust-version of [workspace.package], is the oldest toolchain the tests run on (ci.yml), the rustc that
#   packaging/debian/control and the cargo that the openSUSE spec require, and the Rust the README asks for.
# - The License of both specs names exactly the license expressions of the crates linked into the binaries (cargo
#   tree as Fedora's %cargo_license_summary runs it: no build, dev or proc-macro dependencies, every target) and
#   CC0-1.0 of the metainfo. The SourceLicense of the Fedora spec names exactly those of every crate in the tarball's
#   vendor/ and the licenses in LICENSES/. So a new dependency with a new license can't make them wrong unnoticed.
#
# In the specs, License and SourceLicense are written as %{shrink:…} with one expression per line, joined by AND; an
# expression with more than one license is put in parentheses.
#
# Check level: host (see scripts/run-checks.sh)
#
# Usage: scripts/check-packaging.sh
#
# Needs cargo (with the network, unless the crates are already downloaded).

set -euo pipefail

# shellcheck source=scripts/lib.sh
source "$(dirname "$0")/lib.sh"
no_arguments "$@"

cd "$(dirname "$0")/.."

require cargo

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
# The tarball is a file of the GitHub release, which OBS's download_files fetches from the spec's Source; the URL tag is
# the homepage, which hosts no releases.
repository=$(sed -n '/^\[workspace\.package\]/,/^\[/s/^repository = "\(.*\)"$/\1/p' Cargo.toml)
source_url="$repository/releases/download/v%{version}/%{name}-%{version}.tar.xz"
for spec in "$opensuse" "$fedora"; do
  found=$(sed -n 's/^Source0\{0,1\}:[[:space:]]*//p' "$spec")
  if [[ $found != "$source_url" ]]; then
    echo "error: $spec has the source ${found:-(none)}, expected $source_url" >&2
    status=1
  fi
done
metainfo=data/io.github.mowi12.SwayThings.metainfo.xml.in
if grep -q '<release ' "$metainfo"; then
  check_version "$metainfo" "$(grep -o -m 1 '<release [^>]*version="[^"]*"' "$metainfo" | sed 's/.*version="//; s/"$//')"
fi
check_version CHANGELOG.md "$(sed -n 's/^## \([0-9][^ ]*\) (.*/\1/p' CHANGELOG.md | head -n 1)"

# The MSRV.
msrv=$(sed -n '/^\[workspace\.package\]/,/^\[/s/^rust-version = "\(.*\)"$/\1/p' Cargo.toml)
check_msrv() {
  local file=$1 found=$2
  if [[ $found != "$msrv" ]]; then
    echo "error: $file has the MSRV ${found:-(none)}, Cargo.toml has $msrv" >&2
    status=1
  fi
}
check_msrv .github/workflows/ci.yml "$(sed -n 's/^ *toolchain: \["\([^"]*\)", stable\]$/\1/p' .github/workflows/ci.yml)"
check_msrv packaging/debian/control "$(sed -n 's/^ *rustc (>= \([^)]*\)),$/\1/p' packaging/debian/control)"
check_msrv "$opensuse" "$(sed -n 's/^BuildRequires: *cargo >= //p' "$opensuse")"
check_msrv README.md "$(sed -n 's/^You need Rust \([0-9.]*\) or newer.*/\1/p' README.md)"

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
