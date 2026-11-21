//! The pure core of the job layer: the bounded job table, the state
//! machine, the terminal function, the race between completion and
//! cancellation, the bounds and session cancellation.
//!
//! The core has no threads, channels, clock readings, input or output,
//! interior mutability or types of the computation library. It is a step
//! function: [`Machine::step`] takes one [`Input`] (open a session,
//! submit, cancel, cancel a session, a backend's report, take a terminal
//! state) and returns an [`Output`], the answer to that input with the
//! [`Command`]s that the shell carries out (hand a job to the backend, set
//! a job's cancel handle). The typed methods, such as [`Machine::submit`],
//! take the same steps. The order of the results depends only on the order
//! of the inputs, so tests can enumerate every order without threads.
//!
//! Events are applied one at a time in order of arrival, and the first one
//! applied is accepted first:
//!
//! - An accepted cancel request ends a queued job `cancelled` at once,
//!   and moves a running job to `cancel_requested`. Only then does the
//!   core issue the command that sets the job's cancel handle. Acceptance
//!   is the answer to the cancel request; the terminal state comes later.
//!   A cancellation cannot be withdrawn.
//! - A job in `cancel_requested` that receives its `Finished` report ends
//!   `cancelled`, whatever the report holds; its lines are discarded.
//! - A job whose `Finished` report is handled first ends as
//!   [`terminal`] gives; a later cancel request is answered
//!   [`CancelAnswer::AlreadyTerminal`].
//! - Every job has exactly one terminal state, which is absorbing and is
//!   taken exactly once.
//!
//! ```
//! use kosmo_jobs::machine::{CancelAnswer, Machine};
//! use kosmo_jobs::{Finished, Operation, Report, Request, Submission, TerminalState};
//! use kosmo_math_model::version::Versions;
//!
//! let mut core = Machine::new();
//! let session = core.open_session()?;
//! let request = Request::new(Operation::Eval, "", "1/3+1/6");
//! let submission = Submission::new(session, Versions::default(), request);
//! let (accepted, _commands) = core.submit(submission, None);
//! let id = accepted?;
//!
//! // The job started, then a cancel request was accepted before the
//! // backend's end of the job was handled: the cancellation wins.
//! core.report(Report::Started(id));
//! assert_eq!(core.cancel(id).0, CancelAnswer::Requested);
//! core.report(Report::Finished(Finished::new(id, Vec::new(), None)));
//! let terminal = core.take(id).ok_or("not terminal")?;
//! assert_eq!(terminal.record().state(), TerminalState::Cancelled);
//! assert!(terminal.record().lines().is_empty());
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use std::collections::BTreeMap;

use kosmo_math_model::version::Versions;

use crate::report::RequestEffect;
use crate::{
    EchoedVersions, Finished, JobId, JobRequest, MAX_LIVE_JOBS, Rejected, Report, SessionId,
    StopReason, Submission,
};

/// One input of the core's step function.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Input {
    /// Open a session.
    OpenSession,
    /// Submit a job, with the absolute deadline that the shell computed
    /// from the submission's time limit.
    Submit {
        /// The submission.
        submission: Submission,
        /// The absolute deadline, if the submission has a time limit.
        deadline: Option<u64>,
    },
    /// Cancel one job.
    Cancel(JobId),
    /// Cancel every job of a session that has not reached its terminal
    /// state.
    CancelSession(SessionId),
    /// A backend's report.
    Report(Report),
    /// Take a job's terminal state.
    Take(JobId),
}

/// What one step of the core gives back: the answer to its input and the
/// commands that the shell carries out, in order.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Output {
    /// The answer to the input.
    pub answer: Answer,
    /// The commands for the shell.
    pub commands: Vec<Command>,
}

/// The answer to one input, of the variant that matches the input.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Answer {
    /// The answer to [`Input::OpenSession`].
    Session(Result<SessionId, Rejected>),
    /// The answer to [`Input::Submit`].
    Submitted(Result<JobId, Rejected>),
    /// The answer to [`Input::Cancel`].
    Cancel(CancelAnswer),
    /// The answer to [`Input::CancelSession`].
    SessionCancelled(CancelCount),
    /// The answer to [`Input::Report`].
    Report(ReportEffect),
    /// The answer to [`Input::Take`].
    Taken(Option<Terminal>),
}

/// A command of the core to the shell.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    /// Hand the accepted job to the backend.
    Hand(JobRequest),
    /// Set the cancel handle of the job: in the backend port, call
    /// [`JobBackend::cancel`](crate::JobBackend::cancel).
    SetCancel(JobId),
}

/// The answer to a cancel request.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CancelAnswer {
    /// Accepted: the job had not started as far as the job layer knows, and
    /// is now `cancelled`. A backend that has not started it stops it before
    /// its first request; whatever it reports for the job is discarded.
    Cancelled,
    /// Accepted: the job was running and is now in `cancel_requested`; its
    /// terminal state, `cancelled`, follows its `Finished` report.
    Requested,
    /// The job was already in `cancel_requested`.
    AlreadyRequested,
    /// The job had already reached its terminal state.
    AlreadyTerminal,
    /// No job with this identity was accepted.
    Unknown,
}

/// The cancellations that one session cancellation accepted.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct CancelCount {
    /// Queued jobs that are now `cancelled`.
    pub queued: usize,
    /// Running jobs that are now in `cancel_requested`.
    pub requested: usize,
}

/// What a backend's report did.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ReportEffect {
    /// The job started running.
    Started,
    /// The job reached its terminal state, which waits to be taken.
    Terminal,
    /// The report changed no state: the job had already ended, as a job
    /// cancelled while queued has, or the report was not expected.
    Discarded,
}

/// The state of a job.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum JobState {
    /// Handed to the backend and not yet started.
    Queued,
    /// Started.
    Running,
    /// A cancel request was accepted while the job ran.
    CancelRequested,
    /// Ended in this terminal state.
    Terminal(TerminalState),
}

/// The terminal state of a job.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TerminalState {
    /// Every request ran without an error.
    Completed,
    /// A request failed with a fault or a limit, or the backend or its
    /// reports failed.
    Failed,
    /// The job was cancelled.
    Cancelled,
    /// The job's deadline passed, or the backend stopped it by force.
    ResourceExhausted,
}

/// The terminal record of a job: its terminal state, the stop, the lines
/// that it delivers and the versions that its requests echoed.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalRecord {
    state: TerminalState,
    stop: Option<StopReason>,
    lines: Vec<Vec<u8>>,
    echoed: Option<EchoedVersions>,
    internal_error: bool,
}

impl TerminalRecord {
    /// The terminal state.
    pub const fn state(&self) -> TerminalState {
        self.state
    }

    /// The reason why the job stopped before all its requests ran, if it
    /// did.
    pub const fn stop(&self) -> Option<StopReason> {
        self.stop
    }

    /// The output lines that the job delivers, one for each request that
    /// has a line, in order, each with the bytes of its request report, its
    /// final line feed included. A `cancelled` job delivers none.
    pub fn lines(&self) -> &[Vec<u8>] {
        &self.lines
    }

    /// The lines, taken out of the record.
    pub fn into_lines(self) -> Vec<Vec<u8>> {
        self.lines
    }

    /// The versions that the requests echoed: the numbers of the request's
    /// own versions when every echo matched them, the first echo that
    /// differs otherwise, and `None` when no request ran.
    pub const fn echoed(&self) -> Option<EchoedVersions> {
        self.echoed
    }

    /// Whether the job was stopped by force, for a forced stop or an
    /// exhausted CPU budget. The mark follows from the stop reason.
    pub fn forced(&self) -> bool {
        self.stop.is_some_and(StopReason::forced)
    }

    /// Whether the job failed with an internal error: a backend internal
    /// error, echoed versions that differ from the request's, or reports
    /// that do not fit the request, outside `cancel_requested`.
    pub const fn internal_error(&self) -> bool {
        self.internal_error
    }
}

/// A job's terminal state as the submitter takes it.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Terminal {
    id: JobId,
    session: SessionId,
    versions: Versions,
    record: TerminalRecord,
}

impl Terminal {
    /// The job.
    pub const fn id(&self) -> JobId {
        self.id
    }

    /// The job's session.
    pub const fn session(&self) -> SessionId {
        self.session
    }

    /// The versions of the job's requests.
    pub const fn versions(&self) -> Versions {
        self.versions
    }

    /// The terminal record.
    pub const fn record(&self) -> &TerminalRecord {
        &self.record
    }

    /// The terminal record, taken out.
    pub fn into_record(self) -> TerminalRecord {
        self.record
    }
}

/// Computes the terminal record of a job from its backend's `Finished`
/// report.
///
/// `cancel_requested` tells whether the core had accepted a cancel request
/// while the job ran; `versions` and `requests` are the job's versions and
/// its number of requests. The rules, in order:
///
/// 1. In `cancel_requested` the job is `cancelled`, whatever the report
///    holds, and delivers no line. The record keeps the reported stop, or
///    cancellation when there is none.
/// 2. Otherwise a backend internal error, an echo whose number for any of
///    the three versions differs from that of `versions`, or reports that
///    do not fit the job (more reports than requests, a report after one
///    whose error stopped the job, or fewer reports without a stop) give
///    `failed`, marked as an internal error, without lines.
/// 3. Otherwise the first stop decides: a request whose errors include
///    `Cancelled` or `DeadlineExceeded`, or else the reported stop.
///    Cancellation gives `cancelled` without lines; a passed deadline, a
///    forced stop or an exhausted CPU budget gives `resource_exhausted`
///    with the lines of the requests that ran.
/// 4. Without a stop, a request that failed with a fault or a limit, also
///    while writing its line, gives `failed`, and otherwise the job is
///    `completed`; both deliver the lines of all requests.
///
/// A stop thus takes precedence over earlier faults and limits.
pub fn terminal(
    cancel_requested: bool,
    versions: Versions,
    requests: usize,
    finished: Finished,
) -> TerminalRecord {
    let (reports, reported_stop) = finished.into_parts();
    let mut request_stop = None;
    let mut failed = false;
    let mut misfit = reports.len() > requests;
    let mut differing = None;
    for report in &reports {
        if request_stop.is_some() {
            misfit = true;
        }
        let echo = report.echoed();
        if differing.is_none() && !echoes(echo, versions) {
            differing = Some(echo);
        }
        match report.effect() {
            RequestEffect::Succeeds => {}
            RequestEffect::Fails => failed = true,
            RequestEffect::Stops(reason) => request_stop = request_stop.or(Some(reason)),
        }
    }
    if request_stop.is_none() && reported_stop.is_none() && reports.len() < requests {
        misfit = true;
    }
    let echoed = differing.or_else(|| reports.first().map(|_| EchoedVersions::from(versions)));
    let stop = request_stop.or(reported_stop);
    let record = |state, stop, lines, internal_error| TerminalRecord {
        state,
        stop,
        lines,
        echoed,
        internal_error,
    };
    if cancel_requested {
        return record(
            TerminalState::Cancelled,
            stop.or(Some(StopReason::Cancellation)),
            Vec::new(),
            false,
        );
    }
    if stop == Some(StopReason::BackendInternalError) || differing.is_some() || misfit {
        return record(TerminalState::Failed, stop, Vec::new(), true);
    }
    let lines = || reports.into_iter().filter_map(|r| r.into_line()).collect();
    match stop {
        Some(StopReason::Cancellation) => record(TerminalState::Cancelled, stop, Vec::new(), false),
        Some(_) => record(TerminalState::ResourceExhausted, stop, lines(), false),
        None if failed => record(TerminalState::Failed, None, lines(), false),
        None => record(TerminalState::Completed, None, lines(), false),
    }
}

/// Whether `echo` gives the numbers of `versions`, compared one version at
/// a time.
fn echoes(echo: EchoedVersions, versions: Versions) -> bool {
    echo.source() == versions.source.get()
        && echo.context() == versions.context.get()
        && echo.settings() == versions.settings.get()
}

/// The phase of a job in the table.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Phase {
    Queued,
    Running,
    CancelRequested,
    Terminal(TerminalRecord),
}

/// A live job: queued, running, in `cancel_requested`, terminal but not
/// taken, or not yet finished by the backend.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Entry {
    session: SessionId,
    versions: Versions,
    requests: usize,
    phase: Phase,
    /// The backend returned the job's `Finished`.
    finished: bool,
    /// The submitter took the terminal state.
    taken: bool,
}

/// The pure core: the table of live jobs and the identity counters.
///
/// At most [`MAX_LIVE_JOBS`] jobs are live. A job is live from its
/// acceptance until its terminal state has been taken and its backend has
/// returned its `Finished` report, so a job cancelled while queued stays
/// live until its backend's report arrives. Each live job keeps room for
/// its terminal record, so a terminal state never waits for room and is
/// never lost.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Machine {
    jobs: BTreeMap<JobId, Entry>,
    next_job: Option<u64>,
    next_session: Option<u64>,
}

impl Default for Machine {
    fn default() -> Machine {
        Machine::new()
    }
}

impl Machine {
    /// A core with no job; the first job and session identities are 1.
    pub const fn new() -> Machine {
        Machine {
            jobs: BTreeMap::new(),
            next_job: Some(1),
            next_session: Some(1),
        }
    }

    /// Takes one step: applies `input` and returns the answer and the
    /// commands for the shell.
    pub fn step(&mut self, input: Input) -> Output {
        let (answer, commands) = match input {
            Input::OpenSession => (Answer::Session(self.open_session()), Vec::new()),
            Input::Submit {
                submission,
                deadline,
            } => {
                let (answer, commands) = self.submit(submission, deadline);
                (Answer::Submitted(answer), commands)
            }
            Input::Cancel(id) => {
                let (answer, commands) = self.cancel(id);
                (Answer::Cancel(answer), commands)
            }
            Input::CancelSession(session) => {
                let (count, commands) = self.cancel_session(session);
                (Answer::SessionCancelled(count), commands)
            }
            Input::Report(report) => (Answer::Report(self.report(report)), Vec::new()),
            Input::Take(id) => (Answer::Taken(self.take(id)), Vec::new()),
        };
        Output { answer, commands }
    }

    /// Opens a session, or refuses when every session identity has been
    /// used.
    pub fn open_session(&mut self) -> Result<SessionId, Rejected> {
        let value = self.next_session.ok_or(Rejected::Exhausted)?;
        self.next_session = value.checked_add(1);
        Ok(SessionId::new(value))
    }

    /// Whether a submission would be accepted now: `Ok` unless
    /// [`MAX_LIVE_JOBS`] jobs are live or every job identity has been
    /// used. The shell asks before it reads the clock for the deadline.
    pub fn admits(&self) -> Result<(), Rejected> {
        let live = self.jobs.len();
        if live >= MAX_LIVE_JOBS {
            return Err(Rejected::Full { live });
        }
        match self.next_job {
            Some(_) => Ok(()),
            None => Err(Rejected::Exhausted),
        }
    }

    /// Submits a job with the absolute deadline `deadline`. On acceptance
    /// the job is queued, and the command hands it to the backend in the
    /// same step.
    pub fn submit(
        &mut self,
        submission: Submission,
        deadline: Option<u64>,
    ) -> (Result<JobId, Rejected>, Vec<Command>) {
        if let Err(rejected) = self.admits() {
            return (Err(rejected), Vec::new());
        }
        let Some(value) = self.next_job else {
            return (Err(Rejected::Exhausted), Vec::new());
        };
        self.next_job = value.checked_add(1);
        let id = JobId::new(value);
        self.jobs.insert(
            id,
            Entry {
                session: submission.session(),
                versions: submission.versions(),
                requests: submission.requests().len(),
                phase: Phase::Queued,
                finished: false,
                taken: false,
            },
        );
        let request = JobRequest::accepted(id, submission, deadline);
        (Ok(id), vec![Command::Hand(request)])
    }

    /// Cancels one job, as the module documentation describes.
    pub fn cancel(&mut self, id: JobId) -> (CancelAnswer, Vec<Command>) {
        let Some(entry) = self.jobs.get_mut(&id) else {
            let accepted = self.next_job.is_none_or(|next| id.get() < next);
            let answer = if accepted && id.get() >= 1 {
                CancelAnswer::AlreadyTerminal
            } else {
                CancelAnswer::Unknown
            };
            return (answer, Vec::new());
        };
        let answer = match entry.phase {
            Phase::Queued => {
                entry.phase = Phase::Terminal(TerminalRecord {
                    state: TerminalState::Cancelled,
                    stop: Some(StopReason::Cancellation),
                    lines: Vec::new(),
                    echoed: None,
                    internal_error: false,
                });
                CancelAnswer::Cancelled
            }
            Phase::Running => {
                entry.phase = Phase::CancelRequested;
                CancelAnswer::Requested
            }
            Phase::CancelRequested => return (CancelAnswer::AlreadyRequested, Vec::new()),
            Phase::Terminal(_) => return (CancelAnswer::AlreadyTerminal, Vec::new()),
        };
        (answer, vec![Command::SetCancel(id)])
    }

    /// Cancels every job of `session` that has not reached its terminal
    /// state, in one step: queued jobs end `cancelled`, running jobs move
    /// to `cancel_requested`, and each gets the command that sets its
    /// cancel handle. Jobs in `cancel_requested` or terminal, and the jobs
    /// of other sessions, are unchanged. It returns the cancellations that
    /// it accepted, so a second call returns zero counts.
    pub fn cancel_session(&mut self, session: SessionId) -> (CancelCount, Vec<Command>) {
        let ids: Vec<JobId> = self
            .jobs
            .iter()
            .filter(|(_, entry)| entry.session == session)
            .map(|(id, _)| *id)
            .collect();
        let mut count = CancelCount::default();
        let mut commands = Vec::new();
        for id in ids {
            let (answer, mut more) = self.cancel(id);
            match answer {
                CancelAnswer::Cancelled => count.queued += 1,
                CancelAnswer::Requested => count.requested += 1,
                CancelAnswer::AlreadyRequested
                | CancelAnswer::AlreadyTerminal
                | CancelAnswer::Unknown => {}
            }
            commands.append(&mut more);
        }
        (count, commands)
    }

    /// Applies a backend's report.
    pub fn report(&mut self, report: Report) -> ReportEffect {
        let id = report.id();
        let Some(entry) = self.jobs.get_mut(&id) else {
            return ReportEffect::Discarded;
        };
        match report {
            Report::Started(_) => match entry.phase {
                Phase::Queued if !entry.finished => {
                    entry.phase = Phase::Running;
                    ReportEffect::Started
                }
                _ => ReportEffect::Discarded,
            },
            Report::Finished(finished) => {
                if entry.finished {
                    return ReportEffect::Discarded;
                }
                entry.finished = true;
                let cancel_requested = match entry.phase {
                    Phase::Queued | Phase::Running => false,
                    Phase::CancelRequested => true,
                    Phase::Terminal(_) => {
                        self.forget_if_done(id);
                        return ReportEffect::Discarded;
                    }
                };
                let record = terminal(cancel_requested, entry.versions, entry.requests, finished);
                entry.phase = Phase::Terminal(record);
                ReportEffect::Terminal
            }
        }
    }

    /// Takes the terminal state of job `id`, once: `None` unless the job is
    /// terminal and its state has not been taken.
    pub fn take(&mut self, id: JobId) -> Option<Terminal> {
        let entry = self.jobs.get_mut(&id)?;
        if entry.taken {
            return None;
        }
        let Phase::Terminal(record) = &entry.phase else {
            return None;
        };
        let terminal = Terminal {
            id,
            session: entry.session,
            versions: entry.versions,
            record: record.clone(),
        };
        entry.taken = true;
        self.forget_if_done(id);
        Some(terminal)
    }

    /// The state of job `id`, while it is live.
    pub fn state(&self, id: JobId) -> Option<JobState> {
        self.jobs.get(&id).map(|entry| match &entry.phase {
            Phase::Queued => JobState::Queued,
            Phase::Running => JobState::Running,
            Phase::CancelRequested => JobState::CancelRequested,
            Phase::Terminal(record) => JobState::Terminal(record.state),
        })
    }

    /// The number of live jobs.
    pub fn live(&self) -> usize {
        self.jobs.len()
    }

    /// The jobs whose terminal states wait to be taken, in order of
    /// identity.
    pub fn ready(&self) -> Vec<JobId> {
        self.jobs
            .iter()
            .filter(|(_, entry)| !entry.taken && matches!(entry.phase, Phase::Terminal(_)))
            .map(|(id, _)| *id)
            .collect()
    }

    /// The jobs that have not reached their terminal states, in order of
    /// identity.
    pub fn unfinished(&self) -> Vec<JobId> {
        self.jobs
            .iter()
            .filter(|(_, entry)| !matches!(entry.phase, Phase::Terminal(_)))
            .map(|(id, _)| *id)
            .collect()
    }

    /// Removes job `id` once it is no longer live.
    fn forget_if_done(&mut self, id: JobId) {
        if self
            .jobs
            .get(&id)
            .is_some_and(|entry| entry.taken && entry.finished)
        {
            self.jobs.remove(&id);
        }
    }
}

#[cfg(test)]
mod tests;
