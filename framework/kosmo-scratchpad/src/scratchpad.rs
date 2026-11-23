//! The Scratchpad: its current line, versions, submissions, deliveries
//! and reuse.

use std::ops::Range;

use kosmo_jobs::{JobId, Limits, Operation, Request, SessionId, Submission, Terminal};
use kosmo_math_kalyx::convert;
use kosmo_math_model::version::{ContextVersion, SettingsVersion, Versions, judge};
use kosmo_math_model::{EditBuffer, Fragment, Rejected};

use crate::history::{Delivery, Destination, Entry, NotAccepted, NotPrepared, Pending};
use crate::reuse::{NotReused, Part, read_result};
use crate::{MAX_HISTORY_ENTRIES, MAX_VARIABLE_BYTES};

/// The Scratchpad: the current line, the region's versions and latest
/// accepted submission, the history and the discard count.
///
/// The current line is an [`EditBuffer`], which [`line`](Scratchpad::line)
/// shows; every editing operation goes through the Scratchpad, which passes
/// it on unchanged, so the source version starts at 0 and only increases.
/// The context and settings versions are fixed when the Scratchpad is
/// built: no operation changes the context, and kalyx 0.1.0 has no
/// settings.
///
/// The history holds the entries of accepted submissions in the order of
/// acceptance, oldest first, each found by its job identity. Job identities
/// count up within one job layer, so the entries of one Scratchpad belong to
/// the jobs of one job layer; a frontend that builds a new job layer builds
/// a new Scratchpad with it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Scratchpad {
    line: EditBuffer,
    context: ContextVersion,
    settings: SettingsVersion,
    latest: Option<JobId>,
    history: Vec<Entry>,
    discarded: u64,
}

impl Scratchpad {
    /// An empty Scratchpad: an empty line at source version 0, context and
    /// settings version 0, no submission and an empty history.
    pub fn new() -> Scratchpad {
        Scratchpad::default()
    }

    /// An empty Scratchpad with the context version `context` and the
    /// settings version `settings`, which no operation changes afterwards.
    /// Outside tests both are 0, as [`Scratchpad::new`] has them.
    pub fn with_versions(context: ContextVersion, settings: SettingsVersion) -> Scratchpad {
        Scratchpad {
            context,
            settings,
            ..Scratchpad::default()
        }
    }

    /// The current line. It can be read and copied, but only the editing
    /// operations of the Scratchpad change it.
    pub const fn line(&self) -> &EditBuffer {
        &self.line
    }

    /// The region's current versions: the source version of the current
    /// line, and the context and settings versions.
    pub fn versions(&self) -> Versions {
        Versions {
            source: self.line.version(),
            context: self.context,
            settings: self.settings,
        }
    }

    /// The job identity of the latest accepted submission, if any. A refused
    /// submission does not become the latest, and a cancelled or failed one
    /// stays the latest until another is accepted.
    pub const fn latest(&self) -> Option<JobId> {
        self.latest
    }

    /// The entries of the history, oldest first, at most
    /// [`MAX_HISTORY_ENTRIES`].
    pub fn history(&self) -> &[Entry] {
        &self.history
    }

    /// The entry of job `id`, unless it was evicted or never accepted.
    pub fn entry(&self, id: JobId) -> Option<&Entry> {
        find(&self.history, id)
    }

    /// The number of delivered terminal states whose entry was not in the
    /// history.
    pub const fn discarded(&self) -> u64 {
        self.discarded
    }

    /// Inserts a character at the cursor, as
    /// [`EditBuffer::insert_char`] does.
    pub fn insert_char(&mut self, c: char) -> Result<(), Rejected> {
        self.line.insert_char(c)
    }

    /// Inserts text at the cursor, as [`EditBuffer::insert_text`] does.
    pub fn insert_text(&mut self, text: &str) -> Result<(), Rejected> {
        self.line.insert_text(text)
    }

    /// Deletes the selection or the item before the cursor, as
    /// [`EditBuffer::delete_backward`] does.
    pub fn delete_backward(&mut self) -> Result<(), Rejected> {
        self.line.delete_backward()
    }

    /// Deletes the selection or the item after the cursor, as
    /// [`EditBuffer::delete_forward`] does.
    pub fn delete_forward(&mut self) -> Result<(), Rejected> {
        self.line.delete_forward()
    }

    /// Moves the cursor one item to the left, as
    /// [`EditBuffer::move_left`] does.
    pub fn move_left(&mut self) {
        self.line.move_left();
    }

    /// Moves the cursor one item to the right, as
    /// [`EditBuffer::move_right`] does.
    pub fn move_right(&mut self) {
        self.line.move_right();
    }

    /// Moves the cursor to the start of the line, as
    /// [`EditBuffer::move_to_start`] does.
    pub fn move_to_start(&mut self) {
        self.line.move_to_start();
    }

    /// Moves the cursor to the end of the line, as
    /// [`EditBuffer::move_to_end`] does.
    pub fn move_to_end(&mut self) {
        self.line.move_to_end();
    }

    /// Moves the active end of the selection one item to the left, as
    /// [`EditBuffer::extend_left`] does.
    pub fn extend_left(&mut self) {
        self.line.extend_left();
    }

    /// Moves the active end of the selection one item to the right, as
    /// [`EditBuffer::extend_right`] does.
    pub fn extend_right(&mut self) {
        self.line.extend_right();
    }

    /// Selects the whole line, as [`EditBuffer::select_all`] does.
    pub fn select_all(&mut self) {
        self.line.select_all();
    }

    /// Empties the line, as [`EditBuffer::clear`] does. Clearing a line
    /// that is not empty is an edit, which increases the source version.
    pub fn clear(&mut self) -> Result<(), Rejected> {
        self.line.clear()
    }

    /// Inserts a copy of the items `items` of a line of the entry of job
    /// `id` at the cursor of the current line, replacing the selection if
    /// there is one, with brackets where fragment insertion adds them.
    ///
    /// The range is a half-open range of items of the entry's input, for
    /// [`Part::Input`], or of its value read back into a line of its own,
    /// for [`Part::Result`]; any range of either line can be reused, not
    /// only one that forms an operand. Only a value can be reused as a
    /// result: its display text is read back when it is reused. A copy that
    /// changes the line increases its source version by exactly 1, and the
    /// entry stays as it was. An empty range changes nothing when nothing
    /// is selected; with a selection it deletes the selection, as any
    /// insertion replaces it.
    ///
    /// When nothing can be reused, the reason says why and nothing changes:
    /// not the line, its cursor, its selection or its source version.
    pub fn reuse(&mut self, id: JobId, part: Part, items: Range<usize>) -> Result<(), NotReused> {
        let entry = find(&self.history, id).ok_or(NotReused::NotInHistory(id))?;
        let read;
        let snapshot = match part {
            Part::Input => entry.input(),
            Part::Result => {
                read = read_result(entry)?;
                &read
            }
        };
        let fragment = Fragment::new(snapshot.root(), items)
            .ok_or(NotReused::Rejected(Rejected::NotInSnapshot))?;
        self.line
            .insert_fragment(snapshot, fragment)
            .map_err(NotReused::Rejected)
    }

    /// The first step of a submission: converts the current line into a
    /// job of one request and gives it with the pending value that
    /// [`accept`](Scratchpad::accept) takes once the job layer has answered.
    ///
    /// The request has `operation`, `variable` and the linear text of the
    /// line, and the job has the session `session`, which the caller opened
    /// on its job layer, the limits `limits` and, when `time_limit` is
    /// given, that time limit in nanoseconds. Its versions are the source
    /// version of the line's snapshot and the Scratchpad's context and
    /// settings versions. The pending value keeps the snapshot and the
    /// versions, so later edits of the line do not change the entry that
    /// the submission will get.
    ///
    /// Preparing changes nothing: neither the line nor its source version.
    /// The line is checked first: an empty, incomplete or invalid line is
    /// not converted, whatever the variable, and gives its holes or its
    /// first invalid part instead, as [`NotPrepared::NotConverted`]. Then a
    /// variable longer than [`MAX_VARIABLE_BYTES`] gives
    /// [`NotPrepared::VariableTooLong`], so that every entry of the history
    /// stays bounded.
    pub fn prepare(
        &self,
        session: SessionId,
        operation: Operation,
        variable: &str,
        limits: Limits,
        time_limit: Option<u64>,
    ) -> Result<(Submission, Pending), NotPrepared> {
        let input = self.line.snapshot();
        let text = convert(&input).map_err(NotPrepared::NotConverted)?;
        if variable.len() > MAX_VARIABLE_BYTES {
            return Err(NotPrepared::VariableTooLong {
                bytes: variable.len(),
            });
        }
        let versions = Versions {
            source: input.version(),
            context: self.context,
            settings: self.settings,
        };
        let request = Request::new(operation, variable, text);
        let mut submission = Submission::new(session, versions, request).with_limits(limits);
        if let Some(nanos) = time_limit {
            submission = submission.with_time_limit(nanos);
        }
        Ok((
            submission,
            Pending::new(input, versions, operation, variable),
        ))
    }

    /// The second step of a submission: takes the job layer's answer to a
    /// prepared submission with its pending value.
    ///
    /// When the job layer accepted the submission as job `id`, the job
    /// becomes the latest submission and its entry, with the snapshot and
    /// the versions of `pending`, is appended to the history; when the
    /// history held [`MAX_HISTORY_ENTRIES`] entries, the oldest is evicted
    /// first. The job of an evicted entry is not cancelled: its terminal
    /// state, when it is delivered, counts toward the discard count.
    /// Accepting returns `id`.
    ///
    /// When the job layer refused the submission, nothing changes, and the
    /// refusal comes back as [`NotAccepted::Refused`]. Nothing changes
    /// either for an identity that does not come after the latest one,
    /// which another job layer or a second answer would give, as
    /// [`NotAccepted::NotAfterLatest`]: the job layer did accept that job,
    /// and unless it is the job of an entry already, the caller cancels it
    /// or takes its terminal state without delivering it here.
    pub fn accept(
        &mut self,
        pending: Pending,
        answer: Result<JobId, kosmo_jobs::Rejected>,
    ) -> Result<JobId, NotAccepted> {
        let id = answer.map_err(NotAccepted::Refused)?;
        if let Some(latest) = self.latest
            && id <= latest
        {
            return Err(NotAccepted::NotAfterLatest { id, latest });
        }
        self.latest = Some(id);
        self.history.push(Entry::new(id, pending));
        let evicted = self.history.len().saturating_sub(MAX_HISTORY_ENTRIES);
        self.history.drain(..evicted);
        Ok(id)
    }

    /// Delivers the terminal state of a job, which the caller took from its
    /// job layer.
    ///
    /// Only terminal states of this Scratchpad's job layer, the one that
    /// accepted its submissions, may be delivered: job identities count up
    /// within one job layer, so a terminal state of another job layer can
    /// carry the identity of a different job that has an entry here.
    ///
    /// The result is judged against the Scratchpad's current versions and
    /// latest submission, from the job's identity and versions. The terminal
    /// state goes into the entry of its job, read against the entry's
    /// snapshot, whatever the judgement: the entry keeps the result of its
    /// own submission. Without an entry, because it was evicted or the job
    /// was never accepted here, the terminal state counts toward the discard
    /// count. An entry that has an outcome already keeps it.
    ///
    /// The Scratchpad keeps no current result: whoever shows one writes the
    /// result of a current delivery into its result slot.
    pub fn deliver(&mut self, terminal: &Terminal) -> Delivery {
        let id = terminal.id();
        let judgement = judge(
            terminal.versions(),
            &id,
            self.versions(),
            self.latest.as_ref(),
        );
        let destination = match self.history.iter_mut().find(|entry| entry.id() == id) {
            Some(entry) => {
                if entry.finish(terminal) {
                    Destination::Entry
                } else {
                    Destination::AlreadyFinished
                }
            }
            None => {
                self.discarded = self.discarded.saturating_add(1);
                Destination::Discarded
            }
        };
        Delivery {
            judgement,
            destination,
        }
    }
}

/// The entry of job `id` in `history`.
fn find(history: &[Entry], id: JobId) -> Option<&Entry> {
    history.iter().find(|entry| entry.id() == id)
}

#[cfg(test)]
mod tests;
