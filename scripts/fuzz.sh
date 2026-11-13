#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Fuzzes the sway config parser with cargo-fuzz: the target fuzz/fuzz_targets/sway_config.rs reads a config and the files
# it includes from memory. CI calls `lint` and `run` on every change; the scheduled workflow (.github/workflows/fuzz.yml)
# runs longer, keeps the corpus, and turns what it finds into issues with `triage` and `report`.
#
#   lint                    clippy and cargo-deny on the fuzz crate (fuzz/, a workspace of its own)
#   run [SECONDS]           fuzz for SECONDS (default 60) from fuzz/seeds/, the sample configs of ci/integration/ and
#                           the corpus in fuzz/corpus/sway_config/; an input that panics, takes longer than 10 s or more
#                           than 2 GB lands in fuzz/artifacts/sway_config/ and fails the run
#   minimize                minimize the corpus
#   reproduce FILE          run the target once on FILE, which shows the panic
#   triage DIR              minimize each input in fuzz/artifacts/sway_config/ and write what to report about it to DIR
#   report DIR [--dry-run]  open a Bug issue for each input in DIR, or comment on the open issue with the same title;
#                           with --dry-run, only print the issues and comments
#
# Needs cargo (stable is enough: the target runs without a sanitizer, as the parser has no unsafe code), cargo-fuzz and
# a C++ compiler for libFuzzer (c++, or clang++ if there's no c++); lint needs clippy and cargo-deny. report needs only
# gh (logged in, or GH_TOKEN set); it targets the repository of the current checkout, or GH_REPO=owner/name if set.

set -euo pipefail

usage() {
  echo "usage: $0 lint | run [SECONDS] | minimize | reproduce FILE | triage DIR | report DIR [--dry-run]" >&2
  exit 2
}

cd "$(dirname "$0")/.."

readonly target=sway_config
readonly corpus=fuzz/corpus/$target
readonly artifacts=fuzz/artifacts/$target
# Larger than the sample configs bundled with their includes; big enough to reach the parser's limits.
readonly max_len=65536
readonly timeout=10
readonly rss_limit_mb=2048
readonly title_suffix="in the sway config parser"
readonly title_pattern="^Fuzzer: [A-Za-z0-9 _./:-]+ $title_suffix\$"

need() {
  for tool in "$@"; do
    if ! command -v "$tool" >/dev/null; then
      echo "error: needs $tool" >&2
      exit 1
    fi
  done
}

# What cargo fuzz needs to build the target: no sanitizer, and the target of the Rust toolchain (cargo-fuzz's release
# binary is built for musl and would otherwise build for that).
setup_build() {
  need cargo cargo-fuzz
  if ! command -v "${CXX:-c++}" >/dev/null && command -v clang++ >/dev/null; then
    export CXX=clang++
  fi
  host=$(rustc -vV | sed -n 's/^host: //p')
  build=(--sanitizer none --target "$host")
  libfuzzer=(-dict=fuzz/sway_config.dict "-max_len=$max_len" "-timeout=$timeout" "-rss_limit_mb=$rss_limit_mb")
}

# Writes the sample configs of ci/integration/ as seeds to $1: each one as the main config, followed by every other
# sample config as a file it can include, at its path relative to ci/integration/.
bundle_samples() {
  local out=$1 main other
  local -a configs
  mapfile -t configs < <(git ls-files -- 'ci/integration/*.conf')
  rm -rf "$out"
  mkdir -p "$out"
  for main in "${configs[@]}"; do
    {
      cat "$main"
      for other in "${configs[@]}"; do
        if [[ $other != "$main" ]]; then
          printf '\n#=> %s\n' "${other#ci/integration/}"
          cat "$other"
        fi
      done
    } >"$out/$(tr / _ <<<"${main#ci/integration/}")"
  done
}

lint() {
  need cargo cargo-deny
  setup_build
  cargo clippy --manifest-path fuzz/Cargo.toml --locked --target "$host" -- -D warnings
  cargo deny --manifest-path fuzz/Cargo.toml --locked check
}

run() {
  local seconds=${1:-60}
  if [[ ! $seconds =~ ^[0-9]+$ ]] || ((seconds < 1 || seconds > 21600)); then
    echo "error: SECONDS must be a number from 1 to 21600" >&2
    exit 2
  fi
  setup_build
  local samples=fuzz/target/samples
  bundle_samples "$samples"
  mkdir -p "$corpus" "$artifacts"
  # New inputs go to the first directory, the corpus; the seeds stay as they are.
  cargo fuzz run "${build[@]}" "$target" "$corpus" fuzz/seeds "$samples" -- "${libfuzzer[@]}" \
    "-max_total_time=$seconds" "-artifact_prefix=$artifacts/" -print_final_stats=1
}

minimize() {
  setup_build
  mkdir -p "$corpus"
  cargo fuzz cmin "${build[@]}" "$target" "$corpus" -- "${libfuzzer[@]}"
}

reproduce() {
  setup_build
  cargo fuzz run "${build[@]}" "$target" "$1" -- "${libfuzzer[@]}"
}

# Where the target panicked, as `crates/core/src/…:LINE:COLUMN`, from the output of a run on $1. Only characters of
# a path are kept, so nothing of the input itself gets into an issue title.
panic_location() {
  local binary=fuzz/target/$host/release/$target location
  location=$("$binary" "${libfuzzer[@]}" -runs=1 "$1" 2>&1 |
    grep -oE 'panicked at [A-Za-z0-9_./-]+:[0-9]+:[0-9]+' | head -n 1 | sed -E 's/^panicked at //; s#^.*/(crates|fuzz)/#\1/#' ||
    true)
  if [[ $location =~ ^[A-Za-z0-9_./-]+:[0-9]+:[0-9]+$ ]]; then
    echo "$location"
  fi
}

triage() {
  local out=$1 input name kind minimized location title candidate size smallest count=0
  setup_build
  cargo fuzz build "${build[@]}" "$target"
  shopt -s nullglob
  mkdir -p "$out"
  for input in "$artifacts"/crash-* "$artifacts"/timeout-* "$artifacts"/oom-* "$artifacts"/leak-*; do
    name=${input##*/}
    kind=${name%%-*}
    minimized=$input
    if [[ $kind == crash ]]; then
      location=$(panic_location "$input")
      # tmin writes each smaller input that still crashes to minimized-from-SHA1 (of the input before). The smallest
      # that panics at the same place is used, as a step can end on another crash. With -dict, tmin's steps write
      # empty inputs, so it runs without.
      rm -f "$artifacts"/minimized-from-*
      cargo fuzz tmin "${build[@]}" "$target" "$input" --runs 1000 -- "-timeout=$timeout" \
        "-rss_limit_mb=$rss_limit_mb" || true
      smallest=$(wc -c <"$input")
      for candidate in "$artifacts"/minimized-from-*; do
        size=$(wc -c <"$candidate")
        if [[ -n $location ]] && ((size > 0 && size < smallest)) &&
          [[ $(panic_location "$candidate") == "$location" ]]; then
          minimized=$candidate
          smallest=$size
        fi
      done
      if [[ -n $location ]]; then
        title="Fuzzer: panic at $location $title_suffix"
      else
        title="Fuzzer: crash $title_suffix"
      fi
    elif [[ $kind == timeout ]]; then
      title="Fuzzer: timeout $title_suffix"
    elif [[ $kind == oom ]]; then
      title="Fuzzer: out of memory $title_suffix"
    else
      title="Fuzzer: memory leak $title_suffix"
    fi
    count=$((count + 1))
    mkdir -p "$out/$count"
    cp "$minimized" "$out/$count/input"
    printf '%s\n' "$kind" >"$out/$count/kind"
    printf '%s\n' "$title" >"$out/$count/title"
    echo "$name: $title"
  done
  if ((count == 0)); then
    echo "No inputs in $artifacts/"
  elif [[ -n ${GITHUB_OUTPUT-} ]]; then
    # In a workflow, tells the next job that there is something to report.
    echo "found=true" >>"$GITHUB_OUTPUT"
  fi
}

# What the parser did, for the issue.
describe() {
  case $1 in
    crash) echo "panics" ;;
    timeout) echo "takes longer than $timeout s" ;;
    oom) echo "takes more than $rss_limit_mb MB of memory" ;;
    *) echo "leaks memory" ;;
  esac
}

# The input in $1 as a block to paste into a shell, or a note if it's too big for an issue.
reproducer() {
  local size encoded
  size=$(wc -c <"$1")
  encoded=$(base64 --wrap 76 "$1")
  if ((${#encoded} > 40000)); then
    echo "The input ($size bytes) is too big for an issue; it is in the \`fuzz-inputs\` artifact of the run."
    return
  fi
  cat <<TEXT
Save the input ($size bytes) as \`input.conf\`:

\`\`\`sh
base64 --decode >input.conf <<'INPUT'
$encoded
INPUT
\`\`\`
TEXT
  if ((size <= 1024)); then
    cat <<TEXT

As bytes:

\`\`\`text
$(od -A x -t x1 -v "$1")
\`\`\`
TEXT
  fi
}

report() {
  local dir=$1 dry_run=${2:-} report kind title existing number found_title run
  if [[ -n $dry_run && $dry_run != --dry-run ]]; then
    usage
  fi
  run=${RUN_URL:-a local run}
  local body
  # The titles of the issues opened in this run, as hashes so they compare as words.
  local -a open=() created=()
  if [[ -z $dry_run ]] || gh auth status >/dev/null 2>&1; then
    need gh
    mapfile -t open < <(gh issue list --state open --limit 500 --json number,title --jq '.[] | "\(.number)\t\(.title)"')
  fi
  shopt -s nullglob
  for report in "$dir"/*/; do
    kind=$(head -n 1 "$report/kind")
    title=$(head -n 1 "$report/title")
    # The title was made by triage; don't trust a file that says otherwise.
    if [[ ! $title =~ $title_pattern || ! $kind =~ ^(crash|timeout|oom|leak)$ ]]; then
      echo "error: unexpected report in $report" >&2
      exit 1
    fi
    if [[ " ${created[*]} " == *" $(sha1sum <<<"$title" | cut -c1-40) "* ]]; then
      echo "Skipped another input for \"$title\", opened in this run"
      continue
    fi
    existing=""
    for found_title in "${open[@]}"; do
      number=${found_title%%$'\t'*}
      if [[ ${found_title#*$'\t'} == "$title" ]]; then
        existing=$number
        break
      fi
    done

    if [[ -n $existing ]]; then
      body=$(
        cat <<BODY
The scheduled fuzzer found another input on which the parser $(describe "$kind"), in $run.

$(reproducer "$report/input")
BODY
      )
      if [[ -n $dry_run ]]; then
        printf '==> Would comment on #%s (%s):\n%s\n\n' "$existing" "$title" "$body"
      else
        gh issue comment "$existing" --body-file - <<<"$body"
      fi
      continue
    fi

    body=$(
      cat <<BODY
### Is there an existing issue for this?

- [X] I have searched the existing issues

### What happened?

The scheduled fuzzer found an input on which the sway config parser $(describe "$kind"), in $run.

### Steps to reproduce

$(reproducer "$report/input")

Then run \`scripts/fuzz.sh reproduce input.conf\`. The input is a config: a line \`#=> PATH\` starts a file it can include (see \`fuzz/fuzz_targets/sway_config.rs\`).

### What did you expect?

The parser reads any config to the end or up to one of its limits, without a panic, in less than $timeout s and $rss_limit_mb MB.

### Sway Things version

Commit ${GITHUB_SHA:-$(git rev-parse HEAD)}

### Compositor

sway

### Compositor version

Not needed: the parser reads the config without a compositor.

### Distribution

The runner of the workflow run.

### Output of \`sway-things doctor\`

Not needed: the fuzzer runs the parser on files in memory.

### Anything else?

Opened by the scheduled fuzzing workflow. Further inputs with the same title are added as comments while this issue is open.
BODY
    )
    if [[ -n $dry_run ]]; then
      printf '==> Would open "%s":\n%s\n\n' "$title" "$body"
    elif ! gh issue create --title "$title" --type Bug --label area:core --body-file - <<<"$body"; then
      echo "warning: can't open the issue with the type Bug, opening it without" >&2
      gh issue create --title "$title" --label area:core --body-file - <<<"$body"
    fi
    created+=("$(sha1sum <<<"$title" | cut -c1-40)")
  done
}

case "${1-}" in
  lint) lint ;;
  run) run "${2-}" ;;
  minimize) minimize ;;
  reproduce) reproduce "${2:-$(usage)}" ;;
  triage) triage "${2:-$(usage)}" ;;
  report) report "${2:-$(usage)}" "${3-}" ;;
  *) usage ;;
esac
