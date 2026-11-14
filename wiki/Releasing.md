# Releasing

How a version of Sway Things gets from `main` to the GitHub release page and the openSUSE Build Service (OBS). The
mechanics behind it (tarball, specs, Debian packaging, package CI) are in
[Architecture → Packaging](Architecture.md#packaging).

## Overview

Versions follow Semantic Versioning, `0.x` until 1.0. A milestone is a release; closing it means tagging it.

1. A release pull request sets the version and adds the changelog section.
2. After its merge, the maintainer pushes a **signed** tag `vX.Y.Z` on the merge commit.
3. `.github/workflows/release.yml` checks the tag, makes the reproducible tarball and the OBS inputs, builds every
   package from that tarball, attests the files and creates a **draft** release with the notes from `CHANGELOG.md`.
4. The maintainer reviews the draft and publishes it.
5. Publishing makes OBS fetch the release (its `_service`) and build the packages for every repository.

Nothing is public before step 4. If anything fails before that, the tag is deleted, the problem fixed on `main`, and the
tag pushed again.

## Release steps

1. **Ready?** Every issue of the milestone is closed, CI is green on `main` (including the integration matrix and the
   MSRV job), the Packages workflow is green on `main`, and an OBS snapshot of `main` builds in every repository.
2. **Release pull request** from a branch named after the release issue (e.g. `build/89-release-v0-1`):
   - `scripts/bump-version.sh X.Y.Z` sets the version in `Cargo.toml`, both lock files and both specs, adds the
     `<release>` to the AppStream metainfo (dated today, or pass the date as second argument) and regenerates the man
     pages. Commit: `build: set the version to X.Y.Z`.
   - The changelog section `## X.Y.Z (YYYY-MM-DD)` at the top of `CHANGELOG.md`, with the same date as the metainfo: for
     0.1.0 a short hand-written summary; for every later release
     `git cliff --unreleased --tag vX.Y.Z --prepend CHANGELOG.md`, then read and trim it. Commit:
     `docs: add the changelog for X.Y.Z`. The section is also the text of the release page: `scripts/release-notes.sh`
     joins its wrapped lines into one line per paragraph and list item, because GitHub shows every line break there.
   - README, wiki and roadmap describe the released state. Commit: `docs: describe the X.Y.Z release`.
   - The website describes the released state too: list the pages the release has in
     `website/src/data/released-pages.json`, then `scripts/update-website-screenshots.sh` takes their screenshots in
     light and dark (on GTK's Broadway backend; no compositor starts). Commit:
     `docs(website): describe the X.Y.Z release`. The site deploys once the pull request is merged.
   - `scripts/check-packaging.sh` (in CI) checks that all versions agree.
3. **Merge** the pull request.
4. **Tag** the merge commit and push the tag:

   ```sh
   git switch main && git pull
   git tag -s vX.Y.Z -m "Sway Things X.Y.Z"
   git push origin vX.Y.Z
   ```

5. **Watch** the _Release_ workflow. _Verify Release_ (`scripts/verify-release.sh`) refuses the tag unless it is
   annotated and verified, on `main`, matches `Cargo.toml`, and has its changelog section and metainfo `<release>`. If a
   job fails: `git push --delete origin vX.Y.Z` and `git tag -d vX.Y.Z` (the ruleset lets the maintainer), fix on `main`
   through a pull request, tag again.
6. **Review the draft** on the releases page: title, notes, and the files (tarball, `.sha256`, both specs,
   `sway-things.dsc`, `debian.tar.gz`). Download the tarball and check it:

   ```sh
   sha256sum -c sway-things-X.Y.Z.tar.xz.sha256
   gh attestation verify sway-things-X.Y.Z.tar.xz --repo mowi12/sway-things
   ```

7. **Publish** the draft. _Trigger OBS_ runs; `osc results` in the OBS checkout shows the builds, which take about half
   an hour. A repository that fails only on a flaky test is rebuilt with
   `osc rebuild home:mowi12:sway-things sway-things <repository> <arch>`.
8. **Afterwards:** install from OBS on at least one distribution, close the milestone, and pick the next priorities
   ([Roadmap](Roadmap.md)).

## Dry run

To test the workflow without releasing (after changing it, or before the first release): tag the current `main` with
`vX.Y.Z-rc.N`, where `X.Y.Z` is the version in `Cargo.toml` (e.g. `v0.0.0-rc.1` before 0.1.0). The workflow runs as for
a release, without the changelog and metainfo checks, and creates a draft **pre-release** with a fixed note; publishing
a pre-release never triggers OBS. Delete the draft and the tag afterwards.

## Snapshots between releases

OBS also builds snapshots of `main` for testing, uploaded by hand: `scripts/make-tarball.sh` in an up-to-date `main`
checkout, then in the OBS checkout delete the old tarball, copy the tarball, both specs, `sway-things.dsc` and
`debian.tar.gz` from `target/dist/`, and `osc addremove && osc commit`. A snapshot's version is
`<version>+git<commit count>.g<short hash>`, which sorts before the next release. Once the `_service` is in place,
snapshots are no longer uploaded, so that the repositories carry releases only.
