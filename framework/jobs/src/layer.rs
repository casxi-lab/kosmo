//! The shell of the job layer: its API on the job-layer thread.

use crate::controllable::ControllableBackend;
use crate::machine::{CancelAnswer, CancelCount, Command, JobState, Machine, Terminal};
use crate::{JobBackend, JobClock, JobId, Rejected, SessionCancel, SessionId, Submission};

/// The job layer: the pure core with a backend and a clock, used from the
/// one thread that holds it, the job-layer thread.
///
/// [`submit`](JobLayer::submit) accepts a job and hands it to the backend
/// at once, or refuses it. [`cancel`](JobLayer::cancel) and
/// [`cancel_session`](JobLayer::cancel_session) answer at once whether
/// they were accepted; terminal states come later. Reports reach the core
/// only when the job-layer thread calls [`poll`](JobLayer::poll) or
/// [`wait`](JobLayer::wait), and each terminal state is taken once, with
/// [`take`](JobLayer::take) or `wait`. A job stays live, and counts
/// toward [`MAX_LIVE_JOBS`](crate::MAX_LIVE_JOBS), until its terminal state
/// is taken and its backend has finished it.
///
/// [`shutdown`](JobLayer::shutdown), which dropping the job layer also
/// does, cancels every job that has not ended and stops the backend.
pub struct JobLayer<B: JobBackend> {
    machine: Machine,
    backend: B,
    clock: Box<dyn JobClock>,
    shut_down: bool,
}

impl<B: JobBackend> JobLayer<B> {
    /// A job layer with the backend `backend` and the clock `clock`, from
    /// which it computes deadlines.
    pub fn new(backend: B, clock: Box<dyn JobClock>) -> JobLayer<B> {
        JobLayer {
            machine: Machine::new(),
            backend,
            clock,
            shut_down: false,
        }
    }

    /// Opens a session for a submitter, or refuses when every session
    /// identity has been used.
    pub fn open_session(&mut self) -> Result<SessionId, Rejected> {
        self.machine.open_session()
    }

    /// Submits a job. On acceptance the job layer reads its clock once if
    /// the submission has a time limit, writes the reading plus the limit,
    /// saturating, into the job as its absolute deadline, and hands the job
    /// to the backend. While [`MAX_LIVE_JOBS`](crate::MAX_LIVE_JOBS) jobs
    /// are live it refuses with [`Rejected::Full`] and the number of live
    /// jobs, and after the last job identity with [`Rejected::Exhausted`]; a
    /// refusal changes nothing and reads no clock.
    pub fn submit(&mut self, submission: Submission) -> Result<JobId, Rejected> {
        self.machine.admits()?;
        let deadline = submission
            .time_limit()
            .map(|limit| self.clock.now().saturating_add(limit));
        let (accepted, commands) = self.machine.submit(submission, deadline);
        self.execute(commands);
        accepted
    }

    /// Cancels job `id`. The answer says whether the cancellation was
    /// accepted: a queued job is `cancelled` at once, a running one moves
    /// to `cancel_requested` and ends `cancelled` once its backend reports
    /// its end. An accepted cancellation also sets the job's cancellation in
    /// the backend.
    pub fn cancel(&mut self, id: JobId) -> CancelAnswer {
        let (answer, commands) = self.machine.cancel(id);
        self.execute(commands);
        answer
    }

    /// Cancels every job of `session` that has not reached its terminal
    /// state, as [`SessionCancel::cancel_session`] describes.
    pub fn cancel_session(&mut self, session: SessionId) -> CancelCount {
        let (count, commands) = self.machine.cancel_session(session);
        self.execute(commands);
        count
    }

    /// Applies every report that has arrived, without waiting, and returns
    /// the jobs whose terminal states wait to be taken, in order of
    /// identity.
    pub fn poll(&mut self) -> Vec<JobId> {
        while let Some(report) = self.backend.try_report() {
            self.machine.report(report);
        }
        self.machine.ready()
    }

    /// Takes the terminal state of job `id`: `None` unless the job has
    /// reached it and it has not been taken.
    pub fn take(&mut self, id: JobId) -> Option<Terminal> {
        self.machine.take(id)
    }

    /// Waits until job `id` reaches its terminal state, applying reports as
    /// they arrive, and takes it. `None` when the job is not live, its
    /// state was taken already, or the backend has no report to come.
    pub fn wait(&mut self, id: JobId) -> Option<Terminal> {
        loop {
            match self.machine.state(id) {
                None => return None,
                Some(JobState::Terminal(_)) => return self.machine.take(id),
                Some(_) => {}
            }
            let report = self.backend.wait_report()?;
            self.machine.report(report);
        }
    }

    /// Shuts the job layer down: cancels every job that has not reached
    /// its terminal state, stops the backend and applies its remaining
    /// reports, so that every job is terminal. Terminal states can still be
    /// taken afterwards. A job submitted afterwards is handed to the
    /// stopped backend, which in the host stub ends it `failed`.
    pub fn shutdown(&mut self) {
        if self.shut_down {
            return;
        }
        self.shut_down = true;
        for id in self.machine.unfinished() {
            let (_, commands) = self.machine.cancel(id);
            self.execute(commands);
        }
        self.backend.shutdown();
        while let Some(report) = self.backend.try_report() {
            self.machine.report(report);
        }
    }

    /// The state of job `id`, while it is live.
    pub fn state(&self, id: JobId) -> Option<JobState> {
        self.machine.state(id)
    }

    /// The number of live jobs.
    pub fn live_jobs(&self) -> usize {
        self.machine.live()
    }

    /// The backend, to read it, such as the live threads of the host stub.
    /// Only the job layer submits jobs to its backend, cancels them there,
    /// takes their reports and shuts the backend down.
    pub const fn backend(&self) -> &B {
        &self.backend
    }

    /// Carries out the core's commands.
    fn execute(&mut self, commands: Vec<Command>) {
        for command in commands {
            match command {
                Command::Hand(request) => self.backend.submit(request),
                Command::SetCancel(id) => self.backend.cancel(id),
            }
        }
    }
}

impl JobLayer<ControllableBackend> {
    /// The controllable backend, to give its jobs their plans and release
    /// their gates. Submitting, cancelling, taking reports and shutting down
    /// stay with the job layer: a report taken here never reaches it, and
    /// its job stays live.
    pub const fn controllable(&mut self) -> &mut ControllableBackend {
        &mut self.backend
    }
}

impl<B: JobBackend> SessionCancel for JobLayer<B> {
    fn cancel_session(&mut self, session: SessionId) -> CancelCount {
        JobLayer::cancel_session(self, session)
    }
}

impl<B: JobBackend> Drop for JobLayer<B> {
    fn drop(&mut self) {
        self.shutdown();
    }
}
