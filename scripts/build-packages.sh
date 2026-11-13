#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Builds the distribution packages in clean containers, checks them with the distribution's linter and installs them.
# With --container NAME, builds ci/packages/NAME/Dockerfile and runs this script inside it with --build NAME; NAME=all
# runs every container. Not a check (no check- prefix): the builds take long, and making the tarball needs the network.
#
# opensuse-tumbleweed, fedora: the RPM packages for the openSUSE Build Service, from the release tarball and the specs
# that scripts/make-tarball.sh writes next to it, the very files OBS gets. They are made from HEAD of this checkout
# (committed changes only), or --tarball FILE takes those files from an earlier run. The image holds the build
# dependencies; the build itself runs without the network, as on OBS, and as an unprivileged user. rpmbuild runs the
# unit tests in %check. Then rpmlint checks the source and binary packages (errors fail, warnings are printed), and the
# package is installed and runs `sway-things --version` and `--help`. sway is installed for the dependency, but never
# started.
#
# debian-13, ubuntu-26.04: the Debian packages, the same way from the tarball and debian.tar.gz: dpkg-buildpackage runs
# the unit tests, lintian checks the package (errors fail, warnings are printed), the zsh completion must be in
# Debian's vendor-completions, and the package is installed and runs as above.
#
# arch-git: the AUR package sway-things-git (packaging/aur/sway-things-git/), in an Arch Linux container that holds
# only base-devel, as an unprivileged user. Compares .SRCINFO with `makepkg --printsrcinfo` (but for pkgver), builds
# with `makepkg -s` (which installs the declared dependencies, runs check() and fails on a missing build dependency),
# runs namcap on the PKGBUILD and on the packages (errors fail, warnings are printed), then installs the package and
# runs `sway-things --version` and `--help`. The PKGBUILD clones GitHub's main and fetches the crates, so this build
# needs the network and builds main, not the checkout. With --local, it builds the commit HEAD of this checkout instead
# (committed changes only), from a copy of the PKGBUILD whose source points at a clone of the checkout; .SRCINFO and
# namcap still check the PKGBUILD as it is.
#
# Usage: scripts/build-packages.sh --container NAME|all [--tarball FILE] [--local]
#
# Needs docker or podman (CONTAINER_ENGINE overrides the choice), and without --tarball what scripts/make-tarball.sh
# needs; git for --local.

set -euo pipefail

usage() {
  echo "usage: $0 --container NAME|all [--tarball FILE] [--local]" >&2
  exit 2
}

cd "$(dirname "$0")/.."

# Inside the container: build an RPM package from the tarball and the spec SPEC in /work/input. rpmlint checks the
# source and the main package, not the debug packages, and leaves out the FILTERs. Not every rpmlint fails with errors
# (openSUSE's exits 0), so its report decides.
build_rpm() {
  local spec=/work/input/$1
  shift
  local top=$HOME/rpmbuild
  rm -rf "$top"
  mkdir -p "$top/SOURCES"
  cp /work/input/sway-things-*.tar.xz "$top/SOURCES/"
  rpmbuild -ba --define "_topdir $top" "$spec"

  local filter
  for filter in "$@"; do
    echo "addFilter(\"$filter\")"
  done >"$top/rpmlintrc"
  local sources=("$top"/SRPMS/*.src.rpm)
  local packages=("$top"/RPMS/*/sway-things-[0-9]*.rpm)
  local report
  report=$(rpmlint --rpmlintrc "$top/rpmlintrc" "${sources[@]}" "${packages[@]}") || true
  printf '%s\n' "$report"
  if grep -q ': E: ' <<<"$report"; then
    echo "error: rpmlint found errors" >&2
    exit 1
  fi

  # The package is unsigned (OBS signs the repository), and newer rpm refuses unsigned packages by default.
  sudo rpm --install --nosignature "${packages[@]}"
  sway-things --version
  sway-things --help
  echo "The package $(rpm --query --package --nosignature "${packages[@]}") built and installed."
}

# Inside the container: build a Debian package from the tarball and debian.tar.gz in /work/input. lintian leaves out
# the TAGs.
build_deb() {
  local suppress
  suppress=$(
    IFS=,
    echo "$*"
  )
  local top=$HOME/build
  rm -rf "$top"
  mkdir -p "$top"
  local tarballs=(/work/input/sway-things-*.tar.xz)
  tar -xf "${tarballs[0]}" -C "$top"
  local source=("$top"/sway-things-*/)
  tar -xzf /work/input/debian.tar.gz -C "${source[0]}"
  (cd "${source[0]}" && dpkg-buildpackage --unsigned-source --unsigned-changes --build=binary)

  local changes=("$top"/sway-things_*.changes)
  local packages=("$top"/sway-things_*.deb)
  local report
  report=$(lintian --info ${suppress:+--suppress-tags "$suppress"} "${changes[@]}") || true
  printf '%s\n' "$report"
  if grep -q '^E: ' <<<"$report"; then
    echo "error: lintian found errors" >&2
    exit 1
  fi

  local contents
  contents=$(dpkg-deb --contents "${packages[@]}")
  if ! grep -q ' \./usr/share/zsh/vendor-completions/_sway-things$' <<<"$contents" ||
    grep -q 'site-functions' <<<"$contents"; then
    echo "error: the zsh completion isn't (only) in /usr/share/zsh/vendor-completions" >&2
    exit 1
  fi

  sudo dpkg --install "${packages[@]}"
  sway-things --version
  sway-things --help
  echo "The package $(dpkg-deb --show "${packages[@]}" | tr '\t' ' ') built and installed."
}

# Inside the container: build the Debian 13 package.
build_debian_13() {
  build_deb
}

# Inside the container: build the Ubuntu 26.04 package. Its changelog names Debian's distribution unstable: OBS builds
# both from the same one.
build_ubuntu_26_04() {
  build_deb bad-distribution-in-changes-file
}

# Inside the container: build the openSUSE package. The spec has no %changelog, which an OBS home project doesn't need,
# and its name carries the repository, so that OBS picks it for openSUSE_Tumbleweed.
build_opensuse_tumbleweed() {
  build_rpm sway-things-openSUSE_Tumbleweed.spec no-changelogname-tag invalid-spec-name
}

# Inside the container: build the Fedora package. The spec has no %changelog, which an OBS home project doesn't need.
build_fedora() {
  build_rpm sway-things.spec no-changelogname-tag
}

# Inside the container: build the AUR package sway-things-git.
build_arch_git() {
  sudo pacman -Sy --noconfirm --needed archlinux-keyring
  sudo pacman -Su --noconfirm

  local recipe=$PWD/packaging/aur/sway-things-git
  local build=$HOME/build
  rm -rf "$build"
  cp -r "$recipe" "$build"
  cd "$build"

  echo "Comparing .SRCINFO with makepkg --printsrcinfo"
  if ! diff -u --label .SRCINFO --label 'makepkg --printsrcinfo' <(grep -v '^[[:space:]]*pkgver = ' .SRCINFO) \
    <(makepkg --printsrcinfo | grep -v '^[[:space:]]*pkgver = '); then
    echo "error: .SRCINFO is out of date; run makepkg --printsrcinfo >.SRCINFO in packaging/aur/sway-things-git" >&2
    exit 1
  fi

  local repo=/work/local/sway-things.git
  if [[ -d $repo ]]; then
    local commit
    commit=$(cat /work/local/commit)
    echo "Building $commit of the local checkout instead of GitHub's main"
    sed -i "s|^source=(\"\$pkgname::git+\$url.git\")\$|source=(\"\$pkgname::git+file://$repo#commit=$commit\")|" PKGBUILD
    if ! grep -q "^source=(\"\$pkgname::git+file://" PKGBUILD; then
      echo "error: couldn't point the source of the PKGBUILD at the local checkout" >&2
      exit 1
    fi
  fi

  makepkg --syncdeps --noconfirm

  # namcap after the build, so it isn't there while makepkg looks for missing dependencies.
  sudo pacman -S --noconfirm --needed namcap
  # Not the -debug package: its build-id links point into the main package, which namcap reports as errors.
  local packages=(./sway-things-git-[0-9]*.pkg.tar.zst)
  local report
  report=$(namcap "$recipe/PKGBUILD" "${packages[@]}")
  printf '%s\n' "$report"
  if grep -q ' E: ' <<<"$report"; then
    echo "error: namcap found errors" >&2
    exit 1
  fi

  sudo pacman -U --noconfirm "${packages[@]}"
  sway-things --version
  sway-things --help
  echo "The package sway-things-git built and installed."
}

container=""
build=""
tarball=""
from_checkout=false
while [[ $# -gt 0 ]]; do
  case $1 in
    --container)
      container=${2:-$(usage)}
      shift
      ;;
    --build)
      build=${2:-$(usage)}
      shift
      ;;
    --tarball)
      tarball=$(realpath -m -- "${2:-$(usage)}")
      shift
      ;;
    --local) from_checkout=true ;;
    *) usage ;;
  esac
  shift
done

if [[ -n $build ]]; then
  case $build in
    opensuse-tumbleweed) build_opensuse_tumbleweed ;;
    fedora) build_fedora ;;
    debian-13) build_debian_13 ;;
    ubuntu-26.04) build_ubuntu_26_04 ;;
    arch-git) build_arch_git ;;
    *)
      echo "error: unknown package build: $build" >&2
      exit 1
      ;;
  esac
  exit
fi

if [[ -z $container ]]; then
  usage
fi
engine=${CONTAINER_ENGINE:-$(command -v docker || command -v podman || true)}
if [[ -z $engine ]]; then
  echo "error: needs docker or podman" >&2
  exit 1
fi
if [[ $container == all ]]; then
  names=(ci/packages/*/)
  names=("${names[@]#ci/packages/}")
  names=("${names[@]%/}")
elif [[ -f ci/packages/$container/Dockerfile ]]; then
  names=("$container")
else
  echo "error: unknown container: $container" >&2
  exit 1
fi

# The builds get the release tarball and the files next to it, made from HEAD unless --tarball names them.
for name in "${names[@]}"; do
  case $name in
    opensuse-tumbleweed | fedora | debian-13 | ubuntu-26.04)
      if [[ -z $tarball ]]; then
        if [[ -n $(git status --porcelain) ]]; then
          echo "warning: uncommitted changes aren't built, only HEAD" >&2
        fi
        rm -rf target/packages
        scripts/make-tarball.sh --out target/packages
        tarball=$(compgen -G "$PWD/target/packages/sway-things-*.tar.xz")
      fi
      if [[ ! -f $tarball || $tarball != *.tar.xz ]]; then
        echo "error: not a tarball of scripts/make-tarball.sh: $tarball" >&2
        exit 1
      fi
      break
      ;;
  esac
done

for name in "${names[@]}"; do
  echo "==> $name"
  # Only the files the image needs go into the build context.
  context=$(mktemp -d)
  trap 'rm -rf "$context"' EXIT
  files=("ci/packages/$name/Dockerfile" scripts/build-packages.sh)
  network=(--network none)
  case $name in
    opensuse-tumbleweed)
      files+=(packaging/rpm/opensuse)
      mkdir "$context/input"
      cp "$tarball" "$(dirname "$tarball")/sway-things-openSUSE_Tumbleweed.spec" "$context/input/"
      ;;
    fedora)
      files+=(packaging/rpm/fedora)
      mkdir "$context/input"
      cp "$tarball" "$(dirname "$tarball")/sway-things.spec" "$context/input/"
      ;;
    debian-13 | ubuntu-26.04)
      files+=(packaging/debian/control)
      mkdir "$context/input"
      cp "$tarball" "$(dirname "$tarball")/debian.tar.gz" "$context/input/"
      ;;
    # The -git package clones its source from GitHub and fetches the crates in prepare(), as it does for users.
    arch-git)
      files+=(packaging/aur/sway-things-git)
      network=()
      mkdir "$context/local"
      if $from_checkout; then
        if [[ -n $(git status --porcelain) ]]; then
          echo "warning: uncommitted changes aren't built, only HEAD" >&2
        fi
        git clone --quiet --bare --no-local . "$context/local/sway-things.git"
        git rev-parse HEAD >"$context/local/commit"
      fi
      ;;
  esac
  tar -c "${files[@]}" | tar -x -C "$context"
  "$engine" build --quiet --tag "sway-things-packages:$name" --file "$context/ci/packages/$name/Dockerfile" "$context" \
    >/dev/null
  rm -rf "$context"
  trap - EXIT
  "$engine" run --rm "${network[@]}" "sway-things-packages:$name"
done
