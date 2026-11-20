//! A backend whose reports tests arrange: the controllable backend.
//!
//! [`ControllableBackend`] implements [`JobBackend`] on the calling thread,
//! without threads, sleeps or clocks. A test gives each job a [`Plan`],
//! keyed by the job's submission order: the number of jobs the backend
//! received before it, counting from 0. A plan lists the reports of the
//! requests that run and the stop that the backend reports, and closes
//! [`Gate`]s at chosen places, each with a [`Release`] condition. The
//! backend runs a job as far as its gates allow whenever something
//! happens: the job arrives, it gets its plan, a gate is released or the
//! job is cancelled. A job without a plan waits before it starts.
//!
//! Like a runner of the host stub, the backend checks a job's cancellation
//! before each of the job's requests, the first included, and stops the job
//! there with [`StopReason::Cancellation`] when it was cancelled. Reports
//! become available to [`JobBackend::try_report`] in the order the backend
//! makes them.
//!
//! ```
//! use kosmo_jobs::controllable::{ControllableBackend, Gate, Plan, Release};
//! use kosmo_jobs::machine::CancelAnswer;
//! use kosmo_jobs::{JobClock, JobLayer, Operation, Request, Submission, TerminalState};
//! use kosmo_math_model::version::Versions;
//!
//! struct Still;
//! impl JobClock for Still {
//!     fn now(&mut self) -> u64 { 0 }
//!     fn fork(&self) -> Box<dyn JobClock> { Box::new(Still) }
//! }
//!
//! let mut backend = ControllableBackend::new();
//! // The first job starts and waits before its first request.
//! backend.plan(0, Plan::new().gate(Gate::Between(0), Release::Manual));
//! let mut layer = JobLayer::new(backend, Box::new(Still));
//! let session = layer.open_session()?;
//! let request = Request::new(Operation::Eval, "", "1/3+1/6");
//! let id = layer.submit(Submission::new(session, Versions::default(), request))?;
//! layer.poll();
//! assert_eq!(layer.cancel(id), CancelAnswer::Requested);
//!
//! // Released, the backend sees the cancellation before the request.
//! layer.backend_mut().release(0, Gate::Between(0));
//! assert_eq!(layer.poll(), vec![id]);
//! let terminal = layer.take(id).ok_or("not terminal")?;
//! assert_eq!(terminal.record().state(), TerminalState::Cancelled);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::{Finished, JobBackend, JobId, JobRequest, Report, RequestReport, StopReason};

/// A place in a job's run where its plan can make it wait.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Gate {
    /// Before the backend reports `Started`.
    Started,
    /// Before request `k`, counting from 0, before the backend checks the
    /// job's cancellation there.
    Between(usize),
    /// In request `k`, after the check: the request then ends with its
    /// planned report, whether the job was cancelled or not.
    Within(usize),
    /// Before the backend reports `Finished`.
    Finished,
}

/// When a closed gate opens.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Release {
    /// When the test releases it with [`ControllableBackend::release`].
    Manual,
    /// When the job is cancelled, or the test releases it.
    OnCancel,
}

/// What the backend does with one job.
///
/// [`Plan::new`] reports `Started`, runs no request and reports `Finished`
/// without a stop. [`Plan::request`] adds the report of the next request
/// that runs; [`Plan::stop`] sets the stop that the backend reports after
/// the planned requests, such as a forced stop in the request after them.
#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Plan {
    requests: Vec<RequestReport>,
    stop: Option<StopReason>,
    without_started: bool,
    gates: BTreeMap<Gate, Release>,
}

impl Plan {
    /// A plan that reports `Started`, runs no request and reports
    /// `Finished` without a stop.
    pub fn new() -> Plan {
        Plan::default()
    }

    /// This plan with `report` as the report of its next request.
    #[must_use]
    pub fn request(mut self, report: RequestReport) -> Plan {
        self.requests.push(report);
        self
    }

    /// This plan with `stop` as the stop reported after its requests.
    #[must_use]
    pub fn stop(mut self, stop: StopReason) -> Plan {
        self.stop = Some(stop);
        self
    }

    /// This plan without the `Started` report: the job gets only
    /// `Finished`.
    #[must_use]
    pub fn without_started(mut self) -> Plan {
        self.without_started = true;
        self
    }

    /// This plan with `gate` closed until `release` opens it.
    #[must_use]
    pub fn gate(mut self, gate: Gate, release: Release) -> Plan {
        self.gates.insert(gate, release);
        self
    }
}

/// Where a job is in its run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Place {
    Start,
    Between(usize),
    Within(usize),
    Finish(Option<StopReason>),
    Done,
}

/// One job that the backend received.
#[derive(Clone, Debug)]
struct Job {
    request: JobRequest,
    plan: Option<Plan>,
    place: Place,
    ran: Vec<RequestReport>,
    cancelled: bool,
    released: BTreeSet<Gate>,
}

impl Job {
    /// Whether `gate` keeps the job waiting.
    fn closed(&self, plan: &Plan, gate: Gate) -> bool {
        match plan.gates.get(&gate) {
            None => false,
            Some(Release::Manual) => !self.released.contains(&gate),
            Some(Release::OnCancel) => !self.cancelled && !self.released.contains(&gate),
        }
    }

    /// Runs the job as far as its gates allow, adding its reports to `out`.
    fn advance(&mut self, out: &mut Vec<Report>) {
        let Some(plan) = self.plan.clone() else {
            return;
        };
        let id = self.request.id();
        loop {
            match self.place {
                Place::Start => {
                    if self.closed(&plan, Gate::Started) {
                        return;
                    }
                    if !plan.without_started {
                        out.push(Report::Started(id));
                    }
                    self.place = Place::Between(0);
                }
                Place::Between(k) => {
                    if self.closed(&plan, Gate::Between(k)) {
                        return;
                    }
                    self.place = if k < self.request.requests().len() && self.cancelled {
                        Place::Finish(Some(StopReason::Cancellation))
                    } else if k >= plan.requests.len() {
                        Place::Finish(plan.stop)
                    } else {
                        Place::Within(k)
                    };
                }
                Place::Within(k) => {
                    if self.closed(&plan, Gate::Within(k)) {
                        return;
                    }
                    if let Some(report) = plan.requests.get(k) {
                        self.ran.push(report.clone());
                    }
                    self.place = Place::Between(k + 1);
                }
                Place::Finish(stop) => {
                    if self.closed(&plan, Gate::Finished) {
                        return;
                    }
                    let ran = std::mem::take(&mut self.ran);
                    out.push(Report::Finished(Finished::new(id, ran, stop)));
                    self.place = Place::Done;
                }
                Place::Done => return,
            }
        }
    }

    /// The gate at which the job waits, if it waits at one.
    fn waiting_at(&self) -> Option<Gate> {
        let plan = self.plan.as_ref()?;
        let gate = match self.place {
            Place::Start => Gate::Started,
            Place::Between(k) => Gate::Between(k),
            Place::Within(k) => Gate::Within(k),
            Place::Finish(_) => Gate::Finished,
            Place::Done => return None,
        };
        self.closed(plan, gate).then_some(gate)
    }
}

/// The controllable backend, as the module documentation describes.
#[derive(Clone, Debug, Default)]
pub struct ControllableBackend {
    jobs: Vec<Job>,
    plans: BTreeMap<usize, Plan>,
    pending: VecDeque<Report>,
    sent: Vec<Report>,
    cancels: Vec<JobId>,
    shut_down: bool,
}

impl ControllableBackend {
    /// A backend that has received no job.
    pub fn new() -> ControllableBackend {
        ControllableBackend::default()
    }

    /// Gives the job of submission order `order` the plan `plan`, which
    /// replaces the plan it had. A job that has arrived runs at once as far
    /// as its gates allow.
    pub fn plan(&mut self, order: usize, plan: Plan) {
        match self.jobs.get_mut(order) {
            Some(job) => {
                job.plan = Some(plan);
                self.advance(order);
            }
            None => {
                self.plans.insert(order, plan);
            }
        }
    }

    /// Releases `gate` of the job of submission order `order`, and runs the
    /// job as far as its other gates allow. Releasing a gate of a job that
    /// has not arrived does nothing.
    pub fn release(&mut self, order: usize, gate: Gate) {
        if let Some(job) = self.jobs.get_mut(order) {
            job.released.insert(gate);
            self.advance(order);
        }
    }

    /// The number of jobs received.
    pub fn received(&self) -> usize {
        self.jobs.len()
    }

    /// The job of submission order `order`, if it has arrived.
    pub fn request(&self, order: usize) -> Option<&JobRequest> {
        self.jobs.get(order).map(|job| &job.request)
    }

    /// Whether the job of submission order `order` was cancelled.
    pub fn was_cancelled(&self, order: usize) -> bool {
        self.jobs.get(order).is_some_and(|job| job.cancelled)
    }

    /// The jobs cancelled, in the order of the cancellations.
    pub fn cancels(&self) -> &[JobId] {
        &self.cancels
    }

    /// The gate at which the job of submission order `order` waits, if it
    /// waits at one.
    pub fn waiting_at(&self, order: usize) -> Option<Gate> {
        self.jobs.get(order).and_then(Job::waiting_at)
    }

    /// Every report that the backend made, in order.
    pub fn sent(&self) -> &[Report] {
        &self.sent
    }

    /// Whether the backend was shut down.
    pub const fn is_shut_down(&self) -> bool {
        self.shut_down
    }

    fn advance(&mut self, order: usize) {
        let mut out = Vec::new();
        if let Some(job) = self.jobs.get_mut(order) {
            job.advance(&mut out);
        }
        self.sent.extend(out.iter().cloned());
        self.pending.extend(out);
    }
}

impl JobBackend for ControllableBackend {
    fn submit(&mut self, request: JobRequest) {
        let order = self.jobs.len();
        let mut plan = self.plans.remove(&order);
        if self.shut_down {
            plan = Some(plan.unwrap_or_default());
        }
        self.jobs.push(Job {
            request,
            plan,
            place: Place::Start,
            ran: Vec::new(),
            cancelled: false,
            released: BTreeSet::new(),
        });
        if self.shut_down {
            self.open_all(order);
        }
        self.advance(order);
    }

    fn cancel(&mut self, id: JobId) {
        self.cancels.push(id);
        if let Some(order) = self.jobs.iter().position(|job| job.request.id() == id) {
            if let Some(job) = self.jobs.get_mut(order) {
                job.cancelled = true;
            }
            self.advance(order);
        }
    }

    fn try_report(&mut self) -> Option<Report> {
        self.pending.pop_front()
    }

    /// The next report: this backend never waits, so it is the same as
    /// [`try_report`](JobBackend::try_report).
    fn wait_report(&mut self) -> Option<Report> {
        self.pending.pop_front()
    }

    /// Opens every gate and runs every job to its end; a job without a
    /// plan gets [`Plan::new`].
    fn shutdown(&mut self) {
        self.shut_down = true;
        for order in 0..self.jobs.len() {
            if let Some(job) = self.jobs.get_mut(order) {
                job.plan.get_or_insert_with(Plan::new);
            }
            self.open_all(order);
            self.advance(order);
        }
    }
}

impl ControllableBackend {
    /// Releases every gate of the plan of the job of submission order
    /// `order`.
    fn open_all(&mut self, order: usize) {
        if let Some(job) = self.jobs.get_mut(order) {
            let gates: Vec<Gate> = job
                .plan
                .as_ref()
                .map(|plan| plan.gates.keys().copied().collect())
                .unwrap_or_default();
            job.released.extend(gates);
        }
    }
}
