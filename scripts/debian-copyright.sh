#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Writes the debian/copyright file (machine-readable format 1.0) for the unpacked release tarball TREE of
# scripts/make-tarball.sh, which calls it. Our files are GPL-3.0-or-later, except those whose REUSE information says
# otherwise (an SPDX header or a REUSE.toml annotation for a single path). Every crate in vendor/ gets a paragraph with
# its license expression and authors from cargo metadata; a crate without authors is credited to "The <crate>
# developers", so the copyright lines of the crates are approximate.
#
# Each license gets a paragraph. Those in /usr/share/common-licenses of Debian and Ubuntu refer to it; the others get
# their full text from LICENSES/, else from the first crate in vendor/ (in sorted order) that ships a file for it, e.g.
# LICENSE-MIT, or a plain LICENSE when that crate has no other license. It fails for a license it knows no text for, so
# a dependency with a new license is noticed when the tarball is made.
#
# Usage: scripts/debian-copyright.sh TREE >copyright
#
# Needs cargo (offline: the crates are in TREE/vendor), jq, and GNU awk, grep and sed.

set -euo pipefail

if [[ $# -ne 1 || ! -d $1/vendor ]]; then
  echo "usage: scripts/debian-copyright.sh TREE (an unpacked tarball of scripts/make-tarball.sh)" >&2
  exit 2
fi
tree=$1
for tool in cargo jq; do
  if ! command -v "$tool" >/dev/null; then
    echo "error: needs $tool" >&2
    exit 1
  fi
done

own_copyright="2026 Sway Things contributors"
own_license=GPL-3.0-or-later
upstream=https://github.com/mowi12/sway-things

# An SPDX expression in the syntax of the copyright format: lower-case operators, "with X exception", and a comma
# before an "and" that follows a parenthesized group, the format's way to group. Other parentheses are an error.
dep5_expression() {
  local expression=$1
  expression=$(sed -E 's| / | OR |g; s|([^ ])/([^ ])|\1 OR \2|g; s/^\((.*)\) AND (.*)$/\1, AND \2/' <<<"$expression")
  if [[ $expression == *[\(\)]* ]]; then
    echo "error: can't write the license expression $1 in the copyright format" >&2
    exit 1
  fi
  sed -E 's/ OR / or /g; s/ AND / and /g; s/ WITH ([^ ,]+)/ with \1 exception/g' <<<"$expression"
}

# The license names in a DEP-5 expression, one per line.
license_names() {
  sed -E 's/,? (or|and) /\n/g' <<<"$1"
}

# Where a license's text is: "common:FILE" for /usr/share/common-licenses/FILE, else the file names in a crate that
# hold it, or "only:LICENSE" for a plain LICENSE of a crate with just that license. An exception refers to its
# license in common-licenses and takes only its own text from the crate (lintian rejects a full Apache-2.0 text).
text_source() {
  case $1 in
    Apache-2.0) echo common:Apache-2.0 ;;
    CC0-1.0) echo common:CC0-1.0 ;;
    GPL-3.0-or-later) echo common:GPL-3 ;;
    LGPL-2.1-or-later) echo common:LGPL-2.1 ;;
    "Apache-2.0 with LLVM-exception exception") echo common:Apache-2.0 LICENSE-Apache-2.0_WITH_LLVM-exception ;;
    BSD-2-Clause) echo LICENSE-BSD ;;
    MIT) echo LICENSE-MIT license-mit only:LICENSE ;;
    Unicode-3.0) echo LICENSE-UNICODE ;;
    Unlicense) echo UNLICENSE ;;
    *) echo only:LICENSE ;;
  esac
}

# A text as the body of a License paragraph: every line indented by a space, empty lines as " .".
indent() {
  sed -E 's/[[:space:]]+$//; s/^$/./; s/^/ /'
}

metadata=$(cd "$tree" && cargo metadata --format-version 1 --locked --offline)
# The vendored crates as "name-version<US>license<US>authors<US>name", sorted by directory name. US is the unit
# separator, which read doesn't merge like tabs when a field is empty.
mapfile -t crates < <(jq -r '.packages[] | select(.source != null)
  | [.name + "-" + .version, (.license // ""), (.authors | join(", ")), .name] | join("\u001f")' <<<"$metadata" | LC_ALL=C sort)

# Our files with another license than GPL-3.0-or-later: "path<TAB>copyright<TAB>license", several copyright lines
# separated by "|".
# REUSE-IgnoreStart: the patterns below read REUSE information, they declare none.
own_exceptions() {
  local file license copyright
  while IFS= read -r -d '' file; do
    license=$(head -n 5 "$tree/$file" | grep -o -m 1 'SPDX-License-Identifier: [^ ]*' | sed 's/.*: //') || true
    if [[ -n $license && $license != "$own_license" ]]; then
      copyright=$(head -n 5 "$tree/$file" | grep -o -m 1 'SPDX-FileCopyrightText: .*[^ >-]' | sed 's/.*: //')
      printf '%s\t%s\t%s\n' "$file" "${copyright:-$own_copyright}" "$license"
    fi
  done < <(cd "$tree" && find . -path ./vendor -prune -o -path ./LICENSES -prune -o -type f -printf '%P\0' | sort -z)
  # REUSE.toml: [[annotations]] tables with a single path, a copyright (string or array) and a license.
  awk -v own="$own_license" '
    function flush() {
      if (path != "" && license != "" && license != own) printf "%s\t%s\t%s\n", path, copyright, license
      path = ""; copyright = ""; license = ""
    }
    /^\[\[annotations\]\]/ { flush(); next }
    /^path = "/ { path = $0; sub(/^path = "/, "", path); sub(/".*/, "", path) }
    /^SPDX-FileCopyrightText = / {
      copyright = $0; sub(/^SPDX-FileCopyrightText = \[?/, "", copyright); sub(/\]$/, "", copyright)
      gsub(/"/, "", copyright); gsub(/, /, "|", copyright)
    }
    /^SPDX-License-Identifier = "/ { license = $0; sub(/^[^"]*"/, "", license); sub(/".*/, "", license) }
    END { flush() }
  ' "$tree/REUSE.toml"
}
# REUSE-IgnoreEnd

licenses=("$own_license")

cat <<EOF
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: Sway Things
Upstream-Contact: $upstream/issues
Source: $upstream

Files: *
Copyright: $own_copyright
License: $own_license
EOF

while IFS=$'\t' read -r path copyright license; do
  expression=$(dep5_expression "$license")
  printf '\nFiles: %s\nCopyright: %s\nLicense: %s\n' "$path" "${copyright//|/$'\n '}" "$expression"
  mapfile -t -O "${#licenses[@]}" licenses < <(license_names "$expression")
done < <(own_exceptions)

for crate in "${crates[@]}"; do
  IFS=$'\x1f' read -r dir license authors crate_name <<<"$crate"
  if [[ -z $license ]]; then
    echo "error: the crate $dir declares no license expression" >&2
    exit 1
  fi
  if [[ ! -d $tree/vendor/$dir ]]; then
    echo "error: vendor/$dir is missing" >&2
    exit 1
  fi
  expression=$(dep5_expression "$license")
  printf '\nFiles: vendor/%s/*\nCopyright: %s\nLicense: %s\n' "$dir" "${authors:-The $crate_name developers}" "$expression"
  mapfile -t -O "${#licenses[@]}" licenses < <(license_names "$expression")
done

mapfile -t licenses < <(printf '%s\n' "${licenses[@]}" | LC_ALL=C sort -u)
for name in "${licenses[@]}"; do
  printf '\nLicense: %s\n' "$name"
  read -r -a sources <<<"$(text_source "$name")"
  if [[ ${sources[0]} == common:* ]]; then
    printf ' On Debian and Ubuntu systems, the full text of this license is in\n /usr/share/common-licenses/%s.\n' \
      "${sources[0]#common:}"
    sources=("${sources[@]:1}")
    if ((${#sources[@]} == 0)); then
      continue
    fi
    printf ' .\n'
  fi
  text=""
  if [[ -f $tree/LICENSES/$name.txt ]]; then
    text=$tree/LICENSES/$name.txt
  fi
  for crate in "${crates[@]}"; do
    if [[ -n $text ]]; then
      break
    fi
    IFS=$'\x1f' read -r dir license _ <<<"$crate"
    for source in "${sources[@]}"; do
      file=${source#only:}
      if [[ $source == only:* && $(dep5_expression "$license") != "$name" ]]; then
        continue
      fi
      if [[ -f $tree/vendor/$dir/$file ]]; then
        text=$tree/vendor/$dir/$file
        break 2
      fi
    done
  done
  if [[ -z $text ]]; then
    echo "error: no crate in vendor/ ships a text of the license $name; teach text_source in $0 where it is" >&2
    exit 1
  fi
  if [[ $name == *" exception" ]]; then
    # Only the exception, which follows the license in the crate's file.
    exception=$(sed -n '/^-* *LLVM Exceptions to the Apache 2\.0 License/,$p' "$text")
    if [[ -z $exception ]]; then
      echo "error: $text has no LLVM exception text after the license" >&2
      exit 1
    fi
    indent <<<"$exception"
  else
    indent <"$text"
  fi
done
