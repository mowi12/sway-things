// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::{Path, PathBuf};

use proptest::prelude::*;

use super::{
    DEFAULT_ROOT, Item, MAX_FILE_SIZE, RulesError, RulesErrorKind, XkbRules, parse, rules_dir,
};
use crate::sway_config::{Disk, Files, MemoryFiles};

const BASE: &str = "/usr/share/X11/xkb/rules/evdev.xml";
const EXTRAS: &str = "/usr/share/X11/xkb/rules/evdev.extras.xml";

/// A small registry in the form of xkeyboard-config's `evdev.xml`, with the cases the parser
/// has to handle: translated descriptions, comments, unknown elements and entries without a
/// name.
const SAMPLE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE xkbConfigRegistry SYSTEM "xkb.dtd">
<xkbConfigRegistry version="1.1">
  <modelList>
    <model>
      <configItem>
        <name>pc105</name>
        <description>Generic 105-key PC</description>
        <vendor>Generic</vendor>
      </configItem>
    </model>
    <model>
      <configItem>
        <description>A model without a name</description>
      </configItem>
    </model>
    <model>
      <configItem>
        <name>  thinkpad  </name>
        <description xml:lang="de">IBM ThinkPad (Deutsch)</description>
        <description>IBM ThinkPad 560Z/600/600E/A22E</description>
        <hwList><hwId>0001:0001</hwId></hwList>
      </configItem>
    </model>
  </modelList>
  <layoutList>
    <layout>
      <configItem>
        <name>us</name>
        <!-- Keyboard indicator for English layouts -->
        <shortDescription>en</shortDescription>
        <description>English (US)</description>
        <countryList>
          <iso3166Id>US</iso3166Id>
        </countryList>
        <languageList>
          <iso639Id>eng</iso639Id>
        </languageList>
      </configItem>
      <variantList>
        <variant>
          <configItem>
            <name>intl</name>
            <description>English (US, intl., with dead keys)</description>
          </configItem>
        </variant>
        <variant>
          <configItem>
            <name>dvorak</name>
            <description>English (Dvorak)</description>
          </configItem>
        </variant>
      </variantList>
    </layout>
    <layout>
      <configItem>
        <name>de</name>
        <shortDescription>de</shortDescription>
        <description>German</description>
        <countryList><iso3166Id>DE</iso3166Id></countryList>
        <languageList><iso639Id>ger</iso639Id></languageList>
      </configItem>
      <variantList>
        <variant>
          <configItem>
            <name>nodeadkeys</name>
            <description>German (no dead keys)</description>
          </configItem>
        </variant>
        <variant>
          <configItem popularity="exotic">
            <name>ro</name>
            <description>Romanian (Germany)</description>
            <languageList><iso639Id>rum</iso639Id></languageList>
          </configItem>
        </variant>
      </variantList>
      <somethingNew>ignored</somethingNew>
    </layout>
    <layout>
      <configItem>
        <name></name>
        <description>A layout with an empty name</description>
      </configItem>
      <variantList>
        <variant><configItem><name>lost</name></configItem></variant>
      </variantList>
    </layout>
  </layoutList>
  <optionList>
    <group allowMultipleSelection="true">
      <!-- Each of these switches to the next layout -->
      <configItem>
        <name>grp</name>
        <description>Switching to another layout</description>
      </configItem>
      <option>
        <configItem>
          <name>grp:alt_shift_toggle</name>
          <description>Alt+Shift</description>
        </configItem>
      </option>
      <option>
        <configItem>
          <name>grp:win_space_toggle</name>
          <description>Win+Space</description>
        </configItem>
      </option>
    </group>
    <group>
      <configItem>
        <name>Compose key</name>
        <description>Position of Compose key</description>
      </configItem>
      <option>
        <configItem>
          <name>compose:ralt</name>
          <description>Right Alt</description>
        </configItem>
      </option>
      <option>
        <configItem>
          <name>compose:menu</name>
        </configItem>
      </option>
    </group>
  </optionList>
  <futureList><entry/></futureList>
</xkbConfigRegistry>
"#;

/// A small registry in the form of `evdev.extras.xml`: a new layout, a variant of a layout
/// of the base file, one the base file has already, and an option of a known group.
const SAMPLE_EXTRAS: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<!DOCTYPE xkbConfigRegistry SYSTEM "xkb.dtd">
<xkbConfigRegistry>
  <modelList/>
  <layoutList>
    <layout>
      <configItem popularity="exotic">
        <name>ancient</name>
        <shortDescription>xx</shortDescription>
        <description>Ancient</description>
        <languageList>
          <iso639Id>got</iso639Id>
          <iso639Id>uga</iso639Id>
        </languageList>
      </configItem>
      <variantList>
        <variant>
          <configItem popularity="exotic">
            <name>runic</name>
            <description>Gothic</description>
          </configItem>
        </variant>
      </variantList>
    </layout>
    <layout>
      <configItem>
        <name>de</name>
        <description>German (from the extras)</description>
      </configItem>
      <variantList>
        <variant>
          <configItem>
            <name>nodeadkeys</name>
            <description>German (no dead keys, from the extras)</description>
          </configItem>
        </variant>
        <variant>
          <configItem>
            <name>us</name>
            <description>German (US)</description>
          </configItem>
        </variant>
      </variantList>
    </layout>
  </layoutList>
  <optionList>
    <group allowMultipleSelection="true">
      <configItem>
        <name>grp</name>
        <description>Switching to another layout</description>
      </configItem>
      <option>
        <configItem>
          <name>grp:sclk_toggle</name>
          <description>Scroll Lock</description>
        </configItem>
      </option>
    </group>
  </optionList>
</xkbConfigRegistry>
"#;

fn sample() -> XkbRules {
    parse(SAMPLE.as_bytes(), false).unwrap()
}

fn names(items: &[Item]) -> Vec<&str> {
    items.iter().map(|item| item.name.as_str()).collect()
}

#[test]
fn models_layouts_variants_and_options_are_read() {
    let rules = sample();

    assert_eq!(names(&rules.models), ["pc105", "thinkpad"]);
    let pc105 = rules.model("pc105").unwrap();
    assert_eq!(pc105.description.as_deref(), Some("Generic 105-key PC"));
    assert_eq!(pc105.vendor.as_deref(), Some("Generic"));

    let layouts: Vec<&str> = rules
        .layouts
        .iter()
        .map(|layout| layout.item.name.as_str())
        .collect();
    assert_eq!(layouts, ["us", "de"]);
    let us = rules.layout("us").unwrap();
    assert_eq!(
        us.item,
        Item {
            name: "us".to_owned(),
            description: Some("English (US)".to_owned()),
            short_description: Some("en".to_owned()),
            vendor: None,
            languages: vec!["eng".to_owned()],
            countries: vec!["US".to_owned()],
            extra: false,
        }
    );
    assert_eq!(names(&us.variants), ["intl", "dvorak"]);

    let groups: Vec<(&str, bool)> = rules
        .option_groups
        .iter()
        .map(|group| (group.item.name.as_str(), group.multiple))
        .collect();
    assert_eq!(groups, [("grp", true), ("Compose key", false)]);
    assert_eq!(
        names(rules.options_of("grp")),
        ["grp:alt_shift_toggle", "grp:win_space_toggle"]
    );
}

#[test]
fn lookups_give_the_description_of_a_name() {
    let rules = sample();

    assert_eq!(rules.layout("de").unwrap().item.label(), "German");
    assert_eq!(
        rules.variant("de", "nodeadkeys").unwrap().label(),
        "German (no dead keys)"
    );
    assert_eq!(
        rules.model("thinkpad").unwrap().label(),
        "IBM ThinkPad 560Z/600/600E/A22E"
    );
    assert_eq!(rules.option("compose:ralt").unwrap().label(), "Right Alt");
    // Without a description, the name is the label.
    assert_eq!(
        rules.option("compose:menu").unwrap().label(),
        "compose:menu"
    );
    assert_eq!(
        rules.option_group("Compose key").unwrap().item.label(),
        "Position of Compose key"
    );

    assert!(rules.layout("xx").is_none());
    assert!(rules.variant("de", "intl").is_none());
    assert!(rules.variant("xx", "intl").is_none());
    assert!(rules.model("pc104").is_none());
    assert!(rules.option("grp:nope").is_none());
    assert_eq!(rules.options_of("nope"), []);
}

#[test]
fn entries_without_a_name_and_unknown_elements_are_skipped() {
    let rules = sample();

    assert!(
        rules
            .models
            .iter()
            .all(|model| model.label() != "A model without a name")
    );
    assert!(
        rules
            .layouts
            .iter()
            .all(|layout| !layout.item.name.is_empty())
    );
    assert!(
        rules
            .layouts
            .iter()
            .all(|layout| layout.variant("lost").is_none())
    );
    // Names are trimmed, and a translated description isn't the English one.
    assert!(rules.model("thinkpad").is_some());
}

#[test]
fn exotic_entries_are_extra() {
    let rules = sample();

    assert!(rules.variant("de", "ro").unwrap().extra);
    assert!(!rules.variant("de", "nodeadkeys").unwrap().extra);

    let extras = parse(SAMPLE_EXTRAS.as_bytes(), true).unwrap();
    assert!(extras.layouts.iter().all(|layout| layout.item.extra));
    assert!(extras.variant("de", "us").unwrap().extra);
}

#[test]
fn the_extras_are_merged_into_the_base_file() {
    let files = MemoryFiles::new()
        .file(BASE, SAMPLE)
        .file(EXTRAS, SAMPLE_EXTRAS);
    let (rules, errors) = XkbRules::load(&files);
    assert_eq!(errors, []);

    let layouts: Vec<(&str, bool)> = rules
        .layouts
        .iter()
        .map(|layout| (layout.item.name.as_str(), layout.item.extra))
        .collect();
    assert_eq!(layouts, [("us", false), ("de", false), ("ancient", true)]);

    // A layout of the base file keeps its entry and gets the new variants of the extras.
    let de = rules.layout("de").unwrap();
    assert_eq!(de.item.label(), "German");
    assert_eq!(names(&de.variants), ["nodeadkeys", "ro", "us"]);
    assert_eq!(de.variants[0].label(), "German (no dead keys)");
    assert!(!de.variants[0].extra);
    assert!(de.variants[2].extra);

    assert_eq!(rules.variant("ancient", "runic").unwrap().label(), "Gothic");
    assert_eq!(
        names(rules.options_of("grp")),
        [
            "grp:alt_shift_toggle",
            "grp:win_space_toggle",
            "grp:sclk_toggle"
        ]
    );
    assert!(rules.option("grp:sclk_toggle").unwrap().extra);
    assert_eq!(rules.option_groups.len(), 2);
}

#[test]
fn the_config_root_comes_from_the_environment() {
    assert_eq!(
        rules_dir(&MemoryFiles::new()),
        Path::new(DEFAULT_ROOT).join("rules")
    );
    assert_eq!(
        rules_dir(&MemoryFiles::new().var("XKB_CONFIG_ROOT", "")),
        Path::new(DEFAULT_ROOT).join("rules")
    );

    let files = MemoryFiles::new()
        .var("XKB_CONFIG_ROOT", "/opt/xkb")
        .file("/opt/xkb/rules/evdev.xml", SAMPLE)
        .file(BASE, SAMPLE_EXTRAS);
    assert_eq!(rules_dir(&files), Path::new("/opt/xkb/rules"));
    let (rules, errors) = XkbRules::load(&files);
    assert_eq!(errors, []);
    assert!(rules.layout("us").is_some());
    assert!(rules.layout("ancient").is_none());
}

#[test]
fn a_missing_base_file_leaves_the_lists_empty() {
    let files = MemoryFiles::new().file(EXTRAS, SAMPLE_EXTRAS);
    let (rules, errors) = XkbRules::load(&files);
    assert_eq!(rules, XkbRules::default());
    assert_eq!(
        errors,
        [RulesError {
            path: PathBuf::from(BASE),
            kind: RulesErrorKind::Missing,
        }]
    );
    assert_eq!(
        errors[0].to_string(),
        "/usr/share/X11/xkb/rules/evdev.xml: not found"
    );
}

#[test]
fn a_broken_base_file_leaves_the_lists_empty() {
    let files = MemoryFiles::new()
        .file(BASE, "<xkbConfigRegistry><modelList>")
        .file(EXTRAS, SAMPLE_EXTRAS);
    let (rules, errors) = XkbRules::load(&files);
    assert_eq!(rules, XkbRules::default());
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].path, Path::new(BASE));
    assert!(matches!(errors[0].kind, RulesErrorKind::Xml(_)));
}

#[test]
fn a_broken_extras_file_keeps_the_base_file() {
    let files = MemoryFiles::new()
        .file(BASE, SAMPLE)
        .file(EXTRAS, "<html/>");
    let (rules, errors) = XkbRules::load(&files);
    assert_eq!(rules, sample());
    assert_eq!(
        errors,
        [RulesError {
            path: PathBuf::from(EXTRAS),
            kind: RulesErrorKind::NotRegistry,
        }]
    );
}

#[test]
fn files_that_are_not_registries_are_refused() {
    let cases: [(&[u8], RulesErrorKind); 5] = [
        (b"", RulesErrorKind::Xml(String::new())),
        (b"<a>\xff</a>", RulesErrorKind::NotUtf8),
        (b"<modelList/>", RulesErrorKind::NotRegistry),
        (
            b"<xkbConfigRegistry><modelList></xkbConfigRegistry>",
            RulesErrorKind::Xml(String::new()),
        ),
        (b"not xml", RulesErrorKind::Xml(String::new())),
    ];
    for (content, expected) in cases {
        let error = parse(content, false).unwrap_err();
        match expected {
            RulesErrorKind::Xml(_) => assert!(matches!(error, RulesErrorKind::Xml(_)), "{error}"),
            expected => assert_eq!(error, expected),
        }
    }
}

#[test]
fn entities_are_refused_and_nothing_is_fetched() {
    // A billion laughs: entities that expand to far more text than the file has.
    let laughs = r#"<?xml version="1.0"?>
<!DOCTYPE xkbConfigRegistry [
  <!ENTITY a "aaaaaaaaaa">
  <!ENTITY b "&a;&a;&a;&a;&a;&a;&a;&a;&a;&a;">
  <!ENTITY c "&b;&b;&b;&b;&b;&b;&b;&b;&b;&b;">
]>
<xkbConfigRegistry><modelList><model><configItem><name>&c;</name></configItem></model></modelList></xkbConfigRegistry>"#;
    assert_eq!(
        parse(laughs.as_bytes(), false),
        Err(RulesErrorKind::EntityDeclaration)
    );

    // An external entity: the file is never read.
    let external = r#"<!DOCTYPE xkbConfigRegistry [<!ENTITY secret SYSTEM "file:///etc/passwd">]>
<xkbConfigRegistry><modelList><model><configItem><name>&secret;</name></configItem></model></modelList></xkbConfigRegistry>"#;
    assert_eq!(
        parse(external.as_bytes(), false),
        Err(RulesErrorKind::EntityDeclaration)
    );

    // An entity that isn't declared is an error of the XML, not looked up anywhere.
    let undeclared = r#"<!DOCTYPE xkbConfigRegistry SYSTEM "http://example.invalid/xkb.dtd">
<xkbConfigRegistry><modelList><model><configItem><name>&secret;</name></configItem></model></modelList></xkbConfigRegistry>"#;
    assert!(matches!(
        parse(undeclared.as_bytes(), false),
        Err(RulesErrorKind::Xml(_))
    ));

    // The predefined entities and character references are text.
    let predefined = r"<xkbConfigRegistry><modelList><model><configItem><name>a&amp;b&#x41;</name></configItem></model></modelList></xkbConfigRegistry>";
    let rules = parse(predefined.as_bytes(), false).unwrap();
    assert_eq!(names(&rules.models), ["a&bA"]);
}

#[test]
fn large_files_are_refused() {
    let mut content = b"<xkbConfigRegistry>".to_vec();
    content.resize(MAX_FILE_SIZE - "</xkbConfigRegistry>".len(), b' ');
    content.extend_from_slice(b"</xkbConfigRegistry>");
    assert_eq!(parse(&content, false), Ok(XkbRules::default()));

    content.insert(1, b' ');
    assert_eq!(parse(&content, false), Err(RulesErrorKind::TooLarge));
    let (rules, errors) = XkbRules::load(&MemoryFiles::new().file(BASE, content));
    assert_eq!(rules, XkbRules::default());
    assert_eq!(errors[0].kind, RulesErrorKind::TooLarge);
}

#[test]
fn disk_reads_no_more_than_the_limit() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("evdev.xml");
    std::fs::write(&path, vec![b' '; 100]).unwrap();
    assert_eq!(Disk.read_bounded(&path, 10).unwrap().len(), 11);
    assert_eq!(Disk.read_bounded(&path, 100).unwrap().len(), 100);
    assert_eq!(
        Disk.read_bounded(&directory.path().join("nope"), 10)
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::NotFound
    );
}

/// The registry of this system, if it has one: it is read without errors and has the layouts
/// every xkeyboard-config has.
#[test]
fn the_registry_of_the_system_is_read() {
    let base = rules_dir(&Disk).join("evdev.xml");
    if !base.exists() {
        return;
    }
    let (rules, errors) = XkbRules::load(&Disk);
    assert_eq!(errors, []);
    assert!(rules.layout("us").is_some());
    assert!(rules.model("pc105").is_some());
    assert_ne!(rules.options_of("grp"), []);
}

proptest! {
    /// Any bytes at all: the parser ends, without a panic.
    #[test]
    fn any_bytes_parse(content in proptest::collection::vec(any::<u8>(), 0..400)) {
        let _ = parse(&content, false);
    }

    /// Registry-like text: whatever is read has a name.
    #[test]
    fn registry_like_text_parses(
        body in "(<(/)?(modelList|model|layoutList|layout|variantList|variant|optionList|group|option|configItem|name|description)>|[a-z &;<>]){0,60}",
    ) {
        let text = format!("<xkbConfigRegistry>{body}</xkbConfigRegistry>");
        if let Ok(rules) = parse(text.as_bytes(), false) {
            prop_assert!(rules.models.iter().all(|model| !model.name.is_empty()));
            prop_assert!(rules.layouts.iter().all(|layout| !layout.item.name.is_empty()));
        }
    }

    /// Cutting the sample anywhere never panics, and loading never panics either.
    #[test]
    fn truncated_samples_parse(cut in 0..SAMPLE.len()) {
        let content = &SAMPLE.as_bytes()[..cut];
        let _ = parse(content, false);
        let _ = XkbRules::load(&MemoryFiles::new().file(BASE, content).file(EXTRAS, content));
    }
}
