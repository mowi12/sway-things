#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Installs check tools from their GitHub releases, at the version and checksum tools/binaries.txt pins: downloads the
# asset, verifies its SHA-256, unpacks it and puts the binary NAME into DIR. In GitHub Actions, adds DIR to the PATH of
# the next steps; elsewhere prints it.
#
# Usage: scripts/install-tool.sh [--dir DIR] NAME…   (DIR defaults to $RUNNER_TEMP/tools, or target/tools)
#
# Needs gh (with GH_TOKEN in CI), sha256sum, tar and, for a zip asset, unzip.

set -euo pipefail

# shellcheck source=scripts/lib.sh
source "$(dirname "$0")/lib.sh"
help_option "$@"

usage() {
  echo "usage: $0 [--dir DIR] NAME…" >&2
  exit 2
}

cd "$(dirname "$0")/.."

dir=""
if [[ ${1-} == --dir ]]; then
  [[ -n ${2-} ]] || usage
  dir=$2
  shift 2
fi
(($#)) || usage
dir=$(realpath -m -- "${dir:-${RUNNER_TEMP:-target}/tools}")

require gh sha256sum tar

mkdir -p "$dir"
for name in "$@"; do
  read -r _ version sha256 repository tag asset < <(awk -v name="$name" '$1 == name' tools/binaries.txt) || {
    echo "error: $name is not in tools/binaries.txt" >&2
    exit 1
  }
  tag=${tag//\{version\}/$version}
  asset=${asset//\{version\}/$version}
  work=$(mktemp -d)
  gh release download "$tag" --repo "$repository" --pattern "$asset" --output "$work/$asset"
  echo "$sha256  $work/$asset" | sha256sum --check --quiet
  mkdir "$work/unpacked"
  case $asset in
    *.zip)
      require unzip
      unzip -q "$work/$asset" -d "$work/unpacked"
      ;;
    *.tar.gz) tar -xzf "$work/$asset" -C "$work/unpacked" ;;
    *)
      echo "error: unknown archive: $asset" >&2
      exit 1
      ;;
  esac
  binary=$(find "$work/unpacked" -type f -name "$name" -print -quit)
  if [[ -z $binary ]]; then
    echo "error: $asset holds no $name" >&2
    exit 1
  fi
  install -m 755 "$binary" "$dir/$name"
  rm -rf "$work"
  echo "installed $name $version into $dir"
done

if [[ -n ${GITHUB_PATH-} ]]; then
  echo "$dir" >>"$GITHUB_PATH"
else
  echo "add $dir to PATH to use it"
fi
