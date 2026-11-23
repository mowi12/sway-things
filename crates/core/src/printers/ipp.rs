// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The binary encoding of IPP messages (RFC 8010, section 3): what a request to CUPS is made of
//! and what its answer is read from.
//!
//! A [`Message`] is a version, an operation or status code, a request id, groups of
//! [`Attribute`]s, and the document data that follows them. [`Message::decode`] reads what CUPS
//! sends, which includes strings that printers and other users chose: it never panics, limits how
//! deep collections nest, and keeps strings that aren't UTF-8 as lossy text. Nothing decoded is
//! trusted; the callers clean the strings they show.
//!
//! ```
//! use sway_things_core::printers::ipp::{Message, Value, tag};
//!
//! let request = Message::request(0x4002, 1).with(tag::OPERATION, "requested-attributes",
//!     vec![Value::Keyword("printer-name".to_owned())]);
//! let decoded = Message::decode(&request.encode())?;
//! assert_eq!(decoded, request);
//! # Ok::<(), sway_things_core::printers::ipp::DecodeError>(())
//! ```

use std::fmt;

/// The tags of IPP: the delimiters that start a group and the tags of the values.
pub mod tag {
    /// The operation attributes.
    pub const OPERATION: u8 = 0x01;
    /// The attributes of a job.
    pub const JOB: u8 = 0x02;
    /// The end of the attributes; the document data follows.
    pub const END: u8 = 0x03;
    /// The attributes of a printer.
    pub const PRINTER: u8 = 0x04;
    /// Attributes the receiver didn't support.
    pub const UNSUPPORTED_GROUP: u8 = 0x05;
    /// The attributes of a subscription.
    pub const SUBSCRIPTION: u8 = 0x06;
    /// The attributes of an event notification.
    pub const EVENT_NOTIFICATION: u8 = 0x07;

    /// Out of band: the value isn't supported.
    pub const UNSUPPORTED: u8 = 0x10;
    /// Out of band: the value is unknown.
    pub const UNKNOWN: u8 = 0x12;
    /// Out of band: there is no value.
    pub const NO_VALUE: u8 = 0x13;
    /// A signed 32-bit integer.
    pub const INTEGER: u8 = 0x21;
    /// A boolean.
    pub const BOOLEAN: u8 = 0x22;
    /// An enum: a signed 32-bit integer with a meaning.
    pub const ENUM: u8 = 0x23;
    /// Bytes.
    pub const OCTET_STRING: u8 = 0x30;
    /// A date and time (RFC 2579, 11 bytes).
    pub const DATE_TIME: u8 = 0x31;
    /// A resolution: two integers and a unit.
    pub const RESOLUTION: u8 = 0x32;
    /// A range of integers.
    pub const RANGE: u8 = 0x33;
    /// The start of a collection.
    pub const BEGIN_COLLECTION: u8 = 0x34;
    /// Text with its language.
    pub const TEXT_WITH_LANGUAGE: u8 = 0x35;
    /// A name with its language.
    pub const NAME_WITH_LANGUAGE: u8 = 0x36;
    /// The end of a collection.
    pub const END_COLLECTION: u8 = 0x37;
    /// Text.
    pub const TEXT: u8 = 0x41;
    /// A name.
    pub const NAME: u8 = 0x42;
    /// A keyword.
    pub const KEYWORD: u8 = 0x44;
    /// A URI.
    pub const URI: u8 = 0x45;
    /// A URI scheme.
    pub const URI_SCHEME: u8 = 0x46;
    /// A character set.
    pub const CHARSET: u8 = 0x47;
    /// A natural language.
    pub const LANGUAGE: u8 = 0x48;
    /// A MIME media type.
    pub const MIME_TYPE: u8 = 0x49;
    /// The name of a member of a collection.
    pub const MEMBER_NAME: u8 = 0x4A;
}

/// How deep collections may nest: deeper ones are refused, so a message can't exhaust the stack.
const MAX_DEPTH: usize = 16;
/// How many values a message may have, counting the members of collections: far more than any
/// answer of CUPS, so a hostile one can't make the decoder build millions of tiny values.
const MAX_VALUES: usize = 100_000;

/// A value of an attribute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    /// An integer.
    Integer(i32),
    /// A boolean.
    Boolean(bool),
    /// An enum value.
    Enum(i32),
    /// Text, with or without a language.
    Text(String),
    /// A name, with or without a language.
    Name(String),
    /// A keyword.
    Keyword(String),
    /// A URI.
    Uri(String),
    /// A URI scheme.
    UriScheme(String),
    /// A character set.
    Charset(String),
    /// A natural language.
    Language(String),
    /// A MIME media type.
    MimeType(String),
    /// Bytes.
    OctetString(Vec<u8>),
    /// A date and time, as its 11 bytes.
    DateTime([u8; 11]),
    /// A resolution: across, down, and the unit (3: dots per inch, 4: per centimeter).
    Resolution(i32, i32, i8),
    /// A range of integers, both ends included.
    Range(i32, i32),
    /// A collection: its members.
    Collection(Vec<Attribute>),
    /// An out-of-band value: [`tag::UNSUPPORTED`], [`tag::UNKNOWN`], [`tag::NO_VALUE`] or another
    /// one of the tags below 0x20.
    OutOfBand(u8),
    /// A value of a tag this module doesn't know, kept as its bytes.
    Other(u8, Vec<u8>),
}

impl Value {
    /// The string of a text, name, keyword, URI or other string value.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Text(s)
            | Self::Name(s)
            | Self::Keyword(s)
            | Self::Uri(s)
            | Self::UriScheme(s)
            | Self::Charset(s)
            | Self::Language(s)
            | Self::MimeType(s) => Some(s),
            _ => None,
        }
    }

    /// The number of an integer or enum value.
    pub fn as_int(&self) -> Option<i32> {
        match self {
            Self::Integer(n) | Self::Enum(n) => Some(*n),
            _ => None,
        }
    }

    /// The boolean of a boolean value.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Boolean(b) => Some(*b),
            _ => None,
        }
    }

    /// The tag the value is encoded with.
    fn tag(&self) -> u8 {
        match self {
            Self::Integer(_) => tag::INTEGER,
            Self::Boolean(_) => tag::BOOLEAN,
            Self::Enum(_) => tag::ENUM,
            Self::Text(_) => tag::TEXT,
            Self::Name(_) => tag::NAME,
            Self::Keyword(_) => tag::KEYWORD,
            Self::Uri(_) => tag::URI,
            Self::UriScheme(_) => tag::URI_SCHEME,
            Self::Charset(_) => tag::CHARSET,
            Self::Language(_) => tag::LANGUAGE,
            Self::MimeType(_) => tag::MIME_TYPE,
            Self::OctetString(_) => tag::OCTET_STRING,
            Self::DateTime(_) => tag::DATE_TIME,
            Self::Resolution(..) => tag::RESOLUTION,
            Self::Range(..) => tag::RANGE,
            Self::Collection(_) => tag::BEGIN_COLLECTION,
            Self::OutOfBand(tag) | Self::Other(tag, _) => *tag,
        }
    }
}

/// An attribute: a name and one or more values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attribute {
    /// Its name.
    pub name: String,
    /// Its values; IPP has no attribute without one.
    pub values: Vec<Value>,
}

impl Attribute {
    /// The first value, as a string.
    pub fn str(&self) -> Option<&str> {
        self.values.first().and_then(Value::as_str)
    }

    /// The first value, as an integer.
    pub fn int(&self) -> Option<i32> {
        self.values.first().and_then(Value::as_int)
    }

    /// The first value, as a boolean.
    pub fn bool(&self) -> Option<bool> {
        self.values.first().and_then(Value::as_bool)
    }

    /// Every value that is a string.
    pub fn strs(&self) -> impl Iterator<Item = &str> {
        self.values.iter().filter_map(Value::as_str)
    }

    /// Every value that is a number.
    pub fn ints(&self) -> impl Iterator<Item = i32> + '_ {
        self.values.iter().filter_map(Value::as_int)
    }
}

/// A group of attributes: the operation's, a printer's, a job's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    /// The delimiter tag that starts it: [`tag::OPERATION`], [`tag::PRINTER`], [`tag::JOB`], …
    pub tag: u8,
    /// Its attributes, in order.
    pub attributes: Vec<Attribute>,
}

impl Group {
    /// The attribute `name`, if the group has it.
    pub fn get(&self, name: &str) -> Option<&Attribute> {
        self.attributes.iter().find(|attr| attr.name == name)
    }
}

/// An IPP request or response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    /// The IPP version: major and minor.
    pub version: (u8, u8),
    /// The operation of a request, the status of a response.
    pub code: u16,
    /// The request id, which the response repeats.
    pub request_id: i32,
    /// The groups of attributes, in order.
    pub groups: Vec<Group>,
    /// The document data after the attributes.
    pub data: Vec<u8>,
}

/// Why bytes are no IPP message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    /// The message ends in the middle of something.
    Truncated,
    /// A value comes before the first group.
    NoGroup,
    /// A value without a name comes before any attribute.
    NoAttribute,
    /// A value has the wrong length for its tag.
    BadValue(u8),
    /// A collection is malformed or nests deeper than allowed.
    BadCollection,
    /// The message has more values than allowed.
    TooManyValues,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated => f.write_str("the message is cut off"),
            Self::NoGroup => f.write_str("an attribute is outside of any group"),
            Self::NoAttribute => f.write_str("a value belongs to no attribute"),
            Self::BadValue(tag) => write!(f, "a value with the tag {tag:#04x} is malformed"),
            Self::BadCollection => f.write_str("a collection is malformed"),
            Self::TooManyValues => f.write_str("the message has too many values"),
        }
    }
}

impl std::error::Error for DecodeError {}

impl Message {
    /// A request for the operation `operation` with the id `request_id`, in IPP 2.0, with the
    /// operation group and its first two attributes: `attributes-charset` (UTF-8) and
    /// `attributes-natural-language` (English).
    pub fn request(operation: u16, request_id: i32) -> Self {
        Self {
            version: (2, 0),
            code: operation,
            request_id,
            groups: vec![Group {
                tag: tag::OPERATION,
                attributes: vec![
                    Attribute {
                        name: "attributes-charset".to_owned(),
                        values: vec![Value::Charset("utf-8".to_owned())],
                    },
                    Attribute {
                        name: "attributes-natural-language".to_owned(),
                        values: vec![Value::Language("en".to_owned())],
                    },
                ],
            }],
            data: Vec::new(),
        }
    }

    /// The message with the attribute `name` with `values` added to the last group with the tag
    /// `group`, or to a new group with it at the end.
    #[must_use]
    pub fn with(mut self, group: u8, name: &str, values: Vec<Value>) -> Self {
        self.add(group, name, values);
        self
    }

    /// Adds the attribute `name` with `values`, as [`with`](Self::with).
    pub fn add(&mut self, group: u8, name: &str, values: Vec<Value>) {
        let attribute = Attribute {
            name: name.to_owned(),
            values,
        };
        match self.groups.last_mut() {
            Some(last) if last.tag == group => last.attributes.push(attribute),
            _ => self.groups.push(Group {
                tag: group,
                attributes: vec![attribute],
            }),
        }
    }

    /// The groups with the tag `group`.
    pub fn groups(&self, group: u8) -> impl Iterator<Item = &Group> {
        self.groups.iter().filter(move |g| g.tag == group)
    }

    /// The attribute `name` of the first group with the tag `group`.
    pub fn get(&self, group: u8, name: &str) -> Option<&Attribute> {
        self.groups(group).find_map(|g| g.get(name))
    }

    /// The `status-message` of a response, if it has one.
    pub fn status_message(&self) -> Option<&str> {
        self.get(tag::OPERATION, "status-message")
            .and_then(Attribute::str)
    }

    /// The message in the binary encoding.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(256 + self.data.len());
        out.push(self.version.0);
        out.push(self.version.1);
        out.extend_from_slice(&self.code.to_be_bytes());
        out.extend_from_slice(&self.request_id.to_be_bytes());
        for group in &self.groups {
            out.push(group.tag);
            for attribute in &group.attributes {
                encode_attribute(&mut out, &attribute.name, &attribute.values);
            }
        }
        out.push(tag::END);
        out.extend_from_slice(&self.data);
        out
    }

    /// Reads a message from `bytes`.
    ///
    /// # Errors
    ///
    /// A [`DecodeError`] if `bytes` aren't a well-formed message.
    pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader {
            bytes,
            at: 0,
            values: 0,
        };
        let version = (reader.u8()?, reader.u8()?);
        let code = reader.u16()?;
        let request_id = reader.i32()?;
        let mut groups: Vec<Group> = Vec::new();
        loop {
            let tag = reader.u8()?;
            if tag == tag::END {
                break;
            }
            if tag < 0x10 {
                groups.push(Group {
                    tag,
                    attributes: Vec::new(),
                });
                continue;
            }
            let group = groups.last_mut().ok_or(DecodeError::NoGroup)?;
            let name = reader.string()?;
            let value = reader.value(tag, 0)?;
            if name.is_empty() {
                let last = group
                    .attributes
                    .last_mut()
                    .ok_or(DecodeError::NoAttribute)?;
                last.values.push(value);
            } else {
                group.attributes.push(Attribute {
                    name,
                    values: vec![value],
                });
            }
        }
        Ok(Self {
            version,
            code,
            request_id,
            groups,
            data: bytes.get(reader.at..).unwrap_or_default().to_vec(),
        })
    }
}

/// Writes `bytes` with their length in front.
fn put(out: &mut Vec<u8>, bytes: &[u8]) {
    // A string longer than IPP allows is cut; nothing Sway Things sends comes near it.
    let len = u16::try_from(bytes.len()).unwrap_or(u16::MAX);
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(bytes.get(..usize::from(len)).unwrap_or(bytes));
}

/// Writes the attribute `name` with `values`; the values after the first have no name.
fn encode_attribute(out: &mut Vec<u8>, name: &str, values: &[Value]) {
    for (i, value) in values.iter().enumerate() {
        out.push(value.tag());
        put(out, if i == 0 { name.as_bytes() } else { b"" });
        encode_value(out, value);
    }
}

/// Writes the length and bytes of `value`; for a collection, its members and its end.
fn encode_value(out: &mut Vec<u8>, value: &Value) {
    match value {
        Value::Integer(n) | Value::Enum(n) => put(out, &n.to_be_bytes()),
        Value::Boolean(b) => put(out, &[u8::from(*b)]),
        Value::Text(s)
        | Value::Name(s)
        | Value::Keyword(s)
        | Value::Uri(s)
        | Value::UriScheme(s)
        | Value::Charset(s)
        | Value::Language(s)
        | Value::MimeType(s) => put(out, s.as_bytes()),
        Value::OctetString(bytes) | Value::Other(_, bytes) => put(out, bytes),
        Value::DateTime(bytes) => put(out, bytes),
        Value::Resolution(x, y, unit) => {
            let mut bytes = Vec::with_capacity(9);
            bytes.extend_from_slice(&x.to_be_bytes());
            bytes.extend_from_slice(&y.to_be_bytes());
            bytes.extend_from_slice(&unit.to_be_bytes());
            put(out, &bytes);
        }
        Value::Range(low, high) => {
            let mut bytes = Vec::with_capacity(8);
            bytes.extend_from_slice(&low.to_be_bytes());
            bytes.extend_from_slice(&high.to_be_bytes());
            put(out, &bytes);
        }
        Value::OutOfBand(_) => put(out, b""),
        Value::Collection(members) => {
            put(out, b"");
            for member in members {
                for (i, value) in member.values.iter().enumerate() {
                    if i == 0 {
                        out.push(tag::MEMBER_NAME);
                        put(out, b"");
                        put(out, member.name.as_bytes());
                    }
                    out.push(value.tag());
                    put(out, b"");
                    encode_value(out, value);
                }
            }
            out.push(tag::END_COLLECTION);
            put(out, b"");
            put(out, b"");
        }
    }
}

/// Reads a message from the front.
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
    /// How many values were read so far.
    values: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], DecodeError> {
        let end = self.at.checked_add(n).ok_or(DecodeError::Truncated)?;
        let slice = self.bytes.get(self.at..end).ok_or(DecodeError::Truncated)?;
        self.at = end;
        Ok(slice)
    }

    fn u8(&mut self) -> Result<u8, DecodeError> {
        Ok(self.take(1)?.first().copied().unwrap_or_default())
    }

    fn u16(&mut self) -> Result<u16, DecodeError> {
        let bytes = self.take(2)?;
        Ok(u16::from_be_bytes(
            bytes.try_into().map_err(|_| DecodeError::Truncated)?,
        ))
    }

    fn i32(&mut self) -> Result<i32, DecodeError> {
        let bytes = self.take(4)?;
        Ok(i32::from_be_bytes(
            bytes.try_into().map_err(|_| DecodeError::Truncated)?,
        ))
    }

    /// Bytes with their length in front.
    fn sized(&mut self) -> Result<&[u8], DecodeError> {
        let len = self.u16()?;
        self.take(usize::from(len))
    }

    /// A string with its length in front, as lossy UTF-8.
    fn string(&mut self) -> Result<String, DecodeError> {
        Ok(String::from_utf8_lossy(self.sized()?).into_owned())
    }

    /// The value of an attribute with the tag `tag`, after its name; `depth` collections deep.
    fn value(&mut self, tag: u8, depth: usize) -> Result<Value, DecodeError> {
        self.values += 1;
        if self.values > MAX_VALUES {
            return Err(DecodeError::TooManyValues);
        }
        let bytes = self.sized()?;
        let string = || String::from_utf8_lossy(bytes).into_owned();
        let int = || -> Result<i32, DecodeError> {
            Ok(i32::from_be_bytes(
                bytes.try_into().map_err(|_| DecodeError::BadValue(tag))?,
            ))
        };
        Ok(match tag {
            tag::INTEGER => Value::Integer(int()?),
            tag::ENUM => Value::Enum(int()?),
            tag::BOOLEAN => match bytes {
                [b] => Value::Boolean(*b != 0),
                _ => return Err(DecodeError::BadValue(tag)),
            },
            tag::TEXT => Value::Text(string()),
            tag::NAME => Value::Name(string()),
            tag::TEXT_WITH_LANGUAGE => Value::Text(with_language(bytes)?),
            tag::NAME_WITH_LANGUAGE => Value::Name(with_language(bytes)?),
            tag::KEYWORD => Value::Keyword(string()),
            tag::URI => Value::Uri(string()),
            tag::URI_SCHEME => Value::UriScheme(string()),
            tag::CHARSET => Value::Charset(string()),
            tag::LANGUAGE => Value::Language(string()),
            tag::MIME_TYPE => Value::MimeType(string()),
            tag::OCTET_STRING => Value::OctetString(bytes.to_vec()),
            tag::DATE_TIME => {
                Value::DateTime(bytes.try_into().map_err(|_| DecodeError::BadValue(tag))?)
            }
            tag::RESOLUTION => match bytes {
                [x0, x1, x2, x3, y0, y1, y2, y3, unit] => Value::Resolution(
                    i32::from_be_bytes([*x0, *x1, *x2, *x3]),
                    i32::from_be_bytes([*y0, *y1, *y2, *y3]),
                    i8::from_be_bytes([*unit]),
                ),
                _ => return Err(DecodeError::BadValue(tag)),
            },
            tag::RANGE => match bytes {
                [l0, l1, l2, l3, h0, h1, h2, h3] => Value::Range(
                    i32::from_be_bytes([*l0, *l1, *l2, *l3]),
                    i32::from_be_bytes([*h0, *h1, *h2, *h3]),
                ),
                _ => return Err(DecodeError::BadValue(tag)),
            },
            tag::BEGIN_COLLECTION => self.collection(depth + 1)?,
            tag::END_COLLECTION | tag::MEMBER_NAME => return Err(DecodeError::BadCollection),
            0x10..=0x1F => Value::OutOfBand(tag),
            other => Value::Other(other, bytes.to_vec()),
        })
    }

    /// The members of a collection, after its start, up to and with its end.
    fn collection(&mut self, depth: usize) -> Result<Value, DecodeError> {
        if depth > MAX_DEPTH {
            return Err(DecodeError::BadCollection);
        }
        let mut members: Vec<Attribute> = Vec::new();
        loop {
            let tag = self.u8()?;
            if tag < 0x10 {
                return Err(DecodeError::BadCollection);
            }
            // Members have no names of their own: the member name is a value before them.
            let name = self.sized()?;
            if !name.is_empty() {
                return Err(DecodeError::BadCollection);
            }
            match tag {
                tag::END_COLLECTION => {
                    self.sized()?;
                    return Ok(Value::Collection(members));
                }
                tag::MEMBER_NAME => {
                    let member = self.string()?;
                    members.push(Attribute {
                        name: member,
                        values: Vec::new(),
                    });
                }
                other => {
                    let value = self.value(other, depth)?;
                    let member = members.last_mut().ok_or(DecodeError::BadCollection)?;
                    member.values.push(value);
                }
            }
        }
    }
}

/// The text of a text or name with its language: the language and the text, each with its
/// length in front.
fn with_language(bytes: &[u8]) -> Result<String, DecodeError> {
    let mut reader = Reader {
        bytes,
        at: 0,
        values: 0,
    };
    reader.sized()?;
    reader.string()
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn a_request_is_encoded_as_rfc_8010_shows() {
        let request = Message::request(0x000B, 1).with(
            tag::OPERATION,
            "printer-uri",
            vec![Value::Uri("ipp://localhost/printers/a".to_owned())],
        );
        let bytes = request.encode();
        assert_eq!(&bytes[..8], &[2, 0, 0, 0x0B, 0, 0, 0, 1]);
        assert_eq!(bytes[8], tag::OPERATION);
        assert_eq!(bytes[9], tag::CHARSET);
        assert_eq!(&bytes[10..12], &[0, 18]);
        assert_eq!(&bytes[12..30], b"attributes-charset");
        assert_eq!(&bytes[30..32], &[0, 5]);
        assert_eq!(&bytes[32..37], b"utf-8");
        assert_eq!(*bytes.last().unwrap(), tag::END);
    }

    #[test]
    fn several_values_and_groups_are_read() {
        let message = Message::request(0x4002, 7)
            .with(
                tag::PRINTER,
                "marker-levels",
                vec![Value::Integer(10), Value::Integer(-1)],
            )
            .with(
                tag::PRINTER,
                "printer-is-shared",
                vec![Value::Boolean(true)],
            );
        let mut message = message;
        message.groups.push(Group {
            tag: tag::PRINTER,
            attributes: vec![Attribute {
                name: "printer-name".to_owned(),
                values: vec![Value::Name("b".to_owned())],
            }],
        });
        message.data = b"%PDF".to_vec();
        let decoded = Message::decode(&message.encode()).unwrap();
        assert_eq!(decoded, message);
        assert_eq!(decoded.groups(tag::PRINTER).count(), 2);
        let levels: Vec<i32> = decoded
            .get(tag::PRINTER, "marker-levels")
            .unwrap()
            .ints()
            .collect();
        assert_eq!(levels, [10, -1]);
    }

    #[test]
    fn collections_round_trip() {
        let size = Value::Collection(vec![
            Attribute {
                name: "x-dimension".to_owned(),
                values: vec![Value::Integer(21000)],
            },
            Attribute {
                name: "y-dimension".to_owned(),
                values: vec![Value::Integer(29700)],
            },
        ]);
        let col = Value::Collection(vec![Attribute {
            name: "media-size".to_owned(),
            values: vec![size],
        }]);
        let message =
            Message::request(0x000B, 2).with(tag::PRINTER, "media-col-default", vec![col]);
        assert_eq!(Message::decode(&message.encode()).unwrap(), message);
    }

    #[test]
    fn text_with_language_keeps_the_text() {
        let mut bytes = Message::request(0x000B, 3).encode();
        bytes.pop();
        bytes.push(tag::PRINTER);
        bytes.push(tag::TEXT_WITH_LANGUAGE);
        bytes.extend_from_slice(&[0, 12]);
        bytes.extend_from_slice(b"printer-info");
        bytes.extend_from_slice(&[0, 10, 0, 2]);
        bytes.extend_from_slice(b"de");
        bytes.extend_from_slice(&[0, 4]);
        bytes.extend_from_slice(b"B\xc3\xbcr");
        bytes.push(tag::END);
        let message = Message::decode(&bytes).unwrap();
        assert_eq!(
            message.get(tag::PRINTER, "printer-info").unwrap().str(),
            Some("Bür")
        );
    }

    #[test]
    fn broken_messages_are_errors() {
        assert_eq!(Message::decode(&[2, 0, 0]), Err(DecodeError::Truncated));
        // No end tag.
        assert_eq!(
            Message::decode(&[2, 0, 0, 0, 0, 0, 0, 1]),
            Err(DecodeError::Truncated)
        );
        // A value before any group.
        assert_eq!(
            Message::decode(&[
                2, 0, 0, 0, 0, 0, 0, 1, 0x21, 0, 1, b'a', 0, 4, 0, 0, 0, 1, 3
            ]),
            Err(DecodeError::NoGroup)
        );
        // An additional value without an attribute.
        assert_eq!(
            Message::decode(&[2, 0, 0, 0, 0, 0, 0, 1, 1, 0x21, 0, 0, 0, 4, 0, 0, 0, 1, 3]),
            Err(DecodeError::NoAttribute)
        );
        // An integer of three bytes.
        assert_eq!(
            Message::decode(&[
                2, 0, 0, 0, 0, 0, 0, 1, 1, 0x21, 0, 1, b'a', 0, 3, 0, 0, 1, 3
            ]),
            Err(DecodeError::BadValue(0x21))
        );
        // Strings that aren't UTF-8 are kept as lossy text.
        let message =
            Message::decode(&[2, 0, 0, 0, 0, 0, 0, 1, 1, 0x41, 0, 1, b'a', 0, 1, 0xFF, 3]).unwrap();
        assert_eq!(message.get(1, "a").unwrap().str(), Some("\u{FFFD}"));
    }

    #[test]
    fn deep_collections_are_refused() {
        let mut bytes = vec![2, 0, 0, 0, 0, 0, 0, 1, 4, 0x34, 0, 1, b'c', 0, 0];
        for _ in 0..100 {
            bytes.extend_from_slice(&[0x4A, 0, 0, 0, 1, b'm', 0x34, 0, 0, 0, 0]);
        }
        assert_eq!(Message::decode(&bytes), Err(DecodeError::BadCollection));
    }

    #[test]
    fn too_many_values_are_refused() {
        let mut bytes = vec![2, 0, 0, 0, 0, 0, 0, 1, 4];
        bytes.extend_from_slice(&[0x21, 0, 1, b'a', 0, 4, 0, 0, 0, 1]);
        for _ in 0..MAX_VALUES {
            bytes.extend_from_slice(&[0x21, 0, 0, 0, 4, 0, 0, 0, 1]);
        }
        bytes.push(tag::END);
        assert_eq!(Message::decode(&bytes), Err(DecodeError::TooManyValues));
    }

    fn string() -> impl Strategy<Value = String> {
        "[a-z\\-]{0,12}"
    }

    fn scalar() -> impl Strategy<Value = Value> {
        prop_oneof![
            any::<i32>().prop_map(Value::Integer),
            any::<bool>().prop_map(Value::Boolean),
            any::<i32>().prop_map(Value::Enum),
            ".{0,20}".prop_map(Value::Text),
            ".{0,20}".prop_map(Value::Name),
            string().prop_map(Value::Keyword),
            string().prop_map(Value::Uri),
            string().prop_map(Value::MimeType),
            proptest::collection::vec(any::<u8>(), 0..20).prop_map(Value::OctetString),
            any::<[u8; 11]>().prop_map(Value::DateTime),
            (any::<i32>(), any::<i32>(), any::<i8>())
                .prop_map(|(x, y, u)| Value::Resolution(x, y, u)),
            (any::<i32>(), any::<i32>()).prop_map(|(l, h)| Value::Range(l, h)),
            prop_oneof![
                Just(tag::UNKNOWN),
                Just(tag::NO_VALUE),
                Just(tag::UNSUPPORTED)
            ]
            .prop_map(Value::OutOfBand),
        ]
    }

    fn value() -> impl Strategy<Value = Value> {
        scalar().prop_recursive(3, 24, 4, |inner| {
            proptest::collection::vec(("[a-z]{1,8}", proptest::collection::vec(inner, 1..3)), 0..4)
                .prop_map(|members| {
                    Value::Collection(
                        members
                            .into_iter()
                            .map(|(name, values)| Attribute { name, values })
                            .collect(),
                    )
                })
        })
    }

    fn message() -> impl Strategy<Value = Message> {
        let attribute = (
            "[a-z][a-z\\-]{0,15}",
            proptest::collection::vec(value(), 1..4),
        )
            .prop_map(|(name, values)| Attribute { name, values });
        let group = (
            prop_oneof![Just(tag::OPERATION), Just(tag::PRINTER), Just(tag::JOB)],
            proptest::collection::vec(attribute, 0..5),
        )
            .prop_map(|(tag, attributes)| Group { tag, attributes });
        (
            any::<u16>(),
            any::<i32>(),
            proptest::collection::vec(group, 0..4),
            proptest::collection::vec(any::<u8>(), 0..16),
        )
            .prop_map(|(code, request_id, groups, data)| Message {
                version: (2, 0),
                code,
                request_id,
                groups,
                data,
            })
    }

    proptest! {
        #[test]
        fn messages_round_trip(message in message()) {
            prop_assert_eq!(Message::decode(&message.encode()), Ok(message));
        }

        #[test]
        fn any_bytes_decode_without_panicking(bytes in proptest::collection::vec(any::<u8>(), 0..512)) {
            let _ = Message::decode(&bytes);
        }

        #[test]
        fn cut_messages_decode_without_panicking(message in message(), cut in any::<prop::sample::Index>()) {
            let bytes = message.encode();
            let _ = Message::decode(&bytes[..cut.index(bytes.len() + 1)]);
        }
    }
}
