#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Tests scripts/install-from-obs.sh, the script behind https://sway-things.org/install.
#
# Without options, offline: in a clean Debian container (so nothing of this machine leaks in), the script runs with
# --dry-run under dash, bash and busybox sh against an os-release of every supported system and must print the right
# repository and commands; unsupported systems (another distribution, a derivative, another architecture, an old
# release) must fail without changing anything; and the fingerprint it computes from the project's public key must be
# the pinned one.
#
# With --container NAME (opensuse-tumbleweed, fedora, debian-13, ubuntu-26.04 or all), online: in a clean container of
# that distribution, the script installs the package from the published OBS repository, piped into sh on a terminal
# (script(1)) and answering its questions and the package manager's there, `sway-things
# --version` must run, a second run must keep the repository, and --uninstall must remove the package, the repository
# and the key. Run by .github/workflows/packages.yml, since it needs the network and the published packages.
#
# Check level: container (see scripts/run-checks.sh)
#
# Usage: scripts/check-install-from-obs.sh [--container NAME|all]
#
# Needs docker or podman (CONTAINER_ENGINE overrides the choice).

set -euo pipefail

# shellcheck source=scripts/lib.sh
source "$(dirname "$0")/lib.sh"
help_option "$@"

cd "$(dirname "$0")/.."

script=scripts/install-from-obs.sh
fingerprint=$(sed -n 's/^KEY_FINGERPRINT=//p' "$script")

engine=$(container_engine)

# Clean images of the distributions, pinned to a digest in ci/images/Dockerfile, where Dependabot updates them.
declare -A images=()
for name in opensuse-tumbleweed fedora debian-13 ubuntu-26.04; do
  images[$name]=$(sed -n "s/^FROM \([^ ]*\) AS $name\$/\1/p" ci/images/Dockerfile)
  if [[ -z ${images[$name]} ]]; then
    echo "error: ci/images/Dockerfile has no image $name" >&2
    exit 1
  fi
done

# Inside the offline container: one dry run per system and shell. EXPECTED is the pinned fingerprint.
# shellcheck disable=SC2153 # EXPECTED comes from the container's environment
offline() {
  local failures=0
  local dir shell output status
  dir=$(mktemp -d)
  # check NAME ARCH EXPECTED_EXIT PATTERN… : the dry run on the os-release $dir/NAME must exit as expected and print
  # every pattern (grep -E).
  check() {
    local name=$1 arch=$2 expected=$3
    shift 3
    local shell output status
    for shell in dash bash 'busybox sh'; do
      status=0
      output=$(SWAY_THINGS_OS_RELEASE=$dir/$name SWAY_THINGS_ARCH=$arch $shell /work/install.sh --dry-run --yes 2>&1) ||
        status=$?
      local pattern ok=1
      [[ $status == "$expected" ]] || ok=0
      for pattern in "$@"; do
        grep -qE -- "$pattern" <<<"$output" || ok=0
      done
      if ((ok)); then
        echo "ok: $name ($arch) under $shell"
      else
        echo "FAILED: $name ($arch) under $shell exited $status (expected $expected); output:" >&2
        printf '%s\n' "$output" | sed 's/^/  | /' >&2
        failures=$((failures + 1))
      fi
    done
  }
  printf 'ID="opensuse-tumbleweed"\nVERSION_ID="20261118"\nPRETTY_NAME="openSUSE Tumbleweed"\n' >"$dir/tumbleweed"
  printf 'ID=fedora\nVERSION_ID=44\nPRETTY_NAME="Fedora Linux 44"\n' >"$dir/fedora"
  printf 'ID=fedora\nVERSION_ID=45\nREDHAT_SUPPORT_PRODUCT_VERSION=rawhide\nPRETTY_NAME="Fedora Linux 45 (Rawhide)"\n' \
    >"$dir/rawhide"
  printf 'ID=debian\nVERSION_ID="13"\nPRETTY_NAME="Debian GNU/Linux 13 (trixie)"\n' >"$dir/debian"
  printf 'ID=debian\nPRETTY_NAME="Debian GNU/Linux forky/sid"\n' >"$dir/sid"
  printf 'ID=debian\nVERSION_ID="12"\nPRETTY_NAME="Debian GNU/Linux 12 (bookworm)"\n' >"$dir/bookworm"
  printf 'ID=ubuntu\nID_LIKE=debian\nVERSION_ID="26.04"\nPRETTY_NAME="Ubuntu 26.04 LTS"\n' >"$dir/ubuntu"
  printf 'ID=linuxmint\nID_LIKE="ubuntu debian"\nVERSION_ID="22"\nPRETTY_NAME="Linux Mint 22"\n' >"$dir/mint"
  printf 'ID=arch\nPRETTY_NAME="Arch Linux"\n' >"$dir/arch"

  local obs='https://download\.opensuse\.org/repositories/home:/mowi12:/sway-things'
  check tumbleweed x86_64 0 "from $obs/openSUSE_Tumbleweed" "openSUSE_Tumbleweed/repodata/repomd\.xml\.key" \
    '^\+ (sudo )?rpm --import ' "^\\+ (sudo )?zypper --non-interactive addrepo --refresh $obs/openSUSE_Tumbleweed/" \
    '^\+ (sudo )?zypper --non-interactive install sway-things$' 'Dry run: nothing was changed'
  check tumbleweed aarch64 1 'only on x86_64' 'Nothing was changed'
  check fedora x86_64 0 "from $obs/Fedora_44" \
    '^\+ write /etc/yum\.repos\.d/home:mowi12:sway-things\.repo:' '^    \[home_mowi12_sway-things\]$' \
    "^    baseurl=$obs/Fedora_44/$" "^    gpgkey=$obs/Fedora_44/repodata/repomd\\.xml\\.key$" '^    gpgcheck=1$' \
    '^\+ (sudo )?dnf -y install sway-things$'
  check fedora aarch64 0 "Fedora Linux 44 \\(aarch64\\)"
  check rawhide x86_64 0 "from $obs/Fedora_Rawhide"
  check debian x86_64 0 "from $obs/Debian_13" "$obs/Debian_13/Release\\.key" \
    '^\+ write /etc/apt/sources\.list\.d/sway-things\.sources:' "^    URIs: $obs/Debian_13/$" '^    Suites: /$' \
    '^    Signed-By: /etc/apt/keyrings/sway-things\.asc$' '^\+ (sudo )?apt-get -y install sway-things$'
  check sid aarch64 0 "from $obs/Debian_Unstable"
  check bookworm x86_64 1 'Debian 12 has no packages'
  check ubuntu x86_64 0 "from $obs/xUbuntu_26\\.04" "^    URIs: $obs/xUbuntu_26\\.04/$"
  check mint x86_64 1 "Linux Mint 22 isn't one of the distributions" 'Nothing was changed'
  check arch x86_64 1 "Arch Linux isn't one of the distributions"
  check debian riscv64 1 'not riscv64'
  check debian x86_64 0 'Neither sway nor SwayFX is installed'

  # Without a terminal and without --yes, it asks nothing and stops, and says why (the container has no terminal).
  for shell in dash bash 'busybox sh'; do
    status=0
    output=$(SWAY_THINGS_OS_RELEASE=$dir/debian $shell /work/install.sh </dev/null 2>&1) || status=$?
    if ((status == 0)) || ! grep -q 'no terminal to ask on' <<<"$output"; then
      echo "FAILED: without a terminal and --yes under $shell, it exited $status and said:" >&2
      printf '%s\n' "$output" | sed 's/^/  | /' >&2
      failures=$((failures + 1))
    else
      echo "ok: no terminal, no --yes: stops and says why under $shell"
    fi
  done
  if dash /work/install.sh --bogus >/dev/null 2>&1; then
    echo "FAILED: an unknown option was accepted" >&2
    failures=$((failures + 1))
  fi

  # The fingerprint the script computes from the public key, without gpg, must be the pinned one.
  local found
  cp /work/key.asc "$dir/key.asc"
  # The wrapper reads its argument when it runs, not now.
  # shellcheck disable=SC2016
  printf '%s\n' '. /work/functions.sh' 'fingerprint "$1"' >"$dir/fingerprint.sh"
  for shell in dash bash 'busybox sh'; do
    found=$($shell "$dir/fingerprint.sh" "$dir/key.asc")
    if [[ $found == "$EXPECTED" ]]; then
      echo "ok: fingerprint under $shell"
    else
      echo "FAILED: fingerprint under $shell is '$found', not $EXPECTED" >&2
      failures=$((failures + 1))
    fi
  done
  # A key file with a second key, in a second block, in the same one, or as a subkey, is refused: rpm and apt would
  # trust both.
  cat "$dir/key.asc" "$dir/key.asc" >"$dir/two-blocks.asc"
  sed -e '1,/^$/d' -e '/^=/,$d' -e '/^-----END/,$d' "$dir/key.asc" | tr -d '\r\n' | base64 -d >"$dir/key.bin"
  {
    printf '%s\n\n' '-----BEGIN PGP PUBLIC KEY BLOCK-----'
    cat "$dir/key.bin" "$dir/key.bin" | base64
    printf '%s\n' '-----END PGP PUBLIC KEY BLOCK-----'
  } >"$dir/two-keys.asc"
  # A subkey after the primary key: its packet is the primary key's body with the tag of a public subkey (14, in the
  # old format with a two-byte length, as the key has it), before the user ID.
  {
    printf '%s\n\n' '-----BEGIN PGP PUBLIC KEY BLOCK-----'
    {
      head -c 528 "$dir/key.bin"
      printf '\271'
      head -c 528 "$dir/key.bin" | tail -c +2
      tail -c +529 "$dir/key.bin"
    } | base64
    printf '%s\n' '-----END PGP PUBLIC KEY BLOCK-----'
  } >"$dir/subkey.asc"
  local file
  for file in two-blocks two-keys subkey; do
    if dash "$dir/fingerprint.sh" "$dir/$file.asc" >/dev/null 2>&1; then
      echo "FAILED: a key file with a second key ($file) was accepted" >&2
      failures=$((failures + 1))
    else
      echo "ok: a key file with a second key ($file) is refused"
    fi
  done
  ((failures == 0))
}

run_offline() {
  local work
  work=$(mktemp -d)
  trap 'rm -rf "$work"' RETURN
  cp "$script" "$work/install.sh"
  # The script's functions without its last line, which runs it.
  sed '$d' "$script" >"$work/functions.sh"
  cp ci/install/home_mowi12_sway-things.asc "$work/key.asc"
  # The image has dash and bash; busybox comes from Debian's archive. The dry runs themselves download nothing.
  # What runs in the container is a file of its own, not a string for `bash -c`.
  {
    echo 'set -euo pipefail'
    echo 'apt-get -qq update && apt-get -qq install -y busybox >/dev/null'
    declare -f offline
    echo offline
  } >"$work/offline.sh"
  "$engine" run --rm -e EXPECTED="$fingerprint" -e DEBIAN_FRONTEND=noninteractive -v "$work:/work:ro" \
    "${images[debian-13]}" bash /work/offline.sh
}

# Inside an online container: install, run, install again, uninstall.
# shellcheck disable=SC2153 # DISTRO and EXPECTED come from the container's environment
online() {
  set -euo pipefail
  # curl, and script(1) for a terminal.
  case $DISTRO in
    debian-* | ubuntu-*) apt-get -qq update && apt-get -qq install -y curl ca-certificates bsdutils >/dev/null ;;
    opensuse-*) zypper --non-interactive --quiet install curl util-linux >/dev/null ;;
    fedora) dnf -q -y install curl util-linux-script >/dev/null ;;
  esac
  # As the README has it: piped into sh, without --yes, on a terminal. Someone answers "y" on the terminal every few
  # seconds (zypper drops what was typed before it asks): to installing sway along, and to the package manager.
  # It stops quietly once script(1) is gone, so only script's status counts.
  (
    trap '' PIPE
    while sleep 3; do echo y 2>/dev/null || exit 0; done
  ) | SHELL=/work/piped.sh script -qe /dev/null
  sway-things --version
  # Again: the repository stays as it is, the package is kept up to date.
  local sources=/etc/apt/sources.list.d/sway-things.sources key=/etc/apt/keyrings/sway-things.asc before
  before=$(cat "$sources" "$key" 2>/dev/null || true)
  sh /work/install.sh --yes | tee /tmp/again
  case $DISTRO in
    debian-* | ubuntu-*)
      # apt's two files are written again: they must be the same as before.
      if [[ $(cat "$sources" "$key") != "$before" ]]; then
        echo "FAILED: the second run changed $sources or $key" >&2
        exit 1
      fi
      ;;
    *)
      if ! grep -q 'is there already' /tmp/again; then
        echo "FAILED: the second run didn't keep the repository" >&2
        exit 1
      fi
      ;;
  esac
  sh /work/install.sh --yes --uninstall
  # bash remembers where it found sway-things.
  hash -r
  if command -v sway-things >/dev/null; then
    echo "FAILED: sway-things is still installed" >&2
    exit 1
  fi
  for file in /etc/apt/sources.list.d/sway-things.sources /etc/apt/keyrings/sway-things.asc \
    /etc/yum.repos.d/home:mowi12:sway-things.repo /etc/zypp/repos.d/home_mowi12_sway-things.repo; do
    if [[ -e $file ]]; then
      echo "FAILED: $file is still there" >&2
      exit 1
    fi
  done
  # rpm 4 names the key after the last eight digits of its fingerprint, rpm 6 after all of them.
  if command -v rpm >/dev/null &&
    { rpm -q gpg-pubkey 2>/dev/null | grep -qi -e "^gpg-pubkey-${EXPECTED: -8}-" -e "^gpg-pubkey-$EXPECTED-" ||
      rpmkeys --list 2>/dev/null | grep -qi "^$EXPECTED"; }; then
    echo "FAILED: the signing key is still in rpm's keyring" >&2
    exit 1
  fi
  echo "ok: $DISTRO installs, runs, installs again and uninstalls"
}

run_online() {
  local name=$1
  [[ -n ${images[$name]:-} ]] || {
    echo "error: unknown container $name" >&2
    exit 2
  }
  echo "==> $name"
  local work
  work=$(mktemp -d)
  trap 'rm -rf "$work"' RETURN
  cp "$script" "$work/install.sh"
  # As the README has it, piped into sh. script(1) runs $SHELL when it is given no command (the util-linux of Debian
  # 13 has no `-- command`), so this is the program it runs, not a `-c` string.
  printf '%s\n' '#!/bin/sh' 'cat /work/install.sh | sh' >"$work/piped.sh"
  chmod +x "$work/piped.sh"
  {
    declare -f online
    echo online
  } >"$work/online.sh"
  "$engine" run --rm -e DISTRO="$name" -e EXPECTED="$fingerprint" -v "$work:/work:ro" "${images[$name]}" \
    bash /work/online.sh
}

case ${1:-} in
  '') run_offline ;;
  --container)
    [[ -n ${2:-} ]] || {
      echo "usage: $0 [--container NAME|all]" >&2
      exit 2
    }
    if [[ $2 == all ]]; then
      for name in opensuse-tumbleweed fedora debian-13 ubuntu-26.04; do
        run_online "$name"
      done
    else
      run_online "$2"
    fi
    ;;
  *)
    echo "usage: $0 [--container NAME|all]" >&2
    exit 2
    ;;
esac
