// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The stylesheet of the Bar page's preview: the CSS waybar gets (ours, then the user's own
//! stylesheet), confined to the preview.
//!
//! The preview builds the widgets waybar builds, with its names and classes (`window#waybar`,
//! `.modules-left > widget > #clock`, `#workspaces button.focused`, a group as `#net`), so the
//! same rules match. Every selector is put under the preview's root class, and `window#waybar`
//! becomes that class itself, so no rule reaches the rest of the app. The colors a stylesheet
//! defines (`@define-color`) get the root's prefix, since GTK looks named colors up across every
//! stylesheet of the display.
//!
//! The user's stylesheet is data: it only ever becomes CSS of the preview. `@import`, animations
//! and every other at-rule are left out, and so is a declaration with a `url()` that isn't a
//! file on this computer: nothing is fetched. A relative `url()` is resolved against the
//! stylesheet's folder, as waybar does. What GTK 4 can't parse (waybar draws with GTK 3) the
//! preview's style provider skips.

use std::collections::HashSet;
use std::fmt::Write as _;
use std::io::Read as _;
use std::path::{Component, Path, PathBuf};

use super::style::{self, css_string};
use crate::state::State;
use crate::sway_config::SwayConfig;

/// The largest stylesheet of the user's the preview reads.
const MAX_EXTRA: u64 = 1024 * 1024;

/// The stylesheet of a preview and how the user's own stylesheet went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewCss {
    /// The CSS, confined to the preview's root class.
    pub css: String,
    /// The user's own stylesheet.
    pub extra: Extra,
}

/// The user's own stylesheet (`bar.style.extra_css`), as the preview found it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Extra {
    /// None is set.
    None,
    /// It was read and is part of the preview.
    Read(PathBuf),
    /// It is set but couldn't be read (missing, too large, not text).
    Unreadable(PathBuf),
}

/// The stylesheet of the preview of the bar `state` sets up, under the root class `root`: the
/// rules Sway Things generates for waybar (`theme.css`), then the user's own stylesheet, as
/// waybar's `style.css` imports them. `sway` is the user's sway config, for the colors it sets;
/// `modules` the definitions of the modules, for the groups.
pub fn stylesheet(
    state: &State,
    sway: Option<&SwayConfig>,
    modules: &super::Modules,
    root: &str,
) -> PreviewCss {
    let mut css = scope(&style::stylesheet(state, sway, modules), root, None);
    let extra = match style::extra_css_active(state) {
        Some(path) => {
            let path = PathBuf::from(path);
            match read_limited(&path) {
                Some(text) => {
                    css.push_str(&scope(&text, root, path.parent()));
                    Extra::Read(path)
                }
                None => Extra::Unreadable(path),
            }
        }
        None => Extra::None,
    };
    PreviewCss { css, extra }
}

/// The text of the file at the absolute `path`, if it is at most [`MAX_EXTRA`] bytes of UTF-8.
fn read_limited(path: &Path) -> Option<String> {
    if !path.is_absolute() {
        return None;
    }
    let file = std::fs::File::open(path).ok()?;
    let mut bytes = Vec::new();
    file.take(MAX_EXTRA + 1).read_to_end(&mut bytes).ok()?;
    if u64::try_from(bytes.len()).ok()? > MAX_EXTRA {
        return None;
    }
    String::from_utf8(bytes).ok()
}

/// The id and the class waybar gives the widget of module `name`: `clock#date` is `#clock.date`,
/// `custom/vpn#1` is `#custom-vpn.1`, `sway/workspaces` is `#workspaces` and `group/net` is
/// `#net`.
pub fn css_name(name: &str) -> (String, Option<String>) {
    let (kind, instance) = match name.split_once('#') {
        Some((kind, instance)) => (kind, Some(instance.to_owned())),
        None => (name, None),
    };
    let id = match kind.strip_prefix("custom/") {
        Some(custom) => format!("custom-{custom}"),
        None => kind.rsplit('/').next().unwrap_or(kind).to_owned(),
    };
    (id, instance.filter(|instance| !instance.is_empty()))
}

/// `css` confined to the widgets under the class `root`. Relative `url()`s are resolved against
/// the folder `base`, and left out without one.
pub fn scope(css: &str, root: &str, base: Option<&Path>) -> String {
    let statements = statements(&strip_comments(css));
    let prefix = format!("{}_", root.replace('-', "_"));
    let colors: HashSet<String> = statements
        .iter()
        .filter_map(|statement| match statement {
            Statement::At { name, prelude, .. } if name.eq_ignore_ascii_case("define-color") => {
                let name = ident(prelude.trim_start());
                (!name.is_empty()).then(|| name.to_owned())
            }
            _ => None,
        })
        .collect();
    let rewrite = Rewrite {
        colors: &colors,
        prefix: &prefix,
        base,
    };
    let mut out = String::new();
    for statement in statements {
        match statement {
            Statement::At {
                name,
                prelude,
                block: None,
            } if name.eq_ignore_ascii_case("define-color") => {
                let prelude = prelude.trim_start();
                let color = ident(prelude);
                let definition = rewrite.value(&prelude[color.len()..]);
                if let (false, Some(definition)) = (color.is_empty(), definition) {
                    let _ = writeln!(out, "@define-color {prefix}{color} {};", definition.trim());
                }
            }
            // `@import` (nothing is fetched or read), `@keyframes` and the rest.
            Statement::At { .. } => {}
            Statement::Rule { prelude, block } => {
                let selectors: Vec<String> = split_top(&prelude, ',')
                    .iter()
                    .filter_map(|selector| scope_selector(selector, root))
                    .collect();
                if selectors.is_empty() || has_top(&block, '{') {
                    continue;
                }
                let declarations: Vec<String> = split_top(&block, ';')
                    .iter()
                    .filter_map(|declaration| rewrite.declaration(declaration))
                    .collect();
                if declarations.is_empty() {
                    continue;
                }
                out.push_str(&selectors.join(",\n"));
                out.push_str(" {\n");
                for declaration in declarations {
                    out.push_str("  ");
                    out.push_str(&declaration);
                    out.push_str(";\n");
                }
                out.push_str("}\n");
            }
        }
    }
    out
}

/// A statement at the top of a stylesheet.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Statement {
    /// `@name prelude;` or `@name prelude { block }`.
    At {
        name: String,
        prelude: String,
        block: Option<String>,
    },
    /// `selectors { declarations }`.
    Rule { prelude: String, block: String },
}

/// `css` without its comments; strings are kept as they are.
fn strip_comments(css: &str) -> String {
    let chars: Vec<char> = css.chars().collect();
    let mut out = String::with_capacity(css.len());
    let mut at = 0;
    while let Some(&char) = chars.get(at) {
        if char == '"' || char == '\'' {
            let end = string_end(&chars, at);
            out.extend(&chars[at..end]);
            at = end;
        } else if char == '/' && chars.get(at + 1) == Some(&'*') {
            at += 2;
            while at < chars.len() && !(chars[at] == '*' && chars.get(at + 1) == Some(&'/')) {
                at += 1;
            }
            at = (at + 2).min(chars.len());
            out.push(' ');
        } else {
            out.push(char);
            at += 1;
        }
    }
    out
}

/// The index after the string that starts at `start` (its quote), or the end of `chars`.
fn string_end(chars: &[char], start: usize) -> usize {
    let quote = chars[start];
    let mut at = start + 1;
    while let Some(&char) = chars.get(at) {
        at += 1;
        if char == '\\' {
            at += 1;
        } else if char == quote || char == '\n' {
            break;
        }
    }
    at.min(chars.len())
}

/// Scans `chars` from `at` to the first of `stops` outside strings, parentheses and braces;
/// the index of that character, or `None` at the end.
fn scan(chars: &[char], mut at: usize, stops: &[char]) -> Option<usize> {
    let mut depth = 0_usize;
    while let Some(&char) = chars.get(at) {
        match char {
            '"' | '\'' => {
                at = string_end(chars, at);
                continue;
            }
            _ if depth == 0 && stops.contains(&char) => return Some(at),
            '(' | '[' => depth += 1,
            ')' | ']' => depth = depth.saturating_sub(1),
            _ => {}
        }
        at += 1;
    }
    None
}

/// The index of the `}` that closes the block opened just before `at`, or `None` at the end.
fn block_end(chars: &[char], mut at: usize) -> Option<usize> {
    let mut depth = 1_usize;
    while let Some(&char) = chars.get(at) {
        match char {
            '"' | '\'' => {
                at = string_end(chars, at);
                continue;
            }
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(at);
                }
            }
            _ => {}
        }
        at += 1;
    }
    None
}

/// The statements of `css` (without comments). Garbage up to the next `;` or `}` is skipped,
/// and a block that isn't closed is left out.
fn statements(css: &str) -> Vec<Statement> {
    let chars: Vec<char> = css.chars().collect();
    let text = |from: usize, to: usize| chars[from..to].iter().collect::<String>();
    let mut out = Vec::new();
    let mut at = 0;
    while at < chars.len() {
        if chars[at].is_whitespace() {
            at += 1;
            continue;
        }
        let at_rule = chars[at] == '@';
        let start = if at_rule { at + 1 } else { at };
        let Some(stop) = scan(&chars, start, &[';', '{', '}']) else {
            break;
        };
        match chars[stop] {
            '{' => {
                let Some(end) = block_end(&chars, stop + 1) else {
                    break;
                };
                let block = text(stop + 1, end);
                out.push(if at_rule {
                    let head = text(start, stop);
                    let name = ident(&head).to_owned();
                    Statement::At {
                        prelude: head[name.len()..].to_owned(),
                        name,
                        block: Some(block),
                    }
                } else {
                    Statement::Rule {
                        prelude: text(start, stop),
                        block,
                    }
                });
                at = end + 1;
            }
            ';' if at_rule => {
                let head = text(start, stop);
                let name = ident(&head).to_owned();
                out.push(Statement::At {
                    prelude: head[name.len()..].to_owned(),
                    name,
                    block: None,
                });
                at = stop + 1;
            }
            // A stray `;` or `}`: skipped with what came before it.
            _ => at = stop + 1,
        }
    }
    out
}

/// The CSS identifier at the start of `text` (letters, digits, `-` and `_`).
fn ident(text: &str) -> &str {
    let end = text
        .find(|char: char| !(char.is_alphanumeric() || char == '-' || char == '_'))
        .unwrap_or(text.len());
    &text[..end]
}

/// The parts of `text` between the `separator`s outside strings and parentheses.
fn split_top(text: &str, separator: char) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut parts = Vec::new();
    let mut at = 0;
    while let Some(stop) = scan(&chars, at, &[separator]) {
        parts.push(chars[at..stop].iter().collect());
        at = stop + 1;
    }
    parts.push(chars[at..].iter().collect());
    parts
}

/// Whether `text` has `char` outside strings and parentheses.
fn has_top(text: &str, char: char) -> bool {
    let chars: Vec<char> = text.chars().collect();
    scan(&chars, 0, &[char]).is_some()
}

/// `selector` under the class `root`: `window#waybar` (or `window`, `#waybar`) at its start
/// becomes the root itself, `*` the root and everything in it, and anything else something in
/// the root. `None` for an empty selector or one that starts with a combinator.
fn scope_selector(selector: &str, root: &str) -> Option<String> {
    let selector = selector.split_whitespace().collect::<Vec<_>>().join(" ");
    if selector.is_empty() || selector.starts_with(['>', '+', '~']) {
        return None;
    }
    if selector == "*" {
        return Some(format!(".{root}, .{root} *"));
    }
    let head = after_word(&selector, "window")
        .map(|rest| after_word(rest, "#waybar").unwrap_or(rest))
        .or_else(|| after_word(&selector, "#waybar"));
    Some(match head {
        Some(rest) => format!(".{root}{rest}"),
        None => format!(".{root} {selector}"),
    })
}

/// What follows `word` at the start of `text`, if `word` is a whole word there.
fn after_word<'a>(text: &'a str, word: &str) -> Option<&'a str> {
    let rest = text.strip_prefix(word)?;
    (!rest.starts_with(|char: char| char.is_alphanumeric() || char == '-' || char == '_'))
        .then_some(rest)
}

/// How the values of a stylesheet are rewritten.
struct Rewrite<'a> {
    /// The colors the stylesheet defines.
    colors: &'a HashSet<String>,
    /// What their names get in front.
    prefix: &'a str,
    /// The folder of the stylesheet, for relative URLs.
    base: Option<&'a Path>,
}

impl Rewrite<'_> {
    /// `declaration` for the preview: `None` for an empty one, an animation, or one whose value
    /// can't be kept.
    fn declaration(&self, declaration: &str) -> Option<String> {
        let (property, value) = declaration.split_once(':')?;
        let property = property.trim();
        if property.is_empty()
            || !property
                .chars()
                .all(|char| char.is_alphanumeric() || char == '-' || char == '_')
            || property.to_ascii_lowercase().starts_with("animation")
        {
            return None;
        }
        let value = self.value(value)?;
        let value = value.trim();
        (!value.is_empty()).then(|| format!("{property}: {value}"))
    }

    /// `value` with the names of the stylesheet's colors prefixed and each `url()` made
    /// absolute; `None` if it has a `url()` that isn't a file here.
    fn value(&self, value: &str) -> Option<String> {
        let chars: Vec<char> = value.chars().collect();
        let mut out = String::with_capacity(value.len());
        let mut at = 0;
        while let Some(&char) = chars.get(at) {
            if char == '"' || char == '\'' {
                let end = string_end(&chars, at);
                out.extend(&chars[at..end]);
                at = end;
            } else if char == '@' {
                let rest: String = chars[at + 1..].iter().collect();
                let name = ident(&rest);
                if self.colors.contains(name) {
                    out.push('@');
                    out.push_str(self.prefix);
                } else {
                    out.push('@');
                }
                out.push_str(name);
                at += 1 + name.chars().count();
            } else if chars[at..]
                .iter()
                .take(4)
                .collect::<String>()
                .eq_ignore_ascii_case("url(")
            {
                let close = scan(&chars, at + 4, &[')'])?;
                let inner: String = chars[at + 4..close].iter().collect();
                let url = unquote(inner.trim());
                out.push_str("url(");
                out.push_str(&css_string(&self.url(&url)?));
                out.push(')');
                at = close + 1;
            } else {
                out.push(char);
                at += 1;
            }
        }
        Some(out)
    }

    /// The URL GTK gets for `url`: a file here as a `file://` URL (a relative one in the
    /// stylesheet's folder), a `data:` or `resource:` URL as it is; `None` for anything else,
    /// which would be fetched.
    fn url(&self, url: &str) -> Option<String> {
        if url.chars().any(char::is_control) || url.is_empty() {
            return None;
        }
        let scheme = url
            .split_once(':')
            .map(|(scheme, _)| scheme)
            .filter(|scheme| {
                !scheme.is_empty()
                    && scheme
                        .chars()
                        .all(|char| char.is_ascii_alphanumeric() || "+-.".contains(char))
            });
        match scheme.map(str::to_ascii_lowercase).as_deref() {
            Some("file") => {
                let path = url.strip_prefix("file://")?;
                path.starts_with('/').then(|| url.to_owned())
            }
            Some("data" | "resource") => Some(url.to_owned()),
            Some(_) => None,
            None if url.starts_with("//") => None,
            None => {
                let path = if url.starts_with('/') {
                    PathBuf::from(url)
                } else {
                    self.base?.join(url)
                };
                style::file_url(&normalized(&path).to_string_lossy())
            }
        }
    }
}

/// `path` with `.` and `..` resolved by its words, without asking the file system.
fn normalized(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}

/// `text` without the quotes around it, with escapes resolved.
fn unquote(text: &str) -> String {
    let quoted = (text.starts_with('"') && text.ends_with('"'))
        || (text.starts_with('\'') && text.ends_with('\''));
    if !quoted || text.len() < 2 {
        return text.to_owned();
    }
    let mut out = String::new();
    let mut chars = text[1..text.len() - 1].chars();
    while let Some(char) = chars.next() {
        if char == '\\' {
            if let Some(next) = chars.next() {
                out.push(next);
            }
        } else {
            out.push(char);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::{Value, bar as keys};

    #[test]
    fn waybar_names_become_ids_and_classes() {
        assert_eq!(css_name("clock"), ("clock".to_owned(), None));
        assert_eq!(
            css_name("clock#date"),
            ("clock".to_owned(), Some("date".to_owned()))
        );
        assert_eq!(
            css_name("custom/vpn#1"),
            ("custom-vpn".to_owned(), Some("1".to_owned()))
        );
        assert_eq!(css_name("sway/workspaces"), ("workspaces".to_owned(), None));
        assert_eq!(css_name("group/net"), ("net".to_owned(), None));
        assert_eq!(
            css_name("image#expand"),
            ("image".to_owned(), Some("expand".to_owned()))
        );
    }

    #[test]
    fn selectors_stay_inside_the_preview() {
        let css = "window#waybar { background: transparent; }\n\
                   window#waybar > box, .modules-left { color: red; }\n\
                   window#waybar.empty #window { border-color: transparent; }\n\
                   * { font-size: 12px; }\n\
                   #workspaces button.focused, #clock:hover { color: blue; }\n\
                   tooltip label { color: white; }\n\
                   window { color: green; }\n\
                   #waybar.top { margin: 1px; }\n\
                   windowed { color: black; }\n";
        let scoped = scope(css, "st-wb-1", None);
        assert_eq!(
            scoped,
            ".st-wb-1 {\n  background: transparent;\n}\n\
             .st-wb-1 > box,\n.st-wb-1 .modules-left {\n  color: red;\n}\n\
             .st-wb-1.empty #window {\n  border-color: transparent;\n}\n\
             .st-wb-1, .st-wb-1 * {\n  font-size: 12px;\n}\n\
             .st-wb-1 #workspaces button.focused,\n.st-wb-1 #clock:hover {\n  color: blue;\n}\n\
             .st-wb-1 tooltip label {\n  color: white;\n}\n\
             .st-wb-1 {\n  color: green;\n}\n\
             .st-wb-1.top {\n  margin: 1px;\n}\n\
             .st-wb-1 windowed {\n  color: black;\n}\n"
        );
        // Every selector starts with the root.
        for line in scoped.lines().filter(|line| line.ends_with('{')) {
            for selector in line.trim_end_matches('{').split(',') {
                assert!(selector.trim().starts_with(".st-wb-1"), "{selector}");
            }
        }
    }

    #[test]
    fn colors_get_the_preview_s_prefix() {
        let css = "@define-color pill rgba(18, 18, 22, 0.45);\n\
                   @define-color accent_dk shade(@pill, 0.8);\n\
                   #clock { background: @pill; color: alpha(@accent_dk, 0.5); \
                   border-color: @theme_fg_color; }";
        assert_eq!(
            scope(css, "st-wb-2", None),
            "@define-color st_wb_2_pill rgba(18, 18, 22, 0.45);\n\
             @define-color st_wb_2_accent_dk shade(@st_wb_2_pill, 0.8);\n\
             .st-wb-2 #clock {\n  background: @st_wb_2_pill;\n  \
             color: alpha(@st_wb_2_accent_dk, 0.5);\n  border-color: @theme_fg_color;\n}\n"
        );
    }

    #[test]
    fn nothing_is_fetched_or_imported() {
        let css = "@import url(\"https://example.com/x.css\");\n\
                   @import \"local.css\";\n\
                   @keyframes pulse { from { color: red; } to { color: blue; } }\n\
                   @media (min-width: 1px) { #clock { color: red; } }\n\
                   #a { background-image: url(\"https://example.com/a.png\"); color: red; }\n\
                   #b { background-image: url(//example.com/b.png); }\n\
                   #c { background-image: url('icons/claude.svg'); }\n\
                   #d { background-image: url(../shared/d.svg); animation: pulse 1s; }\n\
                   #e { background-image: url(\"file:///usr/share/e.png\"); }\n\
                   #f { background-image: url(ftp://example.com/f.png); }";
        let scoped = scope(css, "r", Some(Path::new("/home/me/.config/waybar")));
        assert!(!scoped.contains("example.com"), "{scoped}");
        assert!(!scoped.contains("import"), "{scoped}");
        assert!(!scoped.contains("keyframes"), "{scoped}");
        assert!(!scoped.contains("animation"), "{scoped}");
        assert!(!scoped.contains("@media"), "{scoped}");
        assert!(scoped.contains(".r #a {\n  color: red;\n}"), "{scoped}");
        assert!(!scoped.contains(".r #b"), "{scoped}");
        assert!(!scoped.contains(".r #f"), "{scoped}");
        assert!(
            scoped.contains("url(\"file:///home/me/.config/waybar/icons/claude.svg\")"),
            "{scoped}"
        );
        assert!(
            scoped.contains("url(\"file:///home/me/.config/shared/d.svg\")"),
            "{scoped}"
        );
        assert!(
            scoped.contains("url(\"file:///usr/share/e.png\")"),
            "{scoped}"
        );
        // Without a folder, a relative URL can't be resolved.
        assert!(!scope("#c { background-image: url(x.svg); }", "r", None).contains("url"));
    }

    #[test]
    fn broken_css_doesn_t_break_out() {
        // Comments, strings with braces, a rule that isn't closed, stray braces.
        let css = "/* } window { */ #a { font-family: \"a}b\"; }\n\
                   } #b { color: red; }\n\
                   #c { color: red; } #d { color: blue;";
        let scoped = scope(css, "r", None);
        assert_eq!(
            scoped,
            ".r #a {\n  font-family: \"a}b\";\n}\n.r #b {\n  color: red;\n}\n\
             .r #c {\n  color: red;\n}\n"
        );
        // A selector can't end the rule list early, nor reach outside with a combinator.
        assert_eq!(
            scope("> label, #x { color: red; }", "r", None),
            ".r #x {\n  color: red;\n}\n"
        );
        // Nested blocks are left out as a whole.
        assert_eq!(
            scope("#x { color: red; #y { color: blue; } }", "r", None),
            ""
        );
        assert_eq!(scope("", "r", None), "");
        assert_eq!(scope("{}{{}}}};;;@", "r", None), "");
    }

    #[test]
    fn the_preview_reads_ours_and_the_user_s_stylesheet() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = State::load(
            crate::registry::Registry::builtin(),
            crate::state::Paths::new(
                &dir.path().join("config"),
                &dir.path().join("state"),
                "host",
            )
            .unwrap(),
        );
        let set = |state: &mut State, key: &str, value: Value| {
            state.set(key, None, value, None).unwrap();
        };
        set(&mut state, keys::THEME, Value::Choice("plain".to_owned()));
        assert_eq!(
            stylesheet(&state, None, &crate::bar::Modules::default(), "r"),
            PreviewCss {
                css: String::new(),
                extra: Extra::None
            }
        );
        let extra = dir.path().join("style.css");
        set(
            &mut state,
            keys::EXTRA_CSS,
            Value::Text(extra.to_string_lossy().into_owned()),
        );
        assert_eq!(
            stylesheet(&state, None, &crate::bar::Modules::default(), "r").extra,
            Extra::Unreadable(extra.clone())
        );
        std::fs::write(&extra, "#clock { color: red; background: url(i.svg); }").unwrap();
        let read = stylesheet(&state, None, &crate::bar::Modules::default(), "r");
        assert_eq!(read.extra, Extra::Read(extra.clone()));
        let icon = style::file_url(&dir.path().join("i.svg").to_string_lossy()).unwrap();
        assert_eq!(
            read.css,
            format!(".r #clock {{\n  color: red;\n  background: url(\"{icon}\");\n}}\n")
        );
        // A generated look comes first, confined like the rest.
        set(&mut state, keys::THEME, Value::Choice("custom".to_owned()));
        let css = stylesheet(&state, None, &crate::bar::Modules::default(), "r").css;
        assert!(css.starts_with(".r, .r * {"), "{css}");
        assert!(css.ends_with(&read.css), "{css}");
        // Too large to be a stylesheet.
        std::fs::write(&extra, vec![b' '; 2 * 1024 * 1024]).unwrap();
        assert_eq!(
            stylesheet(&state, None, &crate::bar::Modules::default(), "r").extra,
            Extra::Unreadable(extra)
        );
    }
}
