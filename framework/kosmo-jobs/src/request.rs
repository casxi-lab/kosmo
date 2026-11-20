//! What a submitter gives the job layer and what a backend receives.

use std::fmt;

use kosmo_math_model::version::Versions;

use crate::MAX_REQUESTS_PER_JOB;

/// The identity of a job, which the job layer assigns when it accepts a
/// submission.
///
/// Identities count up from 1 within one job layer; none is reused and the
/// counter never wraps, so a submission after the last identity is
/// rejected. A rejected submission uses no identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct JobId(u64);

impl JobId {
    /// The identity as a number.
    pub const fn get(self) -> u64 {
        self.0
    }

    pub(crate) const fn new(value: u64) -> JobId {
        JobId(value)
    }
}

impl fmt::Display for JobId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "job {}", self.0)
    }
}

/// A session, which the job layer issues at
/// [`open_session`](crate::JobLayer::open_session).
///
/// A session groups the jobs of one submitter so that
/// [`cancel_session`](crate::SessionCancel::cancel_session) can cancel them
/// together. It is opaque: only the job layer issues one, counting up from
/// 1, and none is reused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SessionId(u64);

impl SessionId {
    /// The session as a number.
    pub const fn get(self) -> u64 {
        self.0
    }

    pub(crate) const fn new(value: u64) -> SessionId {
        SessionId(value)
    }
}

/// The operation of one request: what the computation library does with
/// the linear text.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Operation {
    /// Evaluates the text.
    Eval,
    /// Expands the text as a polynomial in the variable.
    Expand,
    /// Differentiates the text with respect to the variable.
    Diff,
}

impl Operation {
    /// The name of the operation in kalyx's command entry: `eval`, `expand`
    /// or `diff`. No other name can be submitted.
    pub const fn name(self) -> &'static str {
        match self {
            Operation::Eval => "eval",
            Operation::Expand => "expand",
            Operation::Diff => "diff",
        }
    }
}

/// One request of a job: an operation, its variable and the linear text.
///
/// The linear text is what formula conversion produced from a snapshot of
/// the editing structure; the job layer passes it on unchanged, and mapping
/// ranges of it back to the snapshot stays with the submitter. The
/// variable is passed on unchanged too, and the computation library judges
/// it; `eval` does not read it.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Request {
    operation: Operation,
    variable: String,
    input: String,
}

impl Request {
    /// A request of `operation` with `variable` on the linear text `input`.
    pub fn new(
        operation: Operation,
        variable: impl Into<String>,
        input: impl Into<String>,
    ) -> Request {
        Request {
            operation,
            variable: variable.into(),
            input: input.into(),
        }
    }

    /// The operation.
    pub const fn operation(&self) -> Operation {
        self.operation
    }

    /// The variable.
    pub fn variable(&self) -> &str {
        &self.variable
    }

    /// The linear text.
    pub fn input(&self) -> &str {
        &self.input
    }
}

/// The nine limits of every request of a job.
///
/// Each request of a job gets its own copy: the limits of one request are
/// not shared with the next, and nothing scales them by the number of
/// requests. The defaults, [`Limits::DEFAULT`], are the values of
/// `Limits::new()` in kalyx 0.1.0. Change single fields of
/// [`Limits::new`]:
///
/// ```
/// use kosmo_jobs::Limits;
///
/// let mut limits = Limits::new();
/// limits.max_input_bytes = 4;
/// assert_eq!(limits.max_work, 100_000);
/// assert_eq!(Limits::default(), Limits::DEFAULT);
/// ```
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Limits {
    /// The work units that the request may charge. Default 100,000.
    pub max_work: u64,
    /// The bytes of the request's input. Default 65,536.
    pub max_input_bytes: u64,
    /// The bytes of all display and serialization of the request,
    /// cumulative. Default 65,536.
    pub max_output_bytes: u64,
    /// The nesting depth of syntax, terms and values. Default 128.
    pub max_depth: u64,
    /// The nodes retained at one time. Default 8,192.
    pub max_nodes: u64,
    /// The logical storage bytes retained at one time. Default 262,144.
    pub max_storage_bytes: u64,
    /// The bits of one integer. Default 4,096.
    pub max_integer_bits: u64,
    /// The absolute value of one integer exponent; also degrees and orders
    /// of derivatives. Default 4,096.
    pub max_exponent: u64,
    /// The frames open at one time. Default 32.
    pub max_frames: u64,
}

impl Limits {
    /// The default limits, equal to `Limits::new()` of kalyx 0.1.0.
    pub const DEFAULT: Limits = Limits {
        max_work: 100_000,
        max_input_bytes: 65_536,
        max_output_bytes: 65_536,
        max_depth: 128,
        max_nodes: 8_192,
        max_storage_bytes: 262_144,
        max_integer_bits: 4_096,
        max_exponent: 4_096,
        max_frames: 32,
    };

    /// The default limits, [`Limits::DEFAULT`].
    pub const fn new() -> Limits {
        Limits::DEFAULT
    }
}

impl Default for Limits {
    /// The default limits, [`Limits::DEFAULT`].
    fn default() -> Limits {
        Limits::DEFAULT
    }
}

/// What a submitter gives the job layer for one job.
///
/// It holds the session, 1 to [`MAX_REQUESTS_PER_JOB`] requests, the three
/// versions that every request carries, the limits of each request and an
/// optional time limit. The job layer assigns the job identity when it
/// accepts the submission, and turns the time limit into an absolute
/// deadline then, so neither is part of a submission.
///
/// ```
/// use kosmo_jobs::{Operation, Request, SessionId, Submission};
/// use kosmo_math_model::version::Versions;
///
/// fn submission(session: SessionId, versions: Versions) -> Submission {
///     Submission::new(session, versions, Request::new(Operation::Eval, "", "1/3+1/6"))
///         .with_time_limit(1_000_000_000)
/// }
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Submission {
    session: SessionId,
    versions: Versions,
    requests: Vec<Request>,
    limits: Limits,
    time_limit: Option<u64>,
}

impl Submission {
    /// A job of the single request `request`, with the default limits and
    /// no time limit.
    pub fn new(session: SessionId, versions: Versions, request: Request) -> Submission {
        Submission {
            session,
            versions,
            requests: vec![request],
            limits: Limits::DEFAULT,
            time_limit: None,
        }
    }

    /// A job of several requests, run in order, with the default limits and
    /// no time limit; `None` unless there are 1 to
    /// [`MAX_REQUESTS_PER_JOB`] requests.
    pub fn several(
        session: SessionId,
        versions: Versions,
        requests: Vec<Request>,
    ) -> Option<Submission> {
        if requests.is_empty() || requests.len() > MAX_REQUESTS_PER_JOB {
            return None;
        }
        Some(Submission {
            session,
            versions,
            requests,
            limits: Limits::DEFAULT,
            time_limit: None,
        })
    }

    /// This submission with the limits `limits` for each request.
    #[must_use]
    pub fn with_limits(mut self, limits: Limits) -> Submission {
        self.limits = limits;
        self
    }

    /// This submission with a time limit of `nanos` nanoseconds of the job
    /// layer's clock, counted from the acceptance of the job.
    #[must_use]
    pub fn with_time_limit(mut self, nanos: u64) -> Submission {
        self.time_limit = Some(nanos);
        self
    }

    /// The session.
    pub const fn session(&self) -> SessionId {
        self.session
    }

    /// The versions.
    pub const fn versions(&self) -> Versions {
        self.versions
    }

    /// The requests, in order.
    pub fn requests(&self) -> &[Request] {
        &self.requests
    }

    /// The limits of each request.
    pub const fn limits(&self) -> Limits {
        self.limits
    }

    /// The time limit in nanoseconds, if any.
    pub const fn time_limit(&self) -> Option<u64> {
        self.time_limit
    }
}

/// A job as a backend receives it.
///
/// The job layer builds one for every submission it accepts, so its fields
/// are always complete: the job identity, the session, 1 to
/// [`MAX_REQUESTS_PER_JOB`] requests, the three versions, the limits of
/// each request and the job's absolute deadline, a reading of the job
/// layer's clock, if the submission had a time limit. A deadline has passed
/// when a reading of the clock is at least the deadline.
///
/// There is no cancellation field: the host stub hands the runner a
/// [`WorkItem`](crate::WorkItem), which carries the job's
/// [`CancelHandle`](crate::CancelHandle) beside the request.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JobRequest {
    id: JobId,
    session: SessionId,
    requests: Vec<Request>,
    versions: Versions,
    limits: Limits,
    deadline: Option<u64>,
}

impl JobRequest {
    pub(crate) fn accepted(id: JobId, submission: Submission, deadline: Option<u64>) -> JobRequest {
        JobRequest {
            id,
            session: submission.session,
            requests: submission.requests,
            versions: submission.versions,
            limits: submission.limits,
            deadline,
        }
    }

    /// The job identity.
    pub const fn id(&self) -> JobId {
        self.id
    }

    /// The session.
    pub const fn session(&self) -> SessionId {
        self.session
    }

    /// The requests, in order; 1 to [`MAX_REQUESTS_PER_JOB`].
    pub fn requests(&self) -> &[Request] {
        &self.requests
    }

    /// The versions that every request carries.
    pub const fn versions(&self) -> Versions {
        self.versions
    }

    /// The limits of each request.
    pub const fn limits(&self) -> Limits {
        self.limits
    }

    /// The absolute deadline, a reading of the job layer's clock in
    /// nanoseconds, if the job has one.
    pub const fn deadline(&self) -> Option<u64> {
        self.deadline
    }
}

/// Why the job layer refused to accept a submission or to open a session.
///
/// A refusal changes nothing: no job is created, no identity is used,
/// nothing blocks and no job is evicted.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Rejected {
    /// [`MAX_LIVE_JOBS`](crate::MAX_LIVE_JOBS) jobs are live; `live` is
    /// their number.
    Full {
        /// The number of live jobs.
        live: usize,
    },
    /// Every identity has been used.
    Exhausted,
}

impl fmt::Display for Rejected {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Rejected::Full { live } => write!(f, "full: {live} jobs are live"),
            Rejected::Exhausted => f.write_str("every identity has been used"),
        }
    }
}

impl std::error::Error for Rejected {}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> Request {
        Request::new(Operation::Eval, "", "1")
    }

    #[test]
    fn a_job_holds_one_to_four_requests() {
        assert_eq!(MAX_REQUESTS_PER_JOB, 4);
        let session = SessionId::new(1);
        let versions = Versions::default();
        assert_eq!(Submission::several(session, versions, Vec::new()), None);
        for count in 1..=4 {
            let requests = vec![request(); count];
            let submission = Submission::several(session, versions, requests).unwrap();
            assert_eq!(submission.requests().len(), count);
        }
        let five = vec![request(); 5];
        assert_eq!(Submission::several(session, versions, five), None);
    }

    #[test]
    fn the_operations_have_the_names_of_the_command_entry() {
        let names = [Operation::Eval, Operation::Expand, Operation::Diff].map(Operation::name);
        assert_eq!(names, ["eval", "expand", "diff"]);
    }

    #[test]
    fn the_default_limits_are_those_of_kalyx() {
        let limits = Limits::new();
        assert_eq!(
            [
                limits.max_work,
                limits.max_input_bytes,
                limits.max_output_bytes,
                limits.max_depth,
                limits.max_nodes,
                limits.max_storage_bytes,
                limits.max_integer_bits,
                limits.max_exponent,
                limits.max_frames,
            ],
            [
                100_000, 65_536, 65_536, 128, 8_192, 262_144, 4_096, 4_096, 32
            ]
        );
    }

    #[test]
    fn an_accepted_job_keeps_the_submission_and_its_deadline() {
        let submission =
            Submission::new(SessionId::new(2), Versions::default(), request()).with_time_limit(10);
        let job = JobRequest::accepted(JobId::new(5), submission, Some(17));
        assert_eq!(
            (job.id().get(), job.session().get(), job.deadline()),
            (5, 2, Some(17))
        );
        assert_eq!(job.requests(), &[request()]);
        assert_eq!(job.limits(), Limits::DEFAULT);
    }
}
