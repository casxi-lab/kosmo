//! Reusing the input or the result of a history entry in the current line.

use std::fmt;

use kosmo_jobs::{JobId, TerminalState};
use kosmo_math_kalyx::{NotReusable, read_back};
use kosmo_math_model::display::Content;
use kosmo_math_model::{Rejected, Snapshot};

use crate::history::Entry;

/// Which line of an entry a reused range is in. An entry has these two
/// lines only: what was submitted and what came back.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Part {
    /// The submitted line, the entry's input snapshot.
    Input,
    /// The display text of the entry's value, read back into a line of its
    /// own.
    Result,
}

/// Why nothing was reused. The current line, its cursor, its selection and
/// its source version did not change.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NotReused {
    /// No entry of the history has the job identity: the entry was
    /// evicted, or the job was never accepted here.
    NotInHistory(JobId),
    /// The entry's job has not been delivered, so it has no result yet.
    NoResultYet,
    /// The entry's job ended in this terminal state without an output
    /// line, as a cancelled job does.
    NoLine(TerminalState),
    /// The result is an error.
    Error,
    /// The result is a state.
    State,
    /// The result's output line could not be read, or holds a kind of
    /// content that this version cannot reuse.
    Unreadable,
    /// The display text of the value does not read back into a complete
    /// line, for example because it is longer than one insertion may be.
    NotReadBack(NotReusable),
    /// The current line rejected the insertion, or the range runs backwards
    /// or does not lie in the reused line, as
    /// [`Rejected::NotInSnapshot`].
    Rejected(Rejected),
}

impl fmt::Display for NotReused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NotReused::NotInHistory(id) => write!(f, "{id} is not in the history"),
            NotReused::NoResultYet => f.write_str("the entry has no result yet"),
            NotReused::NoLine(_) => f.write_str("the job ended without an output line"),
            NotReused::Error => f.write_str("the result is an error, not a value"),
            NotReused::State => f.write_str("the result is a state, not a value"),
            NotReused::Unreadable => f.write_str("the output line of the result is unreadable"),
            NotReused::NotReadBack(reason) => write!(f, "{reason}"),
            NotReused::Rejected(rejected) => write!(f, "the line rejected the copy: {rejected}"),
        }
    }
}

impl std::error::Error for NotReused {}

/// The value of `entry` read back into a line of its own.
pub(crate) fn read_result(entry: &Entry) -> Result<Snapshot, NotReused> {
    read_back(value(entry)?).map_err(NotReused::NotReadBack)
}

/// The display text of the value of `entry`.
fn value(entry: &Entry) -> Result<&str, NotReused> {
    let outcome = entry.outcome().ok_or(NotReused::NoResultYet)?;
    let item = outcome.item().ok_or(NotReused::NoLine(outcome.state()))?;
    match &item.content {
        Content::Value(text) => Ok(text),
        Content::Error { .. } => Err(NotReused::Error),
        Content::State(_) => Err(NotReused::State),
        Content::Unreadable(_) => Err(NotReused::Unreadable),
        // A kind of content that a later version of the display model adds.
        _ => Err(NotReused::Unreadable),
    }
}

#[cfg(test)]
mod tests;
