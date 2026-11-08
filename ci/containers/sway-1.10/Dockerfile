# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# The oldest supported sway: 1.10.1 from Debian 13.
FROM debian:trixie-slim@sha256:a72b4433de1cf7d6bae0c4cee74d0547f58601a643b3375f070c51600d63b6b2

RUN apt-get update \
  && apt-get install -y --no-install-recommends sway \
  && rm -rf /var/lib/apt/lists/* \
  && useradd --create-home tester

COPY --chown=tester scripts/check-integration.sh /work/scripts/
COPY --chown=tester ci/integration/ /work/ci/integration/
USER tester
WORKDIR /work
CMD ["scripts/check-integration.sh"]
