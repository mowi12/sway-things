#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Checks the installation for both prefixes: the default PREFIX=/usr/local of a source install and PREFIX=/usr, as
# packages do it. For each, builds (a debug build), installs into a temporary DESTDIR and compares the installed files
# with the expected list. Then validates them: the desktop entry with desktop-file-validate, desktop entry and metainfo
# together with appstreamcli validate-tree, each man page with groff (any warning fails), and the completions with the
# syntax check of their shell. Then uninstalls and checks that no file is left. Finally checks that installing for
# another PREFIX than the build's is refused.
#
# Usage: scripts/check-install.sh
#
# Needs cargo, the GTK 4 development files, msgfmt (GNU gettext), desktop-file-validate (desktop-file-utils),
# appstreamcli (AppStream), groff, bash, zsh, fish and diff (GNU diffutils).

set -euo pipefail

cd "$(dirname "$0")/.."

for tool in cargo msgfmt desktop-file-validate appstreamcli groff bash zsh fish diff; do
  if ! command -v "$tool" >/dev/null; then
    echo "error: needs $tool" >&2
    exit 1
  fi
done

export PROFILE=dev
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
app=io.github.mowi12.SwayThings

# Builds for PREFIX=$1, installs into a DESTDIR of its own and checks the installed files and the uninstall.
check_prefix() {
  local prefix=$1
  local dest
  dest=$work/$(basename "$prefix")
  local root=${prefix#/}
  echo "==> PREFIX=$prefix"
  PREFIX=$prefix scripts/build.sh
  PREFIX=$prefix DESTDIR=$dest scripts/install.sh

  # Mode and path of every file below $DESTDIR. The man pages are those in data/man/: a test of crates/cli-docs makes
  # sure there is one per command.
  local expected=(
    "755 $root/bin/sway-things"
    "755 $root/bin/sway-things-gui"
    "644 $root/share/applications/$app.desktop"
    "644 $root/share/bash-completion/completions/sway-things"
    "644 $root/share/fish/vendor_completions.d/sway-things.fish"
    "644 $root/share/icons/hicolor/scalable/apps/$app.svg"
    "644 $root/share/icons/hicolor/symbolic/apps/$app-symbolic.svg"
    "644 $root/share/metainfo/$app.metainfo.xml"
    "644 $root/share/zsh/site-functions/_sway-things"
  )
  local page
  for page in data/man/*.1; do
    expected+=("644 $root/share/man/man1/$(basename "$page")")
  done
  if ! diff -u <(printf '%s\n' "${expected[@]}" | LC_ALL=C sort) \
    <(find "$dest" -type f -printf '%m %P\n' | LC_ALL=C sort); then
    echo "error: the files installed for PREFIX=$prefix differ from the expected ones (- expected, + installed)" >&2
    exit 1
  fi

  local status=0 warnings
  desktop-file-validate "$dest$prefix/share/applications/$app.desktop" || status=1
  appstreamcli validate-tree --no-net "$dest$prefix" || status=1
  for page in "$dest$prefix"/share/man/man1/*.1; do
    warnings=$(groff -man -Tutf8 -ww -z "$page" 2>&1) || status=1
    if [[ -n $warnings ]]; then
      echo "$warnings" >&2
      status=1
    fi
  done
  bash -n "$dest$prefix/share/bash-completion/completions/sway-things" || status=1
  zsh -n "$dest$prefix/share/zsh/site-functions/_sway-things" || status=1
  fish --no-execute "$dest$prefix/share/fish/vendor_completions.d/sway-things.fish" || status=1
  if ((status)); then
    echo "error: the files installed for PREFIX=$prefix don't validate" >&2
    exit 1
  fi

  PREFIX=$prefix DESTDIR=$dest scripts/install.sh --uninstall
  local left
  left=$(find "$dest" -type f -printf '%P\n')
  if [[ -n $left ]]; then
    echo "error: files left after --uninstall for PREFIX=$prefix:" >&2
    echo "$left" >&2
    exit 1
  fi
}

check_prefix /usr/local
check_prefix /usr

# The last build is for /usr: an install for another prefix must be refused.
if PREFIX=/usr/local DESTDIR=$work/guard scripts/install.sh 2>/dev/null; then
  echo "error: installing for PREFIX=/usr/local what was built for /usr wasn't refused" >&2
  exit 1
fi
echo "Install, validation and uninstall work for /usr/local and /usr."
