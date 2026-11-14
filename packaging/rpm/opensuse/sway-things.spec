# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# openSUSE spec for the openSUSE Build Service, built from the release tarball of scripts/make-tarball.sh, which holds
# the crates in vendor/ and a .cargo/config.toml that points cargo at them. Uses the cargo-packaging macros.

Name:           sway-things
Version:        0.0.0
Release:        0
Summary:        Settings app for the sway Wayland compositor and SwayFX
# The binaries with the crates linked into them, and the metainfo (CC0-1.0). scripts/check-packaging.sh keeps it
# current.
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
Source0:        %{url}/releases/download/v%{version}/%{name}-%{version}.tar.xz
BuildRequires:  cargo >= 1.85
BuildRequires:  cargo-packaging
BuildRequires:  pkgconfig(gtk4) >= 4.18
# msgfmt, with the ITS rules for the AppStream metainfo
BuildRequires:  gettext-tools
BuildRequires:  desktop-file-utils
# Owns the icon directories.
BuildRequires:  hicolor-icon-theme
BuildRequires:  AppStream
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

%build
# The binaries look for the translations here.
export SWAY_THINGS_LOCALEDIR=%{_datadir}/locale
%{cargo_build} --locked
PREFIX=%{_prefix} PROFILE=release scripts/build.sh --data-only

%install
PREFIX=%{_prefix} DESTDIR=%{buildroot} PROFILE=release scripts/install.sh

%check
%{cargo_test} --locked --workspace
desktop-file-validate %{buildroot}%{_datadir}/applications/io.github.mowi12.SwayThings.desktop
appstreamcli validate --no-net %{buildroot}%{_datadir}/metainfo/io.github.mowi12.SwayThings.metainfo.xml

%files
%license LICENSE
%doc README.md
%{_bindir}/sway-things
%{_bindir}/sway-things-gui
%{_datadir}/applications/io.github.mowi12.SwayThings.desktop
%{_datadir}/icons/hicolor/scalable/apps/io.github.mowi12.SwayThings.svg
%{_datadir}/icons/hicolor/symbolic/apps/io.github.mowi12.SwayThings-symbolic.svg
%{_datadir}/metainfo/io.github.mowi12.SwayThings.metainfo.xml
%{_mandir}/man1/sway-things.1%{?ext_man}
%{_mandir}/man1/sway-things-*.1%{?ext_man}
%{_datadir}/bash-completion/completions/sway-things
%dir %{_datadir}/zsh
%dir %{_datadir}/zsh/site-functions
%{_datadir}/zsh/site-functions/_sway-things
%dir %{_datadir}/fish
%dir %{_datadir}/fish/vendor_completions.d
%{_datadir}/fish/vendor_completions.d/sway-things.fish
