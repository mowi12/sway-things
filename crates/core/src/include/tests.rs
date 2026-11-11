// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::Path;

use super::{IncludeStatus, Inclusion};
use crate::generate::Locations;
use crate::registry::ConfigFile;
use crate::sway_config::{DiagnosticKind, ExpandError, MemoryFiles};

const MAIN: &str = "/home/me/.config/sway/config";
const DIRECTORY: &str = "/home/me/.config/sway/sway-things";
const LINE: &str = "include ~/.config/sway/sway-things/*.conf";

fn locations() -> Locations {
    Locations::new(
        Path::new(DIRECTORY),
        Path::new("/home/me/.local/state/sway-things"),
    )
}

fn files(main: &str) -> MemoryFiles {
    MemoryFiles::new().var("HOME", "/home/me").file(MAIN, main)
}

fn detect(files: &MemoryFiles) -> IncludeStatus {
    IncludeStatus::detect(Some(Path::new(MAIN)), files, &locations())
}

fn status(main: &str) -> IncludeStatus {
    detect(&files(main))
}

/// Where the includes of the generated files are, as `file name:line`.
fn places(status: &IncludeStatus) -> Vec<String> {
    status
        .lines()
        .iter()
        .map(|include| {
            let file = include.file.file_name().unwrap().to_str().unwrap();
            format!("{file}:{}", include.line)
        })
        .collect()
}

fn texts(status: &IncludeStatus) -> Vec<&str> {
    status
        .after()
        .iter()
        .map(|directive| directive.text.as_str())
        .collect()
}

#[test]
fn the_line_is_found_in_every_spelling() {
    for line in [
        LINE,
        "include $HOME/.config/sway/sway-things/*.conf",
        "include ${HOME}/.config/sway/sway-things/*.conf",
        "include /home/me/.config/sway/sway-things/*.conf",
        "include sway-things/*.conf",
        "include ./sway-things/*",
        "include ../sway/sway-things/[0-9]*.conf",
        "include \"~/.config/sway/sway-things/*.conf\"",
        "INCLUDE ~/.config/sway/*/??-*.conf",
        "set $things ~/.config/sway/sway-things\ninclude $things/*.conf",
        // Two paths in one argument: sway removes the quotes, the expansion splits at the blank.
        "include \"~/.config/sway/sway-things/10-input.conf ~/.config/sway/sway-things/[3-9]*\"",
    ] {
        let status = status(&format!("gaps inner 8\n{line}\n"));
        assert_eq!(status.inclusion(), &Inclusion::Active, "{line}");
        assert!(status.is_active() && status.is_last(), "{line}");
        assert!(status.write_access().is_ok(), "{line}");
        // Every line of the user comes before the generated files.
        let before = status.config().directives.len();
        for file in ConfigFile::ALL {
            assert_eq!(status.position(file), Some(before), "{line}");
        }
    }
}

#[test]
fn the_line_counts_before_any_file_was_generated_and_with_files_on_disk() {
    let main = format!("{LINE}\n");
    // Nothing generated yet: the directory doesn't exist.
    assert!(status(&main).is_active());
    // Some files exist, one of them with only its header.
    let files = files(&main)
        .file(&format!("{DIRECTORY}/10-input.conf"), "# Generated\n")
        .file(
            &format!("{DIRECTORY}/30-appearance.conf"),
            "# Generated\ngaps inner 8\n",
        );
    let status = detect(&files);
    assert!(status.is_active());
    // The generated files aren't part of the user's config.
    assert_eq!(status.config().directives, []);
    assert_eq!(status.config().files.len(), 1);
    assert_eq!(places(&status), ["config:1"]);
}

#[test]
fn a_missing_line_is_read_only_with_the_instructions() {
    for main in [
        "gaps inner 8\n",
        "# include ~/.config/sway/sway-things/*.conf\n",
        "include ~/.config/sway/config.d/*.conf\n",
        "include ~/.config/sway/sway-things/*.config\n",
        "include ~/.config/sway-things/*.conf\n",
        // Not at the top level: sway reads it as `bar include …`.
        "bar {\n    include ~/.config/sway/sway-things/*.conf\n}\n",
        // sway rejects an include with two arguments.
        "include ~/.config/sway/sway-things/*.conf extra\n",
    ] {
        let status = status(main);
        assert_eq!(status.inclusion(), &Inclusion::Missing, "{main}");
        assert!(!status.is_last(), "{main}");
        assert_eq!(status.after(), [], "{main}");
        assert_eq!(status.position(ConfigFile::Input), None, "{main}");

        let error = status.write_access().unwrap_err();
        assert_eq!(error.to_string(), "Add the include line to change settings");
        assert_eq!(error.inclusion(), &Inclusion::Missing);
        assert_eq!(error.instructions().line(), LINE);
        assert_eq!(
            error.explanation(),
            "Sway Things never edits your sway config. It writes its own files and needs one \
             line at the end of ~/.config/sway/config:\n\n    \
             include ~/.config/sway/sway-things/*.conf"
        );
    }
}

#[test]
fn the_line_is_found_in_an_included_file() {
    let files = files("gaps inner 8\ninclude config.d/*\ngaps outer 2\n")
        .file(
            "/home/me/.config/sway/config.d/10-look.conf",
            "smart_gaps on\n",
        )
        .file(
            "/home/me/.config/sway/config.d/50-things.conf",
            format!("font pango:monospace 10\n{LINE}\nfocus_follows_mouse no\n"),
        )
        .file(
            "/home/me/.config/sway/config.d/90-keys.conf",
            "floating_modifier Mod4\n",
        );
    let status = detect(&files);
    assert!(status.is_active());
    assert_eq!(places(&status), ["50-things.conf:2"]);
    // gaps inner, smart_gaps and font run before the generated files, the rest after them.
    assert_eq!(status.position(ConfigFile::Effects), Some(3));
    assert!(!status.is_last());
    assert_eq!(
        texts(&status),
        [
            "focus_follows_mouse no",
            "floating_modifier Mod4",
            "gaps outer 2"
        ]
    );
    assert_eq!(
        status.message(),
        "Sway Things is enabled: ~/.config/sway/config.d/50-things.conf:2 includes its files. 3 \
         of your config lines come after them and win where they set the same thing as Sway \
         Things."
    );
}

#[test]
fn lines_after_the_include_are_listed() {
    let status = status(&format!(
        "gaps inner 8\n{LINE}\ninput type:touchpad {{\n    tap enabled\n}}\ngaps outer 2\n"
    ));
    assert!(status.is_active() && !status.is_last());
    assert_eq!(texts(&status), ["tap enabled", "gaps outer 2"]);
    assert_eq!(status.config().directives.len(), 3);

    let last = self::status(&format!("gaps inner 8\n{LINE}\n# a comment\n\n"));
    assert!(last.is_last());
    assert_eq!(
        last.message(),
        "Sway Things is enabled: ~/.config/sway/config:2 includes its files."
    );
}

#[test]
fn files_of_a_wider_glob_have_their_own_positions() {
    // `*/*.conf` reads the user's `a/` before and `z/` after the generated files.
    let files = files("include ~/.config/sway/*/*.conf\n")
        .file("/home/me/.config/sway/a/one.conf", "gaps inner 8\n")
        .file("/home/me/.config/sway/z/two.conf", "gaps outer 2\n")
        .file(&format!("{DIRECTORY}/20-mine.conf"), "smart_gaps on\n");
    let status = detect(&files);
    assert!(status.is_active());
    // The user's own file in the directory is read between 10-input.conf and the others.
    assert_eq!(status.position(ConfigFile::Input), Some(1));
    assert_eq!(status.position(ConfigFile::Appearance), Some(2));
    assert_eq!(texts(&status), ["smart_gaps on", "gaps outer 2"]);
}

#[test]
fn an_include_of_some_files_is_partial() {
    for (line, missing) in [
        (
            "include ~/.config/sway/sway-things/10-input.conf",
            vec![
                ConfigFile::Appearance,
                ConfigFile::Behavior,
                ConfigFile::Effects,
            ],
        ),
        (
            "include ~/.config/sway/sway-things/[1-4]*.conf",
            vec![ConfigFile::Effects],
        ),
    ] {
        let status = status(&format!("{line}\n"));
        assert_eq!(
            status.inclusion(),
            &Inclusion::Partial {
                missing: missing.clone()
            },
            "{line}"
        );
        assert!(status.write_access().is_err(), "{line}");
        assert_eq!(status.position(ConfigFile::Input), Some(0), "{line}");
        assert_eq!(status.position(ConfigFile::Effects), None, "{line}");
    }
    assert_eq!(
        status("include ~/.config/sway/sway-things/[1-4]*.conf\n").message(),
        "Not enabled yet: your config includes only some of the Sway Things files \
         (~/.config/sway/config:1). Not included: 90-effects.conf. Use this line instead:\n\n    \
         include ~/.config/sway/sway-things/*.conf"
    );
    // Two lines that cover everything together are fine.
    let status = status(
        "include ~/.config/sway/sway-things/[1-4]*.conf\ngaps inner 8\n\
         include ~/.config/sway/sway-things/9*.conf\n",
    );
    assert!(status.is_active());
    assert_eq!(places(&status), ["config:1", "config:3"]);
    assert_eq!(status.position(ConfigFile::Effects), Some(1));
    assert_eq!(texts(&status), ["gaps inner 8"]);
}

#[test]
fn an_include_that_cannot_be_followed_is_unknown() {
    let status = status("gaps inner 8\ninclude $(hostname).conf\n");
    let Inclusion::Unknown { unfollowed } = status.inclusion() else {
        panic!("{:?}", status.inclusion());
    };
    assert_eq!(unfollowed.len(), 1);
    assert_eq!(unfollowed[0].line, 2);
    assert_eq!(
        unfollowed[0].kind,
        DiagnosticKind::UnresolvedInclude {
            pattern: "$(hostname).conf".to_owned(),
            reason: ExpandError::CommandSubstitution,
        }
    );
    assert!(status.write_access().is_err());
    assert_eq!(
        status.message(),
        "Not enabled yet: Sway Things can't follow part of your config:\n\n    \
         ~/.config/sway/config:2: include $(hostname).conf (the path needs a command to be \
         run)\n\nIf the Sway Things line is in there, add it to ~/.config/sway/config \
         instead:\n\n    include ~/.config/sway/sway-things/*.conf"
    );

    // The line itself written with a command is unknown too.
    let own = self::status("include $(pwd)/sway-things/*.conf\n");
    assert!(matches!(own.inclusion(), Inclusion::Unknown { .. }));

    // With the line found elsewhere, what can't be followed doesn't matter.
    let found = self::status(&format!("include $(hostname).conf\n{LINE}\n"));
    assert!(found.is_active());
}

#[test]
fn the_first_include_of_a_file_counts() {
    // sway reads each file once, so the second line does nothing.
    let status = status(&format!("{LINE}\ngaps inner 8\n{LINE}\n"));
    assert!(status.is_active());
    assert_eq!(places(&status), ["config:1"]);
    assert_eq!(texts(&status), ["gaps inner 8"]);
}

#[test]
fn without_a_config_the_instructions_say_to_create_one() {
    let locations = locations();
    let none = MemoryFiles::new().var("HOME", "/home/me");
    let status = IncludeStatus::detect(None, &none, &locations);
    assert_eq!(status.inclusion(), &Inclusion::Missing);
    assert_eq!(status.main(), None);
    let instructions = status.instructions();
    assert_eq!(instructions.config(), Path::new(MAIN));
    assert_eq!(instructions.copy_from(), None);
    assert_eq!(instructions.line(), LINE);

    // sway reads the config of the system, which isn't the user's to edit.
    let system = none.clone().file("/etc/sway/config", "gaps inner 8\n");
    let status = IncludeStatus::detect(Some(Path::new("/etc/sway/config")), &system, &locations);
    assert_eq!(status.inclusion(), &Inclusion::Missing);
    let instructions = status.instructions();
    assert_eq!(instructions.config(), Path::new(MAIN));
    assert_eq!(
        instructions.copy_from(),
        Some(Path::new("/etc/sway/config"))
    );
    assert_eq!(
        instructions.text(),
        "Sway Things never edits your sway config. It writes its own files and needs one line \
         in it. You have no sway config of your own yet: copy /etc/sway/config to \
         ~/.config/sway/config first, then add this line at its end:\n\n    \
         include ~/.config/sway/sway-things/*.conf"
    );

    // A main config that can't be read is missing, not unknown.
    let unreadable = IncludeStatus::detect(Some(Path::new(MAIN)), &none, &locations);
    assert_eq!(unreadable.inclusion(), &Inclusion::Missing);
    assert_eq!(unreadable.config().diagnostics.len(), 1);
}

#[test]
fn the_line_to_copy_fits_the_directory() {
    let line = |directory: &str, files: &MemoryFiles| {
        let locations = Locations::new(Path::new(directory), Path::new("/state"));
        let status = IncludeStatus::detect(Some(Path::new(MAIN)), files, &locations);
        let line = status.instructions().line().to_owned();
        // Whatever the line is, the config with it is active.
        let with_line = files.clone().file(MAIN, format!("{line}\n"));
        let status = IncludeStatus::detect(Some(Path::new(MAIN)), &with_line, &locations);
        assert!(status.is_active(), "{line}");
        line
    };
    let files = files("");
    assert_eq!(line(DIRECTORY, &files), LINE);
    // Outside the home directory, e.g. with XDG_CONFIG_HOME somewhere else.
    assert_eq!(
        line("/data/config/sway/sway-things", &files),
        "include /data/config/sway/sway-things/*.conf"
    );
    // Without HOME there is no `~`.
    let homeless = MemoryFiles::new().file(MAIN, "");
    assert_eq!(
        line(DIRECTORY, &homeless),
        "include /home/me/.config/sway/sway-things/*.conf"
    );
    // Blanks and other characters with a meaning are escaped.
    for directory in [
        "/home/me/my config/sway/sway-things",
        "/home/me/it's/sway-things",
        "/home/me/a$b/[c]/sway-things",
        "/home/me/a\"b/sway-things",
    ] {
        line(directory, &files);
    }
}

#[test]
fn symbolic_links_lead_to_the_same_files() {
    use std::fs;
    use std::os::unix::fs::symlink;

    use crate::sway_config::Disk;

    // ~/.config/sway is a link into a dotfiles repository, and one generated file is a link too.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let dotfiles = root.join("dotfiles/sway");
    fs::create_dir_all(dotfiles.join("sway-things")).unwrap();
    fs::create_dir_all(root.join("config")).unwrap();
    fs::create_dir_all(root.join("elsewhere")).unwrap();
    symlink(&dotfiles, root.join("config/sway")).unwrap();
    fs::write(root.join("elsewhere/input.conf"), "# Generated\n").unwrap();
    symlink(
        root.join("elsewhere/input.conf"),
        dotfiles.join("sway-things/10-input.conf"),
    )
    .unwrap();
    let main = root.join("config/sway/config");
    let locations = Locations::new(&root.join("config/sway/sway-things"), &root.join("state"));

    // Written through the link, and directly into the repository.
    for directory in [
        root.join("config/sway/sway-things"),
        dotfiles.join("sway-things"),
    ] {
        fs::write(
            &main,
            format!("include {}/*.conf\ngaps inner 8\n", directory.display()),
        )
        .unwrap();
        let status = IncludeStatus::detect(Some(&main), &Disk, &locations);
        assert!(status.is_active(), "{}", directory.display());
        assert_eq!(texts(&status), ["gaps inner 8"]);
        assert_eq!(status.config().files.len(), 1);
    }
    // Relative to the config, which sway resolves from where the link leads.
    fs::write(&main, "include sway-things/*.conf\n").unwrap();
    assert!(IncludeStatus::detect(Some(&main), &Disk, &locations).is_last());
    fs::write(&main, "include other/*.conf\n").unwrap();
    assert!(!IncludeStatus::detect(Some(&main), &Disk, &locations).is_active());
}
