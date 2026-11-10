# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# The newest SwayFX release, from Void Linux. SwayFX needs a DRM render node and GLES2: scripts/check-integration.sh
# passes in the host's render nodes (in CI a virtual one from vgem), and Mesa renders in software, so no GPU is needed.
# Its sway binary has the cap_sys_nice file capability, which containers don't grant, so the image removes it.
FROM ghcr.io/void-linux/void-glibc@sha256:7de4630b6d3e0a76e9e68771837c15fc179e320de292b06e29c7292756d29583

RUN xbps-install -Syu xbps \
  && xbps-install -Syu bash shadow swayfx mesa-dri libcap-progs rust cargo gcc \
  && xbps-remove -O \
  && setcap -r /usr/bin/sway \
  && useradd --create-home tester \
  && install -d -o tester /work

ENV WLR_RENDERER=gles2 WLR_RENDERER_ALLOW_SOFTWARE=1 LIBGL_ALWAYS_SOFTWARE=1 GBM_ALWAYS_SOFTWARE=1

COPY --chown=tester scripts/check-integration.sh /work/scripts/
COPY --chown=tester ci/integration/ /work/ci/integration/
COPY --chown=tester Cargo.toml Cargo.lock /work/
COPY --chown=tester crates/ /work/crates/
USER tester
WORKDIR /work
# The IPC client's tests are built on the system they run on.
RUN scripts/check-integration.sh --build
CMD ["scripts/check-integration.sh"]
