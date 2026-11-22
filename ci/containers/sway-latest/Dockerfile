# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# The newest sway release, from Arch Linux. Its sway binary has the cap_sys_nice file capability, which containers
# don't grant, so the image removes it. CUPS's stand-in runs cupsd as the tester, which Arch installs for root only.
FROM archlinux:base@sha256:d4594bb2300cba11f41c2f27923eb56f63d9988909c402f9c95517d1e362c899

RUN pacman -Syu --noconfirm sway kanshi rust gcc gtk4 gtk4-layer-shell libpulse pkgconf ttf-dejavu python-dbusmock \
    pipewire pipewire-pulse wireplumber cups sane \
  && pacman -Scc --noconfirm \
  && setcap -r /usr/bin/sway \
  && chmod 755 /usr/sbin/cupsd \
  && useradd --create-home tester \
  && install -d -o tester /work

COPY --chown=tester scripts/check-integration.sh scripts/lib.sh /work/scripts/
COPY --chown=tester ci/integration/ /work/ci/integration/
COPY --chown=tester Cargo.toml Cargo.lock /work/
COPY --chown=tester crates/ /work/crates/
# The translation template, from which the GUI test in the pseudo-locale makes its catalog.
COPY --chown=tester po/sway-things.pot /work/po/
USER tester
WORKDIR /work
# The tests that need sway are built on the system they run on.
RUN scripts/check-integration.sh --build
CMD ["scripts/check-integration.sh", "--host"]
