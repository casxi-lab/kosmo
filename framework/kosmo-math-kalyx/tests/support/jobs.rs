//! Helpers for the tests that run jobs on the job layer's host stub with
//! the kalyx runner: a fake clock with a gate at a reading of the worker
//! thread, a backend that records the jobs and reports that pass through
//! it, and a job layer that holds both.
//!
//! The tests order events only through the gate and the handshakes of the
//! job layer: a test waits until the worker thread waits at the gate, acts
//! on the job layer, and opens the gate. Nothing sleeps or reads the wall
//! clock. A job that ends without reaching the gate makes the wait fail
//! instead of hang.

use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};

use kosmo_jobs::stub::HostStub;
use kosmo_jobs::{
    Finished, JobBackend, JobClock, JobId, JobLayer, JobRequest, JobRunner, Operation, Report,
    Request, RunReport, SessionId, Submission, WorkItem,
};
use kosmo_math_kalyx::{JobFlag, KalyxPreparer, KalyxRunner, convert};
use kosmo_math_model::Snapshot;
use kosmo_math_model::version::Versions;

use super::{Checked, line};

/// What the forks of a fake clock share.
#[derive(Debug, Default)]
struct State {
    /// The current reading.
    now: u64,
    /// Readings of the clock that the job layer holds.
    layer_readings: usize,
    /// Readings of the forks, which the runner and the contexts it builds
    /// read on the worker thread.
    worker_readings: usize,
    /// The values of the next readings of the forks, in order.
    script: VecDeque<u64>,
    /// The reading of the forks, counting from 1, that waits at the gate.
    gate: Option<usize>,
    /// A reading waits at the gate.
    waiting: bool,
    /// The gate is open.
    open: bool,
    /// Runs of the runner that ended, by returning or unwinding.
    ended: usize,
    /// `ended` when the gate was set.
    ended_at_gate: usize,
}

/// The state of a fake clock with the condition that signals its changes.
#[derive(Debug, Default)]
struct Shared {
    state: Mutex<State>,
    changed: Condvar,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// A fake clock of the job layer.
///
/// The clock that the job layer holds reads the current value. Its forks,
/// which the runner and every context it builds read on the worker thread,
/// count their readings: the reading at the gate waits there until the test
/// opens it, and each reading then takes the next scripted value, if any,
/// as the current one.
pub struct FakeClock {
    shared: Arc<Shared>,
    fork: bool,
}

impl FakeClock {
    /// A clock that reads 0, and the test's control of it.
    pub fn new() -> (FakeClock, ClockControl) {
        let shared = Arc::new(Shared::default());
        let control = ClockControl {
            shared: Arc::clone(&shared),
        };
        (
            FakeClock {
                shared,
                fork: false,
            },
            control,
        )
    }
}

impl JobClock for FakeClock {
    fn now(&mut self) -> u64 {
        let mut state = self.shared.lock();
        if !self.fork {
            state.layer_readings += 1;
            return state.now;
        }
        state.worker_readings += 1;
        if state.gate == Some(state.worker_readings) {
            state.waiting = true;
            self.shared.changed.notify_all();
            while !state.open {
                state = self
                    .shared
                    .changed
                    .wait(state)
                    .unwrap_or_else(PoisonError::into_inner);
            }
            state.waiting = false;
        }
        if let Some(value) = state.script.pop_front() {
            state.now = value;
        }
        state.now
    }

    fn fork(&self) -> Box<dyn JobClock> {
        Box::new(FakeClock {
            shared: Arc::clone(&self.shared),
            fork: true,
        })
    }
}

/// The test's control of a fake clock.
#[derive(Clone, Debug)]
pub struct ClockControl {
    shared: Arc<Shared>,
}

impl ClockControl {
    /// Sets the current reading to `now`; readings never decrease.
    pub fn set(&self, now: u64) {
        let mut state = self.shared.lock();
        state.now = state.now.max(now);
    }

    /// Gives the next readings of the forks the values `values`, in order.
    pub fn script(&self, values: &[u64]) {
        self.shared.lock().script.extend(values);
    }

    /// Makes reading `reading` of the forks, counting from 1, wait at the
    /// gate until [`open`](ClockControl::open).
    pub fn gate_at(&self, reading: usize) {
        let mut state = self.shared.lock();
        state.gate = Some(reading);
        state.ended_at_gate = state.ended;
    }

    /// Waits until a reading of the worker thread waits at the gate. Fails
    /// once a run of the runner has ended since the gate was set while no
    /// reading waits there: that job will not reach the gate.
    pub fn wait_at_gate(&self) -> Checked {
        let mut state = self.shared.lock();
        while !state.waiting {
            if state.ended > state.ended_at_gate {
                return Err("the worker ended a job without reaching the gate".into());
            }
            state = self
                .shared
                .changed
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
        Ok(())
    }

    /// Opens the gate: the reading that waits there, if any, goes on.
    pub fn open(&self) {
        self.shared.lock().open = true;
        self.shared.changed.notify_all();
    }

    /// The number of readings of the clock that the job layer holds.
    pub fn layer_readings(&self) -> usize {
        self.shared.lock().layer_readings
    }

    /// The number of readings of the forks on the worker thread.
    pub fn worker_readings(&self) -> usize {
        self.shared.lock().worker_readings
    }
}

/// A backend that passes everything on to `inner` and records the jobs it
/// receives and the reports that the job layer takes from it.
pub struct Recording<B> {
    inner: B,
    received: Vec<JobRequest>,
    reports: Vec<Report>,
}

impl<B: JobBackend> Recording<B> {
    /// A recording of `inner`.
    pub fn new(inner: B) -> Recording<B> {
        Recording {
            inner,
            received: Vec::new(),
            reports: Vec::new(),
        }
    }

    /// The backend that it records.
    pub fn inner(&self) -> &B {
        &self.inner
    }

    /// The job `id` as the backend received it.
    pub fn received(&self, id: JobId) -> Option<&JobRequest> {
        self.received.iter().find(|request| request.id() == id)
    }

    /// Every report that the job layer took, in order.
    pub fn reports(&self) -> &[Report] {
        &self.reports
    }

    /// The `Finished` of job `id`, once the job layer took it.
    pub fn finished(&self, id: JobId) -> Option<&Finished> {
        self.reports.iter().find_map(|report| match report {
            Report::Finished(finished) if finished.id() == id => Some(finished),
            _ => None,
        })
    }

    fn note(&mut self, report: Option<Report>) -> Option<Report> {
        if let Some(report) = &report {
            self.reports.push(report.clone());
        }
        report
    }
}

impl<B: JobBackend> JobBackend for Recording<B> {
    fn submit(&mut self, request: JobRequest) {
        self.received.push(request.clone());
        self.inner.submit(request);
    }

    fn cancel(&mut self, id: JobId) {
        self.inner.cancel(id);
    }

    fn try_report(&mut self) -> Option<Report> {
        let report = self.inner.try_report();
        self.note(report)
    }

    fn wait_report(&mut self) -> Option<Report> {
        let report = self.inner.wait_report();
        self.note(report)
    }

    fn shutdown(&mut self) {
        self.inner.shutdown();
    }
}

/// The host stub with the kalyx runner, recorded.
pub type KalyxStub = Recording<HostStub<KalyxPreparer>>;

/// A job layer on the host stub with the kalyx runner, whose runner reads a
/// fork of the fake clock of the job layer, and the control of that clock.
/// The fake clock also counts the runs of the runner that end.
///
/// Dropping it opens the gate before the job layer shuts down and joins the
/// worker thread, so a test that fails while the worker waits at the gate
/// still ends.
pub struct KalyxJobs {
    /// The job layer.
    pub layer: JobLayer<KalyxStub>,
    /// The control of its clock.
    pub clock: ClockControl,
}

impl KalyxJobs {
    /// A job layer whose clock reads 0.
    pub fn new() -> Checked<KalyxJobs> {
        let (clock, control) = FakeClock::new();
        let runner = Counted {
            inner: KalyxRunner::new(clock.fork()),
            shared: Arc::clone(&control.shared),
        };
        let stub = HostStub::new(KalyxPreparer::new(), runner)?;
        Ok(KalyxJobs {
            layer: JobLayer::new(Recording::new(stub), Box::new(clock)),
            clock: control,
        })
    }

    /// The `Finished` of job `id` as the runner reported it, once the job
    /// layer took it.
    pub fn finished(&self, id: JobId) -> Checked<&Finished> {
        Ok(self.layer.backend().finished(id).ok_or("no Finished")?)
    }
}

impl Drop for KalyxJobs {
    fn drop(&mut self) {
        self.clock.open();
    }
}

/// The kalyx runner, which passes each work item and token on unchanged,
/// counting in the fake clock's state the runs that end.
struct Counted {
    inner: KalyxRunner,
    shared: Arc<Shared>,
}

/// Counts one ended run when it is dropped, on return or unwinding, and
/// wakes a test that waits at the gate.
struct Ended(Arc<Shared>);

impl Drop for Ended {
    fn drop(&mut self) {
        self.0.lock().ended += 1;
        self.0.changed.notify_all();
    }
}

impl JobRunner<JobFlag> for Counted {
    fn run(&mut self, item: WorkItem, token: JobFlag) -> RunReport {
        let _ended = Ended(Arc::clone(&self.shared));
        self.inner.run(item, token)
    }
}

/// A line typed as one insertion, so its source version is 1: its
/// snapshot, the versions of a request of that snapshot and its linear
/// text.
pub struct Typed {
    /// The snapshot.
    pub snapshot: Snapshot,
    /// The versions: the snapshot's source version, context and settings
    /// version 0.
    pub versions: Versions,
    /// The linear text of the snapshot.
    pub text: String,
}

impl Typed {
    /// A job of one `eval` request of the linear text in `session`, with the
    /// default limits and no time limit.
    pub fn eval(&self, session: SessionId) -> Submission {
        let request = Request::new(Operation::Eval, "", self.text.as_str());
        Submission::new(session, self.versions, request)
    }
}

/// The line `text`, typed as one insertion.
pub fn typed(text: &str) -> Checked<Typed> {
    let snapshot = line(text)?.snapshot();
    let versions = Versions {
        source: snapshot.version(),
        ..Versions::default()
    };
    let text = convert(&snapshot)?;
    Ok(Typed {
        snapshot,
        versions,
        text,
    })
}
