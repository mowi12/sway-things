# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# Builds the AUR package sway-things-git like a clean chroot: base-devel only, an unprivileged user who may run pacman,
# so makepkg -s installs the declared dependencies and nothing else is there. See scripts/build-packages.sh.
FROM archlinux:base@sha256:d4594bb2300cba11f41c2f27923eb56f63d9988909c402f9c95517d1e362c899

# The image has no local pacman key, which updating archlinux-keyring needs.
RUN pacman-key --init \
  && pacman -Sy --noconfirm --needed archlinux-keyring \
  && pacman -Su --noconfirm --needed base-devel \
  && pacman -Scc --noconfirm \
  && useradd --create-home builder \
  && printf 'builder ALL=(root) NOPASSWD: /usr/bin/pacman\n' >/etc/sudoers.d/builder \
  && chmod 440 /etc/sudoers.d/builder

COPY --chown=builder scripts/build-packages.sh scripts/lib.sh /work/scripts/
COPY --chown=builder packaging/aur/sway-things-git/ /work/packaging/aur/sway-things-git/
COPY --chown=builder local/ /work/local/
USER builder
WORKDIR /work
CMD ["scripts/build-packages.sh", "--build", "arch-git"]
