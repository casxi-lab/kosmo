//! Helpers shared by the integration tests.

#![allow(dead_code, reason = "each test file uses some of the helpers")]

use kosmo_math_model::{Completeness, EditBuffer, HoleKind, Item, Rejected};

/// A line with `text` typed into it and the cursor at its end.
pub fn line(text: &str) -> Result<EditBuffer, Rejected> {
    let mut buffer = EditBuffer::new();
    buffer.insert_text(text)?;
    Ok(buffer)
}

/// The characters of `items`. No template can exist yet; one would show as
/// U+FFFD, which no line can hold.
pub fn text(items: &[Item]) -> String {
    items
        .iter()
        .map(|item| match item {
            Item::Char(c) => *c,
            _ => '\u{fffd}',
        })
        .collect()
}

/// Puts the cursor of `buffer` at `index`, without a selection.
pub fn put_cursor(buffer: &mut EditBuffer, index: usize) {
    buffer.move_to_start();
    for _ in 0..index {
        buffer.move_right();
    }
}

/// The holes of an incomplete line as (kind, index) pairs, or `None` for a
/// complete or invalid line.
pub fn holes(completeness: &Completeness) -> Option<Vec<(HoleKind, usize)>> {
    match completeness {
        Completeness::Incomplete(holes) => Some(
            holes
                .iter()
                .map(|hole| (hole.kind, hole.position.index))
                .collect(),
        ),
        Completeness::Complete | Completeness::Invalid(_) => None,
    }
}
