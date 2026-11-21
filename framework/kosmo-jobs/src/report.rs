//! What a backend reports about the jobs it received.

use kosmo_math_model::version::Versions;

use crate::JobId;

/// A report of a backend about one job.
///
/// A backend reports [`Report::Started`] for every job that starts and
/// exactly one [`Report::Finished`] for every job it receives. A job that
/// never starts gets only `Finished`, and the job layer handles it the same
/// way.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Report {
    /// The job started running.
    Started(JobId),
    /// The job ended: the reports of the requests that ran, and the stop
    /// that the backend saw, if any.
    Finished(Finished),
}

impl Report {
    /// The job that the report is about.
    pub const fn id(&self) -> JobId {
        match self {
            Report::Started(id) => *id,
            Report::Finished(finished) => finished.id,
        }
    }
}

/// The end of a job as its backend reports it.
///
/// It holds a report for each request that ran, in order, and the stop
/// that the backend saw: between requests, a cancellation or a passed
/// deadline, before the request that would have run next; in a request,
/// a forced stop or an exhausted CPU budget, after which that request has
/// no report; or a backend internal error. A request whose own error stops
/// the job, a `Cancelled` or `DeadlineExceeded` outcome, is the last report
/// and needs no separate stop.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finished {
    id: JobId,
    requests: Vec<RequestReport>,
    stop: Option<StopReason>,
}

impl Finished {
    /// The end of job `id` with the reports `requests` and the stop
    /// `stop`.
    pub fn new(id: JobId, requests: Vec<RequestReport>, stop: Option<StopReason>) -> Finished {
        Finished { id, requests, stop }
    }

    /// The job.
    pub const fn id(&self) -> JobId {
        self.id
    }

    /// The reports of the requests that ran, in order.
    pub fn requests(&self) -> &[RequestReport] {
        &self.requests
    }

    /// The stop that the backend saw, if any.
    pub const fn stop(&self) -> Option<StopReason> {
        self.stop
    }

    pub(crate) fn into_parts(self) -> (Vec<RequestReport>, Option<StopReason>) {
        (self.requests, self.stop)
    }
}

/// The three versions that the outcome of a request echoed, as numbers:
/// the source, context and settings versions.
///
/// A backend builds the echo from the numbers that its computation gives
/// back, such as the three version numbers of a kalyx `Outcome`, so it can
/// echo any versions, also ones that differ from the request's. The job
/// layer compares each number with the matching version of the job's
/// requests.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct EchoedVersions {
    source: u64,
    context: u64,
    settings: u64,
}

impl EchoedVersions {
    /// The echo of the source version `source`, the context version
    /// `context` and the settings version `settings`.
    pub const fn new(source: u64, context: u64, settings: u64) -> EchoedVersions {
        EchoedVersions {
            source,
            context,
            settings,
        }
    }

    /// The echoed source version.
    pub const fn source(self) -> u64 {
        self.source
    }

    /// The echoed context version.
    pub const fn context(self) -> u64 {
        self.context
    }

    /// The echoed settings version.
    pub const fn settings(self) -> u64 {
        self.settings
    }
}

impl From<Versions> for EchoedVersions {
    /// The echo of `versions` unchanged, as a backend gives it for a
    /// request computed from them.
    fn from(versions: Versions) -> EchoedVersions {
        EchoedVersions::new(
            versions.source.get(),
            versions.context.get(),
            versions.settings.get(),
        )
    }
}

/// The report of one request that ran.
///
/// It holds the kind of the outcome, the class of its error, the bytes of
/// its output line, if it has one, and the three versions that the outcome
/// echoed. The line is the bytes that kalyx's `serialize` wrote for the
/// request, its final line feed included. The job layer neither parses nor
/// changes the line: the submitter reads the value, state or error from it
/// with the display model.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequestReport {
    kind: OutcomeKind,
    error: ErrorClass,
    line: Option<Vec<u8>>,
    echoed: EchoedVersions,
}

impl RequestReport {
    /// A report of an outcome of kind `kind` with the error class `error`,
    /// the line `line` and the echoed versions `echoed`.
    pub fn new(
        kind: OutcomeKind,
        error: ErrorClass,
        line: Option<Vec<u8>>,
        echoed: EchoedVersions,
    ) -> RequestReport {
        RequestReport {
            kind,
            error,
            line,
            echoed,
        }
    }

    /// The kind of the outcome.
    pub const fn kind(&self) -> OutcomeKind {
        self.kind
    }

    /// The class of the request's error.
    pub const fn error(&self) -> ErrorClass {
        self.error
    }

    /// The bytes of the output line as kalyx's `serialize` wrote them, its
    /// final line feed included, if the request has a line.
    pub fn line(&self) -> Option<&[u8]> {
        self.line.as_deref()
    }

    /// The versions that the outcome echoed.
    pub const fn echoed(&self) -> EchoedVersions {
        self.echoed
    }

    pub(crate) fn into_line(self) -> Option<Vec<u8>> {
        self.line
    }

    /// What the request does to its job: it stops it, fails, or neither.
    pub(crate) fn effect(&self) -> RequestEffect {
        let from_kind = match self.kind {
            OutcomeKind::Value | OutcomeKind::State => None,
            OutcomeKind::Fault | OutcomeKind::ResourceLimit => Some(RequestEffect::Fails),
            OutcomeKind::Cancelled => Some(RequestEffect::Stops(StopReason::Cancellation)),
            OutcomeKind::DeadlineExceeded => Some(RequestEffect::Stops(StopReason::Deadline)),
        };
        let from_line = match self.error {
            ErrorClass::NoError | ErrorClass::Outcome => None,
            ErrorClass::Serialize(SerializeFailure::ResourceLimit) => Some(RequestEffect::Fails),
            ErrorClass::Serialize(SerializeFailure::Cancelled) => {
                Some(RequestEffect::Stops(StopReason::Cancellation))
            }
            ErrorClass::Serialize(SerializeFailure::DeadlineExceeded) => {
                Some(RequestEffect::Stops(StopReason::Deadline))
            }
        };
        let errors = [from_kind, from_line];
        if errors.contains(&Some(RequestEffect::Stops(StopReason::Cancellation))) {
            RequestEffect::Stops(StopReason::Cancellation)
        } else if errors.contains(&Some(RequestEffect::Stops(StopReason::Deadline))) {
            RequestEffect::Stops(StopReason::Deadline)
        } else if errors.contains(&Some(RequestEffect::Fails)) {
            RequestEffect::Fails
        } else {
            RequestEffect::Succeeds
        }
    }
}

/// What one request does to its job.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RequestEffect {
    /// The request has no error.
    Succeeds,
    /// The request failed with a fault or a limit; the job goes on.
    Fails,
    /// The request's error stops the job.
    Stops(StopReason),
}

/// The kind of a request's outcome, as kalyx's command entry gives it.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OutcomeKind {
    /// An exact value. The request has no error.
    Value,
    /// A mathematical state, such as an iteration limit. The request has
    /// no error.
    State,
    /// A fault: invalid or unsupported input, a domain error or a defect.
    /// The request fails and the job goes on.
    Fault,
    /// A limit of the request was exceeded. The request fails and the job
    /// goes on: a larger limit lifts it.
    ResourceLimit,
    /// The request stopped at a charge point that saw the job's
    /// cancellation. It stops the job.
    Cancelled,
    /// The request stopped at a charge point that saw the job's deadline
    /// pass. It stops the job.
    DeadlineExceeded,
}

/// The class of a request's error: whether writing its output line failed
/// under the request's own context, and how.
///
/// The error of a request is the error of its outcome, given by the
/// [`OutcomeKind`], together with the error of writing its line, if that
/// failed. A request whose errors include `Cancelled` stops the job as
/// cancelled; otherwise one whose errors include `DeadlineExceeded` stops
/// it as past its deadline; otherwise a request with a fault or a
/// `ResourceLimit` fails, and the job goes on.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ErrorClass {
    /// The outcome is a value or a state, and its line was written.
    NoError,
    /// The outcome is an error of its [`OutcomeKind`], and its error line
    /// was written under the request's context.
    Outcome,
    /// Writing the line under the request's context failed with this
    /// failure. A value or a state is not written again, so the request
    /// has no line. The error line of an error outcome is written again as
    /// the fallback line, under the default limits, a fresh cancel flag and
    /// no deadline; the request has no line only when that failed too.
    Serialize(SerializeFailure),
}

/// How writing an output line failed under the request's context.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SerializeFailure {
    /// Writing the line exceeded a limit of the request: the line did not
    /// fit the `max_output_bytes` or the `max_work` that the request had
    /// left, or the display of a term exceeded a limit such as
    /// `max_integer_bits`.
    ResourceLimit,
    /// A charge point of the writing saw the cancellation.
    Cancelled,
    /// A charge point of the writing saw the deadline pass.
    DeadlineExceeded,
}

/// Why a job stopped before all its requests ran.
///
/// A forced stop and an exhausted CPU budget come only from a backend that
/// can stop a computation by force, such as the planned backend for krepi's
/// job service; the host stub produces neither. Both mark the terminal
/// record as forced.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StopReason {
    /// The job was cancelled.
    Cancellation,
    /// The job's deadline passed.
    Deadline,
    /// The backend stopped the job by force after the grace period that
    /// followed its cancellation or its passed deadline.
    ForcedStop,
    /// The backend stopped the job by force because its CPU budget ran
    /// out.
    CpuBudget,
    /// The backend failed: in the host stub, the worker thread ended before
    /// it reported the job's end, or the job was submitted after the
    /// worker thread ended.
    BackendInternalError,
}

impl StopReason {
    /// Whether the stop was by force: a forced stop or an exhausted CPU
    /// budget.
    pub const fn forced(self) -> bool {
        matches!(self, StopReason::ForcedStop | StopReason::CpuBudget)
    }
}

#[cfg(test)]
mod tests {
    use kosmo_math_model::version::{ContextVersion, SettingsVersion};

    use super::*;

    fn report(kind: OutcomeKind, error: ErrorClass) -> RequestReport {
        RequestReport::new(kind, error, None, EchoedVersions::new(0, 0, 0))
    }

    #[test]
    fn an_echo_keeps_its_three_numbers() {
        let echo = EchoedVersions::new(1, 2, 3);
        assert_eq!((echo.source(), echo.context(), echo.settings()), (1, 2, 3));
        let versions = Versions {
            context: ContextVersion::new(2),
            settings: SettingsVersion::new(3),
            ..Versions::default()
        };
        assert_eq!(EchoedVersions::from(versions), EchoedVersions::new(0, 2, 3));
    }

    #[test]
    fn a_report_gives_back_its_line_bytes_unchanged() {
        let bytes = b"value\t1\t2\t3\t1/2\n".to_vec();
        let echo = EchoedVersions::new(1, 2, 3);
        let written = RequestReport::new(
            OutcomeKind::Value,
            ErrorClass::NoError,
            Some(bytes.clone()),
            echo,
        );
        assert_eq!(written.line(), Some(bytes.as_slice()));
        assert_eq!(written.echoed(), echo);
        assert_eq!(written.into_line(), Some(bytes));
        assert_eq!(report(OutcomeKind::Value, ErrorClass::NoError).line(), None);
    }

    #[test]
    fn values_and_states_have_no_error_unless_their_line_failed() {
        for kind in [OutcomeKind::Value, OutcomeKind::State] {
            assert_eq!(
                report(kind, ErrorClass::NoError).effect(),
                RequestEffect::Succeeds
            );
            assert_eq!(
                report(kind, ErrorClass::Serialize(SerializeFailure::ResourceLimit)).effect(),
                RequestEffect::Fails
            );
            assert_eq!(
                report(
                    kind,
                    ErrorClass::Serialize(SerializeFailure::DeadlineExceeded)
                )
                .effect(),
                RequestEffect::Stops(StopReason::Deadline)
            );
        }
    }

    #[test]
    fn faults_and_limits_fail_and_cancellation_comes_before_the_deadline() {
        assert_eq!(
            report(OutcomeKind::Fault, ErrorClass::Outcome).effect(),
            RequestEffect::Fails
        );
        assert_eq!(
            report(OutcomeKind::ResourceLimit, ErrorClass::Outcome).effect(),
            RequestEffect::Fails
        );
        assert_eq!(
            report(
                OutcomeKind::ResourceLimit,
                ErrorClass::Serialize(SerializeFailure::ResourceLimit)
            )
            .effect(),
            RequestEffect::Fails
        );
        assert_eq!(
            report(
                OutcomeKind::DeadlineExceeded,
                ErrorClass::Serialize(SerializeFailure::DeadlineExceeded)
            )
            .effect(),
            RequestEffect::Stops(StopReason::Deadline)
        );
        assert_eq!(
            report(
                OutcomeKind::DeadlineExceeded,
                ErrorClass::Serialize(SerializeFailure::Cancelled)
            )
            .effect(),
            RequestEffect::Stops(StopReason::Cancellation)
        );
        assert_eq!(
            report(
                OutcomeKind::Fault,
                ErrorClass::Serialize(SerializeFailure::DeadlineExceeded)
            )
            .effect(),
            RequestEffect::Stops(StopReason::Deadline)
        );
    }

    #[test]
    fn the_kind_of_an_outcome_stops_the_job_whose_line_was_written() {
        assert_eq!(
            report(OutcomeKind::DeadlineExceeded, ErrorClass::Outcome).effect(),
            RequestEffect::Stops(StopReason::Deadline)
        );
        assert_eq!(
            report(OutcomeKind::Cancelled, ErrorClass::Outcome).effect(),
            RequestEffect::Stops(StopReason::Cancellation)
        );
    }

    #[test]
    fn only_forced_stops_and_cpu_budgets_are_forced() {
        let forced = [
            StopReason::Cancellation,
            StopReason::Deadline,
            StopReason::ForcedStop,
            StopReason::CpuBudget,
            StopReason::BackendInternalError,
        ]
        .map(StopReason::forced);
        assert_eq!(forced, [false, false, true, true, false]);
    }
}
