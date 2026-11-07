#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Syncs the repository's GitHub labels with .github/labels.yml: renames labels listed as aliases, creates or updates
# every label in the file, and deletes every label that isn't in the file.
#
# Usage: scripts/sync-labels.sh [--dry-run]
#
# Needs gh (logged in, or GH_TOKEN set) and yq v4 (https://github.com/mikefarah/yq). Both are preinstalled on GitHub's
# Ubuntu runners. gh targets the repository of the current checkout, or GH_REPO=owner/name if set.

set -euo pipefail

readonly config=".github/labels.yml"

dry_run=false
case "${1-}" in
  "") ;;
  --dry-run) dry_run=true ;;
  *)
    echo "usage: $0 [--dry-run]" >&2
    exit 2
    ;;
esac

cd "$(dirname "$0")/.."

if ! yq --version 2>/dev/null | grep -q mikefarah; then
  echo "error: needs yq v4 from https://github.com/mikefarah/yq" >&2
  exit 1
fi

changes=0

# Runs a command, or only prints it with --dry-run.
run() {
  changes=$((changes + 1))
  if "$dry_run"; then
    printf 'would run:'
    printf ' %q' "$@"
    printf '\n'
  else
    "$@"
  fi
}

# --- Read and validate the file, before touching anything ---

# One line per label: name, color, description and comma-separated aliases, separated by the ASCII unit separator
# (unlike tabs, it never collapses empty fields, and unlike @tsv it never quotes). Reading into a variable first makes
# a yq failure abort the script (set -e) instead of looking like an empty file.
readonly sep=$'\x1f'
parsed=$(yq ".[] | (.name // \"\") + \"$sep\" + (.color // \"\") + \"$sep\" + (.description // \"\") + \"$sep\" + \
  ((.aliases // []) | join(\",\"))" "$config")

names=()
colors=()
descriptions=()
alias_lists=()
declare -A wanted=() # lowercased name -> 1 (GitHub label names are case-insensitive)
errors=0

while IFS=$sep read -r name color description aliases; do
  if [[ -z $name ]]; then
    echo "error: a label has no name" >&2
    errors=1
    continue
  fi
  if [[ -n ${wanted[${name,,}]+set} ]]; then
    echo "error: duplicate label: $name" >&2
    errors=1
  fi
  if [[ ! $color =~ ^[0-9a-f]{6}$ ]]; then
    echo "error: $name: color must be 6 lowercase hex digits without #" >&2
    errors=1
  fi
  if [[ -z $description || ${#description} -gt 100 ]]; then
    echo "error: $name: description must have 1 to 100 characters" >&2
    errors=1
  fi
  wanted[${name,,}]=1
  names+=("$name")
  colors+=("$color")
  descriptions+=("$description")
  alias_lists+=("$aliases")
done <<<"$parsed"

if ((errors)); then
  exit 1
fi
if ((${#names[@]} == 0)); then
  echo "error: $config defines no labels; refusing to delete every label" >&2
  exit 1
fi

# --- Read the current labels ---

declare -A current_name=() current_color=() current_description=() # keyed by lowercased name

current=$(gh label list --limit 1000 --json name,color,description --jq '.[] | .name, .color, .description')
while IFS= read -r name && IFS= read -r color && IFS= read -r description; do
  [[ -n $name ]] || continue
  current_name[${name,,}]=$name
  current_color[${name,,}]=${color,,}
  current_description[${name,,}]=$description
done <<<"$current"

# --- Rename, create and update ---

for i in "${!names[@]}"; do
  name=${names[i]}
  key=${name,,}

  # Rename the first existing alias, so issues keep the label.
  if [[ -z ${current_name[$key]+set} && -n ${alias_lists[i]} ]]; then
    IFS=, read -ra aliases <<<"${alias_lists[i]}"
    for alias in "${aliases[@]}"; do
      old=${alias,,}
      if [[ -n ${current_name[$old]+set} ]]; then
        run gh label edit "${current_name[$old]}" --name "$name"
        current_name[$key]=$name
        current_color[$key]=${current_color[$old]}
        current_description[$key]=${current_description[$old]}
        unset "current_name[$old]" "current_color[$old]" "current_description[$old]"
        break
      fi
    done
  fi

  if [[ -z ${current_name[$key]+set} ]]; then
    run gh label create "$name" --color "${colors[i]}" --description "${descriptions[i]}"
  elif [[ ${current_name[$key]} != "$name" ||
    ${current_color[$key]} != "${colors[i]}" ||
    ${current_description[$key]} != "${descriptions[i]}" ]]; then
    run gh label edit "${current_name[$key]}" --name "$name" --color "${colors[i]}" \
      --description "${descriptions[i]}"
  fi
done

# --- Delete labels that aren't in the file ---

mapfile -t keys < <(printf '%s\n' "${!current_name[@]}" | sort)
for key in "${keys[@]}"; do
  [[ -n $key ]] || continue
  if [[ -z ${wanted[$key]+set} ]]; then
    run gh label delete "${current_name[$key]}" --yes
  fi
done

if ((changes == 0)); then
  echo "Labels are in sync with $config."
elif "$dry_run"; then
  echo "$changes change(s) would be made."
else
  echo "Made $changes change(s)."
fi
