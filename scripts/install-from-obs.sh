#!/bin/sh
# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Installs Sway Things from the openSUSE Build Service on openSUSE Tumbleweed, Fedora, Debian and Ubuntu: finds the
# system, adds the repository of the OBS project home:mowi12:sway-things with its signing key (whose fingerprint is
# pinned below and checked before the key is trusted) and installs the package. Served at https://sway-things.org/install
# (and /install.sh), so that one command installs the app:
#
#   curl -fsSL https://sway-things.org/install | sh
#
# Usage: sh install-from-obs.sh [--yes] [--dry-run] [--uninstall]
#   (piped: curl -fsSL https://sway-things.org/install | sh -s -- --dry-run)
#
#   --yes        answer every question with yes, ours and the package manager's; needed without a terminal
#   --dry-run    print the commands that would run, and run none of them
#   --uninstall  remove the package, the repository entry and the key, and nothing else
#
# Plain POSIX sh, so it runs piped into sh, bash, dash, zsh or busybox sh. Everything is in functions and runs from the
# last line, so a download cut off halfway runs nothing. scripts/check-install-from-obs.sh tests it.
#
# For tests: SWAY_THINGS_OS_RELEASE names another os-release file and SWAY_THINGS_ARCH another architecture.

# The OBS project and the fingerprint of its signing key (OBS signs all its repositories with the one key of a project).
OBS_PROJECT=home:mowi12:sway-things
OBS_BASE=https://download.opensuse.org/repositories/home:/mowi12:/sway-things
KEY_FINGERPRINT=CB6F077B87FAC932145EE0096C2A5AE8E919FAB8
PACKAGE=sway-things
APT_KEY=/etc/apt/keyrings/sway-things.asc
APT_SOURCE=/etc/apt/sources.list.d/sway-things.sources
# The files the README told Debian and Ubuntu users to make by hand before this script existed.
APT_OLD_KEY=/etc/apt/keyrings/sway-things.gpg
APT_OLD_SOURCE=/etc/apt/sources.list.d/sway-things.list
# zypper and dnf name the repository after the alias in its .repo file.
RPM_ALIAS=home_mowi12_sway-things
DNF_REPO_FILE=/etc/yum.repos.d/home:mowi12:sway-things.repo

say() {
  printf '%s\n' "$*"
}

warn() {
  printf 'warning: %s\n' "$*" >&2
}

die() {
  printf 'error: %s\n' "$*" >&2
  exit 1
}

usage() {
  say "usage: install-from-obs.sh [--yes] [--dry-run] [--uninstall]"
  say "  piped: curl -fsSL https://sway-things.org/install | sh -s -- [options]"
}

# Runs a command as root, or prints it with --dry-run. Without --yes, its input is the terminal, since standard input
# is the script itself when piped: the package manager asks there before it installs anything.
as_root() {
  if [ "$dry_run" = 1 ]; then
    say "+ $*"
    return 0
  fi
  if [ "$yes" = 1 ]; then
    as_root_from "$@" </dev/null
  else
    as_root_from "$@" </dev/tty
  fi || die "this failed: $*"
}

as_root_from() {
  if [ -n "$root" ]; then
    "$root" "$@"
  else
    "$@"
  fi
}

# Writes standard input to the file $1 as root, readable by everyone, or prints that with --dry-run.
write_root() {
  if [ "$dry_run" = 1 ]; then
    say "+ write $1:"
    sed 's/^/    /'
    return 0
  fi
  cat >"$workdir/file"
  as_root install -m 644 "$workdir/file" "$1"
}

# Asks a yes/no question on the terminal (standard input is the script itself when piped). --yes answers it.
confirm() {
  if [ "$yes" = 1 ]; then
    say "$1 [y/N] yes (--yes)"
    return 0
  fi
  if [ "$dry_run" = 1 ]; then
    say "$1 [y/N] (asked; --dry-run goes on)"
    return 0
  fi
  printf '%s [y/N] ' "$1" >/dev/tty
  read -r answer </dev/tty || answer=
  case $answer in
    [yY] | [yY][eE][sS]) return 0 ;;
    *) return 1 ;;
  esac
}

# Downloads the URL $1 into the file $2, over HTTPS only.
download() {
  case $1 in
    https://*) ;;
    *) die "refusing to download over anything but HTTPS: $1" ;;
  esac
  if command -v curl >/dev/null 2>&1; then
    curl --proto '=https' --tlsv1.2 -fsSL -o "$2" "$1"
  elif command -v wget >/dev/null 2>&1; then
    wget --https-only -q -O "$2" "$1"
  else
    die "needs curl or wget to download $1"
  fi
}

# Whether the URL $1 exists (an OBS repository for this release).
exists() {
  if command -v curl >/dev/null 2>&1; then
    curl --proto '=https' --tlsv1.2 -fsSIL -o /dev/null "$1"
  elif command -v wget >/dev/null 2>&1; then
    wget --https-only -q --spider "$1"
  else
    die "needs curl or wget to download from $OBS_BASE"
  fi
}

# Prints the value of the key $1 of the os-release file, without quotes.
os_value() {
  sed -n "s/^$1=//p" "$os_release" | sed -e 's/^"\(.*\)"$/\1/' -e "s/^'\(.*\)'\$/\1/" | head -n 1
}

# Reads the header of the OpenPGP packet at byte $2 of the file $1 into tag, length and header (its size), or fails on
# a header it doesn't know (partial and indeterminate lengths, which keys don't use).
packet_header() {
  # shellcheck disable=SC2046 # the bytes are split on purpose
  set -- $(od -An -tu1 -j "$2" -N6 "$1")
  [ $# -ge 2 ] || return 1
  if [ $(($1 & 192)) -eq 192 ]; then
    # New packet format: the tag in the low six bits, then a one-, two- or five-byte length.
    tag=$(($1 & 63))
    if [ "$2" -lt 192 ]; then
      length=$2 header=2
    elif [ "$2" -lt 224 ] && [ $# -ge 3 ]; then
      length=$((($2 - 192) * 256 + $3 + 192)) header=3
    elif [ "$2" -eq 255 ] && [ $# -ge 6 ]; then
      length=$((($3 << 24) | ($4 << 16) | ($5 << 8) | $6)) header=6
    else
      return 1
    fi
  elif [ $(($1 & 128)) -eq 128 ]; then
    # Old packet format: the tag in bits 2 to 5, the length type in bits 0 and 1.
    tag=$((($1 >> 2) & 15))
    case $(($1 & 3)) in
      0) length=$2 header=2 ;;
      1) [ $# -ge 3 ] || return 1; length=$(($2 * 256 + $3)) header=3 ;;
      2) [ $# -ge 5 ] || return 1; length=$((($2 << 24) | ($3 << 16) | ($4 << 8) | $5)) header=5 ;;
      *) return 1 ;;
    esac
  else
    return 1
  fi
}

# Prints the fingerprint of the one public key in the ASCII-armored OpenPGP key file $1 (a version 4 key: the SHA-1 of
# 0x99, the two-byte length and the body of the public-key packet), in capital hex. Needs only base64, od, sha1sum, wc,
# head and tail, so no gpg has to be installed. Fails on anything else, and on a file with more than one key or with a
# subkey: rpm and apt would trust every key in it, and only the primary one is checked.
fingerprint() {
  [ "$(grep -c '^-----BEGIN PGP' "$1")" = 1 ] || return 1
  binary=$1.bin
  sed -e '1,/^$/d' -e '/^=/,$d' -e '/^-----END/,$d' "$1" | tr -d '\r\n' | base64 -d >"$binary" 2>/dev/null ||
    return 1
  size=$(wc -c <"$binary" | tr -d ' ')
  # Walk every packet: exactly one primary key (tag 6), the first; user IDs and signatures may follow. No subkey (14):
  # only the primary key is hashed, and rpm and apt would trust a subkey too. The OBS key has none.
  offset=0 keys=0
  while [ "$offset" -lt "$size" ]; do
    packet_header "$binary" "$offset" || return 1
    case $tag in
      6)
        [ "$offset" -eq 0 ] || return 1
        keys=$((keys + 1)) key_length=$length key_header=$header
        ;;
      # A secret key has no business in a file of public keys, nor has a subkey here.
      5 | 7 | 14) return 1 ;;
    esac
    offset=$((offset + header + length))
  done
  [ "$offset" -eq "$size" ] && [ "$keys" -eq 1 ] && [ "$key_length" -lt 65536 ] || return 1
  key_version=$(tail -c +$((key_header + 1)) "$binary" | od -An -tu1 -N1 | tr -d ' ')
  [ "$key_version" = 4 ] || return 1
  {
    # shellcheck disable=SC2059 # the format is built from octal escapes on purpose
    printf "\\231\\$(printf '%o' $((key_length >> 8)))\\$(printf '%o' $((key_length & 255)))"
    tail -c +$((key_header + 1)) "$binary" | head -c "$key_length"
  } | sha1sum | cut -d ' ' -f 1 | tr 'a-f' 'A-F'
}

# Downloads the repository's signing key into $workdir/key.asc and checks its fingerprint against the pinned one.
fetch_key() {
  key=$workdir/key.asc
  case $manager in
    apt) key_url=$repo/Release.key ;;
    *) key_url=$repo/repodata/repomd.xml.key ;;
  esac
  if [ "$dry_run" = 1 ]; then
    say "+ download $key_url and check that its fingerprint is $KEY_FINGERPRINT"
    return 0
  fi
  download "$key_url" "$key" || die "can't download the signing key from $key_url"
  found=$(fingerprint "$key") || die "can't read the signing key from $repo"
  if [ "$found" != "$KEY_FINGERPRINT" ]; then
    die "the signing key of $repo has the fingerprint $found, not $KEY_FINGERPRINT; nothing was changed. If the \
project has a new key, a new version of this script knows it: https://sway-things.org/install"
  fi
  say "The signing key's fingerprint is $KEY_FINGERPRINT, as expected."
}

# Finds the distribution, its OBS repository ($repo) and its package manager ($manager), or explains why not.
detect() {
  [ -r "$os_release" ] || die "can't read $os_release, so the distribution is unknown"
  id=$(os_value ID)
  version=$(os_value VERSION_ID)
  name=$(os_value PRETTY_NAME)
  [ -n "$name" ] || name=$id
  arch=${SWAY_THINGS_ARCH:-$(uname -m)}
  directory=
  case $id in
    opensuse-tumbleweed)
      directory=openSUSE_Tumbleweed manager=zypper
      [ "$arch" = x86_64 ] || unsupported "OBS builds Sway Things for openSUSE Tumbleweed only on x86_64, not $arch."
      ;;
    fedora)
      manager=dnf
      if [ "$(os_value REDHAT_SUPPORT_PRODUCT_VERSION)" = rawhide ]; then
        directory=Fedora_Rawhide
      else
        case $version in
          '' | *[!0-9]*) unsupported "Fedora $version has no packages." ;;
          *) directory=Fedora_$version ;;
        esac
      fi
      ;;
    debian)
      manager=apt
      case $version in
        13) directory=Debian_13 ;;
        # Testing and unstable have no VERSION_ID; the packages are built for unstable.
        '') directory=Debian_Unstable ;;
        *) unsupported "Debian $version has no packages; Debian 13 and unstable have." ;;
      esac
      ;;
    ubuntu)
      manager=apt
      case $version in
        26.04) directory=xUbuntu_26.04 ;;
        *) unsupported "Ubuntu $version has no packages; Ubuntu 26.04 has." ;;
      esac
      ;;
    *)
      # Derivatives (ID_LIKE) aren't mapped to their base: nobody has tested them.
      unsupported "$name isn't one of the distributions with packages."
      ;;
  esac
  case $arch in
    x86_64 | aarch64) ;;
    *) unsupported "OBS builds Sway Things only for x86_64 and aarch64, not $arch." ;;
  esac
  repo=$OBS_BASE/$directory
  if [ "$dry_run" != 1 ] && [ "$uninstall" != 1 ] && ! exists "$repo/"; then
    exists "$OBS_BASE/" || die "can't reach $OBS_BASE; is the network up? Nothing was changed."
    unsupported "$name has no packages yet (no $repo)."
  fi
}

unsupported() {
  printf 'error: %s\n' "$1" >&2
  cat >&2 <<'EOF'
Nothing was changed. Packages exist for openSUSE Tumbleweed, Fedora, Debian 13 and unstable, and Ubuntu 26.04.
Everywhere else, build Sway Things from source: https://github.com/mowi12/sway-things#from-source
EOF
  exit 1
}

# Picks how to become root: already root, sudo or doas.
find_root() {
  root=
  if [ "$(id -u)" = 0 ]; then
    return 0
  fi
  if command -v sudo >/dev/null 2>&1; then
    root=sudo
  elif command -v doas >/dev/null 2>&1; then
    root=doas
  elif [ "$dry_run" = 1 ]; then
    root=sudo
  else
    die "needs root: run it as root, or install sudo or doas"
  fi
}

# Says which compositor is installed; the package pulls in sway when neither sway nor SwayFX is there.
check_compositor() {
  if ! command -v sway >/dev/null 2>&1; then
    say "Neither sway nor SwayFX is installed. The package manager will install sway along with Sway Things."
    confirm "Install sway as well?" || die "nothing was changed. Install sway (1.10 or newer) or SwayFX (0.5 or newer) first."
    return 0
  fi
  found=$(sway --version 2>/dev/null | head -n 1)
  say "Found $found."
  number=$(printf '%s\n' "$found" | sed -n 's/.*version \([0-9][0-9]*\.[0-9][0-9]*\).*/\1/p')
  major=${number%%.*} minor=${number#*.}
  case $found in
    swayfx*) old=$([ "${major:-0}" -eq 0 ] && [ "${minor:-0}" -lt 5 ] && echo 1) need="SwayFX 0.5" ;;
    *) old=$({ [ "${major:-0}" -lt 1 ] || { [ "${major:-0}" -eq 1 ] && [ "${minor:-0}" -lt 10 ]; }; } && echo 1) need="sway 1.10" ;;
  esac
  if [ -n "$number" ] && [ "$old" = 1 ]; then
    warn "Sway Things needs $need or newer; the package manager may update or replace it."
    confirm "Go on?" || die "nothing was changed."
  fi
}

installed() {
  case $manager in
    apt) dpkg-query -W -f '${Status}' "$PACKAGE" 2>/dev/null | grep -q 'ok installed' ;;
    *) rpm -q "$PACKAGE" >/dev/null 2>&1 ;;
  esac
}

# The repository and key for zypper or dnf: the key goes into rpm's keyring, checked, so the package manager doesn't
# ask to trust it. dnf's .repo file is written here, as OBS has it, rather than with dnf config-manager, which comes
# only with dnf5-plugins (dnf 5) or dnf-plugins-core (dnf 4), and minimal installs have neither.
add_rpm_repo() {
  fetch_key
  as_root rpm --import "$key"
  case $manager in
    zypper)
      if [ "$dry_run" != 1 ] && zypper --non-interactive repos "$RPM_ALIAS" >/dev/null 2>&1; then
        say "The repository $RPM_ALIAS is there already."
      else
        as_root zypper --non-interactive addrepo --refresh "$repo/$OBS_PROJECT.repo"
      fi
      ;;
    dnf)
      if [ "$dry_run" != 1 ] && [ -f "$DNF_REPO_FILE" ]; then
        say "The repository $DNF_REPO_FILE is there already."
      else
        write_root "$DNF_REPO_FILE" <<EOF
[$RPM_ALIAS]
name=Sway Things ($directory)
type=rpm-md
baseurl=$repo/
gpgcheck=1
gpgkey=$repo/repodata/repomd.xml.key
enabled=1
EOF
      fi
      ;;
  esac
}

# The repository and key for apt, as deb822 with Signed-By; apt reads the armored key directly.
# Whether the .list file is the one the README used to have users make: one entry for this project's repository, with
# the README's key. apt would warn about the repository twice; a file that is anything else stays.
old_apt_entry() {
  [ -f "$APT_OLD_SOURCE" ] || return 1
  lines=$(grep -v -e '^[[:space:]]*$' -e '^[[:space:]]*#' "$APT_OLD_SOURCE")
  case $lines in
    *"
"*) return 1 ;;
    "deb [signed-by=$APT_OLD_KEY] $OBS_BASE/"*" /") return 0 ;;
    *) return 1 ;;
  esac
}

add_apt_repo() {
  fetch_key
  if old_apt_entry; then
    say "Replacing the repository entry made by hand from the README ($APT_OLD_SOURCE)."
    as_root rm -f "$APT_OLD_SOURCE" "$APT_OLD_KEY"
  fi
  as_root mkdir -p /etc/apt/keyrings
  if [ "$dry_run" = 1 ]; then
    say "+ install -m 644 key.asc $APT_KEY"
  else
    as_root install -m 644 "$key" "$APT_KEY"
  fi
  write_root "$APT_SOURCE" <<EOF
Types: deb
URIs: $repo/
Suites: /
Signed-By: $APT_KEY
EOF
}

install_package() {
  case $manager in
    zypper)
      as_root zypper --non-interactive refresh "$RPM_ALIAS"
      if installed; then
        as_root zypper $yes_flag update "$PACKAGE"
      else
        as_root zypper $yes_flag install "$PACKAGE"
      fi
      ;;
    dnf)
      if installed; then
        as_root dnf $yes_flag upgrade "$PACKAGE"
      else
        as_root dnf $yes_flag install "$PACKAGE"
      fi
      ;;
    apt)
      as_root apt-get update
      # install also upgrades an installed package.
      as_root apt-get $yes_flag install "$PACKAGE"
      ;;
  esac
}

remove() {
  case $manager in
    zypper)
      if installed; then as_root zypper $yes_flag remove "$PACKAGE"; fi
      if [ "$dry_run" = 1 ] || zypper --non-interactive repos "$RPM_ALIAS" >/dev/null 2>&1; then
        as_root zypper --non-interactive removerepo "$RPM_ALIAS"
      fi
      remove_rpm_key
      ;;
    dnf)
      if installed; then as_root dnf $yes_flag remove "$PACKAGE"; fi
      as_root rm -f "$DNF_REPO_FILE"
      remove_rpm_key
      ;;
    apt)
      if installed; then as_root apt-get $yes_flag remove "$PACKAGE"; fi
      as_root rm -f "$APT_SOURCE" "$APT_KEY"
      if old_apt_entry; then
        as_root rm -f "$APT_OLD_SOURCE" "$APT_OLD_KEY"
      fi
      ;;
  esac
}

# Removes the key from rpm's keyring. rpm 6 (Fedora 43 and newer) names an imported key by its full fingerprint and
# removes it with rpmkeys --delete; rpm 4 keeps it as the package gpg-pubkey-<last eight of the fingerprint>-<creation
# time>.
remove_rpm_key() {
  full=$(printf '%s' "$KEY_FINGERPRINT" | tr 'A-F' 'a-f')
  short=$(printf '%s' "$full" | tail -c 8)
  if [ "$dry_run" = 1 ]; then
    say "+ rpmkeys --delete $full (rpm 6), or rpm -e gpg-pubkey-$short-* (rpm 4)"
    return 0
  fi
  if rpmkeys --list 2>/dev/null | grep -qi "^${full}[[:space:]]"; then
    as_root rpmkeys --delete "$full"
  fi
  for key_package in $(rpm -q gpg-pubkey 2>/dev/null | grep -i "^gpg-pubkey-$short-"); do
    as_root rpm -e "$key_package"
  done
}

main() {
  yes=0 dry_run=0 uninstall=0
  for argument in "$@"; do
    case $argument in
      --yes | -y) yes=1 ;;
      --dry-run) dry_run=1 ;;
      --uninstall) uninstall=1 ;;
      --help | -h)
        usage
        exit 0
        ;;
      *)
        usage >&2
        exit 2
        ;;
    esac
  done
  # Without --yes, the questions (ours and the package manager's) go to the terminal; without one, nothing can answer.
  # In a subshell: a failed redirection on a special built-in such as : would end the shell itself, without a word.
  if [ "$yes" != 1 ] && [ "$dry_run" != 1 ] && ! (: </dev/tty) 2>/dev/null; then
    die "there is no terminal to ask on. Run it with --yes to answer every question with yes: … | sh -s -- --yes"
  fi
  os_release=${SWAY_THINGS_OS_RELEASE:-/etc/os-release}
  [ -r "$os_release" ] || os_release=/usr/lib/os-release
  detect
  find_root
  yes_flag=
  if [ "$yes" = 1 ]; then
    case $manager in
      zypper) yes_flag=--non-interactive ;;
      *) yes_flag=-y ;;
    esac
  fi
  workdir=$(mktemp -d) || die "can't make a temporary directory"
  trap 'rm -rf "$workdir"' EXIT
  trap 'exit 130' INT TERM

  if [ "$uninstall" = 1 ]; then
    say "Removing Sway Things, its repository and its key from $name."
    remove
    say "Done. Your settings in ~/.config/sway/sway-things are kept; delete them by hand if you like."
    return 0
  fi

  say "Installing Sway Things on $name ($arch) from $repo."
  check_compositor
  case $manager in
    apt) add_apt_repo ;;
    *) add_rpm_repo ;;
  esac
  install_package
  if [ "$dry_run" = 1 ]; then
    say "Dry run: nothing was changed."
    return 0
  fi
  version=$(sway-things --version 2>/dev/null) || die "the package was installed, but sway-things doesn't run"
  say ""
  say "Installed $version. Start it from your app launcher, or run sway-things-gui."
  say "Updates come with your system's updates. To remove it: curl -fsSL https://sway-things.org/install | sh -s -- --uninstall"
}

main "$@"
