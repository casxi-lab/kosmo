//! The display model of exact results, and their rendering as one line of
//! text without a screen.
//!
//! A [`DisplayItem`] is what one result shows: a value, a mathematical
//! state, an error, or an output line of the computation library that could
//! not be read, with the [`Versions`] of the request whose result it is.
//! The place of an error is in the snapshot that the request was converted
//! from, never in the line being edited, whose content may have changed
//! since.
//!
//! The model does not judge whether a result is current. Whoever shows it
//! compares its versions with the current ones and passes the outcome as
//! [`Freshness`], and [`DisplayItem::render`] marks a stale result.
//!
//! ```
//! use kosmo_math_model::display::{Content, DisplayItem, Freshness};
//! use kosmo_math_model::version::{SettingsVersion, Versions};
//!
//! let item = DisplayItem {
//!     versions: Versions::default(),
//!     content: Content::Value("1/2".to_string()),
//! };
//! assert_eq!(item.render(Freshness::Current), "value (source 0, settings 0): 1/2");
//!
//! // After the settings changed, the result is shown as stale.
//! let current = Versions {
//!     settings: SettingsVersion::new(1),
//!     ..Versions::default()
//! };
//! assert_ne!(item.versions, current);
//! assert_eq!(
//!     item.render(Freshness::Stale),
//!     "value (source 0, settings 0, stale): 1/2"
//! );
//! ```

use std::borrow::Cow;
use std::fmt;
use std::ops::Range;

use crate::Place;
use crate::version::Versions;

/// The most bytes of an item's text that a rendering shows after its
/// colon. A longer text is cut and followed by ` ... (L bytes)` with its
/// length `L`, as [`DisplayItem::render`] describes. The bound is the
/// default `max_output_bytes` of kalyx 0.1.0, so that the display text of
/// every value computed under the default limits is shown whole.
pub const MAX_DISPLAY_BYTES: usize = 65_536;

/// One result as the display model holds it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DisplayItem {
    /// The versions of the request whose result it is.
    pub versions: Versions,
    /// What the result shows.
    pub content: Content,
}

/// Whether a result is shown as the current one or as stale, as whoever
/// shows it judges from its versions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Freshness {
    /// The result of the current versions.
    Current,
    /// A result of versions that are no longer current.
    Stale,
}

/// What a result shows.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Content {
    /// An exact value, by its display text as kalyx writes it: text that
    /// its grammar reads back as the same value.
    Value(String),
    /// A mathematical state, by its name, such as `IterationLimit`.
    State(String),
    /// An error, by its kind and its place in the submitted snapshot.
    Error {
        /// What went wrong.
        kind: ErrorKind,
        /// Where in the submitted snapshot it went wrong.
        place: Place,
    },
    /// An output line that could not be read, which is shown as an internal
    /// error.
    Unreadable(Unreadable),
}

/// The kind of an error, as kalyx 0.1.0 names it.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    /// A limit of the request was reached.
    ResourceLimit(LimitExceeded),
    /// The request was cancelled.
    Cancelled,
    /// The deadline of the request passed.
    DeadlineExceeded,
    /// The input does not follow the grammar.
    Syntax,
    /// The input holds a symbol that the operation cannot use.
    UnknownSymbol,
    /// A division by zero.
    DivisionByZero,
    /// An exponent that is not an integer.
    NonIntegerExponent,
    /// The input is not a polynomial in the variable.
    NotPolynomial,
    /// A form that this version of kalyx does not support.
    Unsupported,
    /// A fault of kalyx itself.
    Internal,
    /// A kind that kalyx 0.1.0 does not have, by the name that the output
    /// line gives, as a later version of kalyx writes its new kinds.
    Other(String),
}

impl ErrorKind {
    /// The name of the kind, as an output line of kalyx writes it.
    pub fn name(&self) -> &str {
        match self {
            ErrorKind::ResourceLimit(_) => "ResourceLimit",
            ErrorKind::Cancelled => "Cancelled",
            ErrorKind::DeadlineExceeded => "DeadlineExceeded",
            ErrorKind::Syntax => "Syntax",
            ErrorKind::UnknownSymbol => "UnknownSymbol",
            ErrorKind::DivisionByZero => "DivisionByZero",
            ErrorKind::NonIntegerExponent => "NonIntegerExponent",
            ErrorKind::NotPolynomial => "NotPolynomial",
            ErrorKind::Unsupported => "Unsupported",
            ErrorKind::Internal => "Internal",
            ErrorKind::Other(name) => name,
        }
    }
}

/// The limit that a request reached.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LimitExceeded {
    /// The limit.
    pub limit: LimitId,
    /// Where the limit was reached, such as `Value`, for the limits that
    /// count storage.
    pub scope: Option<String>,
    /// The value of the limit in force.
    pub allowed: u64,
    /// How far the request went.
    pub amount: LimitAmount,
}

/// Which limit a request reached.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LimitId {
    /// A field of kalyx's `Limits`, by its name, such as `max_work`.
    Field(String),
    /// A parameter of the request, by its name as the output line writes
    /// it: escaped, so that it holds no ASCII control character. kalyx
    /// 0.1.0 escapes the bytes `0x00` to `0x1F` and `0x7F` and writes every
    /// other byte as it is.
    Parameter(String),
}

/// How far a request went past a limit.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LimitAmount {
    /// The total at the check.
    Used(u64),
    /// The amount needed.
    Required(u64),
    /// An upper bound of the amount needed.
    Bound(u64),
}

/// Why an output line could not be read.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Unreadable {
    /// The line is not UTF-8.
    NotUtf8,
    /// The line does not end with a line feed.
    Unterminated,
    /// The line holds a control character other than the tabs between its
    /// fields and the final line feed.
    ControlCharacter,
    /// The first field names no kind of line.
    UnknownKind,
    /// The line has the wrong number of fields for its kind.
    FieldCount {
        /// The kind of the line.
        kind: LineKind,
        /// The number of fields it has.
        found: usize,
        /// The number of fields that a line of its kind has.
        expected: usize,
    },
    /// A field is not written as the format of output lines requires.
    Malformed,
    /// The span of an error does not lie in the linear text of the
    /// submitted snapshot.
    SpanOutside {
        /// The span, as the line gives it.
        span: Range<usize>,
        /// The length of the text in bytes.
        len: usize,
    },
    /// The line echoes versions other than those of the request.
    OtherVersions,
}

impl fmt::Display for Unreadable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Unreadable::NotUtf8 => f.write_str("the output line is not UTF-8"),
            Unreadable::Unterminated => {
                f.write_str("the output line does not end with a line feed")
            }
            Unreadable::ControlCharacter => {
                f.write_str("the output line holds a control character")
            }
            Unreadable::UnknownKind => {
                f.write_str("the output line is not a value, state or error line")
            }
            Unreadable::FieldCount {
                kind,
                found,
                expected,
            } => {
                let fields = if *found == 1 { "field" } else { "fields" };
                write!(
                    f,
                    "the output line has {found} {fields}; {} line has {expected}",
                    kind.with_article()
                )
            }
            Unreadable::Malformed => f.write_str("a field of the output line is malformed"),
            Unreadable::SpanOutside { span, len } => write!(
                f,
                "the span {}..{} does not lie in the {len} bytes of the text",
                span.start, span.end
            ),
            Unreadable::OtherVersions => {
                f.write_str("the output line echoes other versions than the request")
            }
        }
    }
}

/// The kind of an output line, which fixes its number of fields.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LineKind {
    /// A value line.
    Value,
    /// A state line.
    State,
    /// An error line.
    Error,
}

impl LineKind {
    /// The kind as the first field of a line writes it, after an article.
    const fn with_article(self) -> &'static str {
        match self {
            LineKind::Value => "a value",
            LineKind::State => "a state",
            LineKind::Error => "an error",
        }
    }
}

impl DisplayItem {
    /// The item rendered as text without a line feed. When the item is read
    /// from an output line, its strings hold no ASCII control character, so
    /// the rendering is one line:
    ///
    /// ```text
    /// <head> (source S, settings T[, stale]): <text>
    /// ```
    ///
    /// - The head is `value`, `state` or `error`, or `internal error` for
    ///   an output line that could not be read.
    /// - `S` and `T` are the source and settings versions of the request.
    ///   The context version is not written: no operation changes it yet.
    /// - `, stale` marks a result shown as [`Freshness::Stale`].
    /// - The text of a value is its display text as it is, and the text of
    ///   a state its name.
    /// - The text of an error is the name of its kind. A `ResourceLimit`
    ///   adds `(L, allowed A, W N)`: `L` is the name of a field of the
    ///   limits, or `parameter` and the escaped name of a parameter,
    ///   followed by ` in` and the scope when there is one; `A` is the
    ///   value of the limit; `W N` is `used N`, `required N` or
    ///   `required at most N`. Then comes the place: ` at items a..b` for
    ///   a fragment, ` at position i` for a position and
    ///   ` on the whole line`. kalyx's message is not shown.
    /// - The text of an output line that could not be read says why, in
    ///   English.
    ///
    /// Text longer than [`MAX_DISPLAY_BYTES`] keeps its longest beginning
    /// that ends on a UTF-8 character boundary within that many bytes,
    /// followed by ` ... (L bytes)` with its length `L`. The rendering
    /// depends only on the item and `freshness`.
    pub fn render(&self, freshness: Freshness) -> String {
        let head = match self.content {
            Content::Value(_) => "value",
            Content::State(_) => "state",
            Content::Error { .. } => "error",
            Content::Unreadable(_) => "internal error",
        };
        let stale = match freshness {
            Freshness::Current => "",
            Freshness::Stale => ", stale",
        };
        let text = self.content.text();
        let (shown, cut) = bounded(&text);
        format!(
            "{head} (source {}, settings {}{stale}): {shown}{cut}",
            self.versions.source.get(),
            self.versions.settings.get()
        )
    }
}

impl Content {
    /// The text after the colon of the rendering, before the bound.
    fn text(&self) -> Cow<'_, str> {
        match self {
            Content::Value(text) | Content::State(text) => Cow::Borrowed(text),
            Content::Error { kind, place } => Cow::Owned(format!(
                "{}{}{}",
                kind.name(),
                limit_of(kind),
                place_of(*place)
            )),
            Content::Unreadable(reason) => Cow::Owned(reason.to_string()),
        }
    }
}

/// The limit of a `ResourceLimit` in brackets after a space, or nothing for
/// any other kind.
fn limit_of(kind: &ErrorKind) -> String {
    let ErrorKind::ResourceLimit(exceeded) = kind else {
        return String::new();
    };
    let limit = match &exceeded.limit {
        LimitId::Field(name) => name.clone(),
        LimitId::Parameter(name) => format!("parameter {name}"),
    };
    let scope = exceeded
        .scope
        .as_ref()
        .map(|scope| format!(" in {scope}"))
        .unwrap_or_default();
    let (word, amount) = match exceeded.amount {
        LimitAmount::Used(amount) => ("used", amount),
        LimitAmount::Required(amount) => ("required", amount),
        LimitAmount::Bound(amount) => ("required at most", amount),
    };
    format!(
        " ({limit}{scope}, allowed {}, {word} {amount})",
        exceeded.allowed
    )
}

/// The place of an error, after a space.
fn place_of(place: Place) -> String {
    match place {
        Place::Line => " on the whole line".to_string(),
        Place::Fragment(fragment) => {
            format!(" at items {}..{}", fragment.start(), fragment.end())
        }
        Place::Position(position) => format!(" at position {}", position.index),
    }
}

/// `text` within [`MAX_DISPLAY_BYTES`], and what follows it: nothing when
/// it fits, or else its length.
fn bounded(text: &str) -> (&str, String) {
    if text.len() <= MAX_DISPLAY_BYTES {
        return (text, String::new());
    }
    let end = text.floor_char_boundary(MAX_DISPLAY_BYTES);
    let shown = text.get(..end).unwrap_or_default();
    (shown, format!(" ... ({} bytes)", text.len()))
}

#[cfg(test)]
mod tests;
