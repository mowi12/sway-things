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
# Usage: scripts/build-packages.sh --container NAME|all [--tarball FILE]
#
# Needs docker or podman (CONTAINER_ENGINE overrides the choice), and without --tarball what scripts/make-tarball.sh
# needs.

set -euo pipefail

usage() {
  echo "usage: $0 --container NAME|all [--tarball FILE]" >&2
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

# Inside the container: build the openSUSE package. The spec has no %changelog, which an OBS home project doesn't need,
# and its name carries the repository, so that OBS picks it for openSUSE_Tumbleweed.
build_opensuse_tumbleweed() {
  build_rpm sway-things-openSUSE_Tumbleweed.spec no-changelogname-tag invalid-spec-name
}

# Inside the container: build the Fedora package. The spec has no %changelog, which an OBS home project doesn't need.
build_fedora() {
  build_rpm sway-things.spec no-changelogname-tag
}

container=""
build=""
tarball=""
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
    *) usage ;;
  esac
  shift
done

if [[ -n $build ]]; then
  case $build in
    opensuse-tumbleweed) build_opensuse_tumbleweed ;;
    fedora) build_fedora ;;
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

# The RPM builds get the release tarball and the specs next to it, made from HEAD unless --tarball names them.
for name in "${names[@]}"; do
  case $name in
    opensuse-tumbleweed | fedora)
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
  esac
  tar -c "${files[@]}" | tar -x -C "$context"
  "$engine" build --quiet --tag "sway-things-packages:$name" --file "$context/ci/packages/$name/Dockerfile" "$context" \
    >/dev/null
  rm -rf "$context"
  trap - EXIT
  "$engine" run --rm "${network[@]}" "sway-things-packages:$name"
done
