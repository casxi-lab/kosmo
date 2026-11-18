//! The mapping of the span of a kalyx error back into the line whose text
//! kalyx read.

use std::fmt;
use std::ops::Range;

use kosmo_math_model::{Fragment, Position, Snapshot};

use crate::conversion::{Linear, Piece};

/// Where in a line an error of kalyx lies.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Place {
    /// The whole line: the error has no span, which an error line writes
    /// as `-`.
    Line,
    /// The items that a non-empty span covers.
    Fragment(Fragment),
    /// The place between two items that an empty span marks, such as the
    /// end of the line.
    Position(Position),
}

/// A span that does not lie in the linear text of the line: it runs
/// backwards or ends past the end of the text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpanOutside {
    /// The span, as given.
    pub span: Range<usize>,
    /// The length of the text in bytes.
    pub len: usize,
}

impl fmt::Display for SpanOutside {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "the span {}..{} does not lie in the {} bytes of the text",
            self.span.start, self.span.end, self.len
        )
    }
}

impl std::error::Error for SpanOutside {}

/// Maps the span of a kalyx error back into the line of `snapshot`.
///
/// The span is the half-open range of byte offsets that an error line of
/// kalyx 0.1.0 gives, into the linear text of the line: the text that
/// [`convert`](crate::convert) writes for it. A line that is not complete
/// is never converted; its span is taken over the line written by the same
/// rules, which for a line without numbers that start or end with a point
/// is the line itself.
///
/// The text is made of pieces, one for each token and each space of the
/// line. A non-empty span maps by pieces: its start goes to the first item
/// of the piece that holds its first byte, and its end to the end of the
/// piece that holds its last byte. So a span over a number covers all its
/// items, the point that conversion left out or wrote a `0` before
/// included, and a non-empty span that starts or ends inside a piece grows
/// to the edges of that piece. kalyx 0.1.0 reports spans that start and
/// end at the edges of tokens.
///
/// `None`, an error without a span, maps to the whole line, and a
/// non-empty span to the fragment of the items it covers. An empty span
/// always maps to a position: the position before the first item of the
/// piece that holds the byte at its offset, which is the position between
/// two pieces when the span lies between them, or the end of the line at
/// the end of the text.
pub fn map_span(snapshot: &Snapshot, span: Option<Range<usize>>) -> Result<Place, SpanOutside> {
    let Some(span) = span else {
        return Ok(Place::Line);
    };
    let linear = Linear::of(snapshot);
    let len = linear.text.len();
    if span.start > span.end || span.end > len {
        return Err(SpanOutside { span, len });
    }
    let start = item_from(&linear.pieces, span.start, snapshot.items().len());
    let end = item_to(&linear.pieces, span.end);
    let block = snapshot.root();
    Ok(match Fragment::new(block, start..end) {
        // A non-empty span covers at least the piece of its first byte.
        Some(fragment) if !span.is_empty() => Place::Fragment(fragment),
        _ => Place::Position(Position {
            block,
            index: start,
        }),
    })
}

/// The item at which a span starting at byte `start` starts: the first
/// item of the piece that holds the byte, or `items` at the end of the
/// text.
fn item_from(pieces: &[Piece], start: usize, items: usize) -> usize {
    pieces
        .iter()
        .find(|piece| piece.bytes.contains(&start))
        .map_or(items, |piece| piece.items.start)
}

/// The item at which a span ending at byte `end` ends: the end of the piece
/// that holds the byte before it, or 0 at the start of the text.
fn item_to(pieces: &[Piece], end: usize) -> usize {
    pieces
        .iter()
        .find(|piece| piece.bytes.start < end && end <= piece.bytes.end)
        .map_or(0, |piece| piece.items.end)
}

#[cfg(test)]
mod tests;
