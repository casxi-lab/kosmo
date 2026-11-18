//! Conversion of a complete line into the linear text that kalyx 0.1.0
//! reads, and the pieces of that text.

use std::fmt;
use std::ops::Range;

use kosmo_math_model::{Completeness, Hole, Invalid, Item, Snapshot, TokenKind};

/// Why a line was not converted: it is not complete, so no text is
/// written for it and nothing is computed from it.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NotConverted {
    /// The line misses something: its holes in reading order, as the
    /// editing structure derives them.
    Incomplete(Vec<Hole>),
    /// The first part of the line that makes it invalid.
    Invalid(Invalid),
}

impl fmt::Display for NotConverted {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NotConverted::Incomplete(holes) => match holes.as_slice() {
                [one] => write!(
                    f,
                    "the line is incomplete: it has a hole at {}",
                    one.position.index
                ),
                holes => write!(f, "the line is incomplete: it has {} holes", holes.len()),
            },
            NotConverted::Invalid(invalid) => {
                let range = invalid.fragment.range();
                write!(
                    f,
                    "the line is invalid at items {}..{}",
                    range.start, range.end
                )
            }
        }
    }
}

impl std::error::Error for NotConverted {}

/// The linear text of a complete line: the text that kalyx 0.1.0 reads for
/// it, or the holes or the invalid part of a line that is not complete.
///
/// The text is the line's characters as they were typed, spaces included,
/// with two changes to numbers: a number that starts with a point gets a
/// `0` before it, so `.5` is written `0.5`, and a number that ends with a
/// point loses it, so `1.` is written `1`. Calls and the reserved names
/// `pi`, `i` and `undefined` are written as they are; kalyx 0.1.0 reports
/// them as unsupported. A complete line has no juxtaposition, no unary
/// plus and only ASCII characters, so the text uses only the grammar of
/// kalyx 0.1.0.
pub fn convert(snapshot: &Snapshot) -> Result<String, NotConverted> {
    match snapshot.completeness() {
        Completeness::Complete => Ok(Linear::of(snapshot).text),
        Completeness::Incomplete(holes) => Err(NotConverted::Incomplete(holes)),
        Completeness::Invalid(invalid) => Err(NotConverted::Invalid(invalid)),
    }
}

/// One piece of a line's linear text: a token, or a space between tokens,
/// with the items it comes from and the bytes it is written as. A piece
/// always has at least one item and one byte.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Piece {
    pub(crate) items: Range<usize>,
    pub(crate) bytes: Range<usize>,
}

/// A line written as linear text, with its pieces in order. The pieces
/// cover every item of the line and every byte of the text, without gaps.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Linear {
    pub(crate) text: String,
    pub(crate) pieces: Vec<Piece>,
}

impl Linear {
    /// The linear writing of any line, complete or not, by the rules of
    /// [`convert`]; only that of a complete line is ever given to kalyx.
    pub(crate) fn of(snapshot: &Snapshot) -> Linear {
        let chars: Vec<char> = snapshot.items().iter().map(character).collect();
        let mut text = String::with_capacity(chars.len());
        let mut pieces = Vec::new();
        let mut tokens = snapshot.tokens().into_iter().peekable();
        let mut index = 0;
        while index < chars.len() {
            let start = text.len();
            let items = match tokens.next_if(|token| token.fragment.start() == index) {
                Some(token) => {
                    let items = token.fragment.range();
                    let written = chars.get(items.clone()).unwrap_or_default();
                    if token.kind == TokenKind::Number {
                        write_number(&mut text, written);
                    } else {
                        text.extend(written);
                    }
                    items
                }
                // A space; the tokenizer leaves only spaces outside tokens.
                None => {
                    text.extend(chars.get(index));
                    index..index + 1
                }
            };
            index = items.end.max(index + 1);
            pieces.push(Piece {
                items,
                bytes: start..text.len(),
            });
        }
        Linear { text, pieces }
    }
}

/// The character of an item. One-line input holds only characters; an
/// item of another kind, which cannot exist until two-dimensional templates
/// do, is written as U+FFFD, which kalyx rejects.
fn character(item: &Item) -> char {
    match item {
        Item::Char(c) => *c,
        _ => char::REPLACEMENT_CHARACTER,
    }
}

/// Writes a number token: a leading point gets a `0` before it and a
/// trailing point is left out, so `.5` is `0.5` and `1.` is `1`.
fn write_number(text: &mut String, number: &[char]) {
    if number.first() == Some(&'.') {
        text.push('0');
    }
    text.extend(number.strip_suffix(&['.']).unwrap_or(number));
}

#[cfg(test)]
mod tests;
