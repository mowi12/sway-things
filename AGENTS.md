# AGENTS

## Overview

Sway Things is a GTK4 settings app for the sway Wayland compositor (and SwayFX), written in Rust. The project is in the
**planning phase**: the design is documented, and there is no code yet.

Prefer the wiki and existing docs over restating them. If you summarize, keep it short and link the canonical page.

## Canonical documentation

- `README.md`: user-facing overview (what, why, planned features)
- `CONTRIBUTING.md`: workflow, commit convention, engineering rules, quality gates
- `.conventional-commit-types.json`: the allowed commit types, source of truth
- `wiki/Home.md`: developer documentation index
- `wiki/Vision-and-Scope.md`: goals, target users, non-goals, prior art
- `wiki/Architecture.md`: crates, registry, state files, generators, parser, conflict detection, backends, CLI, security
  model, testing
- `wiki/UX-Concept.md`: window layout, navigation, apply model, indicators, first run, theming
- `wiki/Roadmap.md`: v0.1 scope and what comes after
- `wiki/Feature-Catalog.md`: the full feature scope per page (unordered)

## Planned structure

```text
.
├── crates/
│   ├── core/        # no GTK: settings registry, state, generators, sway config parser, IPC, backends
│   ├── cli/         # `sway-things` binary: get/set/open/doctor/hook
│   └── gui/         # `sway-things-gui`: gtk4-rs app, rows built from the registry, custom pages
├── scripts/         # one script per check (check-fmt.sh, …), the ONLY place a check command lives; helpers like sync-labels.sh
├── tools/           # hash-locked versions of the check tools installed with pip or npm
├── wiki/            # canonical developer docs, published to the GitHub wiki by CI
├── LICENSES/        # REUSE license texts
├── .github/
│   ├── workflows/   # install tools, then call scripts/ (no check logic in YAML)
│   ├── ISSUE_TEMPLATE/  # forms: bug, feature, docs, refactor + config.yml
│   ├── labels.yml   # label taxonomy, synced to GitHub
│   ├── dependabot.yml
│   └── pull_request_template.md
├── justfile         # convenience wrapper around scripts/ (`just check`)
├── Cargo.toml       # workspace: shared package metadata and lints; Cargo.lock is committed
├── clippy.toml      # lets tests use unwrap/expect/panic
├── REUSE.toml, dprint.json, .markdownlint-cli2.yaml, _typos.toml, lychee.toml, deny.toml, cliff.toml, .editorconfig
└── .conventional-commit-types.json
```

Most of this doesn't exist yet.

## Hard rules

Never break these, even if a task seems to ask for it. Ask instead.

1. **Never write a file Sway Things didn't create**, especially the user's `~/.config/sway/config`. Generated files
   carry a header and live in directories we own.
2. **Never build shell strings.** Use `std::process::Command` with argument arrays. Never `sh -c`.
3. **External strings are data.** Device names, SSIDs, Bluetooth names, file and window names go through the typed sway
   command builders.
4. **No `unsafe`** (`unsafe_code = "forbid"` in `[workspace.lints]`). **No `unwrap`/`expect`/`panic!`/`todo!`/`dbg!`**
   outside tests (clippy denies them). Never weaken the workspace lints; a needed `#[allow]` states its `reason`.
5. **No privileged code, no daemon, no network access, no telemetry.**
6. **Generated sway config must pass `sway --validate`** before it atomically replaces the old file.
7. **New settings start in the registry** (`crates/core`). UI rows, search, CLI keys and defaults all derive from it.
8. **Check commands live only in `scripts/`.** Never duplicate a check command in a workflow or the justfile; call the
   script.

## Conventions

- Commits: Conventional Commits with the types from `.conventional-commit-types.json`, scope = page id or area
  (`feat(keyboard): …`, `fix(core): …`). PRs land as **merge commits** and every PR commit stays in the history, so each
  one must be conventional and meaningful on its own. Update branches by rebasing, never by merging `main` in.
- Branches: `<type>/<issue>-<slug>` (`feat/23-keyboard-repeat-rate`), type = the PR's main purpose.
- Pull requests: required for every change, 1 approving review, all checks green. Fill in the PR template checklist.
  Link the issue with `Closes #N`; labels and milestones go on the issue, not the PR.
- Language: American English everywhere (code, UI strings, docs, commits).
- Every source file (`.rs`, scripts) starts with the REUSE SPDX header
  (`SPDX-FileCopyrightText: 2026 Sway Things contributors`, `SPDX-License-Identifier: GPL-3.0-or-later`). Everything
  else is covered by `REUSE.toml`; add a pattern there for a new file type without a header.
- Before finishing a change, run `just fmt` and `just check` (or the relevant `scripts/check-*.sh`).
- Docs: update `wiki/` in the same change, so the docs always describe the current state.
- Target platform: sway ≥ 1.10 and SwayFX ≥ 0.5; GTK ≥ 4.18; Rust MSRV 1.85 (Debian 13). Don't use language features or
  crates that need a newer rustc unless the maintainers agree to raise the MSRV.
- Crates: every crate in `crates/` inherits the `[workspace.package]` fields and sets `[lints] workspace = true`. Shared
  dependency versions go in `[workspace.dependencies]`.
- Async: glib main loop + async-io based crates (zbus, swayipc-async, async-process). No tokio; `crates/core` stays
  executor-agnostic.
- License: `GPL-3.0-or-later`. New dependencies must be GPL-compatible (enforced by cargo-deny).
- Workflows: pin every action to a full commit SHA with a version comment (`@<sha> # v7.0.1`), set `permissions: {}` at
  the top and grant each job only what it needs. Downloaded tools are pinned to a version and verified against their
  SHA-256 checksum; packages from Ubuntu's signed repositories need no pin. Every workflow, job and step `name` is in
  Title Case (`Preview Label Changes`, `Check Out Repository`).
- Labels: edit `.github/labels.yml`, never labels on github.com (the sync deletes unknown labels).

## Where to look

| Task | Location |
| --- | --- |
| What is in scope? | `wiki/Vision-and-Scope.md`, `wiki/Feature-Catalog.md` |
| How does a setting flow from UI to sway? | `wiki/Architecture.md` → "Data flow" |
| How must a page behave? | `wiki/UX-Concept.md` |
| What's next? | `wiki/Roadmap.md` |
