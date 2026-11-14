// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

// The IBM Plex faces the site uses, self-hosted from the @fontsource packages: only the Latin subset and the weights
// the styles use. The build copies each file into dist with a hashed name.

import mono400 from '@fontsource/ibm-plex-mono/files/ibm-plex-mono-latin-400-normal.woff2?url';
import condensed700 from '@fontsource/ibm-plex-sans-condensed/files/ibm-plex-sans-condensed-latin-700-normal.woff2?url';
import sans400 from '@fontsource/ibm-plex-sans/files/ibm-plex-sans-latin-400-normal.woff2?url';
import sans600 from '@fontsource/ibm-plex-sans/files/ibm-plex-sans-latin-600-normal.woff2?url';

// The Latin range of the @fontsource files.
const latin = 'U+0000-00FF,U+0131,U+0152-0153,U+02BB-02BC,U+02C6,U+02DA,U+02DC,U+0304,U+0308,U+0329,U+2000-206F,'
  + 'U+20AC,U+2122,U+2191,U+2193,U+2212,U+2215,U+FEFF,U+FFFD';

export const faces = [
  { family: 'IBM Plex Sans Condensed', weight: 700, url: condensed700, preload: true },
  { family: 'IBM Plex Sans', weight: 400, url: sans400, preload: true },
  { family: 'IBM Plex Sans', weight: 600, url: sans600, preload: false },
  { family: 'IBM Plex Mono', weight: 400, url: mono400, preload: false },
];

/** The @font-face rules for every face. */
export const fontFaceCss = faces
  .map(({ family, weight, url }) =>
    `@font-face{font-family:"${family}";font-style:normal;font-weight:${weight};font-display:swap;`
    + `src:url(${url}) format("woff2");unicode-range:${latin}}`
  )
  .join('');
