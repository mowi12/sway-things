# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# The newest sway release, from Arch Linux. Its sway binary has the cap_sys_nice file capability, which containers
# don't grant, so the image removes it.
FROM archlinux:base@sha256:b1c2fe6e717bfd5673af8fc217e1e1c3ce0dcbb1d53c3414c8425e7ce79fc6d9

RUN pacman -Syu --noconfirm sway rust gcc \
  && pacman -Scc --noconfirm \
  && setcap -r /usr/bin/sway \
  && useradd --create-home tester \
  && install -d -o tester /work

COPY --chown=tester scripts/check-integration.sh /work/scripts/
COPY --chown=tester ci/integration/ /work/ci/integration/
COPY --chown=tester Cargo.toml Cargo.lock /work/
COPY --chown=tester crates/ /work/crates/
USER tester
WORKDIR /work
# The tests that need sway are built on the system they run on.
RUN scripts/check-integration.sh --build
CMD ["scripts/check-integration.sh"]
