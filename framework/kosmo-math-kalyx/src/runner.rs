//! The kalyx runner: the runner of the job layer's host stub that executes
//! the requests of a job with kalyx 0.1.0.

use kalyx::command::{Settings, serialize};
use kalyx::context::{CancelFlag, Clock};
use kalyx::{Context, Outcome, Settled, execute};
use kosmo_jobs::{
    CancelHandle, CancelTarget, EchoedVersions, JobClock, JobId, JobPreparer, JobRunner, Limits,
    OutcomeKind, Request, RequestReport, RunReport, SerializeFailure, StopReason, WorkItem,
};
use kosmo_math_model::version::Versions;

/// The preparing half of the kalyx runner, which stays with the host stub
/// on the job-layer thread.
///
/// For each job it makes a new kalyx `CancelFlag`. The job's cancel handle
/// sets the flag, and the token, a [`JobFlag`], carries a clone of it to the
/// running half. The preparing half keeps nothing.
#[derive(Clone, Copy, Debug, Default)]
pub struct KalyxPreparer;

impl KalyxPreparer {
    /// The preparing half.
    pub const fn new() -> KalyxPreparer {
        KalyxPreparer
    }
}

impl JobPreparer for KalyxPreparer {
    type Token = JobFlag;

    fn prepare(&mut self, _id: JobId) -> (CancelHandle, JobFlag) {
        let flag = CancelFlag::new();
        (CancelHandle::new(FlagTarget(flag.clone())), JobFlag(flag))
    }
}

/// The token of the kalyx runner for one job: the job's kalyx
/// `CancelFlag`, which the job's cancel handle sets and which only the
/// running half reads.
#[derive(Debug)]
pub struct JobFlag(CancelFlag);

/// The target of a job's cancel handle: the job's flag.
struct FlagTarget(CancelFlag);

impl CancelTarget for FlagTarget {
    fn set(&self) {
        self.0.cancel();
    }
}

/// The running half of the kalyx runner, which the host stub moves to its
/// worker thread.
///
/// It runs the requests of a job in order. Before each request, the first
/// included, it checks the job's flag and then, when the job has a
/// deadline, whether a reading of its clock is at least the deadline; it
/// stops the job there with [`StopReason::Cancellation`] or
/// [`StopReason::Deadline`] and runs no further request.
///
/// Each request runs on a kalyx `Request` built from the job: its operation
/// by name, its variable and linear text unchanged, the three versions of
/// the job and the default `Settings`. Its context is
/// `Context::with_observers(limits, clock, flag)`, with the job's limits, a
/// fork of the runner's clock and a clone of the job's flag, and with the
/// job's deadline when it has one. After the request the runner writes the
/// outcome's line with `serialize` under the same context. When that fails
/// for an error outcome, it writes the line again, the fallback line, under
/// `Context::with_observers(Limits::new(), clock, flag)` with a fork of its
/// clock, a new flag and no deadline; a value or a state is not written
/// again. No context of the runner reads a real clock.
///
/// The report of a request holds the kind of its outcome, the class of the
/// error of writing its line, its line and the three version numbers that
/// the outcome echoed, which the job layer compares with the job's
/// versions. A request whose outcome or line stopped at the cancellation or
/// at the deadline is the last one: the job stops there, and the report
/// carries the stop.
#[derive(Debug)]
pub struct KalyxRunner {
    clock: ClockFork,
}

impl KalyxRunner {
    /// The running half, reading `clock`.
    ///
    /// `clock` is a fork of the clock that the job layer was built with: the
    /// job layer computes each deadline from a reading of its clock, and the
    /// runner and every context it builds compare readings of forks of
    /// `clock` with that deadline. Nothing checks that the two clocks read
    /// the same time; a runner given another clock compares its readings
    /// with deadlines of a different clock.
    pub fn new(clock: Box<dyn JobClock>) -> KalyxRunner {
        KalyxRunner {
            clock: ClockFork(clock),
        }
    }
}

impl JobRunner<JobFlag> for KalyxRunner {
    fn run(&mut self, item: WorkItem, token: JobFlag) -> RunReport {
        let job = item.request();
        let limits = kalyx_limits(job.limits());
        let mut reports = Vec::new();
        for request in job.requests() {
            if token.0.is_cancelled() {
                return RunReport::new(reports, Some(StopReason::Cancellation));
            }
            if let Some(deadline) = job.deadline()
                && self.clock.0.now() >= deadline
            {
                return RunReport::new(reports, Some(StopReason::Deadline));
            }
            let ran = run_request(
                request,
                job.versions(),
                limits,
                job.deadline(),
                &self.clock,
                &token.0,
            );
            let stops = ran.stops;
            reports.push(ran.report);
            if stops {
                break;
            }
        }
        RunReport::new(reports, None)
    }
}

/// The runner's clock, a fork of the job layer's clock.
struct ClockFork(Box<dyn JobClock>);

impl std::fmt::Debug for ClockFork {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ClockFork")
    }
}

impl ClockFork {
    /// A kalyx clock on a new fork of this clock, for one context.
    fn for_context(&self) -> Box<dyn Clock> {
        Box::new(KalyxClock(self.0.fork()))
    }
}

/// The adapter that gives a kalyx context the job layer's clock: a fork of
/// the clock that the job layer was built with, read as kalyx's `Clock`.
///
/// Its readings are those of that clock, in nanoseconds, so a deadline that
/// the job layer computed is a reading of this clock too. Its `fork` is a
/// fork of the same clock.
struct KalyxClock(Box<dyn JobClock>);

impl Clock for KalyxClock {
    fn now(&mut self) -> u64 {
        self.0.now()
    }

    fn fork(&self) -> Box<dyn Clock> {
        Box::new(KalyxClock(self.0.fork()))
    }
}

/// What running one request gave: its report, and whether it stops the
/// job.
struct Ran {
    report: RequestReport,
    stops: bool,
}

/// Runs `request` of a job with the versions `versions`, the limits
/// `limits` and the deadline `deadline`, on forks of `clock` and with the
/// job's flag `flag`.
fn run_request(
    request: &Request,
    versions: Versions,
    limits: kalyx::Limits,
    deadline: Option<u64>,
    clock: &ClockFork,
    flag: &CancelFlag,
) -> Ran {
    let mut context = Context::with_observers(limits, clock.for_context(), Box::new(flag.clone()));
    if let Some(deadline) = deadline {
        context = context.with_deadline(deadline);
    }
    let outcome = execute(&kalyx_request(request, versions), &mut context);
    report(&outcome, &mut context, clock)
}

/// The report of `outcome`, whose line is written under `context`, the
/// context of its request, or else as the fallback line on a fork of
/// `clock`.
fn report(outcome: &Outcome, context: &mut Context, clock: &ClockFork) -> Ran {
    let kind = outcome_kind(outcome);
    let echoed = EchoedVersions::new(
        outcome.input_version(),
        outcome.context_version(),
        outcome.settings_version(),
    );
    // The class of the job layer, not kalyx's own `ErrorClass` of an error.
    let (line, error) = match serialize(outcome, context) {
        Ok(line) => {
            let error = match outcome.result() {
                Ok(_) => kosmo_jobs::ErrorClass::NoError,
                Err(_) => kosmo_jobs::ErrorClass::Outcome,
            };
            (Some(line), error)
        }
        Err(failure) => {
            let line = match outcome.result() {
                Err(_) => fallback_line(outcome, clock),
                Ok(_) => None,
            };
            let failure = serialize_failure(failure.kind());
            (line, kosmo_jobs::ErrorClass::Serialize(failure))
        }
    };
    let stops = matches!(kind, OutcomeKind::Cancelled | OutcomeKind::DeadlineExceeded)
        || matches!(
            error,
            kosmo_jobs::ErrorClass::Serialize(
                SerializeFailure::Cancelled | SerializeFailure::DeadlineExceeded
            )
        );
    Ran {
        report: RequestReport::new(kind, error, line, echoed),
        stops,
    }
}

/// The fallback line of an error outcome: its line under the default
/// limits, a fork of `clock`, a new flag and no deadline, or `None` when
/// even that fails.
fn fallback_line(outcome: &Outcome, clock: &ClockFork) -> Option<Vec<u8>> {
    let mut fresh = Context::with_observers(
        kalyx::Limits::new(),
        clock.for_context(),
        Box::new(CancelFlag::new()),
    );
    serialize(outcome, &mut fresh).ok()
}

/// The kind of the outcome of a request.
fn outcome_kind(outcome: &Outcome) -> OutcomeKind {
    match outcome.result() {
        Ok(Settled::Value(_)) => OutcomeKind::Value,
        // A state; kalyx 0.1.0 settles in nothing else.
        Ok(_) => OutcomeKind::State,
        Err(error) => match error.kind() {
            kalyx::ErrorKind::ResourceLimit => OutcomeKind::ResourceLimit,
            kalyx::ErrorKind::Cancelled => OutcomeKind::Cancelled,
            kalyx::ErrorKind::DeadlineExceeded => OutcomeKind::DeadlineExceeded,
            // Every other error of kalyx is a fault.
            _ => OutcomeKind::Fault,
        },
    }
}

/// How writing a line failed, from the kind of the error of `serialize`.
fn serialize_failure(kind: kalyx::ErrorKind) -> SerializeFailure {
    match kind {
        kalyx::ErrorKind::ResourceLimit => SerializeFailure::ResourceLimit,
        kalyx::ErrorKind::Cancelled => SerializeFailure::Cancelled,
        kalyx::ErrorKind::DeadlineExceeded => SerializeFailure::DeadlineExceeded,
        // A fault, such as an internal error in the display of a term.
        _ => SerializeFailure::Fault,
    }
}

/// The kalyx request of `request` with the versions `versions`.
fn kalyx_request(request: &Request, versions: Versions) -> kalyx::Request {
    kalyx::Request::new(
        request.operation().name(),
        request.variable(),
        request.input(),
    )
    .with_input_version(versions.source.get())
    .with_context_version(versions.context.get())
    .with_settings(Settings::new())
    .with_settings_version(versions.settings.get())
}

/// The kalyx limits of `limits`, field by field.
fn kalyx_limits(limits: Limits) -> kalyx::Limits {
    kalyx::Limits::new()
        .with_max_work(limits.max_work)
        .with_max_input_bytes(limits.max_input_bytes)
        .with_max_output_bytes(limits.max_output_bytes)
        .with_max_depth(limits.max_depth)
        .with_max_nodes(limits.max_nodes)
        .with_max_storage_bytes(limits.max_storage_bytes)
        .with_max_integer_bits(limits.max_integer_bits)
        .with_max_exponent(limits.max_exponent)
        .with_max_frames(limits.max_frames)
}

#[cfg(test)]
mod tests;
