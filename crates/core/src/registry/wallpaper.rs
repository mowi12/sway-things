// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Settings of the Wallpaper page (`man 5 sway-output`, `bg`). sway hands the background to
//! swaybg. Without a `bg` line sway starts no swaybg, and the screen stays black.
//!
//! The image, its fill mode and the color are one `output <name> bg …` line: an image with its
//! mode and the color as fallback, or without an image the color alone (`solid_color`).
//!
//! An output can show a collection instead of one image: the `bg` line then names a link in the
//! state folder that points at the current picture of the collection
//! ([`Values::wallpaper_picture`]), and `exec sway-things hook wallpaper` changes the picture
//! at login and starts the timer that changes it later. The settings of a collection are stored
//! per collection, by its name.
//!
//! The keys and the check of a collection name are here, at the bottom of the core, and
//! [`crate::wallpaper`] re-exports them.

use std::path::Path;
use std::str::FromStr;

#[cfg(doc)]
use super::Values;
use super::read::ReadError;
use super::{
    Choice, ConfigFile, Context, Directive, Page, RenderError, Scope, Setting, Value, ValueError,
    When,
};
use crate::command::output::Fill;
use crate::command::{Color, SwayCmd};
use crate::i18n::Text;
use crate::msg;

/// The image of an output, with `~/` for the home folder; empty for none (per output).
pub const IMAGE: &str = "wallpaper.image";
/// How the image covers the output (per output).
pub const FILL: &str = "wallpaper.fill";
/// The color of an output without an image, and around an image that doesn't cover it.
pub const COLOR: &str = "wallpaper.color";
/// The collection an output shows, by name; empty for none (per output).
pub const COLLECTION: &str = "wallpaper.collection";
/// The pictures and folders of a collection, with `~/` for the home folder.
pub const PICTURES: &str = "collection.pictures";
/// When the next picture is shown: `login`, `daily` or `interval`.
pub const CHANGE: &str = "collection.change";
/// The minutes between two pictures, for `interval`.
pub const INTERVAL: &str = "collection.interval";
/// Which picture comes next: `shuffle` or `in_order`.
pub const ORDER: &str = "collection.order";

/// Whether `name` can name a collection: 1 to 64 characters, no control characters and no
/// spaces at either end.
pub fn is_collection_name(name: &str) -> bool {
    (1..=64).contains(&name.chars().count())
        && name.trim() == name
        && !name.chars().any(char::is_control)
}

const CHANGE_CHOICES: &[Choice] = &[
    Choice::new("login", msg!("At login")),
    Choice::new("daily", msg!("Daily")),
    Choice::new("interval", msg!("After the interval")),
];

const ORDER_CHOICES: &[Choice] = &[
    Choice::new("shuffle", msg!("Shuffle")),
    Choice::new("in_order", msg!("In order")),
];

/// The longest interval, in minutes: 30 days.
const MAX_INTERVAL: i64 = 30 * 24 * 60;

const FILL_CHOICES: &[Choice] = &[
    Choice::new("fill", msg!("Fill the screen, cut off what doesn’t fit")),
    Choice::new("fit", msg!("Fit inside the screen")),
    Choice::new("stretch", msg!("Stretch to the screen")),
    Choice::new("center", msg!("Center at its own size")),
    Choice::new("tile", msg!("Tile at its own size")),
];

/// The color of an output without a background: swaybg isn't started, and nothing is drawn.
const BLACK: Color = Color::rgb(0, 0, 0);

const SECTION: crate::i18n::Msg = msg!("Background");
const GROUP: (&str, crate::i18n::Msg) = ("wallpaper.background", msg!("Wallpaper"));
const KEYWORDS: &[crate::i18n::Msg] =
    &[msg!("background"), msg!("desktop picture"), msg!("swaybg")];

pub(super) fn settings() -> Vec<Setting> {
    [
        Setting::text(IMAGE, Page::Wallpaper, "")
            .section(SECTION)
            .group(GROUP.0, GROUP.1)
            .title(msg!("Image"))
            .subtitle(msg!(
                "An absolute path, or one in your home folder (~/…). Without one, the color fills \
                 the screen"
            ))
            .placeholder(msg!("No image"))
            .keywords(KEYWORDS)
            .scope(Scope::PerOutput)
            .requires_tools(&["swaybg"])
            .sway(
                Directive::output("bg"),
                ConfigFile::Output,
                background,
                |args| Ok(Value::Text(read_background(args)?.image)),
            )
            .build(),
        Setting::choice(FILL, Page::Wallpaper, "fill", FILL_CHOICES)
            .section(SECTION)
            .group(GROUP.0, GROUP.1)
            .title(msg!("Fit"))
            .subtitle(msg!("How the image covers the screen"))
            .keywords(&[
                msg!("fill"),
                msg!("fit"),
                msg!("stretch"),
                msg!("center"),
                msg!("tile"),
                msg!("scale"),
            ])
            .scope(Scope::PerOutput)
            .requires_tools(&["swaybg"])
            .depends_on(IMAGE, When::Set)
            .sway(
                Directive::output("bg"),
                ConfigFile::Output,
                background,
                |args| {
                    Ok(Value::Choice(
                        read_background(args)?.fill.keyword().to_owned(),
                    ))
                },
            )
            .build(),
        Setting::opaque_color(COLOR, Page::Wallpaper, BLACK)
            .section(SECTION)
            .group(GROUP.0, GROUP.1)
            .title(msg!("Color"))
            .subtitle(msg!(
                "Fills the screen without an image, and the space around an image that doesn’t \
                 cover it"
            ))
            .keywords(&[msg!("solid color"), msg!("fallback")])
            .scope(Scope::PerOutput)
            .requires_tools(&["swaybg"])
            .sway(
                Directive::output("bg"),
                ConfigFile::Output,
                background,
                |args| Ok(Value::Color(read_background(args)?.color)),
            )
            .build(),
        Setting::text(COLLECTION, Page::Wallpaper, "")
            .section(SECTION)
            .group(GROUP.0, GROUP.1)
            .title(msg!("Collection"))
            .subtitle(msg!(
                "The collection whose pictures take turns, in place of the image"
            ))
            .placeholder(msg!("No collection"))
            .keywords(&[
                msg!("collection"),
                msg!("slideshow"),
                msg!("cycle"),
                msg!("rotate"),
            ])
            .scope(Scope::PerOutput)
            .requires_tools(&["swaybg"])
            .check_text(check_collection)
            .sway(
                Directive::output("bg"),
                ConfigFile::Output,
                background,
                // A `bg` line of the user's names a file, never a collection.
                |args| read_background(args).map(|_| Value::Text(String::new())),
            )
            .build(),
    ]
    .into_iter()
    .chain(collection_settings())
    .collect()
}

/// The settings of a collection.
fn collection_settings() -> Vec<Setting> {
    vec![
        Setting::list(PICTURES, Page::Wallpaper, &[])
            .section(msg!("Collections"))
            .title(msg!("Pictures"))
            .subtitle(msg!(
                "Pictures and folders of the collection; a folder adds every picture in it"
            ))
            .keywords(&[msg!("collection"), msg!("pictures"), msg!("folder")])
            .scope(Scope::PerCollection)
            .build(),
        Setting::choice(CHANGE, Page::Wallpaper, "daily", CHANGE_CHOICES)
            .section(msg!("Collections"))
            .title(msg!("Change picture"))
            .subtitle(msg!(
                "When the next picture of the collection is shown; always at login too"
            ))
            .keywords(&[msg!("collection"), msg!("daily"), msg!("interval")])
            .scope(Scope::PerCollection)
            .build(),
        Setting::integer(INTERVAL, Page::Wallpaper, 30, 1, MAX_INTERVAL)
            .section(msg!("Collections"))
            .title(msg!("Interval"))
            .subtitle(msg!("Minutes until the next picture"))
            .keywords(&[msg!("collection"), msg!("minutes"), msg!("hours")])
            .scope(Scope::PerCollection)
            .depends_on(CHANGE, When::OneOf(&["interval"]))
            .build(),
        Setting::choice(ORDER, Page::Wallpaper, "shuffle", ORDER_CHOICES)
            .section(msg!("Collections"))
            .title(msg!("Order"))
            .subtitle(msg!("Which picture comes next"))
            .keywords(&[msg!("collection"), msg!("shuffle"), msg!("random")])
            .scope(Scope::PerCollection)
            .build(),
    ]
}

/// A collection name, or empty for none.
fn check_collection(name: &str) -> Result<(), ValueError> {
    if name.is_empty() || is_collection_name(name) {
        Ok(())
    } else {
        Err(ValueError::NotACollectionName)
    }
}

/// `output <name> bg <image> <fill> <color>`, or `bg <color> solid_color` without an image. With
/// a collection, the image is the link to its current picture; the generated config also runs
/// `sway-things hook wallpaper` then (see `generate`).
fn background(ctx: &Context<'_>) -> Result<Vec<SwayCmd>, RenderError> {
    let output = ctx.output()?;
    let color = ctx.color(COLOR)?;
    let fill = Fill::from_keyword(ctx.choice(FILL)?).unwrap_or(Fill::Fill);
    if !ctx.text(COLLECTION)?.is_empty() {
        // A collection without a picture has no link to name: the color fills the screen.
        return Ok(vec![match ctx.wallpaper_picture() {
            Some(link) => output.background_image(&link, fill, color)?,
            None => output.background_color(color),
        }]);
    }
    let image = ctx.text(IMAGE)?;
    if image.is_empty() {
        return Ok(vec![output.background_color(color)]);
    }
    Ok(vec![output.background_image(
        Path::new(&image),
        fill,
        color,
    )?])
}

/// What a `bg` line sets.
#[derive(Debug, Clone, PartialEq)]
struct Background {
    /// The image, or empty for a color alone.
    image: String,
    fill: Fill,
    color: Color,
}

/// Reads the arguments of `bg` as sway's `output_cmd_background` does: `<color> solid_color`, or
/// the words of the file up to the first fill mode, the mode, and an optional fallback color.
/// Arguments after those belong to the next subcommand of the line.
fn read_background(args: &[String]) -> Result<Background, ReadError> {
    let rejected = |reason: Text| ReadError::Rejected(reason);
    let (Some(first), Some(second)) = (args.first(), args.get(1)) else {
        return Err(rejected(
            msg!("expected an image and a fill mode, or a color and solid_color").into(),
        ));
    };
    if second.eq_ignore_ascii_case("solid_color") {
        return Ok(Background {
            image: String::new(),
            fill: Fill::Fill,
            color: sway_color(first)
                .ok_or_else(|| rejected(msg!("colors must look like #rrggbb").into()))?,
        });
    }
    let Some(mode) = args
        .iter()
        .position(|arg| Fill::from_keyword(arg).is_some())
    else {
        return Err(rejected(msg!("the fill mode is missing").into()));
    };
    let (Some(fill), Some(words)) = (
        args.get(mode).and_then(|arg| Fill::from_keyword(arg)),
        args.get(..mode).filter(|words| !words.is_empty()),
    ) else {
        return Err(rejected(msg!("the image is missing").into()));
    };
    let image = image_path(&words.join(" "))?;
    // sway 1.10 rejects a malformed fallback color, later versions ignore it.
    let color = match args.get(mode + 1).filter(|arg| arg.starts_with('#')) {
        Some(fallback) => sway_color(fallback)
            .ok_or_else(|| rejected(msg!("colors must look like #rrggbb").into()))?,
        None => BLACK,
    };
    Ok(Background { image, fill, color })
}

/// A color as sway's `bg` takes it: `#` and six hex digits.
fn sway_color(text: &str) -> Option<Color> {
    let digits = text.strip_prefix('#')?;
    (digits.len() == 6 && digits.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then(|| Color::from_str(text).ok())
        .flatten()
}

/// The file of a `bg` line as `wordexp(3)` reads it, without running anything: quotes and
/// backslashes go, a leading `~/` stays for the home folder. A path that needs a variable or a
/// command, or that is relative to the config file, can't be held by the setting.
fn image_path(word: &str) -> Result<String, ReadError> {
    let unsupported = |reason: crate::i18n::Msg| ReadError::Unsupported(reason.into());
    let mut out = String::with_capacity(word.len());
    let mut quote = None;
    let mut chars = word.chars();
    while let Some(character) = chars.next() {
        match (quote, character) {
            (None, '\'' | '"') => quote = Some(character),
            (Some(open), _) if open == character => quote = None,
            (Some('\''), _) => out.push(character),
            (_, '$' | '`') => {
                return Err(unsupported(msg!(
                    "the path of the image uses a variable or a command"
                )));
            }
            (_, '\\') => out.extend(chars.next()),
            (None, '~') if !out.is_empty() || !word.starts_with("~/") => {
                return Err(unsupported(msg!(
                    "the path of the image uses ~ for another user"
                )));
            }
            _ => out.push(character),
        }
    }
    if quote.is_some() {
        return Err(ReadError::Rejected(
            msg!("a quote in the path isn't closed").into(),
        ));
    }
    if !out.starts_with('/') && !out.starts_with("~/") {
        return Err(unsupported(msg!(
            "the path of the image is relative to the config file"
        )));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{BLACK, Background, Fill, read_background};
    use crate::command::Color;
    use crate::registry::ReadError;

    fn read(line: &str) -> Result<Background, ReadError> {
        let args: Vec<String> = line.split(' ').map(ToOwned::to_owned).collect();
        read_background(&args)
    }

    fn image(image: &str, fill: Fill, color: Color) -> Background {
        Background {
            image: image.to_owned(),
            fill,
            color,
        }
    }

    #[test]
    fn lines_read_like_sway_reads_them() {
        assert_eq!(
            read("#285577 solid_color"),
            Ok(Background {
                image: String::new(),
                fill: Fill::Fill,
                color: Color::rgb(0x28, 0x55, 0x77),
            })
        );
        assert_eq!(
            read("~/Pictures/lake.jpg FIT #102030"),
            Ok(image(
                "~/Pictures/lake.jpg",
                Fill::Fit,
                Color::rgb(0x10, 0x20, 0x30)
            ))
        );
        assert_eq!(
            read("/usr/share/backgrounds/sway/Sway_Wallpaper_Blue_1920x1080.png fill"),
            Ok(image(
                "/usr/share/backgrounds/sway/Sway_Wallpaper_Blue_1920x1080.png",
                Fill::Fill,
                BLACK
            ))
        );
        // The words up to the mode are the file; what follows the color is the next subcommand.
        assert_eq!(
            read("/my pictures/a.png tile #000000 scale 2"),
            Ok(image("/my pictures/a.png", Fill::Tile, BLACK))
        );
        // Quotes for `wordexp`, as Sway Things writes them.
        assert_eq!(
            read(r#"~/'it'"'"'s.png' center #ffffff"#),
            Ok(image(
                "~/it's.png",
                Fill::Center,
                Color::rgb(0xff, 0xff, 0xff)
            ))
        );
        assert_eq!(
            read(r"/a\ b.png fill"),
            Ok(image("/a b.png", Fill::Fill, BLACK))
        );
    }

    #[test]
    fn lines_sway_rejects_or_the_setting_cant_hold_are_told_apart() {
        for line in [
            "#285577",
            "red solid_color",
            "/a.png",
            "fill",
            "/a.png fill #abc",
        ] {
            assert!(matches!(read(line), Err(ReadError::Rejected(_))), "{line}");
        }
        for line in [
            "$HOME/a.png fill",
            "`echo /a.png` fill",
            "~me/a.png fill",
            "a.png fill",
        ] {
            assert!(
                matches!(read(line), Err(ReadError::Unsupported(_))),
                "{line}"
            );
        }
    }
}
