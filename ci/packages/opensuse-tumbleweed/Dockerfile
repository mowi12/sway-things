# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# Builds the openSUSE package with rpmbuild, as the repository openSUSE_Tumbleweed on the openSUSE Build Service does.
# The image holds the build dependencies of the spec, rpmlint, and sway for the install test (sway never runs).
FROM opensuse/tumbleweed@sha256:dc714bafa362b1aa58a6fd2fcea9c2469255c1fb8f4433e943e44fd61195c09c

COPY packaging/rpm/opensuse/sway-things.spec /tmp/
# Tumbleweed is rolling: the image catches up with the repositories first.
RUN zypper --non-interactive dist-upgrade --no-recommends \
  && zypper --non-interactive install --no-recommends rpm-build rpmlint sudo sway \
  && rpmspec --query --buildrequires /tmp/sway-things.spec | tr -d ' ' \
    | xargs zypper --non-interactive install --no-recommends \
  && zypper clean --all \
  && useradd --create-home builder \
  && printf 'builder ALL=(root) NOPASSWD: /usr/bin/rpm\n' >/etc/sudoers.d/builder \
  && chmod 440 /etc/sudoers.d/builder

COPY --chown=builder scripts/build-packages.sh /work/scripts/
COPY --chown=builder input/ /work/input/
USER builder
WORKDIR /work
CMD ["scripts/build-packages.sh", "--build", "opensuse-tumbleweed"]
