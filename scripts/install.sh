#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Installs what scripts/build.sh built, and the committed man pages, completions and icons, into DESTDIR/PREFIX. Only
# copies: it runs neither cargo nor msgfmt, so running it as root creates no files in the build directory. With
# --uninstall, removes those files again (files only, never directories), without needing a build.
#
# Usage: scripts/install.sh [--uninstall]
#   PREFIX            where to install (default /usr/local); must be the PREFIX of scripts/build.sh
#   DESTDIR           a staging directory to install into, e.g. for a package (default none)
#   PROFILE           the cargo profile the binaries were built with (default release)
#   CARGO_TARGET_DIR  cargo's target directory (default target)
#
# Needs install (GNU coreutils).

set -euo pipefail

cd "$(dirname "$0")/.."

uninstall=false
case ${1:-} in
  "") ;;
  --uninstall) uninstall=true ;;
  *)
    echo "usage: scripts/install.sh [--uninstall]" >&2
    exit 2
    ;;
esac

prefix=${PREFIX:-/usr/local}
profile=${PROFILE:-release}
if [[ $prefix != /* ]]; then
  echo "error: PREFIX must be an absolute path, not $prefix" >&2
  exit 1
fi
case $profile in
  release) profile_dir=release ;;
  dev) profile_dir=debug ;;
  *) profile_dir=$profile ;;
esac
out=${CARGO_TARGET_DIR:-target}/$profile_dir
dest=${DESTDIR:-}$prefix

app=io.github.mowi12.SwayThings
bash_completion=share/bash-completion/completions/sway-things
zsh_completion=share/zsh/site-functions/_sway-things
fish_completion=share/fish/vendor_completions.d/sway-things.fish

if $uninstall; then
  shopt -s nullglob
  files=(
    "$dest/bin/sway-things"
    "$dest/bin/sway-things-gui"
    "$dest/share/applications/$app.desktop"
    "$dest/share/metainfo/$app.metainfo.xml"
    "$dest/share/man/man1/sway-things.1"
    "$dest/share/man/man1/sway-things-"*.1
    "$dest/$bash_completion"
    "$dest/$zsh_completion"
    "$dest/$fish_completion"
    "$dest/share/locale/"*/LC_MESSAGES/sway-things.mo
    "$dest/share/icons/hicolor/"*/apps/"$app"*.svg
  )
  rm -f -- "${files[@]}"
  echo "Removed Sway Things from $dest"
  exit 0
fi

if [[ ! -d $out/data ]]; then
  echo "error: $out/data is missing; run scripts/build.sh first" >&2
  exit 1
fi
built_for=$(cat "$out/data/prefix")
if [[ $built_for != "$prefix" ]]; then
  echo "error: built for PREFIX=$built_for, not $prefix: the binaries would look for translations in the wrong place;" \
    "run scripts/build.sh with PREFIX=$prefix" >&2
  exit 1
fi

install -Dm755 "$out/sway-things" "$dest/bin/sway-things"
install -Dm755 "$out/sway-things-gui" "$dest/bin/sway-things-gui"
install -Dm644 "$out/data/$app.desktop" "$dest/share/applications/$app.desktop"
install -Dm644 "$out/data/$app.metainfo.xml" "$dest/share/metainfo/$app.metainfo.xml"
for catalog in "$out"/data/locale/*/LC_MESSAGES/sway-things.mo; do
  if [[ -f $catalog ]]; then
    install -Dm644 "$catalog" "$dest/share/${catalog#"$out/data/"}"
  fi
done
for page in data/man/*.1; do
  install -Dm644 "$page" "$dest/share/man/man1/$(basename "$page")"
done
install -Dm644 data/completions/sway-things.bash "$dest/$bash_completion"
install -Dm644 data/completions/_sway-things "$dest/$zsh_completion"
install -Dm644 data/completions/sway-things.fish "$dest/$fish_completion"
if [[ -d data/icons ]]; then
  while IFS= read -r -d '' icon; do
    install -Dm644 "$icon" "$dest/share/icons/${icon#data/icons/}"
  done < <(find data/icons -type f -print0)
fi

echo "Installed Sway Things into $dest"
if [[ -z ${DESTDIR:-} ]]; then
  echo "Distribution packages refresh the caches themselves; after installing by hand, you may want to run" \
    "update-desktop-database, gtk-update-icon-cache and mandb."
fi
