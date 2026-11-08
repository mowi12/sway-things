# Contributing to Sway Things

Thanks for your interest! Sway Things is in the **planning phase**. There's no code yet, but the design is written down
and open for discussion. Please follow our [Code of Conduct](CODE_OF_CONDUCT.md), and report security issues privately
as described in [SECURITY.md](SECURITY.md), never in public issues.

## Read first

1. [wiki/Home.md](wiki/Home.md): the developer documentation index
2. [wiki/Vision-and-Scope.md](wiki/Vision-and-Scope.md): what we build and what we deliberately don't
3. [wiki/Architecture.md](wiki/Architecture.md): how it works, including the security rules

## Issues

- **Issues on demand.** The full scope lives in [wiki/Feature-Catalog.md](wiki/Feature-Catalog.md). Open an issue when
  you start or plan an item, not before.
- Use the **issue forms** (Bug, Feature, Docs, Refactor). Blank issues are disabled.
- **Labels** are defined in [.github/labels.yml](.github/labels.yml): `kind:`, `area:`, `page:`, `effort:`, `status:`,
  `compat:swayfx`, plus `good first issue` and `help wanted`. To change a label, edit the file. The PR shows a preview,
  and `scripts/sync-labels.sh` applies it after merge. Labels created on github.com get deleted.
- **Milestones** are releases. Only `v0.1` exists for now.
- **No fixed ownership.** Anyone may pick any issue: assign yourself, and talk in the issue before starting something
  large.

## Pull requests

- **Trunk-based.** `main` is the only long-lived branch and is always green.
- **Branch names:** `<type>/<issue>-<slug>`, where the type is the conventional commit type of the PR's main purpose:
  `chore/2-add-issue-forms`, `feat/23-keyboard-repeat-rate`.
- **Every change goes through a pull request**:
  - all required CI checks must pass, and **nobody can bypass them**;
  - **one approving review** is required (the repository owner may bypass the review, never CI);
  - new commits dismiss earlier approvals, and all review conversations must be resolved;
  - no force pushes to `main`.
- **Merge commits only**: a PR lands as a merge commit ("Merge pull request #N from …"), and every commit of the PR
  stays in the history as is. Keep commits meaningful and conventional, and clean them up with an interactive rebase
  before merging. Branches are deleted automatically after merge.
- **Updating your branch:** when `main` has moved on, use **Update with rebase** (or rebase locally), never a merge
  commit. A "Merge branch 'main' into …" commit would end up in the history and isn't a conventional commit.
- **Auto-merge** is available: enable it, and the PR merges once it's approved and all checks are green.
- Keep PRs small and fill in the PR template checklist (tick each item, or cross it out with a reason).
- **Labels and milestones go on the issue**, not on the PR. Link the issue in the PR description with `Closes #N`.

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

Commit messages feed the generated `CHANGELOG.md` ([git-cliff](https://git-cliff.org/)), so write them for users, in
American English.

## Engineering rules (non-negotiable)

Rules 1–7 protect users' systems; rules 8–10 keep the code base consistent and the supply chain safe. Details and
reasons are in [wiki/Architecture.md](wiki/Architecture.md).

1. **Never write a file Sway Things didn't create.** The only exceptions are official programmatic stores
   (gsettings/dconf, D-Bus services). The user's sway config is never touched.
2. **Never build shell strings.** Run processes with argument arrays. Never `sh -c`.
3. **Treat external strings as data.** Device names, SSIDs, Bluetooth names, file names and window titles can be
   attacker-controlled. Escape them with the typed sway command builders. Never interpolate them.
4. **No `unsafe`.** The root `Cargo.toml` forbids `unsafe_code` for every crate.
5. **No panics in non-test code.** `unwrap`, `expect`, `panic!`, `todo!` and `dbg!` are denied by clippy. Handle errors.
6. **No privileged code.** System changes only go through existing polkit-guarded D-Bus services.
7. **Validate before replacing.** Generated sway config must pass `sway --validate` before it replaces the old file.
   Writes are atomic. **No network access, no telemetry.**
8. **No tokio, executor-agnostic core.** Async I/O uses async-io based crates on the glib main loop.
9. **REUSE headers.** Every source file (`.rs`, scripts) starts with:

   ```text
   SPDX-FileCopyrightText: 2026 Sway Things contributors
   SPDX-License-Identifier: GPL-3.0-or-later
   ```

   Files that can't carry a header (Markdown, YAML, JSON, TOML, …) are covered by [REUSE.toml](REUSE.toml). A new file
   type without a header gets a pattern there, otherwise `reuse lint` fails.

10. **Pin GitHub Actions to a full commit SHA**, with the version as a comment:
    `uses: actions/checkout@<40-character SHA> # v7.0.1`. Tags can be moved; a SHA can't. Dependabot keeps the pins
    current. Tools a workflow downloads are pinned to a version and verified against their SHA-256 checksum; packages
    from Ubuntu's signed repositories need no pin. Give every workflow job only the `permissions` it needs. Workflow,
    job and step names use Title Case (`Preview Label Changes`).

## Checks

**Every check is one script in `scripts/`, and that script is the only place its command is written down**. CI calls the
scripts directly; the `justfile` just wraps them for convenience. There are no git hooks. Run checks yourself before
pushing:

```sh
just check              # run every check, like CI
scripts/check-fmt.sh    # or run a single check directly
just fmt                # format Rust, Markdown, TOML, YAML and JSON
```

Each script names the tools it needs in its header. Tools from npm and PyPI are pinned with hashes in `tools/`:
`npm ci --prefix tools` installs the npm ones where the scripts find them; install the PyPI ones into a virtual
environment with `pip install --require-hashes -r tools/requirements.txt`.

| Check | Tools |
| --- | --- |
| Formatting | `rustfmt` (default style); `dprint` for Markdown (120 columns), TOML, YAML, JSON |
| Rust lints | `clippy` with `-D warnings`, pedantic + safety picks; every `#[allow]` needs a `reason` |
| Other lints | `markdownlint-cli2`, `actionlint`, `shellcheck`, `desktop-file-validate`, `appstreamcli validate` |
| Spelling | `typos`, locale en-US |
| Links | `lychee --offline` (relative links + anchors); external links weekly, dead ones open a `kind:docs` issue |
| Licensing | `reuse lint` |
| Dependencies | `cargo deny check` (advisories, GPL-compatible licenses, sources) |
| Tests | unit + golden-file tests on MSRV **1.85** and stable |
| Integration | headless sway 1.10 (oldest supported) and latest, plus SwayFX latest |
| Commits | every PR commit against `.conventional-commit-types.json` |
| Fuzzing | sway config parser (scheduled) |
| Code scanning | CodeQL default setup for Rust and GitHub Actions, on every PR and weekly |

Editors pick up indentation and line endings from `.editorconfig`.

## Documentation

- Developer documentation lives in [`wiki/`](wiki/Home.md) and gets published to the GitHub wiki by CI. Edit the files
  in the repo, never on github.com.
- Link wiki pages as files (`Architecture.md#security-model`, `../README.md`), so the links work in the repo. The
  publish workflow rewrites them for the wiki.
- Write in **American English**.
- Update the wiki in the same change as the code it describes.

## Releases

Semantic Versioning, `0.x` until 1.0. Closing a milestone means tagging its release. Pushing a version tag triggers the
release workflow: a source tarball with vendored crates for the openSUSE Build Service and the AUR, and the changelog
generated from the commits.

## AI-assisted contributions

AI agents follow [AGENTS.md](AGENTS.md). The same rules, checks and review standards apply to human and AI-written code.
