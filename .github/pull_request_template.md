Closes #<!-- issue number -->

## What I have made

<!-- What was added, removed or changed, and why. E.g. "I added the key repeat rows to the Keyboard page." -->

## Screenshots

<!-- For UI changes: before and after. Delete this section otherwise. -->

## Checklist

Either tick an item or cross it out (`~~like this~~`) and give a reason why it doesn't apply.

### Author

- [ ] Every commit is a meaningful [Conventional Commit](https://github.com/mowi12/sway-things/blob/main/CONTRIBUTING.md#commit-messages), because each one lands on `main`
- [ ] I have added tests (unit, golden files, integration) that prove my fix works or my feature behaves as intended
- [ ] I have manually tested my changes on sway and/or SwayFX
- [ ] I have updated the docs: `wiki/`, `README.md` for user-facing changes, and `AGENTS.md` if structure, tooling or conventions changed

### Reviewer

- [ ] The changes match the linked issue
- [ ] The [engineering rules](https://github.com/mowi12/sway-things/blob/main/CONTRIBUTING.md#engineering-rules-non-negotiable) are respected. Checks cover the rest; these need a human eye: no writes to files Sway Things didn't create, generated sway config validated before it replaces the old file, no shell strings, external strings only through the typed command builders, no privileged code, no network access or telemetry, no tokio, least-privilege workflow permissions
