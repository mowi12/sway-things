# Roadmap

Only **v0.1** is planned in detail. Everything after it is an **unordered backlog** in the
[Feature Catalog](Feature-Catalog.md). Features aren't mapped to later versions; priorities get set once v0.1 is done.
Milestones on GitHub correspond to releases.

## v0.1: prove the pipeline

**Goal:** the whole architecture works end to end on real machines (vanilla sway and SwayFX), with low UI risk.

### Core

- [x] Settings registry with the first entries
- [x] State: `settings.toml` + host overlay + `[app]` preferences, file watcher, schema version, history snapshots
- [x] Typed sway command builders with escaping
- [x] IPC client (async-io based): run command, `get_*`, event subscriptions; sway vs. SwayFX detection
- [x] Generators with header, manifest hashes, atomic writes, `sway --validate`
- [x] Sway config parser (includes, variables, blocks, file:line) + conflict detection
- [ ] Include-line detection, first-run instructions, read-only mode until enabled
- [ ] CLI: `get`, `set` (terminal countdown, or `--yes` without a TTY, for risky settings), `reset`, `open`, `doctor`,
      `show-generated`
- [ ] App shell: sidebar with all pages listed (unbuilt ones show a placeholder), setting-level search, keyboard
      navigation, single instance + deep links, theming with sway accent, light and dark
- [ ] Narrow mode: decide the width threshold from the real pages, then document it in [UX Concept](UX-Concept.md)
- [ ] Indicators: reset, overridden / your config wins, requires sway ≥ x.y, requires SwayFX
- [ ] "Sway Things" preferences section (accent source, safety-net timeout, editor command, show sway directives)
- [ ] gettext wiring (American English source strings only at first)

### Four pages

| Page | What it proves |
| --- | --- |
| Mouse & Touchpad | per-device scope and host overlay, input hotplug events, device disable with safety net and lockout protection, registry row builder at volume |
| Keyboard | safety net for layout changes, reorderable list widget, XKB rules parsing |
| Windows (incl. SwayFX effects) | conflict detection on `client.*`, live accent theming of the app, live preview widget, SwayFX gating + `90-effects.conf` |
| Window Behavior | many simple rows; cheap breadth |

### Definition of done

- Running it on real setups with hand-written configs in dotfiles repos changes **no** hand-written file (checked with
  `git status` on the dotfiles repos).
- Every setting on the four pages can be set from the GUI and the CLI, survives a sway reload, and shows conflicts with
  hand-written lines correctly.
- All CI checks are green, including the integration matrix (sway 1.10, sway latest, SwayFX latest) and the MSRV job.
- Installable from OBS (at least openSUSE Tumbleweed) and the AUR (`-git`).

## After v0.1

Pick items from the [Feature Catalog](Feature-Catalog.md) and open issues when work starts. 1.0 criteria get defined
once the backlog priorities are clearer.
