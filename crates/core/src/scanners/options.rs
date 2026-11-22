// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What a scanner can do, from `scanimage -d DEVICE -A`: its sources (flatbed, document feeder),
//! its modes (color, gray, black and white), its resolutions and the size of its scan area. Only
//! what the scanner offers is offered, with the values it names, which scanimage takes back as
//! they are.
//!
//! `-A` prints an option a line, indented, with its constraint and its value in brackets:
//!
//! ```text
//!     --mode Lineart|Gray|Color [Color]
//!     --resolution 75|150|300|600dpi [150]
//!     --resolution 1..1200dpi (in steps of 1) [300]
//!     --source Flatbed|ADF [Flatbed]
//!     -x 0..215.9mm (in steps of 0.0999908) [215.9]
//! ```
//!
//! An option that can't be set now says `[inactive]` instead of a value; the descriptions are
//! the lines that don't start with `-`.

/// What a scan mode is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModeKind {
    /// In color.
    Color,
    /// In shades of gray.
    Gray,
    /// In black and white only (line art).
    Lineart,
}

/// A value of an option as the scanner names it, with what it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice<K> {
    /// The value, as scanimage takes it.
    pub value: String,
    /// What it is.
    pub kind: K,
}

/// What a source is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SourceKind {
    /// The glass (flatbed, platen).
    Flatbed,
    /// The document feeder, one side of each sheet.
    Feeder,
    /// The document feeder, both sides.
    FeederDuplex,
    /// Something else (film, transparency adapter): its value is shown as it is.
    Other,
}

impl SourceKind {
    /// Whether it feeds several sheets, so a scan goes on until the feeder is empty.
    pub fn feeds(self) -> bool {
        matches!(self, Self::Feeder | Self::FeederDuplex)
    }
}

/// What a scanner can do.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Options {
    /// Its sources, in its order; empty if it has one only.
    pub sources: Vec<Choice<SourceKind>>,
    /// The source set now.
    pub source: Option<String>,
    /// Its modes, color first; empty if it doesn't say.
    pub modes: Vec<Choice<ModeKind>>,
    /// The mode set now.
    pub mode: Option<String>,
    /// The resolutions to offer, in dpi, lowest first; empty if it doesn't say.
    pub resolutions: Vec<u32>,
    /// The resolution set now.
    pub resolution: Option<u32>,
    /// The largest scan area, width and height in mm, if it can be set.
    pub area: Option<(f64, f64)>,
}

/// The resolutions offered within a range, as scan apps offer them.
const COMMON_RESOLUTIONS: [u32; 9] = [75, 100, 150, 200, 300, 400, 600, 1200, 2400];

impl Options {
    /// The resolution to scan documents at: 300 dpi, else the offered one closest to it.
    pub fn document_resolution(&self) -> Option<u32> {
        self.resolutions
            .iter()
            .copied()
            .min_by_key(|dpi| dpi.abs_diff(300))
    }

    /// The resolution of a preview: the lowest offered from 75 dpi up, else the lowest.
    pub fn preview_resolution(&self) -> Option<u32> {
        self.resolutions
            .iter()
            .copied()
            .find(|dpi| *dpi >= 75)
            .or_else(|| self.resolutions.first().copied())
    }

    /// The value of the mode `kind`, if the scanner has one.
    pub fn mode_of(&self, kind: ModeKind) -> Option<&str> {
        self.modes
            .iter()
            .find(|mode| mode.kind == kind)
            .map(|mode| mode.value.as_str())
    }
}

/// What a line of `-A` says about one option.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Line<'a> {
    /// Its name with its dashes: `--mode`, `-x`.
    name: &'a str,
    /// What it may be: `Gray|Color`, `1..1200dpi (in steps of 1)`.
    constraint: &'a str,
    /// Its value now, or `inactive`; empty if it has none.
    value: &'a str,
}

/// The option on `line`, if it is one: indented, starting with a dash.
fn option_line(line: &str) -> Option<Line<'_>> {
    let trimmed = line.trim();
    if !line.starts_with(' ') || !trimmed.starts_with('-') {
        return None;
    }
    let (name, rest) = trimmed.split_once(' ').unwrap_or((trimmed, ""));
    // A boolean's `[=(yes|no)]` sticks to its name.
    let name = name.split('[').next().unwrap_or(name);
    let rest = rest.trim();
    let (constraint, value) = match rest.rsplit_once(" [") {
        Some((constraint, value)) if value.ends_with(']') => {
            (constraint.trim(), value.trim_end_matches(']'))
        }
        _ if rest.starts_with('[') && rest.ends_with(']') => ("", &rest[1..rest.len() - 1]),
        _ => (rest, ""),
    };
    Some(Line {
        name,
        constraint,
        value,
    })
}

/// The values of a list constraint (`Gray|Color`, `75|150|300dpi`), with the unit taken off the
/// last one; `None` for a range.
fn list(constraint: &str, unit: &str) -> Option<Vec<String>> {
    if constraint.contains("..") {
        return None;
    }
    let constraint = constraint.strip_suffix(unit).unwrap_or(constraint);
    let values: Vec<String> = constraint
        .split('|')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect();
    (!values.is_empty()).then_some(values)
}

/// The bounds of a range constraint (`1..1200dpi (in steps of 1)`, `0..215.9mm`) in `unit`.
fn range(constraint: &str, unit: &str) -> Option<(f64, f64)> {
    let bounds = constraint.split(" (").next().unwrap_or(constraint).trim();
    let (low, high) = bounds.split_once("..")?;
    let high = high.strip_suffix(unit)?;
    Some((low.trim().parse().ok()?, high.trim().parse().ok()?))
}

/// What the mode `value` is: color, gray or black and white; `None` for others (halftone).
fn mode_kind(value: &str) -> Option<ModeKind> {
    let value = value.to_ascii_lowercase();
    // Color, or the British spelling some backends use.
    if value.contains("colo") {
        Some(ModeKind::Color)
    } else if value.contains("gray") || value.contains("grey") {
        Some(ModeKind::Gray)
    } else if value.contains("lineart")
        || value.contains("line art")
        || value.contains("binary")
        || value.contains("black")
        || value.contains("monochrome")
    {
        Some(ModeKind::Lineart)
    } else {
        None
    }
}

/// What the source `value` is.
fn source_kind(value: &str) -> SourceKind {
    let value = value.to_ascii_lowercase();
    let feeder = value.contains("adf") || value.contains("feeder") || value.contains("sheet");
    if feeder && (value.contains("duplex") || value.contains("both") || value.contains("back")) {
        SourceKind::FeederDuplex
    } else if feeder {
        SourceKind::Feeder
    } else if value.contains("flatbed") || value.contains("platen") || value.contains("glass") {
        SourceKind::Flatbed
    } else {
        SourceKind::Other
    }
}

/// The resolutions of a constraint: the listed ones, or the common ones within a range.
fn resolutions(constraint: &str) -> Vec<u32> {
    let mut values: Vec<u32> = if let Some(listed) = list(constraint, "dpi") {
        listed
            .iter()
            .filter_map(|value| value.parse::<f64>().ok())
            .filter(|value| value.is_finite() && *value >= 1.0 && *value <= 100_000.0)
            .filter_map(|value| format!("{value:.0}").parse().ok())
            .collect()
    } else if let Some((low, high)) = range(constraint, "dpi") {
        COMMON_RESOLUTIONS
            .into_iter()
            .filter(|dpi| f64::from(*dpi) >= low && f64::from(*dpi) <= high)
            .collect()
    } else {
        Vec::new()
    };
    values.sort_unstable();
    values.dedup();
    values
}

/// What `scanimage -A` printed about a scanner. Options it doesn't have, or that are inactive,
/// stay empty; a mode that is neither color, gray nor black and white (halftone) is left out.
pub fn parse(output: &str) -> Options {
    let mut options = Options::default();
    for line in output.lines().filter_map(option_line) {
        let inactive = line.value == "inactive";
        match line.name {
            "--source" if !inactive => {
                let values = list(line.constraint, "").unwrap_or_default();
                if values.len() > 1 {
                    options.sources = values
                        .into_iter()
                        .map(|value| Choice {
                            kind: source_kind(&value),
                            value,
                        })
                        .collect();
                }
                options.source = (!line.value.is_empty()).then(|| line.value.to_owned());
            }
            "--mode" if !inactive => {
                let mut modes: Vec<Choice<ModeKind>> = list(line.constraint, "")
                    .unwrap_or_default()
                    .into_iter()
                    .filter_map(|value| {
                        Some(Choice {
                            kind: mode_kind(&value)?,
                            value,
                        })
                    })
                    .collect();
                // The first of each kind, color first.
                modes.sort_by_key(|mode| mode.kind as u8);
                modes.dedup_by_key(|mode| mode.kind);
                options.modes = modes;
                options.mode = (!line.value.is_empty()).then(|| line.value.to_owned());
            }
            "--resolution" if !inactive => {
                options.resolutions = resolutions(line.constraint);
                options.resolution = line
                    .value
                    .trim_end_matches("dpi")
                    .parse::<f64>()
                    .ok()
                    .filter(|value| value.is_finite() && *value >= 1.0 && *value <= 100_000.0)
                    .and_then(|value| format!("{value:.0}").parse().ok());
            }
            "-x" if !inactive => {
                if let Some((_, width)) = range(line.constraint, "mm") {
                    let height = options.area.map_or(0.0, |(_, height)| height);
                    options.area = Some((width, height));
                }
            }
            "-y" if !inactive => {
                if let Some((_, height)) = range(line.constraint, "mm") {
                    let width = options.area.map_or(0.0, |(width, _)| width);
                    options.area = Some((width, height));
                }
            }
            _ => {}
        }
    }
    // An area needs both sides.
    if options
        .area
        .is_some_and(|(width, height)| width <= 0.0 || height <= 0.0)
    {
        options.area = None;
    }
    options
}

/// A paper size a scan can be limited to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Paper {
    /// Its name, which isn't translated (A4, Letter).
    pub name: &'static str,
    /// Its width and height in mm.
    pub size: (f64, f64),
}

/// The paper sizes offered, where the scanner's area holds them.
pub const PAPERS: [Paper; 5] = [
    Paper {
        name: "A4",
        size: (210.0, 297.0),
    },
    Paper {
        name: "Letter",
        size: (215.9, 279.4),
    },
    Paper {
        name: "Legal",
        size: (215.9, 355.6),
    },
    Paper {
        name: "A5",
        size: (148.0, 210.0),
    },
    Paper {
        name: "A6",
        size: (105.0, 148.0),
    },
];

/// The papers of [`PAPERS`] an area of `area` (width and height in mm) holds; a scanner's area
/// is often a little short of a paper's (215.9 × 296.9 mm for A4), so 1.5 mm are forgiven.
pub fn papers_for(area: (f64, f64)) -> Vec<Paper> {
    PAPERS
        .into_iter()
        .filter(|paper| paper.size.0 <= area.0 + 1.5 && paper.size.1 <= area.1 + 1.5)
        .collect()
}

/// The paper to start with in a locale whose `LC_PAPER` (or `LC_ALL`, `LANG`) is `locale`:
/// Letter in the United States and Canada, else A4.
pub fn default_paper(locale: &str) -> &'static str {
    let region = locale
        .split(['.', '@'])
        .next()
        .and_then(|name| name.split_once('_'))
        .map(|(_, region)| region);
    match region {
        Some("US" | "CA" | "MX" | "PH" | "CL" | "CO" | "VE") => "Letter",
        _ => "A4",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What SANE's test backend prints, shortened.
    const TEST_BACKEND: &str = "\
Output format is not set, using pnm as a default.

All options specific to device `test':
  Scan Mode:
    --mode Gray|Color [Gray]
        Selects the scan mode (e.g., lineart, monochrome, or color).
    --depth 1|8|16 [8]
        Number of bits per sample.
    --hand-scanner[=(yes|no)] [no]
        Simulate a hand-scanner.
    --three-pass[=(yes|no)] [inactive]
        Simulate a three-pass scanner.
    --resolution 1..1200dpi (in steps of 1) [0.000762939]
        Sets the resolution of the scanned image.
    --source Flatbed|Automatic Document Feeder [Flatbed]
        If Automatic Document Feeder is selected, the feeder will be 'empty'
        after 10 scans.
  Geometry:
    -l 0..200mm (in steps of 1) [0]
        Top-left x position of scan area.
    -t 0..200mm (in steps of 1) [0]
        Top-left y position of scan area.
    -x 0..200mm (in steps of 1) [80]
        Width of scan-area.
    -y 0..200mm (in steps of 1) [100]
        Height of scan-area.
";

    /// What sane-airscan prints for an eSCL scanner with a feeder.
    const AIRSCAN: &str = "\
All options specific to device `airscan:e0:HP LaserJet MFP M28w (5E1A2B)':
  Standard:
    --resolution 75|100|150|200|300|600dpi [300]
        Sets the resolution of the scanned image.
    --mode Color|Gray|Lineart [Color]
        Selects the scan mode (e.g., lineart, monochrome, or color).
    --source Flatbed|ADF|ADF Duplex [Flatbed]
        Selects the scan source (such as a document-feeder).
  Geometry:
    -l 0..215.9mm [0]
        Top-left x position of scan area.
    -t 0..297.011mm [0]
        Top-left y position of scan area.
    -x 0..215.9mm [215.9]
        Width of scan-area.
    -y 0..297.011mm [297.011]
        Height of scan-area.
  Enhancement:
    --brightness -100..100% (in steps of 1) [0]
        Controls the brightness of the acquired image.
";

    /// A vendor backend with modes of its own names and a fixed resolution list.
    const VENDOR: &str = "\
All options specific to device `epson2:libusb:001:004':
  Scan Mode:
    --mode Binary|Gray|Color [Binary]
        Selects the scan mode.
    --resolution 75|300|600|1200dpi [75]
        Sets the resolution of the scanned image.
    --source [inactive]
  Geometry:
    -x 0..215.9mm [215.9]
    -y 0..297.18mm [297.18]
";

    #[test]
    fn the_test_backend_is_read() {
        let options = parse(TEST_BACKEND);
        assert_eq!(
            options.modes,
            [
                Choice {
                    value: "Color".to_owned(),
                    kind: ModeKind::Color
                },
                Choice {
                    value: "Gray".to_owned(),
                    kind: ModeKind::Gray
                },
            ]
        );
        assert_eq!(options.mode.as_deref(), Some("Gray"));
        assert_eq!(
            options.resolutions,
            [75, 100, 150, 200, 300, 400, 600, 1200]
        );
        // A resolution below 1 dpi is no value.
        assert_eq!(options.resolution, None);
        assert_eq!(options.document_resolution(), Some(300));
        assert_eq!(options.preview_resolution(), Some(75));
        assert_eq!(
            options
                .sources
                .iter()
                .map(|source| (source.value.as_str(), source.kind))
                .collect::<Vec<_>>(),
            [
                ("Flatbed", SourceKind::Flatbed),
                ("Automatic Document Feeder", SourceKind::Feeder)
            ]
        );
        assert_eq!(options.area, Some((200.0, 200.0)));
        assert_eq!(
            papers_for((200.0, 200.0))
                .iter()
                .map(|paper| paper.name)
                .collect::<Vec<_>>(),
            ["A6"]
        );
    }

    #[test]
    fn an_escl_scanner_with_a_feeder_is_read() {
        let options = parse(AIRSCAN);
        assert_eq!(options.resolutions, [75, 100, 150, 200, 300, 600]);
        assert_eq!(options.resolution, Some(300));
        assert_eq!(options.mode_of(ModeKind::Lineart), Some("Lineart"));
        assert_eq!(
            options
                .sources
                .iter()
                .map(|source| source.kind)
                .collect::<Vec<_>>(),
            [
                SourceKind::Flatbed,
                SourceKind::Feeder,
                SourceKind::FeederDuplex
            ]
        );
        assert!(SourceKind::FeederDuplex.feeds() && !SourceKind::Flatbed.feeds());
        assert_eq!(options.area, Some((215.9, 297.011)));
        assert_eq!(
            papers_for(options.area.unwrap_or_default())
                .iter()
                .map(|paper| paper.name)
                .collect::<Vec<_>>(),
            ["A4", "Letter", "A5", "A6"]
        );
    }

    #[test]
    fn vendor_names_and_inactive_options_are_read() {
        let options = parse(VENDOR);
        assert_eq!(options.mode_of(ModeKind::Lineart), Some("Binary"));
        assert_eq!(options.mode_of(ModeKind::Color), Some("Color"));
        assert_eq!(options.resolutions, [75, 300, 600, 1200]);
        assert_eq!(options.document_resolution(), Some(300));
        assert_eq!(options.sources, []);
        assert_eq!(options.source, None);
    }

    #[test]
    fn nothing_to_read_gives_no_options() {
        assert_eq!(parse(""), Options::default());
        assert_eq!(
            parse("scanimage: open of device nosuch:0 failed: Invalid argument\n"),
            Options::default()
        );
        // A broken range and a broken list give nothing, not a panic.
        let broken = "    --resolution ..dpi [x]\n    --mode | [Color]\n    -x 0..mm [0]\n";
        let options = parse(broken);
        assert!(options.resolutions.is_empty() && options.modes.is_empty());
        assert_eq!(options.area, None);
    }

    #[test]
    fn the_paper_follows_the_region() {
        assert_eq!(default_paper("en_US.UTF-8"), "Letter");
        assert_eq!(default_paper("fr_CA"), "Letter");
        assert_eq!(default_paper("de_DE.UTF-8@euro"), "A4");
        assert_eq!(default_paper("C"), "A4");
        assert_eq!(default_paper(""), "A4");
    }
}
