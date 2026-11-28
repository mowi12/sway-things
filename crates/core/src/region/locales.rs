// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The locales of the system: which UTF-8 locales are installed (`locale -a -v`), their names
//! in the user's language (from iso-codes), what their formats look like (`locale -k`), and how
//! to add a missing one on each distribution.
//!
//! A locale is named as glibc names it: a language (ISO 639, `de`), maybe a territory (ISO 3166,
//! `DE`), a codeset (`.UTF-8`, also spelled `.utf8`) and maybe a modifier (`@latin`). Sway Things
//! only offers UTF-8 locales and writes them as `ll_CC.UTF-8[@modifier]`: glibc finds `de_DE.utf8`
//! by that name, and a locale installed without a codeset in its name but in UTF-8 (`aa_ER`) too.
//! Every program runs `locale` with an argument array and the locale in its environment, never
//! through a shell.

use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;
use std::time::Duration;

use futures_lite::FutureExt;

use crate::i18n::{Iso, Text, tr_iso};
use crate::msg;
use crate::registry::region::is_utf8;
pub use crate::registry::region::{Locale, is_locale_name};
use crate::scanners::look::Family;

/// Where iso-codes keeps its lists as JSON.
const ISO_CODES: &str = "/usr/share/iso-codes/json";

/// The categories of formats `locale -k` is asked about for the preview.
const PREVIEW_CATEGORIES: [&str; 5] = [
    "LC_TIME",
    "LC_NUMERIC",
    "LC_MONETARY",
    "LC_PAPER",
    "LC_MEASUREMENT",
];

/// How long `locale` may take.
const TIMEOUT: Duration = Duration::from_secs(5);

impl Locale {
    /// The name in the user's language, from iso-codes: `German (Germany)`, `Esperanto`,
    /// `Serbian (Serbia, latin)`. A code iso-codes doesn't know stays a code.
    pub fn display_name(&self) -> String {
        if self.language == "C" {
            return msg_text(msg!("Plain (C locale, English without formats)"));
        }
        let names = iso_names();
        let language = names.languages.get(&self.language).map_or_else(
            || self.language.clone(),
            |name| tr_iso(Iso::Language, name).to_owned(),
        );
        let territory = self.territory.as_ref().map(|code| {
            names.countries.get(code).map_or_else(
                || code.clone(),
                |name| tr_iso(Iso::Country, name).to_owned(),
            )
        });
        match (territory, self.modifier.clone()) {
            (None, None) => language,
            (Some(place), None) | (None, Some(place)) => crate::i18n::trf(
                // TRANSLATORS: A locale's name: its language and its country, such as
                // "German (Germany)".
                msg!("{language} ({country})"),
                &[("language", &language), ("country", &place)],
            ),
            (Some(country), Some(modifier)) => crate::i18n::trf(
                // TRANSLATORS: A locale's name: its language, its country and a variant, such as
                // "Serbian (Serbia, latin)".
                msg!("{language} ({country}, {variant})"),
                &[
                    ("language", &language),
                    ("country", &country),
                    ("variant", &modifier),
                ],
            ),
        }
    }
}

/// The text of `msg` in the user's language.
fn msg_text(msg: crate::i18n::Msg) -> String {
    crate::i18n::tr(msg).to_owned()
}

/// The names of the installed UTF-8 locales in `locale -a -v`'s output, as Sway Things writes
/// them, each once and sorted: `de_DE.utf8` and a `de_DE` in UTF-8 are both `de_DE.UTF-8`. The C
/// locale and locales in another codeset are left out.
pub fn parse_installed(verbose: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut current: Option<&str> = None;
    for line in verbose.lines() {
        if let Some(rest) = line.strip_prefix("locale:") {
            current = rest.split_whitespace().next();
            continue;
        }
        let Some((key, value)) = line.split_once('|') else {
            continue;
        };
        if key.trim() != "codeset" || !is_utf8(value.trim()) {
            continue;
        }
        let Some(locale) = current.take().and_then(Locale::parse) else {
            continue;
        };
        if locale.language != "C" {
            names.push(locale.name());
        }
    }
    names.sort();
    names.dedup();
    names
}

/// The installed UTF-8 locales, as [`parse_installed`] gives them; empty if `locale` can't be
/// run (a system without glibc's `locale`).
pub async fn installed() -> Vec<String> {
    run_locale(&["-a", "-v"], None)
        .await
        .map(|output| parse_installed(&output))
        .unwrap_or_default()
}

/// Runs `locale <args>`, with `locale` as the only locale of its environment if given, and gives
/// its output; `None` if it failed, didn't answer in time, or `locale` doesn't exist there.
async fn run_locale(args: &[&str], locale: Option<&str>) -> Option<String> {
    let run = async {
        let mut command = async_process::Command::new("locale");
        command
            .args(args)
            .stdin(async_process::Stdio::null())
            .env_remove("LANGUAGE");
        if let Some(locale) = locale {
            command.env("LC_ALL", locale);
        }
        let output = command.output().await.ok()?;
        // glibc's `locale` warns ("Cannot set LC_ALL to default locale") and goes on with the C
        // locale when the locale isn't installed.
        if !output.status.success() || (locale.is_some() && !output.stderr.is_empty()) {
            return None;
        }
        Some(String::from_utf8_lossy(&output.stdout).into_owned())
    };
    let timeout = async {
        async_io::Timer::after(TIMEOUT).await;
        None
    };
    run.or(timeout).await
}

/// The keywords of `locale -k` (`d_fmt="%d.%m.%Y"`, `grouping=3;3`) by name, strings without
/// their quotes.
pub fn parse_keywords(text: &str) -> HashMap<String, String> {
    text.lines()
        .filter_map(|line| {
            let (name, value) = line.split_once('=')?;
            let value = value
                .strip_prefix('"')
                .and_then(|value| value.strip_suffix('"'))
                .unwrap_or(value);
            Some((name.to_owned(), value.to_owned()))
        })
        .collect()
}

/// The keywords of the preview's categories for `locale`; `None` if it isn't installed.
pub async fn keywords(locale: &str) -> Option<HashMap<String, String>> {
    if !is_locale_name(locale) {
        return None;
    }
    let mut args = vec!["-k"];
    args.extend(PREVIEW_CATEGORIES);
    run_locale(&args, Some(locale))
        .await
        .map(|output| parse_keywords(&output))
}

/// A moment to show in the preview, in local time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Moment {
    /// The year.
    pub year: i32,
    /// The month, 1 to 12.
    pub month: u8,
    /// The day of the month, 1 to 31.
    pub day: u8,
    /// The day of the week, 0 for Sunday to 6 for Saturday.
    pub weekday: u8,
    /// The hour, 0 to 23.
    pub hour: u8,
    /// The minute.
    pub minute: u8,
    /// The second.
    pub second: u8,
}

/// What the formats of a locale look like: each line of the preview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preview {
    /// A date (`24.11.2026`).
    pub date: String,
    /// A time (`21:35:12`).
    pub time: String,
    /// A number (`1.234.567,89`).
    pub number: String,
    /// An amount of money (`1.234,56 €`).
    pub money: String,
    /// The paper size (`A4`).
    pub paper: Text,
    /// The units (`Metric`).
    pub units: Text,
}

impl Preview {
    /// The preview of the formats in `keywords` ([`keywords`]) at `moment`.
    pub fn new(keywords: &HashMap<String, String>, moment: &Moment) -> Self {
        let get = |name: &str| keywords.get(name).map_or("", String::as_str);
        let number = |name: &str, default: i64| {
            keywords
                .get(name)
                .and_then(|value| value.parse::<i64>().ok())
                .unwrap_or(default)
        };
        let date = strftime(non_empty(get("d_fmt"), "%m/%d/%y"), keywords, moment);
        let time = strftime(non_empty(get("t_fmt"), "%H:%M:%S"), keywords, moment);
        let number_text = group(
            "1234567",
            get("grouping"),
            get("thousands_sep"),
            non_empty(get("decimal_point"), "."),
            "89",
        );
        let digits = usize::try_from(number("frac_digits", 2).clamp(0, 4)).unwrap_or(2);
        let fraction = "5678".get(..digits).unwrap_or_default();
        let amount = group(
            "1234",
            get("mon_grouping"),
            get("mon_thousands_sep"),
            non_empty(get("mon_decimal_point"), "."),
            fraction,
        );
        let symbol = get("currency_symbol");
        let money = if symbol.is_empty() {
            amount
        } else {
            let space = if number("p_sep_by_space", 0) == 1 {
                "\u{a0}"
            } else {
                ""
            };
            if number("p_cs_precedes", 1) == 1 {
                format!("{symbol}{space}{amount}")
            } else {
                format!("{amount}{space}{symbol}")
            }
        };
        Self {
            date,
            time,
            number: number_text,
            money,
            paper: paper(number("width", 0), number("height", 0)),
            units: if number("measurement", 1) == 2 {
                msg!("US customary (inches, pounds)").into()
            } else {
                msg!("Metric (centimeters, kilograms)").into()
            },
        }
    }
}

fn non_empty<'a>(value: &'a str, default: &'a str) -> &'a str {
    if value.is_empty() { default } else { value }
}

/// The name of the paper size `width` × `height` millimeters.
fn paper(width: i64, height: i64) -> Text {
    match (width, height) {
        (210, 297) => Text::data("A4"),
        (216, 279) => msg!("Letter").into(),
        (0, _) | (_, 0) => msg!("Unknown").into(),
        _ => Text::new(msg!("{width} × {height} mm"))
            .arg("width", width)
            .arg("height", height),
    }
}

/// `whole` (digits) grouped from the right by `grouping` (`3;2`, as `locale -k` prints it: the
/// sizes from the right, the last repeating, `-1` or `0` stopping) with `separator`, then
/// `point` and `fraction` if there is one.
fn group(whole: &str, grouping: &str, separator: &str, point: &str, fraction: &str) -> String {
    let sizes: Vec<i64> = grouping
        .split(';')
        .filter_map(|size| size.trim().parse().ok())
        .collect();
    let digits: Vec<char> = whole.chars().collect();
    let mut groups: Vec<String> = Vec::new();
    let mut end = digits.len();
    let mut index = 0;
    while end > 0 {
        let size = if separator.is_empty() {
            0
        } else {
            sizes
                .get(index)
                .or(sizes.last())
                .copied()
                .unwrap_or_default()
        };
        let Ok(size) = usize::try_from(size) else {
            groups.push(digits[..end].iter().collect());
            break;
        };
        if size == 0 || size >= end {
            groups.push(digits[..end].iter().collect());
            break;
        }
        groups.push(digits[end - size..end].iter().collect());
        end -= size;
        index += 1;
    }
    groups.reverse();
    let mut text = groups.join(separator);
    if !fraction.is_empty() {
        text.push_str(point);
        text.push_str(fraction);
    }
    text
}

/// `format`, a format of strftime as a locale has it, filled in with `moment`, the names of days
/// and months from `keywords`. Covers what the formats of glibc's locales use; the `E` and `O`
/// modifiers are ignored, an unknown conversion is kept as it is.
fn strftime(format: &str, keywords: &HashMap<String, String>, moment: &Moment) -> String {
    strftime_depth(format, keywords, moment, 0)
}

fn strftime_depth(
    format: &str,
    keywords: &HashMap<String, String>,
    moment: &Moment,
    depth: u8,
) -> String {
    let name = |key: &str, index: u8| -> String {
        keywords
            .get(key)
            .and_then(|names| names.split(';').nth(usize::from(index)))
            .unwrap_or_default()
            .to_owned()
    };
    let nested = |key: &str, default: &str| -> String {
        if depth > 2 {
            return String::new();
        }
        let inner = keywords
            .get(key)
            .map(String::as_str)
            .filter(|inner| !inner.is_empty())
            .unwrap_or(default);
        strftime_depth(inner, keywords, moment, depth + 1)
    };
    let hour12 = match moment.hour % 12 {
        0 => 12,
        hour => hour,
    };
    let month = moment.month.saturating_sub(1);
    let mut text = String::new();
    let mut chars = format.chars().peekable();
    while let Some(char) = chars.next() {
        if char != '%' {
            text.push(char);
            continue;
        }
        // Flags and modifiers glibc takes; the preview shows the plain form.
        let mut pad = true;
        while let Some(flag) = chars.peek().copied() {
            if matches!(flag, '-' | '_' | '0' | '^' | '#' | 'E' | 'O') {
                if flag == '-' {
                    pad = false;
                }
                chars.next();
            } else {
                break;
            }
        }
        let two = |value: u8| {
            if pad {
                format!("{value:02}")
            } else {
                value.to_string()
            }
        };
        let Some(conversion) = chars.next() else {
            text.push('%');
            break;
        };
        let piece = match conversion {
            'a' => name("abday", moment.weekday),
            'A' => name("day", moment.weekday),
            'b' | 'h' => name("abmon", month),
            'B' => name("mon", month),
            'd' => two(moment.day),
            'e' => format!("{:>2}", moment.day),
            'm' => two(moment.month),
            'y' => format!("{:02}", moment.year.rem_euclid(100)),
            'Y' => moment.year.to_string(),
            'C' => format!("{:02}", moment.year.div_euclid(100)),
            'H' => two(moment.hour),
            'k' => format!("{:>2}", moment.hour),
            'I' => two(hour12),
            'l' => format!("{hour12:>2}"),
            'M' => two(moment.minute),
            'S' => two(moment.second),
            'p' => name("am_pm", u8::from(moment.hour >= 12)),
            'P' => name("am_pm", u8::from(moment.hour >= 12)).to_lowercase(),
            'T' => format!(
                "{:02}:{:02}:{:02}",
                moment.hour, moment.minute, moment.second
            ),
            'R' => format!("{:02}:{:02}", moment.hour, moment.minute),
            'D' => format!(
                "{:02}/{:02}/{:02}",
                moment.month,
                moment.day,
                moment.year.rem_euclid(100)
            ),
            'F' => format!("{}-{:02}-{:02}", moment.year, moment.month, moment.day),
            'r' => nested("t_fmt_ampm", "%I:%M:%S %p"),
            'x' => nested("d_fmt", "%m/%d/%y"),
            'X' => nested("t_fmt", "%H:%M:%S"),
            'Z' | 'z' => String::new(),
            'n' => "\n".to_owned(),
            't' => "\t".to_owned(),
            '%' => "%".to_owned(),
            other => format!("%{other}"),
        };
        text.push_str(&piece);
    }
    text.trim().to_owned()
}

/// The English names of iso-codes: languages by their ISO 639 code, two or three letters, and
/// countries by their two-letter code.
#[derive(Debug, Default)]
pub struct IsoNames {
    /// The languages.
    pub languages: HashMap<String, String>,
    /// The countries.
    pub countries: HashMap<String, String>,
}

impl IsoNames {
    /// The names in the JSON files of iso-codes in `dir`; missing or unreadable files give no
    /// names.
    pub fn load(dir: &Path) -> Self {
        let read = |file: &str| std::fs::read_to_string(dir.join(file)).unwrap_or_default();
        Self {
            languages: parse_iso(&read("iso_639-3.json"), "639-3", &["alpha_2", "alpha_3"]),
            countries: parse_iso(&read("iso_3166-1.json"), "3166-1", &["alpha_2"]),
        }
    }
}

/// The names in an iso-codes JSON file, `{"<list>": [{"alpha_2": …, "name": …}, …]}`, by each
/// code in `codes`. A country's common name wins over its formal one (`Bolivia`, not `Bolivia,
/// Plurinational State of`); iso-codes translates both.
pub fn parse_iso(json: &str, list: &str, codes: &[&str]) -> HashMap<String, String> {
    #[derive(serde::Deserialize)]
    struct Entry {
        #[serde(flatten)]
        fields: HashMap<String, serde_json::Value>,
    }
    let Ok(mut root) = serde_json::from_str::<HashMap<String, Vec<Entry>>>(json) else {
        return HashMap::new();
    };
    let mut names = HashMap::new();
    for entry in root.remove(list).unwrap_or_default() {
        let text = |key: &str| entry.fields.get(key).and_then(serde_json::Value::as_str);
        let Some(name) = text("common_name").or_else(|| text("name")) else {
            continue;
        };
        for code in codes {
            if let Some(code) = text(code) {
                names.insert(code.to_owned(), crate::dbus::clean(name));
            }
        }
    }
    names
}

/// The names of iso-codes of this system, read once.
fn iso_names() -> &'static IsoNames {
    static NAMES: OnceLock<IsoNames> = OnceLock::new();
    NAMES.get_or_init(|| IsoNames::load(Path::new(ISO_CODES)))
}

/// The command that adds locales on `family`, to show beside the list; `None` where Sway Things
/// doesn't know it. Only ever shown, never run.
pub fn add_command(family: Family) -> Option<&'static str> {
    match family {
        Family::Suse => Some("sudo zypper install glibc-locale"),
        Family::Fedora => Some("sudo dnf install glibc-all-langpacks"),
        Family::Debian => Some("sudo dpkg-reconfigure locales"),
        Family::Arch => Some("sudoedit /etc/locale.gen && sudo locale-gen"),
        Family::Void => {
            Some("sudoedit /etc/default/libc-locales && sudo xbps-reconfigure -f glibc-locales")
        }
        Family::Other => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_taken_apart_in_every_spelling() {
        let german = Locale::parse("de_DE.utf8").unwrap();
        assert_eq!(german.name(), "de_DE.UTF-8");
        assert_eq!(Locale::parse("de_DE").unwrap(), german);
        assert_eq!(
            Locale::parse("sr_RS.UTF-8@latin").unwrap().name(),
            "sr_RS.UTF-8@latin"
        );
        assert_eq!(
            Locale::parse("sr_RS@latin").unwrap().name(),
            "sr_RS.UTF-8@latin"
        );
        assert_eq!(Locale::parse("eo").unwrap().name(), "eo.UTF-8");
        assert_eq!(
            Locale::parse("es_419.UTF-8").unwrap().name(),
            "es_419.UTF-8"
        );
        assert_eq!(Locale::parse("C.UTF-8").unwrap().name(), "C.UTF-8");
        for bad in [
            "",
            "C",
            "POSIX",
            "de_de",
            "DE_DE",
            "de_DE.UTF-8 ",
            "de_DE.UTF-8\nX=1",
            "de_DE.",
            "de_DE@",
            "d",
            "deutsch",
            "de_DE.UTF-8@a b",
            "$(rm)",
            "de_DE;x",
        ] {
            assert!(Locale::parse(bad).is_none(), "{bad:?}");
        }
    }

    #[test]
    fn only_utf8_names_can_be_stored() {
        for good in [
            "de_DE.UTF-8",
            "de_DE.utf8",
            "sr_RS.UTF-8@latin",
            "eo.UTF-8",
            "C.UTF-8",
        ] {
            assert!(is_locale_name(good), "{good}");
        }
        for bad in [
            "de_DE",
            "de_DE.ISO-8859-1",
            "de_DE@euro",
            "C",
            "",
            "de_DE.UTF-8\n",
        ] {
            assert!(!is_locale_name(bad), "{bad:?}");
        }
    }

    #[test]
    fn installed_locales_merge_their_spellings_and_skip_other_codesets() {
        let verbose = "\
locale: aa_DJ           directory: /usr/lib/locale/aa_DJ
-------------------------------------------------------------------------------
    title | Afar language locale for Djibouti (Cadu/Laaqo Dialects).
  codeset | ISO-8859-1

locale: aa_ER           directory: /usr/lib/locale/aa_ER
-------------------------------------------------------------------------------
  codeset | UTF-8

locale: C.utf8          directory: /usr/lib/locale/C.utf8
  codeset | UTF-8

locale: de_DE.utf8      archive: /usr/lib/locale/locale-archive
-------------------------------------------------------------------------------
 language | German
  codeset | UTF-8

locale: de_DE.UTF-8     archive: /usr/lib/locale/locale-archive
  codeset | UTF-8

locale: de_DE@euro      directory: /usr/lib/locale/de_DE@euro
  codeset | ISO-8859-15

locale: sr_RS@latin     directory: /usr/lib/locale/sr_RS@latin
  codeset | UTF-8

locale: POSIX           directory: /usr/lib/locale/POSIX
  codeset | ANSI_X3.4-1968
";
        assert_eq!(
            parse_installed(verbose),
            ["aa_ER.UTF-8", "de_DE.UTF-8", "sr_RS.UTF-8@latin"]
        );
    }

    fn german() -> HashMap<String, String> {
        parse_keywords(
            "abday=\"So;Mo;Di;Mi;Do;Fr;Sa\"
day=\"Sonntag;Montag;Dienstag;Mittwoch;Donnerstag;Freitag;Samstag\"
abmon=\"Jan;Feb;Mär;Apr;Mai;Jun;Jul;Aug;Sep;Okt;Nov;Dez\"
mon=\"M1;M2;März;M4;M5;M6;M7;M8;M9;M10;November;M12\"
am_pm=\";\"
d_fmt=\"%d.%m.%Y\"
t_fmt=\"%T\"
decimal_point=\",\"
thousands_sep=\".\"
grouping=3
currency_symbol=\"€\"
mon_decimal_point=\",\"
mon_thousands_sep=\".\"
mon_grouping=3
frac_digits=2
p_cs_precedes=0
p_sep_by_space=1
height=297
width=210
measurement=1
",
        )
    }

    const MOMENT: Moment = Moment {
        year: 2026,
        month: 11,
        day: 4,
        weekday: 3,
        hour: 21,
        minute: 5,
        second: 9,
    };

    #[test]
    fn the_preview_follows_the_keywords() {
        let preview = Preview::new(&german(), &MOMENT);
        assert_eq!(preview.date, "04.11.2026");
        assert_eq!(preview.time, "21:05:09");
        assert_eq!(preview.number, "1.234.567,89");
        assert_eq!(preview.money, "1.234,56\u{a0}€");
        assert_eq!(preview.paper.english(), "A4");
        assert_eq!(preview.units.english(), "Metric (centimeters, kilograms)");

        let american = parse_keywords(
            "abmon=\"Jan;Feb;Mar;Apr;May;Jun;Jul;Aug;Sep;Oct;Nov;Dec\"
am_pm=\"AM;PM\"
d_fmt=\"%m/%d/%Y\"
t_fmt=\"%r\"
t_fmt_ampm=\"%I:%M:%S %p\"
decimal_point=\".\"
thousands_sep=\",\"
grouping=3;3
currency_symbol=\"$\"
mon_decimal_point=\".\"
mon_thousands_sep=\",\"
mon_grouping=3;3
frac_digits=2
p_cs_precedes=1
p_sep_by_space=0
height=279
width=216
measurement=2",
        );
        let preview = Preview::new(&american, &MOMENT);
        assert_eq!(preview.date, "11/04/2026");
        assert_eq!(preview.time, "09:05:09 PM");
        assert_eq!(preview.number, "1,234,567.89");
        assert_eq!(preview.money, "$1,234.56");
        assert_eq!(preview.paper.english(), "Letter");
        assert_eq!(preview.units.english(), "US customary (inches, pounds)");
    }

    #[test]
    fn numbers_group_as_the_locale_says() {
        assert_eq!(group("1234567", "3;2", ",", ".", "89"), "12,34,567.89");
        assert_eq!(group("1234567", "-1", ".", ",", ""), "1234567");
        assert_eq!(group("1234567", "3", "", ",", "5"), "1234567,5");
        assert_eq!(group("123", "3", ".", ",", ""), "123");
        assert_eq!(group("1234567", "3;-1", " ", ",", ""), "1234 567");
    }

    #[test]
    fn strftime_covers_the_formats_of_glibc() {
        let keywords = german();
        assert_eq!(
            strftime("%A, %e. %B %Y %%", &keywords, &MOMENT),
            "Mittwoch,  4. November 2026 %"
        );
        assert_eq!(
            strftime("%-d.%-m.%y %Ey %Q", &keywords, &MOMENT),
            "4.11.26 26 %Q"
        );
        assert_eq!(strftime("%x %X", &keywords, &MOMENT), "04.11.2026 21:05:09");
        assert_eq!(
            strftime("%F %R %D", &keywords, &MOMENT),
            "2026-11-04 21:05 11/04/26"
        );
        // A format naming itself doesn't loop.
        let mut looping = keywords.clone();
        looping.insert("d_fmt".to_owned(), "%x".to_owned());
        assert_eq!(strftime("%x", &looping, &MOMENT), "");
    }

    #[test]
    fn names_come_from_iso_codes() {
        let languages = parse_iso(
            r#"{"639-3": [{"alpha_3": "deu", "alpha_2": "de", "name": "German"},
                          {"alpha_3": "epo", "alpha_2": "eo", "name": "Esperanto"},
                          {"alpha_3": "tok", "name": "Toki Pona"}]}"#,
            "639-3",
            &["alpha_2", "alpha_3"],
        );
        assert_eq!(languages.get("de").map(String::as_str), Some("German"));
        assert_eq!(languages.get("deu").map(String::as_str), Some("German"));
        assert_eq!(languages.get("tok").map(String::as_str), Some("Toki Pona"));
        let countries = parse_iso(
            r#"{"3166-1": [{"alpha_2": "BO", "name": "Bolivia, Plurinational State of",
                            "common_name": "Bolivia"},
                           {"alpha_2": "DE", "name": "Germany\u001b[31m"}]}"#,
            "3166-1",
            &["alpha_2"],
        );
        assert_eq!(countries.get("BO").map(String::as_str), Some("Bolivia"));
        assert_eq!(
            countries.get("DE").map(String::as_str),
            Some("Germany [31m")
        );
        assert!(parse_iso("not json", "639-3", &["alpha_2"]).is_empty());
    }

    #[test]
    fn every_known_distribution_has_a_command() {
        for family in [
            Family::Suse,
            Family::Fedora,
            Family::Debian,
            Family::Arch,
            Family::Void,
        ] {
            assert!(add_command(family).is_some(), "{family:?}");
        }
        assert_eq!(add_command(Family::Other), None);
    }
}
