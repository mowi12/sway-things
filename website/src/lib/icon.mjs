// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

// The app icon, read from data/icons, so the site always shows the icon the app installs.

import raw from '../../../data/icons/hicolor/scalable/apps/io.github.mowi12.SwayThings.svg?raw';

/** The icon's SVG markup, without its XML comments. */
export const iconSvg = raw
  .replace(/<!--[\s\S]*?-->/g, '')
  .trim();
