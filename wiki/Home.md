# Sway Things Developer Wiki

**Sway Things** is a settings app for the sway Wayland compositor and its companion tools: a searchable sidebar of
pages, instant apply with a safety net, and a strict promise never to edit files it didn't create. Written in Rust with
GTK4. Linux only. Supports sway ≥ 1.10 and SwayFX ≥ 0.5.

This wiki is for developers, human and AI. The user-facing overview is the [README](../README.md) and the website
[sway-things.org](https://sway-things.org).

> The source of these pages is the `wiki/` folder in the repository. CI publishes it to the GitHub wiki. Edit the files
> in the repo, never on github.com.

## Pages

| Page | What it covers |
| --- | --- |
| [Vision & Scope](Vision-and-Scope.md) | Problem, target users, goals, non-goals, prior art |
| [Architecture](Architecture.md) | Overview: crates and modules, settings registry, state files, generated files, version gating, security model; links to the topic pages below |
| ↳ [Applying Settings](Applying-Settings.md) | Data flow of a change, safety net, apply modes, typed sway command builders |
| ↳ [Reading Sway](Reading-Sway.md) | IPC, input devices, themes, XKB rules, config parser, include and conflict detection, key bindings |
| ↳ [Outputs](Outputs.md) | Displays, kanshi profiles, workspace placement, mirroring |
| ↳ [Backends](Backends.md) | BlueZ, NetworkManager, the sound server, GSettings |
| ↳ [Command Line](Command-Line.md) | Commands, doctor, hooks |
| ↳ [UI Architecture](UI-Architecture.md) | The GTK app and translations |
| ↳ [Testing](Testing.md) | Which tests exist and where they run |
| ↳ [Packaging](Packaging.md) | Tarball, packages, OBS, install script, AUR |
| ↳ [Website](Website.md) | sway-things.org |
| [Sway Reference](Sway-Reference.md) | Verified facts about sway, SwayFX, the companion tools and the distributions, with sources |
| [UX Concept](UX-Concept.md) | Window layout, navigation, search, apply model, indicators, first run, theming |
| [Roadmap](Roadmap.md) | How work continues after v0.1, v1.0 criteria, the v0.1 scope |
| [Releasing](Releasing.md) | The release steps and the dry run |
| [Feature Catalog](Feature-Catalog.md) | The complete feature scope, per page, unordered, with the status of every item |

## Status

Version 0.4.0 is released: the core, the CLI and the app with the four pages of the v0.1 scope, the Bluetooth and
Network pages, the Cursor & Tablet, Displays, Sound, Apps Look & Feel, Workspaces and Wallpaper pages and the system
details on About & Maintenance, packaged on the openSUSE Build Service. What v0.1 contained and how work continues is in
the [Roadmap](Roadmap.md); the changes per release are in the [changelog](../CHANGELOG.md).
