# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# Builds the Fedora package with rpmbuild, as the Fedora repositories on the openSUSE Build Service do, on the newest
# Fedora release. The image holds the build dependencies of the spec, rpmlint, and sway for the install test (sway
# never runs).
FROM fedora:45@sha256:a7d72ba2745b100a3351087472e5ee7b2450af7cf331e7673007e3817612839c

COPY packaging/rpm/fedora/sway-things.spec /tmp/
RUN dnf --assumeyes --setopt=install_weak_deps=False install rpm-build rpmlint sudo sway \
  && dnf --assumeyes --setopt=install_weak_deps=False builddep /tmp/sway-things.spec \
  && dnf clean all \
  && useradd --create-home builder \
  && printf 'builder ALL=(root) NOPASSWD: /usr/bin/rpm\n' >/etc/sudoers.d/builder \
  && chmod 440 /etc/sudoers.d/builder

COPY --chown=builder scripts/build-packages.sh /work/scripts/
COPY --chown=builder input/ /work/input/
USER builder
WORKDIR /work
CMD ["scripts/build-packages.sh", "--build", "fedora"]
