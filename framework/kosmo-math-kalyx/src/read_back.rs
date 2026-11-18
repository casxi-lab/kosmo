//! Reading the display text of a kalyx value back into a line of its own.

use std::fmt;

use kosmo_math_model::{Completeness, EditBuffer, Rejected, Snapshot};

use crate::NotConverted;

/// Why a value cannot be reused: its display text does not read back into
/// a complete line. Nothing is inserted anywhere.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NotReusable {
    /// The editing structure rejected the text, for example as longer than
    /// one insertion may be.
    Rejected(Rejected),
    /// The text is a line that is not complete.
    NotComplete(NotConverted),
}

impl fmt::Display for NotReusable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NotReusable::Rejected(rejected) => write!(f, "the value cannot be reused: {rejected}"),
            NotReusable::NotComplete(reason) => write!(f, "the value cannot be reused: {reason}"),
        }
    }
}

impl std::error::Error for NotReusable {}

/// Reads the display text of a value of kalyx 0.1.0 back into a separate
/// line, as a snapshot from which a copy can be inserted into the line
/// being edited.
///
/// The text is typed into a new editing buffer as one insertion, under the
/// limits of the editing structure, and the line must be complete. kalyx
/// 0.1.0 writes every value as text of its own grammar, which kosmo reads
/// as a complete line, so only a text that the editing structure rejects,
/// such as one longer than one insertion may be, is not reusable; the
/// reason says why.
pub fn read_back(display_text: &str) -> Result<Snapshot, NotReusable> {
    let mut buffer = EditBuffer::new();
    buffer
        .insert_text(display_text)
        .map_err(NotReusable::Rejected)?;
    match buffer.completeness() {
        Completeness::Complete => Ok(buffer.snapshot()),
        Completeness::Incomplete(holes) => {
            Err(NotReusable::NotComplete(NotConverted::Incomplete(holes)))
        }
        Completeness::Invalid(invalid) => {
            Err(NotReusable::NotComplete(NotConverted::Invalid(invalid)))
        }
    }
}

#[cfg(test)]
mod tests;
