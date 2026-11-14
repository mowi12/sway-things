# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Fedora spec for the openSUSE Build Service, built from the release tarball of scripts/make-tarball.sh, which holds
# the crates in vendor/. Follows the Fedora Rust packaging guidelines for vendored dependencies.

Name:           sway-things
Version:        0.1.0
Release:        1%{?dist}
Summary:        Settings app for the sway Wayland compositor and SwayFX
# The whole tarball: our files (LICENSES/) and every crate in vendor/. scripts/check-packaging.sh keeps it current.
SourceLicense:  %{shrink:
    (Apache-2.0 OR MIT) AND
    (Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT) AND
    (Apache-2.0 WITH LLVM-exception) AND
    (BSD-2-Clause OR Apache-2.0 OR MIT) AND
    CC-BY-4.0 AND
    CC0-1.0 AND
    GPL-3.0-or-later AND
    ISC AND
    MIT AND
    (MIT OR Apache-2.0) AND
    ((MIT OR Apache-2.0) AND Unicode-3.0) AND
    (MIT OR Apache-2.0 OR LGPL-2.1-or-later) AND
    (Unlicense OR MIT)
}
# The binaries with the crates linked into them (%%cargo_license_summary), and the metainfo (CC0-1.0).
# LICENSE.dependencies lists every crate. scripts/check-packaging.sh keeps it current.
License:        %{shrink:
    (Apache-2.0 OR MIT) AND
    (Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT) AND
    CC0-1.0 AND
    GPL-3.0-or-later AND
    ISC AND
    MIT AND
    (MIT OR Apache-2.0) AND
    (Unlicense OR MIT)
}
URL:            https://github.com/mowi12/sway-things
Source:         %{url}/releases/download/v%{version}/%{name}-%{version}.tar.xz

BuildRequires:  cargo-rpm-macros >= 25
BuildRequires:  pkgconfig(gtk4) >= 4.18
# msgfmt, with the ITS rules for the AppStream metainfo
BuildRequires:  gettext
BuildRequires:  desktop-file-utils
BuildRequires:  appstream
# Either compositor; Fedora only has sway.
Requires:       (sway or swayfx)

%description
Sway Things is a settings app for the sway Wayland compositor and SwayFX: a
searchable sidebar of pages, controls that apply instantly, and a safety net
for risky changes. It never edits your hand-written configuration; it writes
sway configuration files of its own, which your configuration includes with a
single line. The same settings are available on the command line with
sway-things.

%prep
%autosetup -p1
# Replaces the tarball's .cargo/config.toml with Fedora's, which also points at vendor/.
%cargo_prep -v vendor

%build
# The binaries look for the translations here.
export SWAY_THINGS_LOCALEDIR=%{_datadir}/locale
%cargo_build -- --locked
PREFIX=%{_prefix} PROFILE=rpm scripts/build.sh --data-only
%{cargo_license_summary}
%{cargo_license} > LICENSE.dependencies
%{cargo_vendor_manifest}

%install
PREFIX=%{_prefix} DESTDIR=%{buildroot} PROFILE=rpm scripts/install.sh

%check
%cargo_test -- --workspace --locked
desktop-file-validate %{buildroot}%{_datadir}/applications/io.github.mowi12.SwayThings.desktop
appstreamcli validate --no-net %{buildroot}%{_metainfodir}/io.github.mowi12.SwayThings.metainfo.xml

%files
%license LICENSE LICENSE.dependencies cargo-vendor.txt
%doc README.md CHANGELOG.md
%{_bindir}/sway-things
%{_bindir}/sway-things-gui
%{_datadir}/applications/io.github.mowi12.SwayThings.desktop
%{_datadir}/icons/hicolor/scalable/apps/io.github.mowi12.SwayThings.svg
%{_datadir}/icons/hicolor/symbolic/apps/io.github.mowi12.SwayThings-symbolic.svg
%{_metainfodir}/io.github.mowi12.SwayThings.metainfo.xml
%{_mandir}/man1/sway-things.1*
%{_mandir}/man1/sway-things-*.1*
%{bash_completions_dir}/sway-things
%{zsh_completions_dir}/_sway-things
%{fish_completions_dir}/sway-things.fish
