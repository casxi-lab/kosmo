//! Test gates around the job layer's host stub, for tests that need another
//! order of events than the stub gives by itself: a runner on the worker
//! thread that makes one job wait at a gate before the kalyx runner gets it,
//! and a backend around the stub that holds one job's `Finished` until the
//! test releases it. Both implement only the public ports of the job layer,
//! so neither the job layer nor the stub changes for these tests.
//!
//! A gate and a hold name a job by its submission order, counting from 0;
//! the stub's worker runs the jobs in that order. The test waits until the
//! job at the gate waits there, acts on the job layer and opens the gate.
//! Nothing sleeps or reads the wall clock. Waiting at the gate through
//! [`HostJobs::wait_at_gate`] fails instead of hanging when the host stub
//! has not received the job at the gate, once that job has ended without
//! waiting there, or once the worker thread has ended, and dropping
//! [`HostJobs`] opens the gate and
//! releases the hold before the job layer shuts down and joins the worker
//! thread, so a test that fails at any point still ends.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};

use kosmo_jobs::stub::HostStub;
use kosmo_jobs::{
    Finished, JobBackend, JobClock, JobId, JobLayer, JobRequest, JobRunner, Report, RunReport,
    SessionId, WorkItem,
};
use kosmo_math_kalyx::{KalyxPreparer, KalyxRunner};

use super::{Checked, Still};

/// What a [`Gated`] runner and the test's control of its gate share.
#[derive(Debug, Default)]
struct State {
    /// The submission order of the job that waits at the gate, if any.
    gate: Option<usize>,
    /// Runs that started.
    started: usize,
    /// Runs that ended, by returning or unwinding.
    ended: usize,
    /// The job at the gate waits there.
    waiting: bool,
    /// The gate is open.
    open: bool,
    /// The runner was dropped: the worker thread ended.
    gone: bool,
}

/// The state with the condition that signals its changes.
#[derive(Debug, Default)]
struct Shared {
    state: Mutex<State>,
    changed: Condvar,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn wait<'a>(&self, state: MutexGuard<'a, State>) -> MutexGuard<'a, State> {
        self.changed
            .wait(state)
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// Changes the state with `apply` and wakes every waiter.
    fn change(&self, apply: impl FnOnce(&mut State)) {
        apply(&mut self.lock());
        self.changed.notify_all();
    }
}

/// A runner that hands each job on to `inner` unchanged; the job at the
/// gate waits there first, before `inner` gets it.
pub struct Gated<R> {
    inner: R,
    shared: Arc<Shared>,
}

/// The test's control of the gate of a [`Gated`] runner.
pub struct GateControl {
    shared: Arc<Shared>,
}

impl<R> Gated<R> {
    /// A runner around `inner` whose job of submission order `gate`, if
    /// any, waits at the gate, and the control of that gate.
    pub fn new(inner: R, gate: Option<usize>) -> (Gated<R>, GateControl) {
        let shared = Arc::new(Shared::default());
        shared.lock().gate = gate;
        let control = GateControl {
            shared: Arc::clone(&shared),
        };
        (Gated { inner, shared }, control)
    }
}

impl<R> Drop for Gated<R> {
    fn drop(&mut self) {
        self.shared.change(|state| state.gone = true);
    }
}

/// Counts one ended run when it is dropped, on return or unwinding.
struct Ended(Arc<Shared>);

impl Drop for Ended {
    fn drop(&mut self) {
        self.0.change(|state| state.ended += 1);
    }
}

impl<T, R: JobRunner<T>> JobRunner<T> for Gated<R> {
    fn run(&mut self, item: WorkItem, token: T) -> RunReport {
        let _ended = Ended(Arc::clone(&self.shared));
        let mut state = self.shared.lock();
        let order = state.started;
        state.started += 1;
        if state.gate == Some(order) {
            state.waiting = true;
            self.shared.changed.notify_all();
            while !state.open {
                state = self.shared.wait(state);
            }
            state.waiting = false;
        }
        drop(state);
        self.inner.run(item, token)
    }
}

impl GateControl {
    /// Waits until the job at the gate waits there. Fails when there is no
    /// gate, when that job ended without waiting there, or when the worker
    /// thread ended first.
    pub fn wait_at_gate(&self) -> Checked {
        let mut state = self.shared.lock();
        let gate = state.gate.ok_or("no job waits at a gate")?;
        loop {
            if state.waiting {
                return Ok(());
            }
            if state.ended > gate {
                return Err("the job at the gate ended without waiting there".into());
            }
            if state.gone {
                return Err("the worker thread ended before the job at the gate".into());
            }
            state = self.shared.wait(state);
        }
    }

    /// Opens the gate: the job that waits there, if any, goes on.
    pub fn open(&self) {
        self.shared.change(|state| state.open = true);
    }
}

/// A backend that passes everything on to `inner`, except that it holds the
/// `Finished` of the job of submission order `hold` until the test releases
/// it. Every report that `inner` gave, held or not, is kept.
pub struct Holding<B> {
    inner: B,
    hold: Option<usize>,
    /// The jobs received, in submission order.
    received: Vec<JobId>,
    /// The held `Finished`, until it is taken after its release.
    held: Option<Report>,
    released: Rc<Cell<bool>>,
    /// Every report that `inner` gave, in order.
    reports: Vec<Report>,
}

/// The test's control of the hold of a [`Holding`] backend, on the
/// job-layer thread.
pub struct HoldControl {
    released: Rc<Cell<bool>>,
}

impl<B: JobBackend> Holding<B> {
    /// A backend around `inner` that holds the `Finished` of the job of
    /// submission order `hold`, if any, and the control of that hold.
    pub fn new(inner: B, hold: Option<usize>) -> (Holding<B>, HoldControl) {
        let released = Rc::new(Cell::new(false));
        let control = HoldControl {
            released: Rc::clone(&released),
        };
        let backend = Holding {
            inner,
            hold,
            received: Vec::new(),
            held: None,
            released,
            reports: Vec::new(),
        };
        (backend, control)
    }

    /// The backend that it passes on to.
    pub const fn inner(&self) -> &B {
        &self.inner
    }

    /// The `Finished` of job `id` as `inner` reported it, held or not, once
    /// the job layer asked for reports past it.
    pub fn finished(&self, id: JobId) -> Option<&Finished> {
        self.reports.iter().find_map(|report| match report {
            Report::Finished(finished) if finished.id() == id => Some(finished),
            _ => None,
        })
    }

    /// Keeps `report` and gives it on, unless it is the `Finished` to hold.
    fn pass(&mut self, report: Report) -> Option<Report> {
        self.reports.push(report.clone());
        let held = self.hold.and_then(|order| self.received.get(order));
        let holds = matches!(&report, Report::Finished(finished) if Some(&finished.id()) == held);
        if holds && !self.released.get() {
            self.held = Some(report);
            None
        } else {
            Some(report)
        }
    }

    /// The held `Finished`, once it is released.
    fn take_released(&mut self) -> Option<Report> {
        if self.released.get() {
            self.held.take()
        } else {
            None
        }
    }
}

impl<B: JobBackend> JobBackend for Holding<B> {
    fn submit(&mut self, request: JobRequest) {
        self.received.push(request.id());
        self.inner.submit(request);
    }

    fn cancel(&mut self, id: JobId) {
        self.inner.cancel(id);
    }

    fn try_report(&mut self) -> Option<Report> {
        if let Some(report) = self.take_released() {
            return Some(report);
        }
        while let Some(report) = self.inner.try_report() {
            if let Some(report) = self.pass(report) {
                return Some(report);
            }
        }
        None
    }

    fn wait_report(&mut self) -> Option<Report> {
        if let Some(report) = self.take_released() {
            return Some(report);
        }
        while let Some(report) = self.inner.wait_report() {
            if let Some(report) = self.pass(report) {
                return Some(report);
            }
        }
        None
    }

    fn shutdown(&mut self) {
        self.inner.shutdown();
        self.released.set(true);
    }
}

impl HoldControl {
    /// Releases the held `Finished`: the job layer gets it with its next
    /// report. A `Finished` that has not arrived yet is not held at all.
    pub fn release(&self) {
        self.released.set(true);
    }
}

/// The host stub with the kalyx runner behind a gate, and a hold around it.
pub type GatedStub = Holding<HostStub<KalyxPreparer>>;

/// A job layer on the host stub with the kalyx runner behind a gate and a
/// hold around the stub, whose clock always reads 0, a session of it and
/// the controls of the gate and the hold.
///
/// Dropping it opens the gate and releases the hold before the job layer
/// shuts down and joins the worker thread.
pub struct HostJobs {
    /// The job layer.
    pub layer: JobLayer<GatedStub>,
    /// The control of the runner's gate.
    pub gate: GateControl,
    /// The control of the backend's hold.
    pub hold: HoldControl,
    /// The session of the Scratchpad's jobs.
    pub session: SessionId,
}

impl HostJobs {
    /// A job layer whose job of submission order `gate`, if any, waits at
    /// the gate before the kalyx runner, and whose job of order `hold`, if
    /// any, has its `Finished` held.
    pub fn new(gate: Option<usize>, hold: Option<usize>) -> Checked<HostJobs> {
        let clock = Still;
        let (runner, gate_control) = Gated::new(KalyxRunner::new(clock.fork()), gate);
        let stub = HostStub::new(KalyxPreparer::new(), runner)?;
        let (backend, hold_control) = Holding::new(stub, hold);
        let mut layer = JobLayer::new(backend, Box::new(clock));
        let session = layer.open_session()?;
        Ok(HostJobs {
            layer,
            gate: gate_control,
            hold: hold_control,
            session,
        })
    }

    /// Waits until the job at the gate waits there, as
    /// [`GateControl::wait_at_gate`] does, and fails first when the host
    /// stub has not received that job. A job that the stub received reaches
    /// the worker thread while the worker runs, so the wait then ends.
    pub fn wait_at_gate(&self) -> Checked {
        let gate = self
            .gate
            .shared
            .lock()
            .gate
            .ok_or("no job waits at a gate")?;
        if self.layer.backend().received.len() <= gate {
            return Err("the host stub has not received the job at the gate".into());
        }
        self.gate.wait_at_gate()
    }

    /// The `Finished` of job `id` as the host stub reported it, once the
    /// job layer asked for reports past it.
    pub fn finished(&self, id: JobId) -> Checked<&Finished> {
        Ok(self.layer.backend().finished(id).ok_or("no Finished")?)
    }

    /// Opens the gate, releases the hold, shuts the job layer down and
    /// gives the number of the host stub's live worker threads afterwards.
    pub fn shut_down(&mut self) -> usize {
        self.gate.open();
        self.hold.release();
        self.layer.shutdown();
        self.layer.backend().inner().live_threads()
    }
}

impl Drop for HostJobs {
    fn drop(&mut self) {
        self.gate.open();
        self.hold.release();
    }
}
