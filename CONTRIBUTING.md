# Contributing to Sway Things

Thanks for your interest! Sway Things is released (see the [changelog](CHANGELOG.md)); the design of everything still to
come is written down in the wiki and open for discussion. Please follow our [Code of Conduct](CODE_OF_CONDUCT.md), and
report security issues privately as described in [SECURITY.md](SECURITY.md), never in public issues.

## Read first

1. [wiki/Home.md](wiki/Home.md): the developer documentation index
2. [wiki/Vision-and-Scope.md](wiki/Vision-and-Scope.md): what we build and what we deliberately don't
3. [wiki/Architecture.md](wiki/Architecture.md): how it works, including the security rules

## Issues

- **Issues on demand.** The full scope and the status of every item live in
  [wiki/Feature-Catalog.md](wiki/Feature-Catalog.md). Open an issue when you start or plan an item, not before. The PR
  that implements an item ticks its checkbox there; one that drops it strikes it through with the reason.
- Use the **issue forms** (Bug, Feature, Docs, Refactor). Blank issues are disabled.
- The **issue type** says what sort of work it is: Bug, Feature, Maintenance, Docs or Security. The forms set it, so
  titles carry no prefix.
- The **Effort** field (Easy, Medium, Hard) is the expected effort, as in the Feature Catalog. Maintainers set it;
  **Start date** and **Target date** are optional.
- **Labels** are defined in [.github/labels.yml](.github/labels.yml): `area:`, `page:`, `status:`, `compat:swayfx`, plus
  `good first issue` and `help wanted`. To change a label, edit the file. The PR shows a preview, and
  `scripts/sync-labels.sh` applies it after merge. Labels created on github.com get deleted.
- **Milestones** are releases.
- **No fixed ownership.** Anyone may pick any issue: assign yourself when you start working on it, and talk in the issue
  before starting something large.

## Pull requests

- **Trunk-based.** `main` is the only long-lived branch and is always green.
- **Branch names:** `<type>/<issue>-<slug>`, where the type is the conventional commit type of the PR's main purpose and
  the slug is the issue's full title in kebab case, as GitHub's "Create a branch" makes it: `chore/2-add-issue-forms`,
  `feat/84-window-behavior-page`. The purpose shows in branch lists, and the issue number gives the context.
- **PR titles** are the title of the linked issue, without a type prefix: the type already shows in the branch name and
  the commits.
- **Assign yourself** to the PR you open, as to its issue.
- **Draft PRs** are welcome for work in progress: open one early and mark it ready for review when it's done.
- **Every change goes through a pull request**:
  - all required CI checks must pass, and **nobody can bypass them**;
  - **one approving review** is required, four eyes on code that writes `exec` lines and config (the repository owner
    may bypass the review, never CI);
  - new commits dismiss earlier approvals, and all review conversations must be resolved;
  - no force pushes to `main`.
- **Merge commits only**: a PR lands as a merge commit ("Merge pull request #N from …"), and every commit of the PR
  stays in the history as is. Keep commits meaningful and conventional, and clean them up with an interactive rebase
  before merging. The merge commit keeps the PR's boundary visible: it groups the PR's commits and names its number.
  Branches are deleted automatically after merge.
- **Updating your branch:** when `main` has moved on, use **Update with rebase** (or rebase locally), never a merge
  commit. A "Merge branch 'main' into …" commit would end up in the history and isn't a conventional commit.
- **Auto-merge** is available: enable it, and the PR merges once it's approved and all checks are green.
- Keep PRs small and fill in the PR template checklist (tick each item, or cross it out with a reason).
- **Labels and milestones go on the issue**, not on the PR: GitHub's milestone progress counts issues and PRs together,
  so a milestone on both would count the same work twice. Link the issue in the PR description with `Closes #N`.

## Commit messages

We use [Conventional Commits](https://www.conventionalcommits.org/). The allowed types and their meaning are defined in
[.conventional-commit-types.json](.conventional-commit-types.json):

| Type | Use for |
| --- | --- |
| `feat` | Adds, adjusts or removes a feature of the API or UI |
| `fix` | Fixes an API or UI bug introduced by an earlier `feat` |
| `refactor` | Restructures code without changing API or UI behavior |
| `perf` | A refactor that specifically improves performance |
| `style` | Formatting only (whitespace, formatting), no behavior change |
| `test` | Adds or corrects tests |
| `docs` | Changes only documentation |
| `build` | Build tools, dependencies, project version |
| `ci` | CI configuration and scripts |
| `chore` | Maintenance, e.g. `.gitignore` |

The JSON file is the source of truth. If the two disagree, the JSON wins. CI checks **every commit** of a PR against it.

**Scope** is a page id or an area: `feat(keyboard): add layout reordering`, `fix(core): quote device identifiers`,
`docs(wiki): describe conflict detection`. Mark breaking changes with `!` (`feat(cli)!: rename get to show`).

Commit messages are the draft of each `CHANGELOG.md` section ([git-cliff](https://git-cliff.org/)), so write them for
users, in American English. American English is the language of the whole project (code, UI strings, docs), as sway
(`colors`) and GTK (`color-scheme`) use it, and it gives the spell checker one locale.

## Engineering rules (non-negotiable)

Rules 1–7 protect users' systems; rules 8–12 keep the code base consistent and the supply chain safe. Details and
reasons are in [wiki/Architecture.md](wiki/Architecture.md#security-model).

1. **Never write a file Sway Things didn't create.** The only exceptions are official programmatic stores
   (gsettings/dconf, D-Bus services), and the one file the user picks in a Save dialog for something they made in the
   app, such as a scan: written atomically, and replacing a file only when the dialog asked them first. The user's sway
   config is never touched.
2. **Never build shell strings.** Run processes with argument arrays. Never `sh -c`. Where a companion tool's config
   takes a command its tool runs through a shell (mako's `on-button-left`, swaync's scripts and buttons), the command is
   a fixed string of ours that calls `sway-things`, never built from data.
3. **Treat external strings as data.** Device names, SSIDs, Bluetooth names, file names and window titles can be
   attacker-controlled. Escape them with the typed sway command builders. Never interpolate them.
4. **No `unsafe`.** The root `Cargo.toml` forbids `unsafe_code` for every crate.
5. **No panics in non-test code.** `unwrap`, `expect`, `panic!`, `todo!` and `dbg!` are denied by clippy. Handle errors:
   a panic must not end a hook at login or leave a change half applied.
6. **No privileged code, no daemon.** System changes only go through existing polkit-guarded D-Bus services. Nothing
   keeps running: what has to happen without the window runs as a short CLI hook from generated config
   ([No daemon](wiki/Command-Line.md#no-daemon-the-cli-as-a-hook)).
7. **Validate before replacing.** Generated sway config must pass `sway --validate` before it replaces the old file.
   Writes are atomic. **No network access, no telemetry.** The one exception is a bar template whose script reaches the
   network (the check for updates, the agents' usage limits): Sway Things ships it but never runs it, Waybar does, and
   the page adds it only after a dialog says what it sends where and how often. Such a script sends nothing but what it
   needs to its own service, never poses as another program (no borrowed User-Agent), never starts another vendor's tool
   in the background, and never copies, stores or renews a login. **Nothing is exposed by default:** whatever opens a
   port, shares something on the network or makes this computer or its data visible to others (sharing a printer,
   Bluetooth visibility, a listening service) starts off, also where the service below would turn it on by itself, and
   only the user turns it on, for that one thing.
8. **No tokio, executor-agnostic core.** Async I/O uses async-io based crates on the glib main loop, so UI updates run
   directly in futures and CLI hooks start fast.
9. **REUSE headers.** Every source file (`.rs`, scripts) starts with:

   ```text
   SPDX-FileCopyrightText: 2026 Sway Things contributors
   SPDX-License-Identifier: GPL-3.0-or-later
   ```

   Files that can't carry a header (Markdown, YAML, JSON, TOML, …) are covered by [REUSE.toml](REUSE.toml). A new file
   type without a header gets a pattern there, otherwise `reuse lint` fails.

10. **Pin GitHub Actions to a full commit SHA**, with the version as a comment:
    `uses: actions/checkout@<40-character SHA> # v7.0.1`. Tags can be moved; a SHA can't. Dependabot keeps the pins
    current. Tools a workflow downloads are pinned to a version and verified against their SHA-256 checksum, written
    once in `tools/binaries.txt` and installed by `scripts/install-tool.sh`; packages from Ubuntu's or Debian's signed
    repositories need no pin; container images are pinned to a digest. Give every workflow job only the `permissions` it
    needs. Workflow, job and step names use Title Case (`Preview Label Changes`); tool names keep their own spelling
    (`Install cargo-deny`).
11. **New settings start in the registry** (`crates/core/src/registry/`). The rows, search, CLI keys, defaults,
    generated lines and conflict detection all derive from that one definition
    ([Settings registry](wiki/Architecture.md#settings-registry)).
12. **Check commands live only in `scripts/`.** Workflows and the `justfile` call the scripts, never repeat a command
    (see [Checks](#checks)).

## Checks

**Every check is one script in `scripts/`, and that script is the only place its command is written down**, so local
runs and CI can't drift. CI calls the scripts directly; the `justfile` just wraps them for convenience. There are no git
hooks. Run checks yourself before pushing:

```sh
just test               # run the tests of the workspace (the ones that need sway are skipped)
just lint               # run the fast offline checks: no tests, no sway, no network
just check              # run every check that is safe on this machine: offline, never starts sway
just check-all          # run every check, like CI: also network and containers (sway runs only in containers)
just checks             # list every check with its level
scripts/check-fmt.sh    # or run a single check directly
just fmt                # format Rust, Markdown, TOML, YAML and JSON
```

`just` without a recipe lists every development command. Each check script names its level (`lint`, `host`, `network` or
`container`) in its header, and `scripts/run-checks.sh` runs the checks of a level. The tests against a real sway run
only in containers: `scripts/check-integration.sh` builds and runs every one of `ci/containers/`; `--host` would start
the sway on `PATH` instead and is only for the containers themselves.

Every script prints its header with `-h` or `--help` and refuses arguments it doesn't know. The helpers they share
(`require`, `container_engine`, `repo_files`, the help) are in `scripts/lib.sh`, which each script sources.

Each script names the tools it needs in its header. Tools from npm and PyPI are pinned with hashes in `tools/`:
`npm ci --prefix tools` installs the npm ones where the scripts find them; install the PyPI ones into a virtual
environment with `pip install --require-hashes -r tools/requirements.txt`. `scripts/install-tool.sh NAME…` installs the
tools of `tools/binaries.txt` from their GitHub releases (into `target/tools`, unless `--dir` says otherwise).

| Check | Tools |
| --- | --- |
| Formatting | `rustfmt` (default style); `dprint` for Markdown (120 columns), TOML, YAML, JSON, and the website's JavaScript, CSS and Astro files |
| Rust lints | `clippy` with `-D warnings`, pedantic + safety picks; every `#[allow]` needs a `reason` |
| Other lints | `markdownlint-cli2`, `actionlint`, `shellcheck`; `desktop-file-validate` and `appstreamcli validate` on the desktop entry and metainfo after `msgfmt` merged the translations |
| Spelling | `typos`, locale en-US |
| Translation template | `xgettext` ≥ 0.24 (GNU gettext): `po/sway-things.pot` holds exactly the strings marked with `msg!` and those of `data/*.in` |
| Links | `lychee --offline` (relative links + anchors); external links weekly, dead ones open a Docs issue |
| Licensing | `reuse lint` |
| Container images | every image is pinned to a digest, the same one everywhere an image:tag appears, and the workflows' images are in `ci/images/Dockerfile`, where Dependabot updates them (`scripts/check-images.sh`) |
| Dependencies | `cargo deny check` (advisories, GPL-compatible licenses, sources) |
| npm licenses | `license-checker-rseidelsohn`: the packages of `website/` and `tools/` against the allow list of `deny.toml`, with a few reasoned exceptions for build-only packages (`scripts/check-npm-licenses.sh`) |
| Website | `scripts/check-website.sh`: the unit tests of `website/src/lib/`, builds the site, then `html-validate` and `lychee --offline` on the built pages and checks the served install script; `scripts/check-website-performance.sh`: Lighthouse on every page, as a phone and as a desktop (performance ≥ 95, accessibility, best practices and SEO 100) |
| Tests | unit + golden-file tests on MSRV **1.85** and stable, in a Debian 13 container (GTK 4.18) |
| Install | `scripts/build.sh` and `scripts/install.sh` into a temporary `DESTDIR`, for `PREFIX=/usr/local` (the default) and `/usr` (packages): the exact list of installed files, `appstreamcli validate-tree`, `desktop-file-validate`, `groff` warnings in the man pages, the syntax of the bash, zsh and fish completions, and `--uninstall` leaving nothing behind |
| Install script | `scripts/check-install-from-obs.sh`: dry runs of `scripts/install-from-obs.sh` (served at sway-things.org/install) under dash, bash and busybox `sh` in a clean container, for every supported system and some unsupported ones, and the signing key's fingerprint as the script computes it; with `--container all` (in the Packages workflow), a real install, update and uninstall from the published OBS repository on every distribution |
| Packaging | the version in `Cargo.toml`, both RPM specs, the man pages, the newest `CHANGELOG.md` section and the metainfo agree, and so does the MSRV in `Cargo.toml`, the CI test matrix, the Debian and openSUSE build requirements and the README; the specs' `License` (and Fedora's `SourceLicense`) name exactly the licenses of the crates; the AUR `.SRCINFO` agrees with its `PKGBUILD` (`scripts/check-packaging.sh`) |
| Packages | not a pull request check: on every push to `main`, weekly and for a release, the reproducible tarball (`scripts/make-tarball.sh --check`) and the openSUSE Tumbleweed, Fedora, Debian 13 and Ubuntu 26.04 packages built from it offline in containers (`ci/packages/`), with `rpmlint` or `lintian` and an install test; locally `scripts/build-packages.sh --container all`. Also the install script against the published packages (`scripts/check-install-from-obs.sh --container all`) |
| Integration | headless sway 1.10 (oldest supported) and latest, plus SwayFX latest, in containers (`ci/containers/`), which also build and run the IPC client's tests and, on sway, the GUI tests (contrast in light and dark, keyboard navigation); locally `scripts/check-integration.sh --container all` |
| Commits | every PR commit against `.conventional-commit-types.json` |
| Fuzzing | sway config parser with `cargo-fuzz` (`scripts/fuzz.sh`, see [Fuzzing](#fuzzing)): lint and 60 s on every change, 15 min daily |
| Code scanning | CodeQL default setup for Rust and GitHub Actions, on every PR and weekly |

Editors pick up indentation and line endings from `.editorconfig`.

### Package builds

`scripts/build-packages.sh` builds the distribution packages in clean containers with docker or podman. It isn't a check
and `just check` doesn't run it: CI runs it on every push to `main`, weekly and by hand (`packages.yml`), not on pull
requests. Run it yourself when you change the packaging, the build or install scripts, or the dependencies:

```sh
scripts/build-packages.sh --container arch-git --local   # the AUR package, built from your HEAD commit
```

Without `--local`, the `-git` package builds GitHub's `main`, as it does for users. With it, the package is built from
the committed state of your checkout (uncommitted changes are left out). The build needs the network: it fetches the
Arch packages and the crates. When you change `packaging/aur/sway-things-git/PKGBUILD`, set its `pkgver` to the current
version of `main` (`<release>.r<commits>.g<hash>`, what `pkgver()` makes of `git describe --long --abbrev=7`) and
regenerate `.SRCINFO` next to it with `makepkg --printsrcinfo >.SRCINFO` on Arch. The build and
`scripts/check-packaging.sh` (on every pull request) fail while `.SRCINFO` is out of date and print the difference,
which is enough to fix it elsewhere.

## Fuzzing

The sway config parser reads files from shared dotfiles, so it is fuzzed: `fuzz/fuzz_targets/sway_config.rs` feeds it a
config and the files it includes, all in memory. `fuzz/` is a workspace of its own, outside `just check`. It needs
[cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz) (`cargo install cargo-fuzz --locked`, or its release binary), a
C++ compiler for libFuzzer (`c++`, or `clang++`) and the GLib headers the core builds against; the stable toolchain is
enough, as it runs without a sanitizer.

```sh
just fuzz 300                         # fuzz for 300 s (scripts/fuzz.sh run 300)
scripts/fuzz.sh reproduce FILE        # run the target on one input, e.g. from a fuzzer issue
scripts/fuzz.sh lint                  # clippy and cargo-deny on the fuzz crate
```

A run starts from the seeds in `fuzz/seeds/` and the sample configs of `ci/integration/`, bundled with the files they
include, and keeps what it learns in `fuzz/corpus/sway_config/` (not committed; `scripts/fuzz.sh minimize` shrinks it).
An input is one config; a line `#=> PATH` in it starts another file, relative to the main config's directory, and a line
`#=| PATH` puts something there that isn't a regular file, like a FIFO. An input that panics, takes more than 10 s or
more than 2 GB lands in `fuzz/artifacts/sway_config/`, and the run fails. `fuzz/sway_config.dict` gives the fuzzer the
words the parser treats specially.

The daily workflow (`.github/workflows/fuzz.yml`, also started by hand with a duration) fuzzes for 15 minutes and keeps
its corpus in the Actions cache; CI fuzzes every change for 60 s from the seeds. Inputs the fuzzer finds are uploaded as
the run's `fuzz-inputs` artifact. `scripts/fuzz.sh triage` minimizes those inputs and `scripts/fuzz.sh report` opens a
Bug issue for each, titled after where it panicked (or `timeout`, `out of memory`), with the input as base64 and hex, or
comments on the open issue with that title. To see what it would post, without posting:

```sh
scripts/fuzz.sh triage target/fuzz-reports    # from the inputs in fuzz/artifacts/sway_config/
scripts/fuzz.sh report target/fuzz-reports --dry-run
```

A fix for a parser bug the fuzzer found comes with a unit test of that input in `crates/core/src/sway_config/tests.rs`.

## Translatable strings

Every string a user reads is translatable. Write it in American English and mark it with `msg!`, which makes a `Msg`;
show it through `tr` (`sway_things_core::i18n::tr`), which gives the user's language:

```rust
Setting::toggle(TAP, Page::MouseTouchpad, true)
    .title(msg!("Tap to click"))
    .keywords(&[msg!("touchpad"), msg!("click")])
```

A comment starting with `TRANSLATORS:` right above a string goes into the template as a note for translators. The
template also holds the texts of the desktop entry and the AppStream metainfo, from `data/*.desktop.in` and
`data/*.metainfo.xml.in`. After adding, changing or removing a string, in the Rust sources or in those files, run
`scripts/update-pot.sh` to update `po/sway-things.pot`; `scripts/check-pot.sh` and a core test fail while it is out of
date. Both scripts need `xgettext` 0.24 or newer, the first version that reads Rust. Tests that need a string that is
not extracted use `Msg::new`. See [wiki/UI-Architecture.md](wiki/UI-Architecture.md#internationalization) for how
translations are found.

## Man pages and completions

The man pages in `data/man/` and the shell completions in `data/completions/` are generated from the clap definitions
and committed. After changing a command, an argument or a help text in `crates/cli`, or the argument of the app, run
`scripts/update-cli-docs.sh`; a test fails while they are out of date. A version bump and a Dependabot update of clap,
`clap_mangen` or `clap_complete` need the script too. Don't edit the generated files by hand.

## Working on the GUI

- `just gui` starts the app from source, and `just cli ARGS` runs the command line (`just cli doctor`).
  `GTK_DEBUG=interactive` opens the GTK Inspector next to it, to explore the widget tree and try CSS live.
- The GUI tests check the window with every page for contrast in light and dark, the keyboard navigation, and the
  setting rows (one per setting, a change, a locked setting, a failed change). They need a display, so they run in the
  sway integration containers (`scripts/check-integration.sh --container all`). `scripts/gui-screenshots.sh` runs them
  on a private display instead and keeps a PNG of every page in `target/screenshots/`.

See [wiki/UI-Architecture.md](wiki/UI-Architecture.md) for the structure.

## Working on the website

The project site in `website/` is built with Astro: `npm ci --prefix website`, then `just website` serves it with live
reload. It reads `README.md` and the app icon, so keep the README's sections in their shape; the build says when one
doesn't fit. See [wiki/Website.md](wiki/Website.md).

## Documentation

- Developer documentation lives in [`wiki/`](wiki/Home.md) and gets published to the GitHub wiki by CI. Edit the files
  in the repo, never on github.com.
- Link wiki pages as files (`Architecture.md#security-model`, `../README.md`), so the links work in the repo. The
  publish workflow rewrites them for the wiki.
- Write in **American English**.
- Update the wiki in the same change as the code it describes.

## Releases

Semantic Versioning, `0.x` until 1.0. Closing a milestone means tagging its release. A release pull request sets the
version (`scripts/bump-version.sh`) and adds the changelog section; the maintainer then pushes a signed tag, the release
workflow creates a draft release, and publishing it makes the openSUSE Build Service build the packages. The steps and
the dry run are in [wiki/Releasing.md](wiki/Releasing.md).

## AI-assisted contributions

AI agents follow [AGENTS.md](AGENTS.md). The same rules, checks and review standards apply to human and AI-written code.
