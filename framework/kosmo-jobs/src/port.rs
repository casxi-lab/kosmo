//! The ports of the job layer: the backend, the runner of the host stub,
//! the clock and session cancellation.
//!
//! The ports and their types hold no type of the computation library, of
//! krepi or of threads. Port types are `#[non_exhaustive]` and only grow,
//! and the traits only gain methods with defaults, so a backend that krepi
//! provides later implements the same ports.

use std::any::Any;
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

/// What a [`CancelHandle`] sets: in the kalyx runner, a job's cancel flag.
///
/// The runner that makes a handle chooses the target, and gets it back
/// from the handle in the work item with [`CancelHandle::target`].
pub trait CancelTarget: Any + Send + Sync {
    /// Sets the cancellation. Setting it again changes nothing.
    fn set(&self);
}

/// The opaque cancel handle of one job of the host stub.
///
/// A handle is `Send` and can be cloned, and setting it is its only
/// operation: it cannot be read or cleared. The runner's preparing half
/// makes it, on the job-layer thread, from a [`CancelTarget`] that it
/// chooses; the host stub sets it after the job layer accepted a cancel
/// request; and the runner's running half finds its target again on the
/// worker thread through [`CancelHandle::target`].
#[derive(Clone)]
pub struct CancelHandle {
    target: Arc<dyn CancelTarget>,
}

impl CancelHandle {
    /// A handle that sets `target`.
    pub fn new<T: CancelTarget>(target: T) -> CancelHandle {
        CancelHandle {
            target: Arc::new(target),
        }
    }

    /// Sets the cancellation of the job.
    pub fn set(&self) {
        self.target.set();
    }

    /// The target of the handle, if it is a `T`: the runner that made the
    /// handle gets back what it made it from.
    pub fn target<T: CancelTarget>(&self) -> Option<&T> {
        let target: &dyn Any = &*self.target;
        target.downcast_ref::<T>()
    }
}

impl fmt::Debug for CancelHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CancelHandle")
    }
}

/// What the host stub hands to its runner: the job and its cancel handle.
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
/// job, before it hands the job to its worker thread.
pub trait JobPreparer {
    /// Prepares job `id` and gives its cancel handle. The kalyx runner
    /// makes the job's cancel flag here.
    fn prepare(&mut self, id: JobId) -> CancelHandle;
}

/// The running half of a runner of the host stub, which the stub moves to
/// its worker thread.
///
/// The worker reports `Started` for a job, then calls
/// [`run`](JobRunner::run) and reports `Finished` with what it returns. A
/// runner runs one job at a time. Before each request, the first included,
/// it checks the job's cancellation and then its deadline, a deadline
/// having passed when a reading of its clock is at least the deadline; it
/// runs no further request after either, and returns the stop.
pub trait JobRunner: Send + 'static {
    /// Runs the job of `item`.
    fn run(&mut self, item: WorkItem) -> RunReport;
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
/// job-layer thread: on the host by a stand-in for the system's
/// stop-request port, on the device by krepi's job service.
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

    struct Other;

    impl CancelTarget for Other {
        fn set(&self) {}
    }

    #[test]
    fn a_handle_sets_its_target_and_gives_it_back_to_its_maker() {
        let seen = Arc::new(AtomicBool::new(false));
        let handle = CancelHandle::new(Flag(Arc::clone(&seen)));
        let copy = handle.clone();
        assert!(!seen.load(Ordering::SeqCst));
        copy.set();
        assert!(seen.load(Ordering::SeqCst));
        assert!(handle.target::<Flag>().is_some());
        assert!(handle.target::<Other>().is_none());
    }
}
