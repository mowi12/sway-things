// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

import { fileURLToPath } from 'node:url';

import { defineConfig } from 'astro/config';

// The site reads README.md and the app icon from outside website/.
const repoRoot = fileURLToPath(new URL('..', import.meta.url));

export default defineConfig({
  site: 'https://sway-things.org',
  trailingSlash: 'ignore',
  // Astro's dev toolbar loads scripts the published site never has.
  devToolbar: { enabled: false },
  vite: {
    server: { fs: { allow: [repoRoot] } },
  },
});
