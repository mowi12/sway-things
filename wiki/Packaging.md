# Packaging

> Part of the [architecture](Architecture.md). The release tarball, the RPM and Debian packages, the openSUSE Build
> Service, the install script, the AUR and the wallpaper photos kept out of them. The release steps are in
> [Releasing](Releasing.md).

- **openSUSE Build Service** (OBS) builds the openSUSE Tumbleweed, Fedora, Debian 13 and Ubuntu 26.04 packages (and,
  best effort, Fedora Rawhide and Debian unstable) from one project, from the release tarball with the crates vendored,
  offline (see [Release tarball](#release-tarball), [RPM packages](#rpm-packages) and
  [Debian packages](#debian-packages)). **AUR**: release and `-git` packages (see [AUR](#aur)).
- Shipped: both binaries, `io.github.mowi12.SwayThings.desktop`, AppStream metainfo, icon, man pages (`clap_mangen`),
  shell completions (`clap_complete`), translations. Every package installs the same files the same way, through two
  plain bash scripts that need neither `just` nor the generators:
  - **`scripts/build.sh [--data-only] [-- <cargo build arguments>]`** builds into `target/<profile>/`: the binaries
    (`cargo build --locked --profile $PROFILE`, default members only), the desktop entry and metainfo with the
    translations merged in (`scripts/merge-metadata.sh`, `msgfmt`), and a `.mo` file per language in `po/LINGUAS`. The
    binaries look for translations in `$PREFIX/share/locale`, so `build.sh` passes that as `SWAY_THINGS_LOCALEDIR` to
    cargo and stamps `$PREFIX` into `target/<profile>/data/prefix`. `--data-only` skips cargo, for a packaging macro
    that already built the binaries with the same `PROFILE`; it fails if they would look for translations elsewhere.
  - **`scripts/install.sh [--uninstall]`** only copies: the built files and the committed man pages, completions and
    icons, into `$DESTDIR$PREFIX`. It refuses a `PREFIX` other than the one of the build. `--uninstall` removes the same
    files (never directories) without a build. Neither script refreshes the desktop, icon or man caches: packages do
    that in their triggers.
  - Environment: `PREFIX` (default `/usr/local`), `DESTDIR` (install only), `PROFILE` (cargo profile, default `release`;
    `dev` is `target/debug`), `CARGO_TARGET_DIR`. `just build`, `just install` and `just uninstall` wrap them.
- Install layout, below `$DESTDIR$PREFIX`:

  | Path | From |
  | --- | --- |
  | `bin/sway-things`, `bin/sway-things-gui` (755) | cargo |
  | `share/applications/io.github.mowi12.SwayThings.desktop` | `data/io.github.mowi12.SwayThings.desktop.in`, merged |
  | `share/metainfo/io.github.mowi12.SwayThings.metainfo.xml` | `data/io.github.mowi12.SwayThings.metainfo.xml.in`, merged |
  | `share/icons/hicolor/scalable/apps/io.github.mowi12.SwayThings.svg` | `data/icons/hicolor/scalable/apps/` |
  | `share/icons/hicolor/symbolic/apps/io.github.mowi12.SwayThings-symbolic.svg` | `data/icons/hicolor/symbolic/apps/` |
  | `share/man/man1/sway-things.1`, `sway-things-<command>.1`, `sway-things-gui.1` | `data/man/`, uncompressed |
  | `share/bash-completion/completions/sway-things` | `data/completions/sway-things.bash` |
  | `share/zsh/site-functions/_sway-things` | `data/completions/_sway-things` |
  | `share/fish/vendor_completions.d/sway-things.fish` | `data/completions/sway-things.fish` |
  | `share/locale/<language>/LC_MESSAGES/sway-things.mo` | `po/<language>.po` for each language in `po/LINGUAS` |

- What packagers run: `PREFIX=/usr scripts/build.sh -- --frozen` (or `--offline`), then
  `PREFIX=/usr DESTDIR=<staging directory> scripts/install.sh`. With a cargo macro such as `%cargo_build`, the build
  step is `PREFIX=/usr PROFILE=<the macro's profile> scripts/build.sh --data-only`. Build dependencies: Rust ≥ 1.85, the
  GTK 4.18 development files and gettext (`msgfmt`, with the AppStream ITS rules of gettext or of the appstream
  package). The desktop entry is GPL-3.0-or-later; the metainfo is CC0-1.0, as AppStream asks of metadata.
- What packagers adapt: Debian and Ubuntu's zsh doesn't search `/usr/share/zsh/site-functions`, so the Debian packaging
  moves the zsh completion to `/usr/share/zsh/vendor-completions/`, as `dh_installzshcompletions` does.
- No Flatpak. Aim for official distro repositories after 1.0; for Debian,
  [#138](https://github.com/mowi12/sway-things/issues/138) tracks the way into the archive.
- **App icon:** a window whose lower part is an aqua wave, with a short divider above it: tiling softened by sway, in
  sway's focus blue (`#285577`) and aqua (`#2fb5a8`). Hand-written SVG on GNOME's app icon grid: a 128 px canvas with
  the window frame on the 104 px square keyline (12 px margin), whole-number coordinates and one clip path. The symbolic
  icon draws the same window, divider and wave at 16 px, as filled shapes only (GTK recolors fills, not strokes) on
  whole pixels. Both are GPL-3.0-or-later, with the SPDX header in an XML comment. The desktop entry and the metainfo
  name the icon by the app ID; the RPM specs list both files, and the openSUSE spec needs `hicolor-icon-theme` for the
  directories.
- License: `GPL-3.0-or-later`, the norm for GTK desktop apps and what packagers expect; _or later_ keeps it compatible
  with future GPL versions without asking every contributor to relicense.

## Release tarball

`scripts/make-tarball.sh [--ref REF] [--out DIR] [--check]` makes `sway-things-<version>.tar.xz`, the one source every
package builds from:

- The tree of `REF` (`git archive`, so uncommitted files never get in), plus `vendor/`
  (`cargo vendor --locked --versioned-dirs`, every crate of `Cargo.lock`) and a `.cargo/config.toml` that replaces
  crates.io with `vendor/`. So a plain `cargo build` in the unpacked tarball works offline. The script fails if
  vendoring changed `Cargo.lock`. The whole tree, because the unit tests read `ci/integration/` and `po/`.
- **Version:** `X.Y.Z` for a tag `vX.Y.Z`, which must match `[workspace.package] version` and both specs. Any other
  `REF` is a snapshot, `<version>+git<commit count>.g<short hash>` (`0.0.0+git286.g4a10379`), which sorts after
  `<version>` and before the next release in rpm and dpkg; the script writes it into the specs.
- **Reproducible:** sorted names, the commit time as every file's time, no owners, modes independent of the umask,
  single-threaded `xz -9`. `--check` makes everything twice and fails unless the bytes agree; the package workflow runs
  it.
- Next to the tarball (default `target/dist/`): its `.sha256`, and the OBS inputs under names that don't change with the
  version: `sway-things-openSUSE_Tumbleweed.spec` (the openSUSE spec), `sway-things.spec` (the Fedora spec),
  `sway-things.dsc` and `debian.tar.gz` (see [Debian packages](#debian-packages)).

## RPM packages

Two specs, each following its distribution's guidelines, from the same tarball:

| | `packaging/rpm/opensuse/sway-things.spec` | `packaging/rpm/fedora/sway-things.spec` |
| --- | --- | --- |
| Macros | `cargo-packaging`: `%cargo_build --locked` (release profile, `cargo auditable`), `%cargo_test` | `cargo-rpm-macros`: `%cargo_prep -v vendor`, `%cargo_build` (`rpm` profile), `%cargo_test` |
| Install | `PROFILE=release scripts/build.sh --data-only`, then `scripts/install.sh` | `PROFILE=rpm`, the same |
| Licenses | `License` | `License`, `SourceLicense`, `LICENSE.dependencies` and `cargo-vendor.txt` (bundled crates as `Provides: bundled(crate(…))`) |
| Completion directories | owns the zsh and fish ones (`%dir`) | owned by `filesystem` (`%{zsh_completions_dir}`, …) |

- Both build only the default members (no `crates/cli-docs`). `%check` runs `cargo test --workspace --locked` offline
  (unit, golden-file and binary tests, and the freshness test of the committed man pages and completions; the ones that
  need sway or a display are `#[ignore]`d), then `desktop-file-validate` and `appstreamcli validate`.
- `Requires: (sway or swayfx)`, a rich dependency both rpm families support; Fedora has no SwayFX package.
- Fedora `Recommends: webp-pixbuf-loader`, so swaybg and the Wallpaper page read WebP pictures; openSUSE's main
  repository has no such loader.
- Hardening flags and debug info come from the distribution macros; `Cargo.toml` sets no profile that would override
  them.
- **License tags:** `License` is every license expression of the crates linked into the binaries (as Fedora's
  `%cargo_license_summary` counts them: no build, dev or proc-macro dependencies, every target), plus `CC0-1.0` of the
  metainfo. Fedora's `SourceLicense` covers the whole tarball: every crate in `vendor/` and the licenses in `LICENSES/`
  that are in the tarball (not the wallpaper photos' license, which `export-ignore` keeps out with the photos). Each is
  a `%{shrink:…}` with one expression per line; `scripts/check-packaging.sh` fails when one is out of date, so a
  dependency with a new license is caught on its pull request.
- **Source:** the tarball of the GitHub release, written out
  (`https://github.com/mowi12/sway-things/releases/download/v%{version}/…`), not built from `%{url}`: `URL` is the
  homepage, which hosts no releases, and OBS's `download_files` service fetches exactly this URL.
  `scripts/check-packaging.sh` fails when a spec's source isn't the release URL of the repository in `Cargo.toml`.
- No `%changelog` and no `.changes` file: an OBS home project doesn't need them, and the release notes live on GitHub.
  OBS sets `Release` itself.

## Debian packages

`packaging/debian/` holds `control` and `rules`; `make-tarball.sh` adds what depends on the version and the crates:

- **`debian.tar.gz`:** `packaging/debian/` as `debian/`, plus a `changelog` with one entry for this version (revision
  `-1`, distribution `unstable`, dated at the commit) and a `copyright` file.
- **`sway-things.dsc`:** the input of OBS's debtransform, with `Format: 3.0 (quilt)`, the version, the maintainer, the
  `Build-Depends` taken from `control`, and `DEBTRANSFORM-TAR` naming the tarball. debtransform makes the source package
  from the tarball and `debian.tar.gz` (whose entries it adds as they are, so they start with `debian/`) and writes
  `debian/source/format` itself.
- **`copyright`** (machine-readable format 1.0), from `scripts/debian-copyright.sh`: our files as `GPL-3.0-or-later`,
  except those whose REUSE information says otherwise; a paragraph per crate in `vendor/` with its license expression
  and authors from `cargo metadata` (else "The `<crate>` developers", so the copyright lines of crates are approximate).
  Licenses in `/usr/share/common-licenses` are referred to; the others get their text from `LICENSES/` or the first
  vendored crate that ships one (`LICENSE-MIT`, `UNLICENSE`, …; the LLVM exception without the Apache license it
  follows). A license it finds no text for fails the tarball.
- **`rules`:** plain `dh` around `scripts/build.sh` and `scripts/install.sh`, with `CARGO_HOME` in the build directory,
  the linker flags of `dpkg-buildflags` passed to rustc, and line tables for the `-dbgsym` package. Not `dh-cargo`: it
  builds against Debian's packaged crates, not vendored ones. `dh_clean` keeps the vendored `Cargo.toml.orig` files,
  which the crates' checksums cover. `dh_auto_test` runs `cargo test --workspace --offline --locked` (skipped with
  `nocheck`); the install step validates the desktop entry and metainfo and moves the zsh completion to
  `/usr/share/zsh/vendor-completions/`.
- **Maintainer:** `Sway Things Maintainers <packaging@sway-things.org>`. `Depends: sway (>= 1.10) | swayfx`,
  `Recommends: webp-pixbuf-loader` (WebP wallpapers).
- Debian 13's rustc is exactly the MSRV (1.85), so raising the MSRV needs trixie-backports in the OBS repository.

## openSUSE Build Service

Project `home:mowi12:sway-things`, package `sway-things`, repositories `openSUSE_Tumbleweed`, the supported Fedora
releases (`Fedora_<N>`), `Debian_13` and `xUbuntu_26.04`, for x86_64 and aarch64; Tumbleweed only for x86_64, since
openSUSE builds aarch64 in another project (`openSUSE:Factory:ARM`), which as a second path breaks the x86_64 build and
as a repository of its own would need another copy of the openSUSE spec. OBS signs the published repositories with the
project's key.

- **Development branches, best effort:** `Fedora_Rawhide` and `Debian_Unstable` build from the same files (the Fedora
  spec; the `.dsc` and `debian.tar.gz`). They have no container in the package CI, and a failure there, often a library
  transition in the distribution, doesn't block a release.

- **Which spec builds where:** an OBS package holds both specs. Among several, OBS takes `<package>-<repository>.spec`
  for that repository, else `<package>.spec`
  ([Multiple Build Description File Handling](https://openbuildservice.org/help/manuals/obs-user-guide/cha-obs-multibuild)).
  So `sway-things-openSUSE_Tumbleweed.spec` builds in `openSUSE_Tumbleweed` and `sway-things.spec` in every other RPM
  repository, so a new Fedora release needs no new file. The match is on the repository's name in the project, so the
  Tumbleweed repository must keep its default name `openSUSE_Tumbleweed`. Files fetched by a source service count under
  their plain names. Another openSUSE repository (Slowroll, Leap) would need its own copy,
  `sway-things-<repository>.spec`.
- **Debian and Ubuntu:** the Debian repositories build from `sway-things.dsc` with `debian.tar.gz`, through
  debtransform.
- **Sources before v0.1:** the maintainer uploads a snapshot (`make-tarball.sh`, then `osc`).
- **Sources after v0.1:** the package's `_service` fetches the specs, `sway-things.dsc` and `debian.tar.gz` from the
  latest GitHub release (`download_url`), then the tarball the specs name (`download_files`). The services run on the
  server; the release workflow triggers them after the release is published, with an OBS token bound to the package that
  can do nothing but run them.

## Package CI

- `scripts/build-packages.sh --container NAME|all [--tarball FILE]` builds the packages in containers from
  `ci/packages/<name>/Dockerfile` (digest-pinned images): `opensuse-tumbleweed`, `fedora` (the newest release),
  `debian-13` and `ubuntu-26.04`. Without `--tarball`, it makes one from `HEAD`.
  - The image holds the build dependencies of the spec or of `control` (with the network); the build runs with
    `--network none`, as an unprivileged user, like on OBS.
  - Then `rpmlint` checks the source and the main package, not the debug packages. Any error in its report fails the
    build (openSUSE's rpmlint exits 0 even with errors). Left out: `no-changelogname-tag` (the specs have no
    `%changelog`) and, on openSUSE, `invalid-spec-name` (the spec is named for its OBS repository).
  - Debian and Ubuntu: `dpkg-buildpackage --build=binary` from the tarball and `debian.tar.gz`, then `lintian` on the
    `.changes`: any error fails, warnings are printed. Left out on Ubuntu: `bad-distribution-in-changes-file` (the
    changelog names Debian's `unstable`, as OBS builds both from it). The zsh completion must be in `vendor-completions`
    and nowhere else.
  - The package is installed (`rpm --nosignature`, as it is unsigned and OBS signs the repository; `dpkg --install`) and
    runs `sway-things --version` and `--help`. sway is installed for the dependency, never started.
- `.github/workflows/packages.yml` runs it on every push to `main`, weekly, by hand, and from the release workflow
  (`workflow_call` with the tarball of the tag). It makes the tarball once with `--check` and builds every package from
  it. It isn't a required check on pull requests (too slow); `scripts/check-packaging.sh` is, and catches license and
  version drift and a wrong source URL there.

## The install script

`scripts/install-from-obs.sh` installs from OBS with one command, `curl -fsSL https://sway-things.org/install | sh` (the
website serves it as `/install` and `/install.sh`, see [Website](Website.md)).

- **Plain POSIX `sh`**, so it runs piped into `sh`, `bash`, `dash`, `zsh` or busybox `sh`; everything runs from the last
  line, so a download cut off halfway runs nothing. Downloads go over HTTPS only, with `curl` or `wget`.
- **What it finds:** the distribution from `/etc/os-release` (`ID`, `VERSION_ID`; Fedora Rawhide by
  `REDHAT_SUPPORT_PRODUCT_VERSION`, Debian unstable by its missing `VERSION_ID`) and the architecture, mapped to the OBS
  repository. Derivatives aren't mapped to their base (nobody tested them), and Fedora releases count only once OBS has
  their repository. Anything else changes nothing and points to building from source.
- **The key:** OBS signs every repository of the project with one key, whose fingerprint is pinned in the script. The
  script computes the fingerprint of the downloaded key itself (the SHA-1 of the version 4 public-key packet, with
  `base64`, `od` and `sha1sum`), since minimal Debian and Ubuntu systems have no `gpg`, and stops if it differs. It
  walks every packet and refuses a file with a second key (in the same block or another one) or a subkey, since rpm and
  apt would trust every key in the file. A key extended by OBS keeps its fingerprint; a new key needs a new script.
- **What it does:** imports the checked key into rpm and adds the repository (zypper's `addrepo`; for dnf it writes the
  `.repo` file as OBS has it, since `dnf config-manager` comes only with a plugin minimal installs lack), or writes the
  armored key to `/etc/apt/keyrings/sway-things.asc` and a deb822 `sway-things.sources` with `Signed-By` (replacing the
  `.list` and `.gpg` the README used to have users make, but only if the `.list` is exactly that entry), then installs
  or updates the package. Without `--yes`, every question goes to `/dev/tty`, since standard input is the script itself:
  its own (before the package manager installs sway along) and the package manager's; without a terminal it needs
  `--yes`, and says so. A command that fails stops the script with what failed. `--dry-run` prints every command,
  `--uninstall` removes the package, the repository and the key (from rpm 6 with `rpmkeys --delete` and the full
  fingerprint, from rpm 4 as the package `gpg-pubkey-<last eight digits>`).
- **Tests:** `scripts/check-install-from-obs.sh` (CI job _Check Install Script_) runs dry runs under dash, bash and
  busybox `sh` in a clean Debian container, for each supported system and for unsupported ones, and checks the
  fingerprint it computes from `ci/install/home_mowi12_sway-things.asc` (and that a file with two keys is refused), and
  that without a terminal and `--yes` it stops and says why. With `--container NAME|all`, it installs from the published
  repository in clean containers of every distribution, piped into `sh` on a terminal (`script`) and answering there as
  a user would, runs `sway-things --version`, installs again with `--yes` (which must keep the repository: zypper and
  dnf say it is there already, apt's files stay the same) and uninstalls (after which the package, the repository and
  the key, by either name, must be gone); `.github/workflows/packages.yml` does that on every push to `main` and weekly.

## AUR

- `packaging/aur/sway-things-git/` holds the `PKGBUILD`, its `.SRCINFO` and the `LICENSE` of the AUR package
  `sway-things-git`, which builds the latest commit of `main`. The files are 0BSD, as the AUR asks of package sources
  (`.SRCINFO` through `REUSE.toml`, `LICENSE` for the AUR repository).
- The `PKGBUILD` follows the Arch Rust and VCS package guidelines:
  - `prepare()` runs `cargo fetch --locked --target host-tuple`; everything after it is offline and locked (`--frozen`).
    No `--all-features`: it would turn on the core's `test-support` feature.
  - `build()` is `PREFIX=/usr scripts/build.sh -- --frozen`, `check()` runs the unit tests with
    `cargo test --workspace --frozen`, and `package()` is `PREFIX=/usr DESTDIR="$pkgdir" scripts/install.sh`.
  - `pkgver()` counts from the last annotated release tag (`0.1.0.r5.g1a2b3c4`); before the first one it is
    `0.0.0.r<commits>.g<hash>`, so every snapshot sorts below the release.
  - It `provides` `sway-things=<the release part of pkgver>` and conflicts with `sway-things`, the release package.
    `depends` lists the libraries the binaries link (namcap checks that) and `sway`, which SwayFX from the AUR provides;
    `optdepends` names `webp-pixbuf-loader` for WebP wallpapers. `arch=(x86_64)` only.
- `scripts/build-packages.sh --container NAME` builds a package in a clean container from `ci/packages/NAME/` and runs
  itself inside it. For `arch-git`, the container is a digest-pinned `archlinux:base` with only `base-devel` and an
  unprivileged user who may run `pacman`, like a clean chroot: it compares `.SRCINFO` with `makepkg --printsrcinfo` (but
  for `pkgver`), builds with `makepkg -s`, which installs the declared dependencies and nothing else, runs namcap on the
  `PKGBUILD` and the packages (an error fails, warnings are printed), installs the package and runs
  `sway-things --version` and `--help`. The `PKGBUILD` clones GitHub's `main`, so the build needs the network. `--local`
  builds the HEAD commit of the checkout instead, from a copy of the `PKGBUILD` whose `source` points at a bare clone of
  the checkout; `.SRCINFO` and namcap still check the `PKGBUILD` unchanged.
- `.github/workflows/packages.yml` builds `arch-git` as a job of its own on every push to `main`, weekly and by hand,
  but not for a release, since the `-git` package doesn't use the release tarball. It isn't a check on pull requests:
  the `-git` package builds `main`, not the pull request.
- `scripts/check-packaging.sh`, a check on every pull request, compares `.SRCINFO` with the `PKGBUILD` without makepkg:
  it sources the `PKGBUILD` in a subshell and prints what `makepkg --printsrcinfo` would (every field, in makepkg's
  order), leaving out `pkgver` and `pkgrel`. So a `PKGBUILD` change without a regenerated `.SRCINFO` fails the pull
  request rather than the Packages workflow on `main`. Fields set per package in `package_*()` aren't supported.
- `scripts/check-shell.sh` lints the `PKGBUILD`s with shellcheck as bash, without the warnings that makepkg's way of
  sourcing them makes moot (unused metadata variables, `pkgdir`, `cd` without `|| exit` under errexit).

## Wallpaper photos

`wallpapers/` holds photos for the Wallpaper page to offer: one directory per photographer, files named after what they
show, and `wallpapers.toml` as the index (path, title, photographer, copyright, license, size, bytes, SHA-256 to check a
download). They are scaled down to at most 3840 pixels on the longer side and carry the photographer, the copyright and
the terms in their EXIF, IPTC and XMP metadata, without camera serial numbers or GPS positions.

The photos are not under the GPL: their photographers keep all rights and allow only their use as a wallpaper with Sway
Things (`LICENSES/LicenseRef-Sway-Things-Wallpapers.txt`, set per directory in `REUSE.toml`). So they stay out of the
release tarball and the packages (`export-ignore` in `.gitattributes`). Downloading them is network access, which hard
rule 7 rules out today; how the app gets them (and the exception that needs) is the Wallpaper page's to settle. How to
add a photo is in `wallpapers/README.md`.
