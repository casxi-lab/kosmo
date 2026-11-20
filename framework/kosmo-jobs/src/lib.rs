//! The job layer of kosmo: computations run as versioned jobs on a
//! backend that can be replaced, and their results come back as output
//! lines with the versions they were computed from.
//!
//! A submitter opens a session and submits [`Submission`]s: 1 to
//! [`MAX_REQUESTS_PER_JOB`] requests of an operation on linear text, the
//! three [`Versions`](kosmo_math_model::version::Versions) of the content
//! they were converted from, the limits of each request and an optional
//! time limit. The job layer accepts at most [`MAX_LIVE_JOBS`] live jobs,
//! hands each accepted job to its backend as a [`JobRequest`], and turns
//! the backend's reports into exactly one terminal state per job, which the
//! submitter takes once. Whether a result is current is then judged with
//! [`judge`](kosmo_math_model::version::judge) against the versions and
//! the job identity of the result.
//!
//! The crate has four parts:
//!
//! - [`machine`], the pure core: the job table, the state machine, the
//!   terminal function, the bounds and session cancellation, as a step
//!   function without threads, clock readings or input and output;
//! - [`JobLayer`], the shell that uses the core on the job-layer thread,
//!   with the API `submit`, `cancel`, `cancel_session`, `poll`, `wait`,
//!   `take` and `shutdown`;
//! - the ports: [`JobBackend`] for backends, [`JobPreparer`] and
//!   [`JobRunner`] for the two halves of a runner of the host stub,
//!   [`JobClock`] for the clock and [`SessionCancel`] for cancelling a
//!   session;
//! - a backend, [`stub::HostStub`], which runs jobs on one worker thread
//!   with a runner.
//!
//! The crate depends on no computation library: a runner that executes
//! requests with kalyx implements the runner port in another crate. On
//! krepi, a backend for krepi's job service replaces the host stub, and the
//! rest stays the same.

#![forbid(unsafe_code)]

mod layer;
pub mod machine;
mod port;
mod report;
mod request;
pub mod stub;

pub use layer::JobLayer;
pub use machine::{CancelAnswer, CancelCount, JobState, Terminal, TerminalRecord, TerminalState};
pub use port::{
    CancelHandle, CancelTarget, JobBackend, JobClock, JobPreparer, JobRunner, RunReport,
    SessionCancel, WorkItem,
};
pub use report::{
    ErrorClass, Finished, OutcomeKind, Report, RequestReport, SerializeFailure, StopReason,
};
pub use request::{JobId, JobRequest, Limits, Operation, Rejected, Request, SessionId, Submission};

/// The most live jobs of one job layer: queued, running, in
/// `cancel_requested`, terminal but not taken, or not yet finished by the
/// backend. A submission while this many jobs are live is refused as
/// [`Rejected::Full`].
pub const MAX_LIVE_JOBS: usize = 8;

/// The most requests of one job.
pub const MAX_REQUESTS_PER_JOB: usize = 4;
