// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reading one rules registry file (`xkb.dtd` of xkeyboard-config).

use roxmltree::{Document, Node, ParsingOptions};

use super::{Builder, Item, MAX_FILE_SIZE, RulesErrorKind, XkbRules};

/// The most XML nodes read from one file; a rules registry has about 20,000.
const MAX_NODES: u32 = 1_000_000;

/// The namespace of `xml:lang`, which older registries put on translated descriptions.
const XML_NAMESPACE: &str = "http://www.w3.org/XML/1998/namespace";

/// Reads a rules registry file such as `evdev.xml`. With `extra`, every entry is marked as
/// [extra](Item::extra), as for `evdev.extras.xml`.
///
/// Unknown elements are skipped, and so are entries without a name. A name that turns up
/// twice in a list keeps its first entry; the variants or options of the second are added to
/// the first.
///
/// # Errors
///
/// A file larger than [`MAX_FILE_SIZE`], one that isn't UTF-8, declares entities, isn't
/// well-formed XML or isn't a rules registry.
pub fn parse(content: &[u8], extra: bool) -> Result<XkbRules, RulesErrorKind> {
    if content.len() > MAX_FILE_SIZE {
        return Err(RulesErrorKind::TooLarge);
    }
    let text = std::str::from_utf8(content).map_err(|_| RulesErrorKind::NotUtf8)?;
    // The registry names its DTD (`<!DOCTYPE xkbConfigRegistry SYSTEM "xkb.dtd">`), so the
    // parser has to accept a doctype. It never loads the external DTD; refusing entity
    // declarations also rules out entities that expand to more text than the file has.
    if text.contains("<!ENTITY") {
        return Err(RulesErrorKind::EntityDeclaration);
    }
    let options = ParsingOptions {
        allow_dtd: true,
        nodes_limit: MAX_NODES,
        entity_resolver: None,
    };
    let document = Document::parse_with_options(text, options)
        .map_err(|error| RulesErrorKind::Xml(error.to_string()))?;
    let root = document.root_element();
    if root.tag_name().name() != "xkbConfigRegistry" {
        return Err(RulesErrorKind::NotRegistry);
    }

    let mut builder = Builder::default();
    for list in root.children() {
        match list.tag_name().name() {
            "modelList" => {
                for model in children(list, "model") {
                    if let Some(item) = entry(model, extra) {
                        builder.model(item);
                    }
                }
            }
            "layoutList" => {
                for layout in children(list, "layout") {
                    let Some(item) = entry(layout, extra) else {
                        continue;
                    };
                    let index = builder.layout(item);
                    for variants in children(layout, "variantList") {
                        for variant in children(variants, "variant") {
                            if let Some(item) = entry(variant, extra) {
                                builder.variant(index, item);
                            }
                        }
                    }
                }
            }
            "optionList" => {
                for group in children(list, "group") {
                    let Some(item) = entry(group, extra) else {
                        continue;
                    };
                    let multiple = group.attribute("allowMultipleSelection") == Some("true");
                    let index = builder.group(item, multiple);
                    for option in children(group, "option") {
                        if let Some(item) = entry(option, extra) {
                            builder.option(index, item);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    Ok(builder.rules)
}

/// The child elements of `node` called `name`.
fn children<'a, 'input>(
    node: Node<'a, 'input>,
    name: &'static str,
) -> impl Iterator<Item = Node<'a, 'input>> {
    node.children()
        .filter(move |child| child.is_element() && child.tag_name().name() == name)
}

/// The text of `node`, trimmed; none if it is empty.
fn text(node: Node<'_, '_>) -> Option<String> {
    let text = node.text()?.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

/// The text of the first child element of `node` called `name`, without an `xml:lang`.
fn field(node: Node<'_, '_>, name: &'static str) -> Option<String> {
    children(node, name)
        .find(|child| child.attribute((XML_NAMESPACE, "lang")).is_none())
        .and_then(text)
}

/// The texts of the elements `entry` in the lists `list` of `node`.
fn codes(node: Node<'_, '_>, list: &'static str, entry: &'static str) -> Vec<String> {
    children(node, list)
        .flat_map(|list| children(list, entry))
        .filter_map(text)
        .collect()
}

/// The entry described by the `configItem` of `node`; none if it has no name.
fn entry(node: Node<'_, '_>, extra: bool) -> Option<Item> {
    let config = children(node, "configItem").next()?;
    Some(Item {
        name: field(config, "name")?,
        description: field(config, "description"),
        short_description: field(config, "shortDescription"),
        vendor: field(config, "vendor"),
        languages: codes(config, "languageList", "iso639Id"),
        countries: codes(config, "countryList", "iso3166Id"),
        extra: extra || config.attribute("popularity") == Some("exotic"),
    })
}
