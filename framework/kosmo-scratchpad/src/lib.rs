//! The Scratchpad: one line of formula input, the submission of the line as
//! a versioned job, a bounded history of what was submitted with the
//! results, and the reuse of inputs and results of the history in the line.
//! All of its state is in memory: it reads and writes no file and refers to
//! no document.
//!
//! A [`Scratchpad`] holds the current line, an editing buffer of
//! `kosmo-math-model`, and passes each editing operation on to it; it never
//! hands out the buffer itself, so the source version only increases. It is
//! the region whose current versions and latest accepted submission judge
//! each result, as `kosmo_math_model::version::judge` does.
//!
//! A submission takes two steps, because the job layer answers it apart
//! from building it:
//!
//! 1. [`Scratchpad::prepare`] converts the current line into the
//!    `Submission` of one request of the job layer `kosmo-jobs`, and gives a
//!    [`Pending`] value with the snapshot and the versions it was converted
//!    from. It changes nothing; an incomplete or invalid line gives its holes
//!    or its invalid part instead, and a variable longer than
//!    [`MAX_VARIABLE_BYTES`] is not submitted.
//! 2. Once the job layer has answered, [`Scratchpad::accept`] takes the
//!    pending value with the answer. An accepted submission becomes the
//!    latest one and appends an [`Entry`] to the history, keyed by its job
//!    identity; a refused one changes nothing.
//!
//! The Scratchpad holds no job layer: whoever holds the job layer submits,
//! waits or polls, and hands each terminal state to
//! [`Scratchpad::deliver`], which writes it into the entry of its job. The
//! history keeps at most [`MAX_HISTORY_ENTRIES`] entries and evicts the
//! oldest; a terminal state whose entry was evicted counts toward the
//! discard count. [`Scratchpad::reuse`] inserts a copy of a range of an
//! entry's input, or of its value read back into a line, at the cursor.
//!
//! The example runs one job on the controllable backend of the job layer,
//! which answers with the line that kalyx 0.1.0 writes for differentiating
//! `x^2` with respect to `x`, and reuses the result in the hole of `1/`:
//!
//! ```
//! use kosmo_jobs::controllable::{ControllableBackend, Plan};
//! use kosmo_jobs::{
//!     EchoedVersions, ErrorClass, JobClock, JobLayer, Limits, Operation, OutcomeKind,
//!     RequestReport,
//! };
//! use kosmo_math_kalyx::convert;
//! use kosmo_math_model::version::Judgement;
//! use kosmo_scratchpad::{Destination, Part, Scratchpad};
//!
//! struct Still;
//! impl JobClock for Still {
//!     fn now(&mut self) -> u64 { 0 }
//!     fn fork(&self) -> Box<dyn JobClock> { Box::new(Still) }
//! }
//!
//! let line = b"value\t1\t0\t0\t2*x\n".to_vec();
//! let echo = EchoedVersions::new(1, 0, 0);
//! let report = RequestReport::new(OutcomeKind::Value, ErrorClass::NoError, Some(line), echo);
//! let mut backend = ControllableBackend::new();
//! backend.plan(0, Plan::new().request(report));
//! let mut layer = JobLayer::new(backend, Box::new(Still));
//! let session = layer.open_session()?;
//!
//! let mut pad = Scratchpad::new();
//! pad.insert_text("x^2")?;
//! let (submission, pending) = pad.prepare(session, Operation::Diff, "x", Limits::new(), None)?;
//! let id = pad.accept(pending, layer.submit(submission))?;
//! let terminal = layer.wait(id).ok_or("no terminal state")?;
//! let delivery = pad.deliver(&terminal);
//! assert_eq!(delivery.judgement, Judgement::Current);
//! assert_eq!(delivery.destination, Destination::Entry);
//!
//! // Clearing the line is an edit of its own; then the result `2*x` fills
//! // the hole of `1/` in brackets.
//! pad.clear()?;
//! pad.insert_text("1/")?;
//! pad.reuse(id, Part::Result, 0..3)?;
//! assert_eq!(convert(&pad.line().snapshot())?, "1/(2*x)");
//! assert_eq!(pad.history().len(), 1);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

#![forbid(unsafe_code)]

mod history;
mod reuse;
mod scratchpad;

#[cfg(test)]
mod test_jobs;

pub use history::{Delivery, Destination, Entry, NotAccepted, NotPrepared, Outcome, Pending};
pub use reuse::{NotReused, Part};
pub use scratchpad::Scratchpad;

/// The most entries of the history. Accepting a submission when the
/// history holds this many evicts the oldest entry first.
///
/// There is no separate bound on the bytes of the history, because each
/// entry is bounded already: its input holds at most 4,096 items, its
/// variable at most [`MAX_VARIABLE_BYTES`] bytes, and the output line of its
/// result at most the larger of 65,536 bytes and the `max_output_bytes` of
/// its job's limits, which is 65,536 by default.
pub const MAX_HISTORY_ENTRIES: usize = 32;

/// The most bytes of the variable of a submission. Preparing a submission
/// with a longer variable gives [`NotPrepared::VariableTooLong`].
///
/// A variable is a name such as `x`. The bound is the 4,096 bytes that one
/// insertion into the line holds, as many as the longest line, so any name
/// that can be typed into the line fits.
pub const MAX_VARIABLE_BYTES: usize = 4096;
