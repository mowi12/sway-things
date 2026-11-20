# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# The newest SwayFX release, from Void Linux. SwayFX needs a DRM render node and GLES2: scripts/check-integration.sh
# passes in the host's render nodes (in CI a virtual one from vgem), and Mesa renders in software, so no GPU is needed.
# Its sway binary has the cap_sys_nice file capability, which containers don't grant, so the image removes it. CUPS's
# stand-in runs cupsd as the tester, which Void installs for root only.
FROM ghcr.io/void-linux/void-glibc@sha256:7de4630b6d3e0a76e9e68771837c15fc179e320de292b06e29c7292756d29583

RUN xbps-install -Syu xbps \
  && xbps-install -Syu bash shadow swayfx kanshi mesa-dri libcap-progs rust cargo gcc gtk4-devel gtk4-layer-shell-devel pulseaudio-devel pkg-config dejavu-fonts-ttf dbus python3-dbusmock \
    pipewire wireplumber pulseaudio-utils cups cups-filters \
  && xbps-remove -O \
  && setcap -r /usr/bin/sway \
  && chmod 755 /usr/bin/cupsd \
  && useradd --create-home tester \
  && install -d -o tester /work

ENV WLR_RENDERER=gles2 WLR_RENDERER_ALLOW_SOFTWARE=1 LIBGL_ALWAYS_SOFTWARE=1 GBM_ALWAYS_SOFTWARE=1

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
