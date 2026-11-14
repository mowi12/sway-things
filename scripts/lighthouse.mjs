// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Serves the built website (website/dist) on a local port, runs Lighthouse on every page and fails when a category
// scores below its threshold. Called by scripts/check-website-performance.sh after the build.
//
// Usage: node scripts/lighthouse.mjs
//
// Lighthouse and chrome-launcher come from the website's packages (npm ci --prefix website); CHROME_PATH names the
// browser if chrome-launcher doesn't find one.

import { readdir, readFile } from 'node:fs/promises';
import { createServer } from 'node:http';
import { createRequire } from 'node:module';
import { extname, join, normalize } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

// The website's packages, not this directory's.
const require = createRequire(new URL('../website/package.json', import.meta.url));
const { default: lighthouse } = await import(pathToFileURL(require.resolve('lighthouse')).href);
const chromeLauncher = await import(pathToFileURL(require.resolve('chrome-launcher')).href);

// Simulated throttling computes the scores from one trace, so they barely move between runs; the performance score is
// the best of three to absorb what still does. The thresholds sit below what the site reaches today, with room for that
// variance, and above where a real regression lands.
const thresholds = { performance: 0.95, accessibility: 1, 'best-practices': 1, seo: 1 };
const runs = 3;

const dist = fileURLToPath(new URL('../website/dist/', import.meta.url));
const types = {
  '.html': 'text/html; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.svg': 'image/svg+xml',
  '.png': 'image/png',
  '.webp': 'image/webp',
  '.avif': 'image/avif',
  '.woff2': 'font/woff2',
};

// A plain static server like GitHub Pages: a directory serves its index.html, a missing file the 404 page.
const server = createServer(async (request, response) => {
  let path = normalize(decodeURIComponent(new URL(request.url, 'http://localhost').pathname));
  if (path.endsWith('/')) {
    path += 'index.html';
  }
  try {
    const body = await readFile(join(dist, path));
    response.writeHead(200, { 'Content-Type': types[extname(path)] ?? 'application/octet-stream' });
    response.end(body);
  } catch {
    response.writeHead(404, { 'Content-Type': types['.html'] });
    response.end(await readFile(join(dist, '404.html')));
  }
});
await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
const base = `http://127.0.0.1:${server.address().port}`;

const pages = (await readdir(dist, { recursive: true })).filter((file) => file.endsWith('.html')).sort();
const urls = pages.map((page) => `${base}/${page.replace(/(^|\/)index\.html$/, '$1')}`);

const chrome = await chromeLauncher.launch({ chromeFlags: ['--headless=new', '--no-sandbox'] });
let failed = false;
try {
  for (const url of urls) {
    const best = {};
    for (let run = 0; run < runs; run++) {
      const result = await lighthouse(url, {
        port: chrome.port,
        output: 'json',
        logLevel: 'error',
        onlyCategories: Object.keys(thresholds),
      });
      for (const [id, category] of Object.entries(result.lhr.categories)) {
        best[id] = Math.max(best[id] ?? 0, category.score ?? 0);
      }
      if (Object.entries(thresholds).every(([id, minimum]) => best[id] >= minimum)) {
        break;
      }
    }
    for (const [id, minimum] of Object.entries(thresholds)) {
      const ok = best[id] >= minimum;
      failed ||= !ok;
      console.log(`${ok ? 'ok  ' : 'FAIL'} ${url} ${id}: ${Math.round(best[id] * 100)} (minimum ${minimum * 100})`);
    }
  }
} finally {
  await chrome.kill();
  server.close();
}
process.exit(failed ? 1 : 0);
