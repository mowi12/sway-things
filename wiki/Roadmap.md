# Roadmap

Only **v0.1** was planned in detail. After it, versions get no fixed feature list: the scope and the status of every
item live in the [Feature Catalog](Feature-Catalog.md). Milestones on GitHub correspond to releases.

## How work continues

1. **v0.x: build the catalog.** Pick the next item from the [Feature Catalog](Feature-Catalog.md), open an issue and
   implement it. Its checkbox gets ticked in the same PR; an item that turns out not to work is struck through with the
   reason.
2. **Releases.** Once a few items have landed, they ship as the next v0.x release. Releases follow the finished work,
   not a calendar.
3. **Refine.** When the main features are in, the next releases refine and improve what exists instead of adding pages.
4. **v1.0.** Released after that refinement.

## v1.0 criteria

- Every page in the sidebar is built and covers its main features. Not every catalog item has to be done; minor and
  "Maybe" items can follow after 1.0.
- The refinement phase is finished: what exists works reliably on real setups.

## v0.1: prove the pipeline

**Goal:** the whole architecture works end to end on real machines (vanilla sway and SwayFX), with low UI risk. Released
as 0.1.0 on 2026-11-14; the [changelog](../CHANGELOG.md) lists what it contains.

### Core

- [x] Settings registry with the first entries
- [x] State: `settings.toml` + host overlay + `[app]` preferences, file watcher, schema version, history snapshots
- [x] Typed sway command builders with escaping
- [x] IPC client (async-io based): run command, `get_*`, event subscriptions; sway vs. SwayFX detection
- [x] Generators with header, manifest hashes, atomic writes, `sway --validate`
- [x] Sway config parser (includes, variables, blocks, file:line) + conflict detection
- [x] Include-line detection, first-run instructions, read-only mode until enabled
- [x] CLI: `get`, `set` and `reset` (terminal countdown, or `--yes` without a TTY, for risky settings), `open`,
      `doctor`, `show-generated`
- [x] App shell: sidebar with all pages listed (unbuilt ones show a placeholder), setting-level search, keyboard
      navigation, single instance + deep links, theming with sway accent, light and dark
- [x] Narrow mode: decide the width threshold from the real pages, then document it in [UX Concept](UX-Concept.md)
- [x] Indicators: reset, overridden / your config wins, requires sway ≥ x.y, requires SwayFX
- [x] "Sway Things" preferences section (accent source, safety-net timeout, editor command, show sway directives)
- [x] gettext wiring (American English source strings only at first)

### Four pages

| Page | What it proves |
| --- | --- |
| Mouse & Touchpad | per-device scope and host overlay, input hotplug events, device disable with safety net and lockout protection, registry row builder at volume |
| Keyboard | safety net for layout changes, reorderable list widget, XKB rules parsing |
| Windows (incl. SwayFX effects) | conflict detection on `client.*`, live accent theming of the app, SwayFX gating + `90-effects.conf` |
| Window Behavior | many simple rows; cheap breadth |

### Definition of done

- Running it on real setups with hand-written configs in dotfiles repos changes **no** hand-written file (checked with
  `git status` on the dotfiles repos).
- Every setting on the four pages can be set from the GUI and the CLI, survives a sway reload, and shows conflicts with
  hand-written lines correctly.
- All CI checks are green, including the integration matrix (sway 1.10, sway latest, SwayFX latest) and the MSRV job.
- Installable from OBS (at least openSUSE Tumbleweed and Debian 13). The AUR follows after v0.1.
