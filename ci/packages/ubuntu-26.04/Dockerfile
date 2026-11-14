# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# Builds the Debian package with dpkg-buildpackage, as the repository xUbuntu_26.04 on the openSUSE Build Service does.
# The image holds the build dependencies of packaging/debian/control, lintian, and sway for the install test (sway
# never runs).
FROM ubuntu:26.04@sha256:065aa8626841f16057b78b027b464cd59c4fac36c204c72dcbf48d66f5fa8a96

COPY packaging/debian/control /tmp/source/debian/control
RUN apt-get update \
  && DEBIAN_FRONTEND=noninteractive apt-get install --yes --no-install-recommends build-essential lintian sudo sway \
  && DEBIAN_FRONTEND=noninteractive apt-get build-dep --yes --no-install-recommends /tmp/source \
  && rm -rf /var/lib/apt/lists/* /tmp/source \
  && useradd --create-home builder \
  && printf 'builder ALL=(root) NOPASSWD: /usr/bin/dpkg\n' >/etc/sudoers.d/builder \
  && chmod 440 /etc/sudoers.d/builder

COPY --chown=builder scripts/build-packages.sh /work/scripts/
COPY --chown=builder input/ /work/input/
USER builder
WORKDIR /work
CMD ["scripts/build-packages.sh", "--build", "ubuntu-26.04"]
