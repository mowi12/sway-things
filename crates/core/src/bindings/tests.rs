// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::HashMap;
use std::path::Path;

use proptest::prelude::*;

use super::keys::{self, Key, Modifiers};
use super::ours::{self, BINDINGS, MODIFIER, VOLUME};
use super::*;
use crate::command::Target;
use crate::command::model;
use crate::generate::Locations;
use crate::include::IncludeStatus;
use crate::registry::{Registry, Value};
use crate::state::{Paths, State};
use crate::sway_config::{MemoryFiles, parse};

fn words(text: &str) -> Vec<String> {
    text.split(' ').map(str::to_owned).collect()
}

fn line(text: &str) -> Line {
    let mut words = words(text);
    let name = words.remove(0);
    Line::read(&name, &words).unwrap()
}

fn id(text: &str) -> Identity {
    line(text).identity(DEFAULT_MODE, None)
}

#[test]
fn reads_options_keys_and_command() {
    let line = line("bindsym --locked --to-code --no-repeat Mod4+Shift+e exec swaynag -m bye");
    assert!(line.flags.locked && line.flags.to_code && line.flags.no_repeat);
    assert!(!line.unbind && !line.code);
    assert_eq!(line.keys, "Mod4+Shift+e");
    assert_eq!(
        line.combo.modifiers,
        Modifiers::SUPER.union(Modifiers::SHIFT)
    );
    assert_eq!(line.combo.keys, [Key::Sym("e".to_owned())]);
    assert_eq!(line.command.as_deref(), Some("exec swaynag -m bye"));
}

#[test]
fn spellings_of_one_binding_are_the_same_binding() {
    let base = id("bindsym Mod4+Shift+e exit");
    for other in [
        "bindsym Super+Shift+e kill",
        "bindsym shift+mod4+E kill",
        "bindsym Shift+SUPER+e kill",
        "bindsym Mod4++Shift+e kill",
        "bindsym --no-repeat Mod4+Shift+e kill",
        "bindsym --no-warn Mod4+Shift+e kill",
    ] {
        assert_eq!(id(other), base, "{other}");
    }
    assert_eq!(id("bindsym Mod1+x a"), id("bindsym Alt+x b"));
    assert_eq!(id("bindsym Ctrl+x a"), id("bindsym control+x b"));
    assert_eq!(id("bindsym Mod4+a+b a"), id("bindsym Mod4+b+a b"));
    assert_eq!(id("bindsym Prior a"), id("bindsym Page_Up b"));
}

#[test]
fn options_and_kinds_make_other_bindings() {
    let base = id("bindsym Mod4+e exit");
    for other in [
        "bindsym --locked Mod4+e exit",
        "bindsym --release Mod4+e exit",
        "bindsym --inhibited Mod4+e exit",
        "bindsym --input-device=1:1:kbd Mod4+e exit",
        "bindsym Mod4+Shift+e exit",
        "bindcode Mod4+26 exit",
        "bindsym Mod4+Group2+e exit",
    ] {
        assert_ne!(id(other), base, "{other}");
    }
    // Without a keymap a `--to-code` binding stays a keysym binding, as when sway can't
    // translate it.
    assert_eq!(id("bindsym --to-code Mod4+e exit"), base);
}

#[test]
fn mouse_bindings_are_read_like_sway() {
    let button = line("bindsym --whole-window Mod4+button3 kill");
    assert_eq!(button.combo.device, Device::Mouse);
    assert_eq!(button.combo.keys, [Key::Button(0x111)]);
    assert_eq!(
        line("bindsym Mod4+BTN_RIGHT kill").identity(DEFAULT_MODE, None),
        line("bindsym Mod4+button3 kill").identity(DEFAULT_MODE, None),
    );
    // A button on the title bar is another binding than over the whole window.
    assert_ne!(
        id("bindsym Mod4+button3 kill"),
        id("bindsym --whole-window Mod4+button3 kill")
    );
    assert_eq!(line("bindcode 272 kill").combo.device, Device::Mouse);
    assert_eq!(line("bindcode 38 kill").combo.keys, [Key::Code(38)]);
    assert!(Line::read("bindsym", &words("button0 kill")).is_err());
}

#[test]
fn lines_sway_rejects_are_errors() {
    for text in [
        "bindsym Mod4+x",
        "bindsym --locked",
        "bindsym $mod+x exec foot",
        "bindsym Group5+x kill",
        "bindsym Group1+Group2+x kill",
    ] {
        let mut words = words(text);
        let name = words.remove(0);
        assert!(Line::read(&name, &words).is_err(), "{text}");
    }
    assert!(matches!(
        Line::read("exec", &words("foot")),
        Err(LineError::NotABinding)
    ));
    // An unbind needs no command.
    assert!(line("unbindsym Mod4+x").unbind);
}

#[test]
fn labels_read_like_people_read_them() {
    let shown = |text: &str| -> Vec<String> {
        line(text)
            .labels(Modifiers::SUPER, None)
            .iter()
            .map(crate::i18n::Text::english)
            .collect()
    };
    assert_eq!(shown("bindsym Shift+Mod4+e exit"), ["Super", "Shift", "E"]);
    assert_eq!(
        shown("bindsym Ctrl+Mod1+Delete x"),
        ["Ctrl", "Alt", "Delete"]
    );
    assert_eq!(shown("bindsym Mod4+Return x"), ["Super", "Enter"]);
    assert_eq!(shown("bindsym XF86AudioRaiseVolume x"), ["Volume Up"]);
    assert_eq!(shown("bindsym Mod4+KP_1 x"), ["Super", "Keypad 1"]);
    assert_eq!(shown("bindsym Mod4+minus x"), ["Super", "-"]);
    assert_eq!(shown("bindcode Mod4+38 x"), ["Super", "key 38"]);
    assert_eq!(shown("bindsym --whole-window button4 x"), ["Scroll Up"]);
}

struct Us;

impl Keymap for Us {
    fn keycode(&self, name: &str) -> Option<u32> {
        match name {
            "a" => Some(38),
            "e" => Some(26),
            "return" => Some(36),
            _ => None,
        }
    }

    fn keysym(&self, code: u32) -> Option<String> {
        match code {
            38 => Some("a".to_owned()),
            26 => Some("e".to_owned()),
            _ => None,
        }
    }
}

#[test]
fn a_keymap_names_keycodes_and_translates_to_code() {
    let labels: Vec<String> = line("bindcode Mod4+38 x")
        .labels(Modifiers::SUPER, Some(&Us))
        .iter()
        .map(crate::i18n::Text::english)
        .collect();
    assert_eq!(labels, ["Super", "A"]);
    assert_eq!(
        line("bindsym --to-code Mod4+a x").identity(DEFAULT_MODE, Some(&Us)),
        line("bindcode Mod4+38 y").identity(DEFAULT_MODE, Some(&Us)),
    );
}

#[test]
fn commands_are_kept_as_sway_keeps_them() {
    let command = BindCommand::new("  exec   foot  -e 'a  b'  ").unwrap();
    assert_eq!(command.as_str(), "exec foot -e 'a  b'");
    assert_eq!(BindCommand::new(""), Err(CommandError::Empty));
    assert_eq!(BindCommand::new("exec a\\b"), Err(CommandError::Backslash));
    assert_eq!(
        BindCommand::new("exec a\nb"),
        Err(CommandError::ControlCharacter)
    );
    assert_eq!(
        BindCommand::new("exec echo {"),
        Err(CommandError::OpensBlock)
    );
    assert_eq!(BindCommand::new("exec \"a "), Err(CommandError::Unbalanced));
    assert_eq!(
        BindCommand::new("[app_id=x kill"),
        Err(CommandError::Unbalanced)
    );
    assert!(BindCommand::checked("focus left; kill").is_ok());
    assert!(BindCommand::checked("[app_id=foot] kill").is_ok());
    assert_eq!(
        BindCommand::checked("fokus left"),
        Err(CommandError::UnknownCommand("fokus".to_owned()))
    );
}

#[test]
fn actions_make_commands_and_are_read_back() {
    let cases = [
        (
            Action::Run("foot -e htop".to_owned()),
            "exec \"foot -e htop\"",
        ),
        (
            Action::Run("notify-send \"a b\"".to_owned()),
            "exec 'notify-send \"a b\"'",
        ),
        (Action::Run("echo $HOME".to_owned()), "exec \"echo $$HOME\""),
        (Action::Focus(Direction::Left), "focus left"),
        (Action::Move(Direction::Down), "move down"),
        (Action::Workspace(3), "workspace number 3"),
        (
            Action::MoveToWorkspace(10),
            "move container to workspace number 10",
        ),
        (Action::Close, "kill"),
        (Action::Fullscreen, "fullscreen toggle"),
        (Action::Floating, "floating toggle"),
        (Action::Sway("layout tabbed".to_owned()), "layout tabbed"),
    ];
    for (action, command) in cases {
        let built = action.command().unwrap();
        assert_eq!(built.as_str(), command);
        assert_eq!(Action::of(&built), action, "{command}");
    }
    assert_eq!(
        Action::Workspace(0).command(),
        Err(CommandError::InvalidWorkspace)
    );
    assert_eq!(
        Action::Run(" ".to_owned()).command(),
        Err(CommandError::NotAProgram)
    );
    // A hand-written exec without quotes is read as the line it runs.
    let plain = BindCommand::new("exec foot -e htop").unwrap();
    assert_eq!(Action::of(&plain), Action::Run("foot -e htop".to_owned()));
}

#[test]
fn commands_are_described_and_grouped() {
    let described = |command: &str| describe(command).map(|text| text.english());
    assert_eq!(described("kill").as_deref(), Some("Close the window"));
    assert_eq!(
        described("focus left").as_deref(),
        Some("Focus the window to the left")
    );
    assert_eq!(
        described("workspace number 4").as_deref(),
        Some("Go to workspace 4")
    );
    assert_eq!(
        described("move container to workspace number 4").as_deref(),
        Some("Move the window to workspace 4")
    );
    assert_eq!(described("exec \"foot\"").as_deref(), Some("Run foot"));
    assert_eq!(
        described("mode \"resize\"").as_deref(),
        Some("Enter the mode resize")
    );
    assert_eq!(described("focus left; kill"), None);
    let category = |command: &str| Category::of(command);
    assert_eq!(category("kill"), Category::Windows);
    assert_eq!(
        category("move container to workspace 3"),
        Category::Workspaces
    );
    assert_eq!(category("workspace back_and_forth"), Category::Workspaces);
    assert_eq!(category("exec foot"), Category::Apps);
    assert_eq!(
        category("exec wpctl set-mute @DEFAULT_AUDIO_SINK@ toggle"),
        Category::Media
    );
    assert_eq!(
        category("exec 'pactl set-sink-mute @DEFAULT_SINK@ toggle'"),
        Category::Media
    );
    assert_eq!(category("exec swaylock -f"), Category::System);
    assert_eq!(category("reload"), Category::System);
    assert_eq!(category("nop"), Category::Other);
}

fn config(text: &str) -> Vec<crate::sway_config::Directive> {
    let files = MemoryFiles::new().file("/c/config", text);
    parse(Path::new("/c/config"), &files).directives
}

#[test]
fn the_table_follows_sway_through_a_config() {
    let directives = config(
        "bindsym $mod+x kill\n\
         set $mod Mod4\n\
         bindsym $mod+Return exec foot\n\
         bindsym Super+return exec alacritty\n\
         bindsym $mod+d exec menu\n\
         unbindsym Mod4+d\n\
         unbindsym Mod4+q\n\
         mode \"resize\" {\n\
             bindsym Return mode default\n\
         }\n\
         mode resize bindsym Escape mode default\n\
         mode --pango_markup \"two words\" bindsym Escape mode default\n",
    );
    let table = Table::read(user_lines(&directives, None), None);
    // `$mod` wasn't set yet on the first line, which sway rejects.
    let keys: Vec<(&str, &str)> = table
        .entries
        .iter()
        .map(|entry| (entry.bound.mode.as_str(), entry.bound.line.keys.as_str()))
        .collect();
    assert_eq!(
        keys,
        [
            ("default", "Mod4+Return"),
            ("default", "Super+return"),
            ("default", "Mod4+d"),
            ("resize", "Return"),
            ("resize", "Escape"),
            ("two words", "Escape"),
        ]
    );
    assert_eq!(table.entries[0].fate, Fate::Replaced(1));
    assert_eq!(table.entries[1].replaces, [0]);
    assert!(matches!(table.entries[2].fate, Fate::Removed(_)));
    assert_eq!(table.failed.len(), 1);
    assert_eq!(table.entries[3].fate, Fate::InEffect);
    assert_eq!(table.entries[1].bound.source.line, 4);
}

fn state(dir: &tempfile::TempDir) -> State {
    let paths = Paths::new(
        &dir.path().join("config"),
        &dir.path().join("state"),
        "host",
    )
    .unwrap();
    State::load(Registry::builtin(), paths)
}

fn set_items(state: &mut State, items: &[&str]) {
    state
        .set(
            BINDINGS,
            None,
            Value::List(items.iter().map(|item| (*item).to_owned()).collect()),
            None,
        )
        .unwrap();
}

#[test]
fn items_are_checked_and_written_back() {
    for item in [
        "bindsym $mod+Return exec foot",
        "bindsym --locked --to-code $mod+Shift+e exec \"swaynag -m 'bye'\"",
        "unbindsym Mod4+d",
        "unbindcode --locked 121",
        "bindsym --input-device=1:1:AT_Translated_Set_2_keyboard Mod4+x kill",
    ] {
        let line = ours::parse_item(item).unwrap();
        assert_eq!(ours::item_text(&line), item);
        assert_eq!(ours::check_item(item), Ok(()));
    }
    for item in [
        "exec foot",
        "bindsym $mod+Return",
        "bindsym \"$mod+Return\" exec foot",
        "bindsym $mod+Return exec a\\b",
        "mode resize bindsym x kill",
        "bindsym --input-device=\"a b\" x kill",
        "",
    ] {
        assert!(ours::check_item(item).is_err(), "{item}");
    }
    let registry = Registry::builtin();
    let setting = registry.get(BINDINGS).unwrap();
    assert!(
        setting
            .check(&Value::List(vec![
                "bindsym x kill".to_owned(),
                "nope".to_owned()
            ]))
            .is_err()
    );
}

#[test]
fn the_generated_file_unbinds_safely_then_binds() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = state(&dir);
    set_items(
        &mut state,
        &[
            "bindsym $mod+Return exec \"foot\"",
            "unbindsym --to-code Mod4+d",
            "bindsym $mod+space exec \"echo $HOME\"",
        ],
    );
    state.set(VOLUME, None, Value::Bool(true), None).unwrap();
    state
        .set(MODIFIER, None, Value::Choice("alt".to_owned()), None)
        .unwrap();
    let lines: Vec<String> = ours::file_lines(&state)
        .iter()
        .map(|line| line.command().unwrap().render(Target::Config).unwrap())
        .collect();
    assert_eq!(
        lines,
        [
            "bindsym --to-code --no-warn Mod4+d nop",
            "unbindsym --to-code Mod4+d",
            "bindsym --locked --no-warn XF86AudioRaiseVolume exec \"wpctl set-volume -l 1.0 @DEFAULT_AUDIO_SINK@ 5%+\"",
            "bindsym --locked --no-warn XF86AudioLowerVolume exec \"wpctl set-volume @DEFAULT_AUDIO_SINK@ 5%-\"",
            "bindsym --locked --no-warn XF86AudioMute exec \"wpctl set-mute @DEFAULT_AUDIO_SINK@ toggle\"",
            "bindsym --locked --no-warn XF86AudioMicMute exec \"wpctl set-mute @DEFAULT_AUDIO_SOURCE@ toggle\"",
            "bindsym Mod1+Return exec \"foot\"",
            "bindsym Mod1+space exec \"echo $$HOME\"",
        ]
    );
    let ipc: Vec<String> = ours::file_lines(&state)
        .iter()
        .map(|line| line.command().unwrap().render(Target::Ipc).unwrap())
        .collect();
    assert_eq!(ipc[6], "bindsym Mod1+Return 'exec \"foot\"'");
    assert_eq!(ipc[0], "bindsym --to-code --no-warn Mod4+d \"nop\"");
}

/// The arguments a config line gives its handler, as text.
fn config_args(line: &str) -> Vec<String> {
    model::config_command(line.as_bytes())
        .unwrap()
        .iter()
        .map(|arg| String::from_utf8_lossy(arg).into_owned())
        .collect()
}

/// The arguments of each command of an IPC message, as text.
fn ipc_args(message: &str) -> Vec<Vec<String>> {
    model::execute_command(message.as_bytes())
        .unwrap()
        .into_iter()
        .map(|parsed| {
            parsed
                .argv
                .iter()
                .map(|arg| String::from_utf8_lossy(arg).into_owned())
                .collect()
        })
        .collect()
}

/// The command sway keeps for a binding line given its handler's arguments.
fn kept(args: &[String]) -> String {
    let start = super::line::keys_index(&args[1..]) + 2;
    args[start..].join(" ")
}

fn command_text() -> impl Strategy<Value = String> {
    proptest::collection::vec(
        prop_oneof![
            Just("exec".to_owned()),
            Just(" ".to_owned()),
            Just("  ".to_owned()),
            Just("\"".to_owned()),
            Just("'".to_owned()),
            Just(";".to_owned()),
            Just(",".to_owned()),
            Just("$".to_owned()),
            Just("$$".to_owned()),
            Just("[".to_owned()),
            Just("]".to_owned()),
            Just("=".to_owned()),
            "[a-z]{1,4}",
            Just("é".to_owned()),
        ],
        1..12,
    )
    .prop_map(|parts| parts.concat())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2000))]

    /// Whatever a command is, sway keeps exactly it from the line in the generated file and
    /// from the IPC message.
    #[test]
    fn commands_arrive_unchanged(text in command_text(), locked in any::<bool>()) {
        let Ok(command) = BindCommand::new(&text) else {
            return Ok(());
        };
        let mut binding = line("bindsym Mod4+x nop");
        binding.flags.locked = locked;
        binding.command = Some(command.as_str().to_owned());
        let config = binding.config(Modifiers::SUPER, false).unwrap();
        prop_assert_eq!(kept(&config_args(&config)), command.as_str());
        let ipc = binding.ipc(Modifiers::SUPER, false).unwrap();
        let commands = ipc_args(&ipc);
        prop_assert_eq!(commands.len(), 1);
        prop_assert_eq!(kept(&commands[0]), command.as_str());
    }

    /// A program to run reaches the shell as it was typed.
    #[test]
    fn programs_reach_the_shell_unchanged(text in "[a-z \"'$;,|&]{1,20}") {
        let Ok(command) = Action::Run(text.clone()).command() else {
            return Ok(());
        };
        // When the keys are pressed, sway runs the command like an IPC message. The model
        // refuses to look up variables; every `$` is doubled, so there are none.
        let runs = model::execute_command(command.as_str().as_bytes()).unwrap();
        prop_assert_eq!(runs.len(), 1);
        let argv = &runs[0].argv;
        prop_assert_eq!(argv.len(), 2);
        let line = model::strip_quotes(&argv[1]);
        prop_assert_eq!(String::from_utf8_lossy(&line), text.trim());
    }
}

fn status(text: &str, locations: &Locations) -> IncludeStatus {
    let files = MemoryFiles::new()
        .var("HOME", "/home/me")
        .file("/home/me/.config/sway/config", text);
    IncludeStatus::detect(
        Some(Path::new("/home/me/.config/sway/config")),
        &files,
        locations,
    )
}

fn locations() -> Locations {
    Locations::new(
        Path::new("/home/me/.config/sway/sway-things"),
        Path::new("/home/me/.local/state/sway-things"),
    )
}

const USER: &str = "set $mod Mod4\n\
    bindsym $mod+Return exec foot\n\
    bindsym $mod+d exec menu\n\
    bindsym $mod+Shift+q kill\n\
    bindsym --locked XF86AudioMute exec pactl mute\n\
    bindsym $mod+Shift+q exec other\n\
    include ~/.config/sway/sway-things/*.conf\n\
    bindsym $mod+f fullscreen\n";

#[test]
fn the_listing_shows_changes_removals_and_clashes() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = state(&dir);
    set_items(
        &mut state,
        &[
            "unbindsym Mod4+d => bindsym $mod+p exec menu",
            "bindsym $mod+x exec \"firefox\"",
            "bindsym $mod+f floating toggle",
            "unbindsym Mod4+Return",
        ],
    );
    state.set(VOLUME, None, Value::Bool(true), None).unwrap();
    let locations = locations();
    let status = status(USER, &locations);
    assert!(status.is_active());
    let listing = Listing::new(
        &status,
        &state,
        &locations.path(crate::registry::ConfigFile::Bindings),
        None,
    );
    let rows: Vec<(&str, &RowKind, bool)> = listing
        .rows()
        .iter()
        .map(|row| (row.line.keys.as_str(), &row.kind, row.in_effect))
        .collect();
    let original = line("bindsym Mod4+d exec menu");
    assert_eq!(
        rows,
        [
            ("Mod4+Return", &RowKind::Removed { item: 3 }, false),
            ("Mod4+p", &RowKind::Changed { item: 0, original }, true),
            ("Mod4+Shift+q", &RowKind::Hand, false),
            ("XF86AudioMute", &RowKind::Hand, false),
            ("Mod4+Shift+q", &RowKind::Hand, true),
            (
                "XF86AudioRaiseVolume",
                &RowKind::Group { key: VOLUME },
                true
            ),
            (
                "XF86AudioLowerVolume",
                &RowKind::Group { key: VOLUME },
                true
            ),
            ("XF86AudioMute", &RowKind::Group { key: VOLUME }, true),
            ("XF86AudioMicMute", &RowKind::Group { key: VOLUME }, true),
            ("Mod4+x", &RowKind::Added { item: 1 }, true),
            ("Mod4+f", &RowKind::Added { item: 2 }, false),
            ("Mod4+f", &RowKind::Hand, true),
        ]
    );
    let rows = listing.rows();
    // The changed row is the user's line.
    assert_eq!(rows[1].source.line, 3);
    // The later of two lines on the same keys wins, and both say so.
    assert_eq!(rows[2].clashes[0].kind, ClashKind::ReplacedBy);
    assert_eq!(rows[4].clashes[0].kind, ClashKind::Replaces);
    // The media keys replace the user's mute key quietly.
    assert_eq!(rows[7].clashes[0].kind, ClashKind::Replaces);
    assert_eq!(rows[7].clashes[0].source.line, 5);
    // A line after the include wins over ours and can't be changed here.
    assert_eq!(rows[10].clashes[0].kind, ClashKind::ReplacedBy);
    assert!(rows[11].source.after_ours);
    assert!(!rows[11].editable());
    assert!(rows[4].editable());
    // What a new binding of Mod4+x would clash with.
    let new = ours::parse_item("bindsym $mod+x exec foot").unwrap();
    let users = listing.users_of(&new, DEFAULT_MODE, None, None);
    assert_eq!(users.len(), 1);
    assert_eq!(users[0].kind, RowKind::Added { item: 1 });
}

#[test]
fn keysym_and_keycode_bindings_of_one_key_clash() {
    let dir = tempfile::tempdir().unwrap();
    let state = state(&dir);
    let locations = locations();
    let status = status(
        "bindsym Mod4+a exec one\n\
         bindcode Mod4+38 exec two\n\
         bindsym --locked Mod4+e exec three\n\
         bindsym Mod4+e exec four\n\
         include ~/.config/sway/sway-things/*.conf\n",
        &locations,
    );
    let path = locations.path(crate::registry::ConfigFile::Bindings);
    let listing = Listing::new(&status, &state, &path, Some(&Us));
    let kinds: Vec<Vec<ClashKind>> = listing
        .rows()
        .iter()
        .map(|row| row.clashes.iter().map(|clash| clash.kind).collect())
        .collect();
    assert_eq!(
        kinds,
        [
            vec![ClashKind::Shadowed],
            vec![ClashKind::Shadows],
            vec![ClashKind::LockScreenOnly],
            vec![],
        ]
    );
    let without = Listing::new(&status, &state, &path, None);
    assert!(
        without
            .rows()
            .iter()
            .take(2)
            .all(|row| row.clashes.is_empty())
    );
}

#[test]
fn live_changes_send_only_the_difference() {
    let dir = tempfile::tempdir().unwrap();
    let before = state(&dir);
    let locations = locations();
    let status = status(USER, &locations);
    let directives = &status.config().directives;
    let position = status.position(crate::registry::ConfigFile::Bindings);
    let sent = |from: &State, to: &State| -> Vec<String> {
        live::commands(directives, position, from, to)
            .unwrap()
            .iter()
            .map(|command| command.render(Target::Ipc).unwrap())
            .collect()
    };
    let mut changed = before.clone();
    set_items(
        &mut changed,
        &["unbindsym Mod4+d => bindsym $mod+p exec menu"],
    );
    assert_eq!(
        sent(&before, &changed),
        [
            "bindsym --no-warn Mod4+d \"nop\"",
            "unbindsym Mod4+d",
            "bindsym --no-warn Mod4+p \"exec menu\"",
        ]
    );
    // Undoing it binds the user's line again.
    assert_eq!(
        sent(&changed, &before),
        [
            "bindsym --no-warn Mod4+p \"nop\"",
            "unbindsym Mod4+p",
            "bindsym --no-warn Mod4+d \"exec menu\"",
        ]
    );
    // A new modifier moves the bindings of Sway Things only.
    let mut ours = before.clone();
    set_items(&mut ours, &["bindsym $mod+x kill"]);
    let mut alt = ours.clone();
    alt.set(MODIFIER, None, Value::Choice("alt".to_owned()), None)
        .unwrap();
    assert_eq!(
        sent(&ours, &alt),
        [
            "bindsym --no-warn Mod4+x \"nop\"",
            "unbindsym Mod4+x",
            "bindsym --no-warn Mod1+x \"kill\"",
        ]
    );
    // Nothing changes, nothing is sent; a line after the include still wins.
    assert_eq!(sent(&before, &before), Vec::<String>::new());
    let mut covered = before.clone();
    set_items(&mut covered, &["bindsym $mod+f floating toggle"]);
    assert_eq!(sent(&before, &covered), Vec::<String>::new());
}

#[test]
fn the_identity_ignores_what_sway_ignores() {
    let mut seen = HashMap::new();
    for text in ["bindsym Mod4+e a", "bindsym Super+E b"] {
        let identity = id(text);
        *seen.entry(identity).or_insert(0) += 1;
    }
    assert_eq!(seen.len(), 1);
    assert_eq!(keys::keysym_key("Page_Down"), "next");
    assert!(keys::is_plain("$mod+Shift+e"));
    assert!(!keys::is_plain("$mod+\"e\""));
    assert_eq!(
        keys::with_mod("$mod+Shift+e", Modifiers::ALT),
        "Mod1+Shift+e"
    );
}

#[test]
fn edits_keep_the_users_file_and_can_be_undone() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = state(&dir);
    let locations = locations();
    let status = status(USER, &locations);
    let path = locations.path(crate::registry::ConfigFile::Bindings);
    let listing = |state: &State| Listing::new(&status, state, &path, None);
    let index_of = |listing: &Listing, keys: &str| {
        listing
            .rows()
            .iter()
            .position(|row| row.line.keys == keys && row.in_effect)
            .unwrap()
    };
    // Change the keys of a binding of the user's.
    let now = listing(&state);
    let row = index_of(&now, "Mod4+Return");
    let new = ours::parse_item("bindsym $mod+t exec foot").unwrap();
    let items = edit::change(&now, &ours::items(&state), row, &new, None);
    let change = "unbindsym Mod4+Return => bindsym $mod+t exec foot";
    assert_eq!(items, [change]);
    set_items(&mut state, &[change]);
    let now = listing(&state);
    let changed = now
        .rows()
        .iter()
        .position(|row| matches!(row.kind, RowKind::Changed { .. }))
        .unwrap();
    // Rows show the keys for the main modifier.
    assert_eq!(now.rows()[changed].line.keys, "Mod4+t");
    // Changed back to what the config has, the change goes.
    let original = line("bindsym Mod4+Return exec foot");
    assert_eq!(
        edit::change(&now, &ours::items(&state), changed, &original, None),
        Vec::<String>::new()
    );
    assert!(edit::can_restore(&now.rows()[changed]));
    assert_eq!(
        edit::restore(&now, &ours::items(&state), changed),
        Vec::<String>::new()
    );
    // Removing it keeps the unbind; removing a binding of the user's adds one.
    assert_eq!(
        edit::remove(&now, &ours::items(&state), changed),
        ["unbindsym Mod4+Return"]
    );
    let d = index_of(&now, "Mod4+d");
    assert_eq!(
        edit::remove(&now, &ours::items(&state), d),
        [change, "unbindsym Mod4+d"]
    );
    // A new binding on keys of the user's takes their place: a change of their binding.
    let on_d = ours::parse_item("bindsym $mod+d exec \"wofi\"").unwrap();
    assert_eq!(
        edit::add(&now, &ours::items(&state), &on_d, None),
        [change, "unbindsym Mod4+d => bindsym $mod+d exec \"wofi\""]
    );
    // A new binding on the keys a change binds: the user's binding stays removed.
    let on_t = ours::parse_item("bindsym $mod+t kill").unwrap();
    assert_eq!(
        edit::add(&now, &ours::items(&state), &on_t, None),
        ["unbindsym Mod4+Return", "bindsym $mod+t kill"]
    );
}

#[test]
fn a_removal_next_to_an_addition_is_no_change() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = state(&dir);
    // Remove Mod4+d, add Mod4+y, and an unbind whose line is gone from the config.
    set_items(
        &mut state,
        &[
            "unbindsym Mod4+d",
            "bindsym $mod+y kill",
            "unbindsym Mod4+z",
        ],
    );
    let locations = locations();
    let status = status(USER, &locations);
    let path = locations.path(crate::registry::ConfigFile::Bindings);
    let listing = Listing::new(&status, &state, &path, None);
    let kinds: Vec<(&str, &RowKind)> = listing
        .rows()
        .iter()
        .filter(|row| !matches!(row.kind, RowKind::Hand))
        .map(|row| (row.line.keys.as_str(), &row.kind))
        .collect();
    assert_eq!(
        kinds,
        [
            ("Mod4+d", &RowKind::Removed { item: 0 }),
            ("Mod4+y", &RowKind::Added { item: 1 }),
            ("Mod4+z", &RowKind::Removed { item: 2 }),
        ]
    );
    // Restoring the removal touches only its item.
    let d = listing
        .rows()
        .iter()
        .position(|row| row.kind == RowKind::Removed { item: 0 })
        .unwrap();
    assert_eq!(
        edit::restore(&listing, &ours::items(&state), d),
        ["bindsym $mod+y kill", "unbindsym Mod4+z"]
    );
}

#[test]
fn change_items_are_checked() {
    let change =
        "unbindsym --locked XF86AudioMute => bindsym --locked XF86AudioMute exec \"a => b\"";
    let Ok(ours::Item::Change { from, to }) = ours::parse(change) else {
        panic!("{change} isn't a change");
    };
    assert!(from.unbind && !to.unbind);
    assert_eq!(ours::parse(change).unwrap().text(), change);
    // A binding whose command has the separator stays a binding.
    assert!(matches!(
        ours::parse("bindsym Mod4+x exec \"a => b\""),
        Ok(ours::Item::Line(_))
    ));
    assert!(ours::check_item("unbindsym Mod4+x => unbindsym Mod4+y").is_err());
    // A command sway doesn't know is read from the file, but not written or set.
    for typo in [
        "unbindsym Mod4+x => bindsym Mod4+x exce foo",
        "bindsym Mod4+x exce foo",
        "bindsym Mod4+x fokus left",
    ] {
        assert_eq!(ours::check_item(typo), Ok(()), "{typo}");
        assert!(
            matches!(
                ours::parse_strict(typo),
                Err(LineError::Command(CommandError::UnknownCommand(_)))
            ),
            "{typo}"
        );
    }
    let dir = tempfile::tempdir().unwrap();
    let mut state = state(&dir);
    set_items(
        &mut state,
        &["bindsym Mod4+x exce foo", "bindsym Mod4+y kill"],
    );
    let lines = ours::file_lines(&state);
    assert!(lines[0].line.is_err());
    assert!(lines[1].line.is_ok());
    // Commands of SwayFX are known too.
    assert!(ours::parse_strict("bindsym Mod4+b blur enable").is_ok());
}

#[test]
fn toggling_to_code_is_sent_live() {
    let dir = tempfile::tempdir().unwrap();
    let mut plain = state(&dir);
    set_items(&mut plain, &["bindsym $mod+x kill"]);
    let mut to_code = plain.clone();
    set_items(&mut to_code, &["bindsym --to-code $mod+x kill"]);
    let locations = locations();
    let status = status(USER, &locations);
    let directives = &status.config().directives;
    let position = status.position(crate::registry::ConfigFile::Bindings);
    let sent: Vec<String> = live::commands(directives, position, &plain, &to_code)
        .unwrap()
        .iter()
        .map(|command| command.render(Target::Ipc).unwrap())
        .collect();
    assert_eq!(
        sent,
        [
            "bindsym --no-warn Mod4+x \"nop\"",
            "unbindsym Mod4+x",
            "bindsym --to-code --no-warn Mod4+x \"kill\"",
        ]
    );
}

#[test]
fn numeric_and_alias_names_are_the_same_keys() {
    for (a, b) in [
        ("grave", "quoteleft"),
        ("apostrophe", "quoteright"),
        ("grave", "U0060"),
        ("grave", "0x60"),
        ("grave", "u0060"),
        ("apostrophe", "0x0027"),
        ("e", "U0065"),
    ] {
        assert_eq!(keys::keysym_key(a), keys::keysym_key(b), "{a} {b}");
    }
    // The capital letter by number is another keysym than the name, which sway reads as lowercase.
    assert_ne!(keys::keysym_key("0x41"), keys::keysym_key("A"));
    assert_ne!(keys::keysym_key("U20AC"), keys::keysym_key("EuroSign"));
    assert_ne!(keys::keysym_key("Tab"), keys::keysym_key("ISO_Left_Tab"));
}

#[test]
fn only_keys_nobody_types_with_stand_alone() {
    let alone = |keys: &str| keys::leaves_typing(&keys::parse(keys, false, false).unwrap());
    for keys in [
        "Mod4+a",
        "Ctrl+Return",
        "F5",
        "XF86AudioMute",
        "Print",
        "Pause",
        "Menu",
    ] {
        assert!(alone(keys), "{keys}");
    }
    for keys in ["a", "Return", "space", "1", "Tab", "Shift+a", "Mod5+q"] {
        assert!(!alone(keys), "{keys}");
    }
    assert!(alone("Shift+F5"));
}
