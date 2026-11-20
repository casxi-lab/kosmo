//! The host stub: the backend of the job layer on a host, which runs jobs
//! on one worker thread.
//!
//! The stub creates its worker thread when it is built, with a fixed name,
//! [`WORKER_NAME`], and a stack of [`WORKER_STACK_SIZE`] bytes. Jobs reach
//! the worker through a dispatch channel of [`MAX_LIVE_JOBS`] places, and
//! reports return through a channel of `2 * MAX_LIVE_JOBS` places. Every
//! job in the dispatch channel is live, and a live job sends at most two
//! reports and stays live until its `Finished` arrives, so neither channel
//! ever blocks.
//!
//! The worker takes one job at a time, in the order the stub received
//! them: it reports `Started`, hands the job to the runner and reports the
//! runner's result as `Finished`. It takes its next job by itself; the job
//! layer receives the reports when it polls or waits. A job whose
//! cancellation was set before the worker took it still starts, and its
//! runner stops it before its first request.
//!
//! If the worker thread ends before it reports a job's `Finished`, by a
//! panic of the runner for example, a guard reports for that job a
//! `Finished` with no request reports and the stop
//! [`StopReason::BackendInternalError`]. Once the report channel is closed,
//! the stub reports the same for every job that it handed to the worker
//! and that has not finished, and for every job submitted afterwards. The
//! job layer has to be rebuilt to run jobs again.
//!
//! Shutting the stub down, also when it is dropped, closes the dispatch
//! channel and joins the worker, which first hands each remaining job to
//! the runner; a panic of the worker is not raised again. Afterwards
//! [`HostStub::live_threads`] is 0.
//!
//! A runner has two halves. On the job-layer thread, the preparing half
//! makes each job's cancel handle from a target of its choice, and a token,
//! its own value for the job. The stub keeps the handle, which it can only
//! set, and moves the token with the job to the running half on the worker
//! thread, which reads the cancellation from it:
//!
//! ```
//! use std::sync::Arc;
//! use std::sync::atomic::{AtomicBool, Ordering};
//!
//! use kosmo_jobs::stub::HostStub;
//! use kosmo_jobs::{
//!     CancelHandle, CancelTarget, JobClock, JobId, JobLayer, JobPreparer, JobRunner, Operation,
//!     Request, RunReport, StopReason, Submission, TerminalState, WorkItem,
//! };
//! use kosmo_math_model::version::Versions;
//!
//! struct Flag(Arc<AtomicBool>);
//! impl CancelTarget for Flag {
//!     fn set(&self) { self.0.store(true, Ordering::SeqCst) }
//! }
//!
//! struct Prepare;
//! impl JobPreparer for Prepare {
//!     type Token = Arc<AtomicBool>;
//!     fn prepare(&mut self, _id: JobId) -> (CancelHandle, Arc<AtomicBool>) {
//!         let flag = Arc::new(AtomicBool::new(false));
//!         (CancelHandle::new(Flag(Arc::clone(&flag))), flag)
//!     }
//! }
//!
//! // A runner that runs no request: it stops each job for its
//! // cancellation, or reports that the job's deadline passed.
//! struct Run;
//! impl JobRunner<Arc<AtomicBool>> for Run {
//!     fn run(&mut self, _item: WorkItem, flag: Arc<AtomicBool>) -> RunReport {
//!         let stop = if flag.load(Ordering::SeqCst) {
//!             StopReason::Cancellation
//!         } else {
//!             StopReason::Deadline
//!         };
//!         RunReport::new(Vec::new(), Some(stop))
//!     }
//! }
//!
//! struct Still;
//! impl JobClock for Still {
//!     fn now(&mut self) -> u64 { 0 }
//!     fn fork(&self) -> Box<dyn JobClock> { Box::new(Still) }
//! }
//!
//! let mut layer = JobLayer::new(HostStub::new(Prepare, Run)?, Box::new(Still));
//! let session = layer.open_session()?;
//! let request = Request::new(Operation::Eval, "", "1/3");
//! let id = layer.submit(Submission::new(session, Versions::default(), request))?;
//! let terminal = layer.wait(id).ok_or("no terminal state")?;
//! assert_eq!(terminal.record().state(), TerminalState::ResourceExhausted);
//! layer.shutdown();
//! assert_eq!(layer.backend().live_threads(), 0);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use std::collections::{BTreeMap, VecDeque};
use std::fmt;
use std::sync::mpsc::{Receiver, SyncSender, TryRecvError, sync_channel};
use std::thread::{Builder, JoinHandle};

use crate::port::WorkItem;
use crate::{
    CancelHandle, Finished, JobBackend, JobId, JobPreparer, JobRequest, JobRunner, MAX_LIVE_JOBS,
    Report, StopReason,
};

/// The name of the worker thread.
pub const WORKER_NAME: &str = "kosmo-jobs-worker";

/// The stack size of the worker thread in bytes: 2 MiB.
///
/// kalyx 0.1.0's resource profile measured a stack of at most 9,728 bytes
/// for its cases; that is a measurement on one host, not a bound, so the
/// stack is set well above it.
pub const WORKER_STACK_SIZE: usize = 2 * 1024 * 1024;

/// The worker thread of a host stub could not be created.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct WorkerNotStarted;

impl fmt::Display for WorkerNotStarted {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the worker thread of the job layer could not be created")
    }
}

impl std::error::Error for WorkerNotStarted {}

/// The host stub, generic over the runner port: the preparing half `P`
/// stays with the stub, and the running half moves to the worker thread.
pub struct HostStub<P: JobPreparer> {
    preparer: P,
    /// Each job with the token that the preparing half made for it.
    dispatch: Option<SyncSender<(WorkItem, P::Token)>>,
    reports: Receiver<Report>,
    worker: Option<JoinHandle<()>>,
    /// The cancel handles of the jobs handed to the worker whose `Finished`
    /// has not arrived.
    handed: BTreeMap<JobId, CancelHandle>,
    /// `Finished` reports that the stub makes itself, for jobs that the
    /// worker will not finish.
    failed: VecDeque<Report>,
    /// The worker is gone, or the dispatch channel is closed.
    stopped: bool,
}

impl<P: JobPreparer> HostStub<P> {
    /// A stub with the runner halves `preparer` and `runner`, whose worker
    /// thread runs from now on.
    pub fn new<R: JobRunner<P::Token>>(
        preparer: P,
        runner: R,
    ) -> Result<HostStub<P>, WorkerNotStarted> {
        let (dispatch, jobs) = sync_channel::<(WorkItem, P::Token)>(MAX_LIVE_JOBS);
        let (sender, reports) = sync_channel::<Report>(2 * MAX_LIVE_JOBS);
        let worker = Builder::new()
            .name(WORKER_NAME.to_string())
            .stack_size(WORKER_STACK_SIZE)
            .spawn(move || work(runner, &jobs, &sender))
            .map_err(|_| WorkerNotStarted)?;
        Ok(HostStub {
            preparer,
            dispatch: Some(dispatch),
            reports,
            worker: Some(worker),
            handed: BTreeMap::new(),
            failed: VecDeque::new(),
            stopped: false,
        })
    }
}

impl<P: JobPreparer> HostStub<P> {
    /// The number of worker threads that are running: 1 from construction
    /// until the worker ends, and 0 after shutdown.
    pub fn live_threads(&self) -> usize {
        self.worker
            .as_ref()
            .map_or(0, |worker| usize::from(!worker.is_finished()))
    }

    /// Records that the worker will finish no job: every job handed to it
    /// that has not finished gets a `Finished` from the stub.
    fn stop(&mut self) {
        self.stopped = true;
        let handed = std::mem::take(&mut self.handed);
        for id in handed.into_keys() {
            self.failed.push_back(internal_error(id));
        }
    }

    /// Notes a report from the worker before passing it on.
    fn received(&mut self, report: Report) -> Report {
        if let Report::Finished(finished) = &report {
            self.handed.remove(&finished.id());
        }
        report
    }
}

impl<P: JobPreparer> JobBackend for HostStub<P> {
    fn submit(&mut self, request: JobRequest) {
        let id = request.id();
        let (handle, token) = self.preparer.prepare(id);
        let sent = match (&self.dispatch, self.stopped) {
            (Some(dispatch), false) => dispatch
                .try_send((WorkItem::new(request, handle.clone()), token))
                .is_ok(),
            _ => false,
        };
        if sent {
            self.handed.insert(id, handle);
        } else {
            self.failed.push_back(internal_error(id));
        }
    }

    fn cancel(&mut self, id: JobId) {
        if let Some(handle) = self.handed.get(&id) {
            handle.set();
        }
    }

    fn try_report(&mut self) -> Option<Report> {
        if let Some(report) = self.failed.pop_front() {
            return Some(report);
        }
        match self.reports.try_recv() {
            Ok(report) => Some(self.received(report)),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.stop();
                self.failed.pop_front()
            }
        }
    }

    fn wait_report(&mut self) -> Option<Report> {
        if let Some(report) = self.failed.pop_front() {
            return Some(report);
        }
        if self.handed.is_empty() {
            return None;
        }
        match self.reports.recv() {
            Ok(report) => Some(self.received(report)),
            Err(_) => {
                self.stop();
                self.failed.pop_front()
            }
        }
    }

    fn shutdown(&mut self) {
        self.dispatch = None;
        if let Some(worker) = self.worker.take() {
            // A panic of the worker has already been reported as a backend
            // internal error; it is not raised again.
            let _ = worker.join();
        }
    }
}

impl<P: JobPreparer> Drop for HostStub<P> {
    fn drop(&mut self) {
        self.dispatch = None;
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// The `Finished` of a job that the worker did not finish.
fn internal_error(id: JobId) -> Report {
    Report::Finished(Finished::new(
        id,
        Vec::new(),
        Some(StopReason::BackendInternalError),
    ))
}

/// The worker thread: runs each job it receives, with its token, until the
/// dispatch channel closes.
fn work<T, R: JobRunner<T>>(
    mut runner: R,
    jobs: &Receiver<(WorkItem, T)>,
    reports: &SyncSender<Report>,
) {
    while let Ok((item, token)) = jobs.recv() {
        let id = item.request().id();
        let guard = ReportGuard {
            id,
            reports,
            sent: false,
        };
        if reports.send(Report::Started(id)).is_err() {
            // The stub is gone, so no report can arrive anywhere.
            return;
        }
        let (requests, stop) = runner.run(item, token).into_parts();
        guard.finish(Finished::new(id, requests, stop));
    }
}

/// Reports the `Finished` of the worker's current job exactly once: the
/// runner's result, or a backend internal error if the thread ends first.
struct ReportGuard<'a> {
    id: JobId,
    reports: &'a SyncSender<Report>,
    sent: bool,
}

impl ReportGuard<'_> {
    fn finish(mut self, finished: Finished) {
        self.sent = true;
        let _ = self.reports.send(Report::Finished(finished));
    }
}

impl Drop for ReportGuard<'_> {
    fn drop(&mut self) {
        if !self.sent {
            let _ = self.reports.send(internal_error(self.id));
        }
    }
}
