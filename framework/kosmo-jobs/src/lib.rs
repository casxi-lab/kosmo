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
//! - two backends: [`stub::HostStub`], which runs jobs on one worker
//!   thread with a runner, and [`controllable::ControllableBackend`],
//!   whose reports tests arrange.
//!
//! The crate depends on no computation library: the kalyx runner of
//! `kosmo-math-kalyx` implements the runner port there, and runs each job's
//! requests with kalyx on the host stub's worker thread. On krepi, a
//! planned backend for krepi's job service will take the place of the host
//! stub, and the rest stays the same.
//!
//! ```
//! use kosmo_jobs::controllable::{ControllableBackend, Plan};
//! use kosmo_jobs::{
//!     EchoedVersions, ErrorClass, JobClock, JobLayer, Operation, OutcomeKind, Request,
//!     RequestReport, Submission, TerminalState,
//! };
//! use kosmo_math_model::version::Versions;
//!
//! struct Still;
//! impl JobClock for Still {
//!     fn now(&mut self) -> u64 { 0 }
//!     fn fork(&self) -> Box<dyn JobClock> { Box::new(Still) }
//! }
//!
//! // The backend echoes the versions 0, 0 and 0 of the request.
//! let versions = Versions::default();
//! let echo = EchoedVersions::new(0, 0, 0);
//! let line = b"value\t0\t0\t0\t1/2\n".to_vec();
//! let report = RequestReport::new(OutcomeKind::Value, ErrorClass::NoError, Some(line.clone()), echo);
//! let mut backend = ControllableBackend::new();
//! backend.plan(0, Plan::new().request(report));
//!
//! let mut layer = JobLayer::new(backend, Box::new(Still));
//! let session = layer.open_session()?;
//! let request = Request::new(Operation::Eval, "", "1/3+1/6");
//! let id = layer.submit(Submission::new(session, versions, request))?;
//! let terminal = layer.wait(id).ok_or("no terminal state")?;
//! assert_eq!(terminal.record().state(), TerminalState::Completed);
//! assert_eq!(terminal.record().lines(), &[line]);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

#![forbid(unsafe_code)]

pub mod controllable;
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
    EchoedVersions, ErrorClass, Finished, OutcomeKind, Report, RequestReport, SerializeFailure,
    StopReason,
};
pub use request::{JobId, JobRequest, Limits, Operation, Rejected, Request, SessionId, Submission};

/// The most live jobs of one job layer: queued, running, in
/// `cancel_requested`, terminal but not taken, or not yet finished by the
/// backend. A submission while this many jobs are live is refused as
/// [`Rejected::Full`].
pub const MAX_LIVE_JOBS: usize = 8;

/// The most requests of one job.
pub const MAX_REQUESTS_PER_JOB: usize = 4;
