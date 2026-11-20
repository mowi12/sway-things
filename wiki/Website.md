# Website

> Part of the [architecture](Architecture.md). sway-things.org: how it is built from the README.

The project site at [sway-things.org](https://sway-things.org) lives in `website/`: a static site built with
[Astro](https://astro.build/) and served by GitHub Pages. It is one page with anchors, the imprint, the privacy notice
and a 404 page, in English (the imprint in German), and it serves the install script.

- **No drift from the repository.** The page reads its content at build time instead of repeating it:
  - from `README.md` (`src/lib/readme.mjs`): the principles, the feature groups and their pages, the command line
    example, the requirements and the installation instructions per distribution. The build fails when one of these
    sections changes shape, so a README change can't leave the site silently wrong;
  - the app icon from `data/icons/` (inline, and as the favicon), so a new icon reaches the site on its own;
  - the released pages from `src/data/released-pages.json`, which the build checks against the README's feature list,
    and the version of the latest release from `Cargo.toml`;
  - `scripts/install-from-obs.sh`, which `scripts/build-website.sh` copies to `/install` and `/install.sh`.
- **Imprint and privacy.** The operator's name and address are never in the repository. The imprint page fetches the
  operator's own imprint while building, from the page the repository variable `IMPRINT_URL` names
  (`src/lib/imprint.mjs`), and keeps only its text and a few tags (paragraphs, line breaks, emphasis, headings, lists),
  without attributes, links, anything before its first heading or after the end of the `main` or `article` it is in, and
  without the site's navigation, headers, footers and sidebars; a paragraph that is only a link (to the operator's own
  pages) goes, an email link keeps its address. The deploy sets `IMPRINT_REQUIRED`, so it fails rather than publish
  without an imprint, with one whose heading isn't Impressum or Imprint (a maintenance page answered with 200 OK), or
  with an empty one; other builds (pull requests, forks, local) say that their copy has none. The privacy notice
  (`src/pages/privacy.astro`) names no one: the controller is whoever the imprint names. Both are linked from the footer
  of every page.
- **Screenshots**, in light and dark, are PNG files in `src/assets/screenshots/`, made by
  `scripts/update-website-screenshots.sh`: it runs `scripts/gui-screenshots.sh` (the GUI tests on GTK's Broadway
  backend, in the C locale) and copies Mouse & Touchpad, shown at the top, and the released pages marked `"gallery"` in
  `released-pages.json`, a selection of about six below the sidebar. The build turns them into AVIF and WebP in two
  widths, and a `<picture>` shows the one of the visitor's color scheme.
- **Design.** The page is drawn after the app icon: its navy frame around a white pane, the split line between "yours"
  (the user's config) and "ours" (the app and its files), and the teal wave that carries the install section. Colors are
  custom properties in `src/styles/tokens.css`, light and dark, derived from the icon; the wave is
  `src/components/Wave.astro`. A new icon means new values there.
- **Performance and privacy.** No cookies, no analytics, no request to another server. The IBM Plex faces come from the
  `@fontsource` packages, Latin only, the heading and body faces preloaded (`src/lib/fonts.mjs`); their SIL Open Font
  License is served at `/fonts/LICENSE.txt`. The only script is a small inline one that adds copy buttons to the install
  commands; without it the commands stay selectable. Astro's telemetry is off (`ASTRO_TELEMETRY_DISABLED`).
- **Packages.** `website/package.json` pins exact versions, `package-lock.json` is committed, and Dependabot groups
  their updates. `scripts/check-npm-licenses.sh` checks the licenses of the website's and the check tools' packages
  against the allow list of `deny.toml`.
- **Checks** (CI job _Check Website_): `scripts/check-website.sh` runs the unit tests of `src/lib/` (`node --test`),
  builds the site and fails on invalid HTML (html-validate), a broken relative link or anchor (lychee), or a served
  install script that isn't `scripts/install-from-obs.sh`; `scripts/check-website-performance.sh` runs Lighthouse on
  every page, as a phone and as a desktop, served locally, and requires 95 for performance (best of three runs) and 100
  for accessibility, best practices and SEO.
- **Deploy.** `.github/workflows/website.yml` builds (`scripts/build-website.sh`) and deploys to GitHub Pages on every
  push to `main` that changes `website/`, `README.md`, the icons, the install script or the build script, weekly (for a
  changed imprint) and by hand. Like the README, the site describes the released state: a release pull request updates
  the screenshots and the released pages ([Releasing](Releasing.md)).

To work on it: `npm ci --prefix website`, then `npm --prefix website run dev` serves it with live reload.
