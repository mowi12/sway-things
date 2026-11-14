# Sway Things Developer Wiki

**Sway Things** is a settings app for the sway Wayland compositor and its companion tools: a searchable sidebar of
pages, instant apply with a safety net, and a strict promise never to edit files it didn't create. Written in Rust with
GTK4. Linux only. Supports sway ≥ 1.10 and SwayFX.

This wiki is for developers, human and AI. The user-facing overview is the [README](../README.md).

> The source of these pages is the `wiki/` folder in the repository. CI publishes it to the GitHub wiki. Edit the files
> in the repo, never on github.com.

## Pages

| Page | What it covers |
| --- | --- |
| [Vision & Scope](Vision-and-Scope.md) | Problem, target users, goals, non-goals, prior art |
| [Architecture](Architecture.md) | Crates, settings registry, state files, generators, command builders, parser, conflicts, backends, CLI, security, testing, packaging |
| [Sway Reference](Sway-Reference.md) | Verified facts about sway, SwayFX, the companion tools and the distributions, with sources |
| [UX Concept](UX-Concept.md) | Window layout, navigation, search, apply model, indicators, first run, theming |
| [Roadmap](Roadmap.md) | How work continues after v0.1, v1.0 criteria, the v0.1 scope |
| [Releasing](Releasing.md) | The release steps, the dry run and snapshots between releases |
| [Feature Catalog](Feature-Catalog.md) | The complete feature scope, per page, unordered, with the status of every item |

## Status

Version 0.1.0 is released: the core, the CLI and the app with the four pages of the v0.1 scope, packaged on the openSUSE
Build Service. What v0.1 contains and how work continues is in the [Roadmap](Roadmap.md); the changes per release are in
the [changelog](../CHANGELOG.md).
