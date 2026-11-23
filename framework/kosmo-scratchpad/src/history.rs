//! The entries of the history, the pending value of a submission, and what
//! preparing and accepting a submission and delivering a terminal state
//! give.

use std::fmt;

use kosmo_jobs::{JobId, Operation, StopReason, Terminal, TerminalState};
use kosmo_math_kalyx::{NotConverted, read_output_line};
use kosmo_math_model::Snapshot;
use kosmo_math_model::display::DisplayItem;
use kosmo_math_model::version::{Judgement, Versions};

/// A submission that the job layer has not answered yet: the snapshot of
/// the line that it was converted from, its three versions, its operation
/// and its variable, from [`Scratchpad::prepare`](crate::Scratchpad::prepare).
///
/// The Scratchpad keeps no pending submissions; whoever submits keeps this
/// value until the job layer answers and hands it to
/// [`Scratchpad::accept`](crate::Scratchpad::accept) with the answer. Only
/// `prepare` builds one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pending {
    input: Snapshot,
    versions: Versions,
    operation: Operation,
    variable: String,
}

impl Pending {
    pub(crate) fn new(
        input: Snapshot,
        versions: Versions,
        operation: Operation,
        variable: &str,
    ) -> Pending {
        Pending {
            input,
            versions,
            operation,
            variable: variable.to_string(),
        }
    }

    /// The snapshot of the line that the submission was converted from.
    pub const fn input(&self) -> &Snapshot {
        &self.input
    }

    /// The versions of the submission: the source version of its snapshot
    /// and the context and settings versions of the Scratchpad when it was
    /// prepared.
    pub const fn versions(&self) -> Versions {
        self.versions
    }

    /// The operation of its request.
    pub const fn operation(&self) -> Operation {
        self.operation
    }

    /// The variable of its request.
    pub fn variable(&self) -> &str {
        &self.variable
    }
}

/// One entry of the history: an accepted submission, found by its job
/// identity, with the snapshot and the versions it was prepared from, and
/// the outcome of its job once it was delivered.
///
/// An entry is a snapshot of its own: editing the current line, reusing the
/// entry and later results change none of its input, versions, operation
/// and variable, and its outcome is written once.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    id: JobId,
    input: Snapshot,
    versions: Versions,
    operation: Operation,
    variable: String,
    outcome: Option<Outcome>,
}

impl Entry {
    pub(crate) fn new(id: JobId, pending: Pending) -> Entry {
        Entry {
            id,
            input: pending.input,
            versions: pending.versions,
            operation: pending.operation,
            variable: pending.variable,
            outcome: None,
        }
    }

    /// The identity of its job.
    pub const fn id(&self) -> JobId {
        self.id
    }

    /// The snapshot of the line that was submitted.
    pub const fn input(&self) -> &Snapshot {
        &self.input
    }

    /// The versions of the submission.
    pub const fn versions(&self) -> Versions {
        self.versions
    }

    /// The operation of its request.
    pub const fn operation(&self) -> Operation {
        self.operation
    }

    /// The variable of its request.
    pub fn variable(&self) -> &str {
        &self.variable
    }

    /// How its job ended, once its terminal state was delivered; `None`
    /// while the job has not been delivered.
    pub const fn outcome(&self) -> Option<&Outcome> {
        self.outcome.as_ref()
    }

    /// Writes the outcome of `terminal` into the entry; false, changing
    /// nothing, when the entry has an outcome already.
    pub(crate) fn finish(&mut self, terminal: &Terminal) -> bool {
        if self.outcome.is_some() {
            return false;
        }
        self.outcome = Some(Outcome::read(terminal, &self.input, self.versions));
        true
    }
}

/// How the job of an entry ended: its terminal state and stop, whether it
/// failed with an internal error, and the display item read from its output
/// line, if it has one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    state: TerminalState,
    stop: Option<StopReason>,
    internal_error: bool,
    item: Option<DisplayItem>,
}

impl Outcome {
    /// The outcome of `terminal`. A job of the Scratchpad has one request,
    /// so its terminal record delivers at most one line, which is read
    /// against the snapshot and the versions of the submission.
    fn read(terminal: &Terminal, input: &Snapshot, versions: Versions) -> Outcome {
        let record = terminal.record();
        Outcome {
            state: record.state(),
            stop: record.stop(),
            internal_error: record.internal_error(),
            item: record
                .lines()
                .first()
                .map(|line| read_output_line(line, versions, input)),
        }
    }

    /// The terminal state of the job.
    pub const fn state(&self) -> TerminalState {
        self.state
    }

    /// The stop of the job, as its terminal record keeps it.
    pub const fn stop(&self) -> Option<StopReason> {
        self.stop
    }

    /// Whether the job failed with an internal error.
    pub const fn internal_error(&self) -> bool {
        self.internal_error
    }

    /// The display item of the job's output line: a value, a state, an
    /// error placed in the submitted snapshot, or a line that could not be
    /// read. `None` for a job that ended without a line, such as a
    /// cancelled one, one that failed with an internal error, or one whose
    /// deadline passed before its request ran.
    pub const fn item(&self) -> Option<&DisplayItem> {
        self.item.as_ref()
    }
}

/// What [`Scratchpad::deliver`](crate::Scratchpad::deliver) did with a
/// terminal state.
#[must_use = "the judgement says whether the result is the current one"]
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Delivery {
    /// The judgement of the result against the Scratchpad's current
    /// versions and latest submission when it was delivered.
    pub judgement: Judgement,
    /// Where the terminal state went.
    pub destination: Destination,
}

/// Where a delivered terminal state went.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Destination {
    /// Into the entry of its job, which had no outcome before.
    Entry,
    /// Toward the discard count: no entry of the history has its job's
    /// identity, because the entry was evicted or the job was never
    /// accepted here.
    Discarded,
    /// Nowhere: the entry of its job has an outcome already, which stays.
    AlreadyFinished,
}

/// Why [`Scratchpad::prepare`](crate::Scratchpad::prepare) built no
/// submission. Nothing changed.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NotPrepared {
    /// The line converts, but the variable is longer than
    /// [`MAX_VARIABLE_BYTES`](crate::MAX_VARIABLE_BYTES).
    VariableTooLong {
        /// The bytes of the variable that was given.
        bytes: usize,
    },
    /// The line is not converted: it is empty or incomplete, with its holes,
    /// or invalid, with its first invalid part.
    NotConverted(NotConverted),
}

impl fmt::Display for NotPrepared {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NotPrepared::VariableTooLong { bytes } => write!(
                f,
                "the variable has {bytes} bytes; a variable holds at most {} bytes",
                crate::MAX_VARIABLE_BYTES
            ),
            NotPrepared::NotConverted(reason) => write!(f, "{reason}"),
        }
    }
}

impl std::error::Error for NotPrepared {}

/// Why [`Scratchpad::accept`](crate::Scratchpad::accept) appended no entry.
/// Nothing of the Scratchpad changed.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NotAccepted {
    /// The job layer refused the submission.
    Refused(kosmo_jobs::Rejected),
    /// The job identity does not come after the identity of the latest
    /// accepted submission, so it is not a new job of the same job layer,
    /// as a second answer for one submission or an answer of another job
    /// layer gives. The job layer did accept that job, and it runs on;
    /// unless it is the job of an entry already, the caller cancels it or
    /// takes its terminal state without delivering it here.
    NotAfterLatest {
        /// The identity that was given.
        id: JobId,
        /// The identity of the latest accepted submission.
        latest: JobId,
    },
}

impl fmt::Display for NotAccepted {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NotAccepted::Refused(rejected) => {
                write!(f, "the job layer refused the submission: {rejected}")
            }
            NotAccepted::NotAfterLatest { id, latest } => write!(
                f,
                "{id} does not come after the latest accepted submission, {latest}"
            ),
        }
    }
}

impl std::error::Error for NotAccepted {}
