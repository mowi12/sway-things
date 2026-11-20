# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# The oldest supported sway: 1.10.1 from Debian 13, with the oldest supported GTK: 4.18.
FROM debian:trixie-slim@sha256:a72b4433de1cf7d6bae0c4cee74d0547f58601a643b3375f070c51600d63b6b2

RUN apt-get update \
  && apt-get install -y --no-install-recommends ca-certificates cargo sway kanshi libgtk-4-dev libgtk4-layer-shell-dev libpulse-dev pkgconf fonts-dejavu-core dbus-daemon python3-dbusmock \
    pipewire pipewire-pulse wireplumber pulseaudio-utils cups-daemon cups-filters \
  && rm -rf /var/lib/apt/lists/* \
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
