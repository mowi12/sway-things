# Security policy

Sway Things changes the configuration of your desktop session and talks to system services, so we take security reports
seriously.

## Supported versions

Only the latest release gets security fixes. Reports against `main` are welcome too.

## Reporting a vulnerability

**Please don't open a public issue.** Report it privately through GitHub instead:
[Report a vulnerability](https://github.com/mowi12/sway-things/security/advisories/new) (or the repository's
**Security** tab → **Report a vulnerability**).

Please include:

- what an attacker can achieve, and under which conditions;
- steps to reproduce, ideally with a minimal config or input;
- the Sway Things version or commit, the compositor (sway or SwayFX) and its version, and your distribution.

## What to expect

Sway Things is a volunteer project, so responses are best effort. We'll confirm that we received your report, discuss it
with you in the private advisory, and publish the advisory once a fix is released. We credit reporters in the advisory
unless you prefer otherwise.

## Especially relevant

The [security model](https://github.com/mowi12/sway-things/wiki/Architecture#security-model) describes what Sway Things
defends against. Reports in these areas are particularly welcome:

- injection into generated config or commands through external strings (device names, SSIDs, Bluetooth names, file
  names, window titles);
- writes to files Sway Things didn't create;
- privilege escalation through the D-Bus services Sway Things uses;
- crashes or hangs of the sway config parser on crafted input;
- supply chain issues in dependencies, release tarballs or CI workflows.
