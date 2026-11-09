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
| [UX Concept](UX-Concept.md) | Window layout, navigation, search, apply model, indicators, first run, theming |
| [Roadmap](Roadmap.md) | v0.1 scope and what comes after |
| [Feature Catalog](Feature-Catalog.md) | The complete feature scope, per page, unordered |

## Status

Planning phase, no code yet. The first milestone is v0.1, described in the [Roadmap](Roadmap.md).
