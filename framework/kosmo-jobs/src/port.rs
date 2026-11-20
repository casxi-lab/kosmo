//! The ports of the job layer: the backend, the runner of the host stub,
//! the clock and session cancellation.
//!
//! The ports and their types hold no type of the computation library, of
//! krepi or of threads. Port types are `#[non_exhaustive]` and only grow,
//! and the traits only gain methods with defaults, so a backend that krepi
//! provides later implements the same ports.

use std::fmt;
use std::sync::Arc;

use crate::machine::CancelCount;
use crate::{JobId, JobRequest, Report, RequestReport, SessionId, StopReason};

/// The backend port: where the job layer hands its jobs and from where
/// their reports come.
///
/// The job layer calls a backend only through this port. A backend reports
/// [`Report::Started`] for every job that starts and exactly one
/// [`Report::Finished`] for every job it receives, in any order across
/// jobs; the job layer assumes neither an order nor a degree of
/// parallelism. Implementations: [`HostStub`](crate::stub::HostStub), which
/// runs jobs on one worker thread, and
/// [`ControllableBackend`](crate::controllable::ControllableBackend), whose
/// reports tests arrange.
pub trait JobBackend {
    /// Receives a job that the job layer accepted.
    fn submit(&mut self, request: JobRequest);

    /// The job layer accepted a cancel request for job `id`, which the
    /// backend received earlier: it sets the job's cancellation, so that a
    /// job that has not started stops before its first request and a
    /// running one at its next check. The job still gets its `Finished`.
    fn cancel(&mut self, id: JobId);

    /// The next report, if one has arrived, without waiting.
    fn try_report(&mut self) -> Option<Report>;

    /// The next report, waiting for it while a job that the backend
    /// received has not finished; `None` when no report can arrive.
    fn wait_report(&mut self) -> Option<Report>;

    /// Stops the backend. The job layer has cancelled every job that has
    /// not ended before it calls this. When it returns, the backend uses no
    /// thread or other resource of its own, and
    /// [`try_report`](JobBackend::try_report) gives every report still to
    /// come, a `Finished` for each job included.
    fn shutdown(&mut self);
}

/// The monotonic clock of the job layer, in nanoseconds.
///
/// Readings never decrease. A fork reads the same clock: the job layer
/// reads its clock once when it accepts a submission with a time limit,
/// and a runner on the worker thread holds a fork. Tests inject a fake
/// clock that they advance themselves. The shape is that of kalyx's
/// `Clock`, so an adapter can pass a fork into a computation.
pub trait JobClock: Send {
    /// The current reading, in nanoseconds.
    fn now(&mut self) -> u64;

    /// A clock that reads the same time.
    fn fork(&self) -> Box<dyn JobClock>;
}

/// What a [`CancelHandle`] sets: the cancellation that a runner keeps for
/// one job, such as the job's cancel flag in a runner that uses kalyx.
pub trait CancelTarget: Send + Sync {
    /// Sets the cancellation. Setting it again changes nothing.
    fn set(&self);
}

/// The opaque cancel handle of one job of the host stub.
///
/// A handle is `Send` and can be cloned, and setting it is its only
/// operation: nothing can read or clear a cancellation through it. The
/// runner's preparing half makes it on the job-layer thread from a
/// [`CancelTarget`] of its choice, and the host stub sets it after the job
/// layer accepted a cancel request. The runner reads the cancellation only
/// from its own value for the job, the [`JobPreparer::Token`] that the
/// preparing half made with the handle.
#[derive(Clone)]
pub struct CancelHandle {
    target: Arc<dyn CancelTarget>,
}

impl CancelHandle {
    /// A handle that sets `target`.
    pub fn new<T: CancelTarget + 'static>(target: T) -> CancelHandle {
        CancelHandle {
            target: Arc::new(target),
        }
    }

    /// Sets the cancellation of the job.
    pub fn set(&self) {
        self.target.set();
    }
}

impl fmt::Debug for CancelHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CancelHandle")
    }
}

/// What the host stub hands to its runner: the job and its cancel handle.
///
/// The runner's own value for the job, its [`JobPreparer::Token`], comes
/// beside the work item, as the second argument of [`JobRunner::run`].
#[non_exhaustive]
#[derive(Debug)]
pub struct WorkItem {
    request: JobRequest,
    cancel: CancelHandle,
}

impl WorkItem {
    pub(crate) fn new(request: JobRequest, cancel: CancelHandle) -> WorkItem {
        WorkItem { request, cancel }
    }

    /// The job.
    pub const fn request(&self) -> &JobRequest {
        &self.request
    }

    /// The job's cancel handle, which the runner's preparing half made.
    pub const fn cancel_handle(&self) -> &CancelHandle {
        &self.cancel
    }

    /// The job and its cancel handle.
    pub fn into_parts(self) -> (JobRequest, CancelHandle) {
        (self.request, self.cancel)
    }
}

/// The preparing half of a runner of the host stub, which stays on the
/// job-layer thread.
///
/// The host stub calls [`prepare`](JobPreparer::prepare) when it receives a
/// job, before it hands the job to its worker thread. The preparing half
/// keeps nothing for the job: the host stub keeps the cancel handle until
/// the job's `Finished` arrives and then drops it, and moves the token,
/// untouched, to the running half with the job. A runner that uses kalyx,
/// for example, makes the job's cancel flag here, puts one clone of it in
/// the handle's target and gives another as the token.
pub trait JobPreparer {
    /// The runner's own value for one job, which only its running half
    /// reads: the job's cancellation that the handle sets, and anything
    /// else the runner prepares for the job.
    type Token: Send + 'static;

    /// Prepares job `id`: its cancel handle, which the host stub keeps and
    /// sets, and the token that the running half gets with the job.
    fn prepare(&mut self, id: JobId) -> (CancelHandle, Self::Token);
}

/// The running half of a runner of the host stub, which the stub moves to
/// its worker thread; `T` is the [`JobPreparer::Token`] of the preparing
/// half.
///
/// The worker reports `Started` for a job, then calls
/// [`run`](JobRunner::run) and reports `Finished` with what it returns. A
/// runner runs one job at a time. Before each request, the first included,
/// it checks the job's cancellation, through the token, and then its
/// deadline, a deadline having passed when a reading of its clock is at
/// least the deadline; it runs no further request after either, and returns
/// the stop.
pub trait JobRunner<T>: Send + 'static {
    /// Runs the job of `item` with `token`, the value that the preparing
    /// half made for the job.
    fn run(&mut self, item: WorkItem, token: T) -> RunReport;
}

/// What a runner returns for a job: the reports of the requests that ran
/// and the stop it saw, which the host stub reports as `Finished`.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunReport {
    requests: Vec<RequestReport>,
    stop: Option<StopReason>,
}

impl RunReport {
    /// The reports `requests` and the stop `stop`.
    pub fn new(requests: Vec<RequestReport>, stop: Option<StopReason>) -> RunReport {
        RunReport { requests, stop }
    }

    pub(crate) fn into_parts(self) -> (Vec<RequestReport>, Option<StopReason>) {
        (self.requests, self.stop)
    }
}

/// The port that cancels every job of a session, called only on the
/// job-layer thread. [`JobLayer`](crate::JobLayer) implements it. Its
/// planned callers are a stand-in for the system's stop-request port on
/// the host and krepi's job service on the device.
pub trait SessionCancel {
    /// Cancels every job of `session` that has not reached its terminal
    /// state: queued jobs end `cancelled`, running jobs move to
    /// `cancel_requested`, and each job's cancellation is set. It returns
    /// the two counts that this call accepted, does not wait for terminal
    /// states, and a second call accepts nothing.
    fn cancel_session(&mut self, session: SessionId) -> CancelCount;
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::*;

    struct Flag(Arc<AtomicBool>);

    impl CancelTarget for Flag {
        fn set(&self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    #[test]
    fn every_clone_of_a_handle_sets_the_same_target() {
        let seen = Arc::new(AtomicBool::new(false));
        let handle = CancelHandle::new(Flag(Arc::clone(&seen)));
        let copy = handle.clone();
        assert!(!seen.load(Ordering::SeqCst));
        copy.set();
        assert!(seen.load(Ordering::SeqCst));
        handle.set();
        assert!(seen.load(Ordering::SeqCst));
        drop((handle, copy));
        assert_eq!(Arc::strong_count(&seen), 1, "the handles held the target");
    }
}
