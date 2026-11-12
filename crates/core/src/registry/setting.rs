// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The definition of one setting.

use super::page::Page;
use super::read::{ReadError, ReadFn};
use super::sway::{ConfigFile, Directive, RenderFn, SwayMapping};
use super::value::{Choice, Kind, Value, ValueError};
use crate::command::Color;
use crate::command::input::InputType;
use crate::version::{SwayFxVersion, SwayVersion};

/// A translatable source string (a gettext msgid, in American English). Translate it before
/// showing it; search matches the translation and the English original.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Msg(&'static str);

impl Msg {
    /// Marks `msgid` as translatable.
    pub const fn new(msgid: &'static str) -> Self {
        Self(msgid)
    }

    /// The English source string.
    pub const fn msgid(self) -> &'static str {
        self.0
    }
}

/// What a setting applies to. The scope also decides where a value is stored by default: in the
/// shared `settings.toml`, or in the overlay of this machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Scope {
    /// The whole session. Stored in the shared file by default.
    Global,
    /// The whole session, but the value usually differs between machines. Stored in the host
    /// overlay by default.
    PerHost,
    /// Every input device of a type, or one device. The value for the type is stored in the shared
    /// file by default, a value for one device in the host overlay.
    PerDevice(InputType),
    /// One output. Stored in the host overlay by default.
    PerOutput,
}

/// How a changed value takes effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Apply {
    /// Sent over IPC right away, then saved.
    Live,
    /// Saved, then sway reloads its config. For directives sway only reads from config files.
    Reload,
    /// Saved, then the tool behind the setting reloads its config.
    ToolReload,
    /// Saved; takes effect at the next login.
    Relogin,
    /// Sent over IPC right away, but only saved once the user confirms it (the safety net).
    Risky,
}

impl Apply {
    /// Whether a change is sent to sway over IPC.
    pub const fn is_live(self) -> bool {
        matches!(self, Self::Live | Self::Risky)
    }
}

/// What a setting needs from the system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Requirements {
    /// The oldest sway that understands the setting (SwayFX counts by its base sway version).
    pub min_sway: SwayVersion,
    /// The oldest SwayFX that understands the setting, or `None` if vanilla sway does too.
    pub min_swayfx: Option<SwayFxVersion>,
    /// Programs that must be installed, e.g. `swayidle`.
    pub tools: &'static [&'static str],
}

/// The range a slider covers for a number setting, and the step it moves in. It can be narrower
/// than the values the setting accepts: sway takes a repeat delay of a minute, but a slider over
/// that would be useless.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Slider {
    /// The value at the left end.
    pub min: f64,
    /// The value at the right end.
    pub max: f64,
    /// The step between two positions.
    pub step: f64,
}

/// The definition of one setting. Everything about a setting derives from it: the row in the app,
/// the search entry, the command line key, the checks on stored values, the default, the sway
/// commands and the config lines.
///
/// Start with a constructor for the value type ([`Setting::toggle`], [`Setting::integer`], …) and
/// add the rest with the methods of the [`SettingBuilder`] it returns.
#[derive(Debug, Clone)]
pub struct Setting {
    key: &'static str,
    kind: Kind,
    title: Msg,
    subtitle: Option<Msg>,
    keywords: &'static [&'static str],
    page: Page,
    section: Msg,
    slider: Option<Slider>,
    scope: Scope,
    apply: Apply,
    requirements: Requirements,
    sway: Option<SwayMapping>,
    clears_first: Option<&'static str>,
}

impl Setting {
    fn builder(key: &'static str, kind: Kind, page: Page) -> SettingBuilder {
        SettingBuilder(Self {
            key,
            kind,
            title: Msg::new(""),
            subtitle: None,
            keywords: &[],
            page,
            section: Msg::new(""),
            slider: None,
            scope: Scope::Global,
            apply: Apply::Live,
            requirements: Requirements {
                min_sway: SwayVersion::OLDEST_SUPPORTED,
                min_swayfx: None,
                tools: &[],
            },
            sway: None,
            clears_first: None,
        })
    }

    /// An on/off setting.
    pub fn toggle(key: &'static str, page: Page, default: bool) -> SettingBuilder {
        Self::builder(key, Kind::Bool { default }, page)
    }

    /// A whole number within `min ..= max`.
    pub fn integer(
        key: &'static str,
        page: Page,
        default: i64,
        min: i64,
        max: i64,
    ) -> SettingBuilder {
        Self::builder(key, Kind::Integer { default, min, max }, page)
    }

    /// A decimal number within `min ..= max`.
    pub fn decimal(
        key: &'static str,
        page: Page,
        default: f64,
        min: f64,
        max: f64,
    ) -> SettingBuilder {
        Self::builder(key, Kind::Decimal { default, min, max }, page)
    }

    /// One of `choices`; `default` is the id of the default choice.
    pub fn choice(
        key: &'static str,
        page: Page,
        default: &'static str,
        choices: &'static [Choice],
    ) -> SettingBuilder {
        Self::builder(key, Kind::Choice { default, choices }, page)
    }

    /// A color.
    pub fn color(key: &'static str, page: Page, default: Color) -> SettingBuilder {
        Self::builder(key, Kind::Color { default }, page)
    }

    /// Free text.
    pub fn text(key: &'static str, page: Page, default: &'static str) -> SettingBuilder {
        Self::builder(key, Kind::Text { default }, page)
    }

    /// An ordered list of texts.
    pub fn list(key: &'static str, page: Page, default: &'static [&'static str]) -> SettingBuilder {
        Self::builder(key, Kind::List { default }, page)
    }

    /// The key, e.g. `input.touchpad.tap`: the command line key and the path in `settings.toml`.
    pub fn key(&self) -> &'static str {
        self.key
    }

    /// The value type with its default and limits.
    pub fn kind(&self) -> &Kind {
        &self.kind
    }

    /// The default: what applies when sway is configured with nothing.
    pub fn default_value(&self) -> Value {
        self.kind.default_value()
    }

    /// Checks that `value` fits the setting.
    ///
    /// # Errors
    ///
    /// See [`Kind::check`].
    pub fn check(&self, value: &Value) -> Result<(), ValueError> {
        self.kind.check(value)
    }

    /// The title of the row.
    pub fn title(&self) -> Msg {
        self.title
    }

    /// The explanation under the title.
    pub fn subtitle(&self) -> Option<Msg> {
        self.subtitle
    }

    /// Extra search terms; each is a translatable source string.
    pub fn keywords(&self) -> impl Iterator<Item = Msg> + '_ {
        self.keywords.iter().copied().map(Msg::new)
    }

    /// The page the setting is shown on.
    pub fn page(&self) -> Page {
        self.page
    }

    /// The section of the page the setting is shown in, e.g. "Touchpad". Settings of a section
    /// are next to each other in the registry.
    pub fn section(&self) -> Msg {
        self.section
    }

    /// The slider of a number setting: the range given with [`SettingBuilder::slider`], or else
    /// the setting's whole range, in steps of 1 for whole numbers and of a hundredth of the range
    /// for decimals. `None` for other value types.
    pub fn slider(&self) -> Option<Slider> {
        #[expect(
            clippy::cast_precision_loss,
            reason = "the ranges of whole-number settings are far below 2^52"
        )]
        let whole = |min: i64, max: i64| Slider {
            min: min as f64,
            max: max as f64,
            step: 1.0,
        };
        match self.kind {
            Kind::Integer { min, max, .. } => Some(self.slider.unwrap_or_else(|| whole(min, max))),
            Kind::Decimal { min, max, .. } => Some(self.slider.unwrap_or(Slider {
                min,
                max,
                step: (max - min) / 100.0,
            })),
            _ => None,
        }
    }

    /// What the setting applies to.
    pub fn scope(&self) -> Scope {
        self.scope
    }

    /// How a change takes effect.
    pub fn apply(&self) -> Apply {
        self.apply
    }

    /// What the setting needs from the system.
    pub fn requirements(&self) -> Requirements {
        self.requirements
    }

    /// The setting a risky change sends at its default first when it tries both
    /// ([`SettingBuilder::clears_first`]).
    pub fn clears_first(&self) -> Option<&'static str> {
        self.clears_first
    }

    /// The sway directive behind the setting, or `None` for a preference of Sway Things itself.
    pub fn sway(&self) -> Option<&SwayMapping> {
        self.sway.as_ref()
    }

    /// The value a directive of the user's config sets this setting to, as sway reads it, or
    /// `None` if the directive isn't this setting's (see [`SwayMapping::matches`]).
    ///
    /// # Errors
    ///
    /// [`ReadError::Rejected`] if sway refuses the line, and [`ReadError::Unsupported`] if the
    /// value doesn't fit the setting.
    pub fn read(
        &self,
        directive: &crate::sway_config::Directive,
    ) -> Option<Result<Value, ReadError>> {
        let mapping = self.sway.as_ref()?;
        let args = mapping.matches(directive)?;
        Some((mapping.read)(args).and_then(|value| {
            self.check(&value)
                .map(|()| value)
                .map_err(|error| ReadError::Unsupported(error.to_string()))
        }))
    }
}

/// Builds a [`Setting`]. Created by the constructors of [`Setting`], finished with
/// [`SettingBuilder::build`].
#[derive(Debug, Clone)]
#[must_use]
pub struct SettingBuilder(Setting);

impl SettingBuilder {
    /// Sets the title of the row (translatable, American English).
    pub fn title(mut self, title: &'static str) -> Self {
        self.0.title = Msg::new(title);
        self
    }

    /// Sets the explanation under the title (translatable).
    pub fn subtitle(mut self, subtitle: &'static str) -> Self {
        self.0.subtitle = Some(Msg::new(subtitle));
        self
    }

    /// Sets the section of the page the setting is shown in (translatable), e.g. "Touchpad".
    pub fn section(mut self, section: &'static str) -> Self {
        self.0.section = Msg::new(section);
        self
    }

    /// The range and step of the slider of a number setting, if narrower than the values the
    /// setting accepts (which stay valid, e.g. typed into the row's number field).
    pub fn slider(mut self, min: f64, max: f64, step: f64) -> Self {
        self.0.slider = Some(Slider { min, max, step });
        self
    }

    /// Sets extra search terms and synonyms (translatable), beyond the words of the title.
    pub fn keywords(mut self, keywords: &'static [&'static str]) -> Self {
        self.0.keywords = keywords;
        self
    }

    /// Sets what the setting applies to. The default is [`Scope::Global`].
    pub fn scope(mut self, scope: Scope) -> Self {
        self.0.scope = scope;
        self
    }

    /// Sets how a change takes effect. The default is [`Apply::Live`].
    pub fn apply(mut self, apply: Apply) -> Self {
        self.0.apply = apply;
        self
    }

    /// Records that the setting needs at least this sway.
    pub fn min_sway(mut self, version: SwayVersion) -> Self {
        self.0.requirements.min_sway = version;
        self
    }

    /// Records that the setting only works on SwayFX, at least `version`.
    pub fn swayfx_only(mut self, version: SwayFxVersion) -> Self {
        self.0.requirements.min_swayfx = Some(version);
        self
    }

    /// Records the programs that must be installed for the setting to work.
    pub fn requires_tools(mut self, tools: &'static [&'static str]) -> Self {
        self.0.requirements.tools = tools;
        self
    }

    /// Maps the setting to a sway directive: `directive` names it for conflict detection, `file`
    /// is the generated file its config lines go to, `render` builds the commands, and `read`
    /// reads the setting's value back from a line of the user's config.
    ///
    /// Settings that are values of the same directive (the five colors of `client.focused`) pass
    /// the same `directive`, `file` and `render`, which reads all of them; each has its own
    /// `read`.
    pub fn sway(
        mut self,
        directive: Directive,
        file: ConfigFile,
        render: RenderFn,
        read: ReadFn,
    ) -> Self {
        self.0.sway = Some(SwayMapping {
            directive,
            file,
            render,
            read,
        });
        self
    }

    /// Records a setting whose values only fit some values of this one (the variants of the
    /// layouts): when a risky change tries both, `key` is sent at its default before this
    /// setting's new value and the new value of `key`, so no command in between pairs values that
    /// don't fit together.
    pub fn clears_first(mut self, key: &'static str) -> Self {
        self.0.clears_first = Some(key);
        self
    }

    /// The finished setting.
    pub fn build(self) -> Setting {
        self.0
    }
}
