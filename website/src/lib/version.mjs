// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

// The version of the latest release: the workspace version in Cargo.toml, which only a release pull request changes.

import cargo from '../../../Cargo.toml?raw';

const match = /^\[workspace\.package\]\n(?:.*\n)*?version = "([^"]+)"/m.exec(cargo);
if (!match) {
  throw new Error('Cargo.toml has no version in [workspace.package]');
}

export const version = match[1];

/** The version as people say it: 0.1 for 0.1.0, 0.2.1 for 0.2.1. */
export const shortVersion = version.replace(/\.0$/, '');
