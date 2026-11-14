// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

// The SIL Open Font License of the IBM Plex fonts the site serves, which has to travel with the font files.

import mono from '@fontsource/ibm-plex-mono/LICENSE?raw';
import condensed from '@fontsource/ibm-plex-sans-condensed/LICENSE?raw';
import sans from '@fontsource/ibm-plex-sans/LICENSE?raw';

const sections = [
  ['IBM Plex Sans', sans],
  ['IBM Plex Sans Condensed', condensed],
  ['IBM Plex Mono', mono],
];

export function GET() {
  const text = sections.map(([name, license]) => `${name}\n${'='.repeat(name.length)}\n\n${license.trim()}\n`).join(
    '\n',
  );
  return new Response(text, { headers: { 'Content-Type': 'text/plain; charset=utf-8' } });
}
