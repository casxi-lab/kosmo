//! The host stub with a test runner: shutdown, a runner that panics,
//! dropping the job layer, a full dispatch channel, the cancellations that
//! the stub sets, and a worker that panics before or while the job layer
//! shuts down. The runner tells the test when a job entered it and waits
//! there for the test, and tells it too when a job ended without entering,
//! so each test waits only for that handshake, for terminal states and for
//! the worker thread to be joined, never for time.

mod support;

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

use kosmo_jobs::stub::{HostStub, WORKER_NAME};
use kosmo_jobs::{
    CancelAnswer, CancelHandle, CancelTarget, JobBackend, JobId, JobLayer, JobPreparer, JobRequest,
    JobRunner, JobState, MAX_LIVE_JOBS, Operation, Report, Request, RunReport, SessionId,
    StopReason, Submission, TerminalState, WorkItem,
};
use kosmo_math_model::version::Versions;
use support::{Checked, Still, eval, value, versions};

/// The cancel target of the test runner: the job's flag, the test's count
/// of the cancellations set, and a share of the test's count of cancel
/// targets.
struct Flag {
    flag: Arc<AtomicBool>,
    sets: Arc<AtomicUsize>,
    _target: Arc<()>,
}

impl CancelTarget for Flag {
    fn set(&self) {
        self.sets.fetch_add(1, Ordering::SeqCst);
        self.flag.store(true, Ordering::SeqCst);
    }
}

/// The preparing half of the test runner: a new flag for each job, set
/// through the handle and read through the token.
struct Prepare {
    targets: Arc<()>,
    sets: Arc<AtomicUsize>,
}

impl JobPreparer for Prepare {
    type Token = Arc<AtomicBool>;

    fn prepare(&mut self, _id: JobId) -> (CancelHandle, Arc<AtomicBool>) {
        let flag = Arc::new(AtomicBool::new(false));
        let target = Flag {
            flag: Arc::clone(&flag),
            sets: Arc::clone(&self.sets),
            _target: Arc::clone(&self.targets),
        };
        (CancelHandle::new(target), flag)
    }
}

/// The running half of the test runner. Before each request it checks the
/// job's flag and stops the job when the flag is set. A request whose text
/// is `hold` tells the test that its job entered it and waits for the test
/// to open the gate; one whose text is `holdpanic` does the same and then
/// panics without checking the flag again; one whose text is `panic`
/// panics; every other request gives the value of its text. A job with a
/// `hold` or `holdpanic` request that ends without entering one, because
/// its flag was set first, tells the test that it did not enter. The runner
/// shares the test's `runner_alive`, and notes the name of the thread that
/// runs each job and whether it saw the job's cancellation.
struct Run {
    _runner_alive: Arc<()>,
    threads: Arc<Mutex<Vec<Option<String>>>>,
    seen: Arc<Mutex<Vec<(JobId, bool)>>>,
    entered: Sender<(JobId, bool)>,
    gate: Receiver<()>,
}

impl JobRunner<Arc<AtomicBool>> for Run {
    #[allow(
        clippy::panic,
        reason = "the runner panics on purpose to end the worker thread"
    )]
    fn run(&mut self, item: WorkItem, flag: Arc<AtomicBool>) -> RunReport {
        let name = std::thread::current().name().map(String::from);
        if let Ok(mut threads) = self.threads.lock() {
            threads.push(name);
        }
        let request = item.request();
        let holds = request
            .requests()
            .iter()
            .any(|one| matches!(one.input(), "hold" | "holdpanic"));
        let mut entered = false;
        let mut reports = Vec::new();
        let mut stop = None;
        for one in request.requests() {
            if flag.load(Ordering::SeqCst) {
                stop = Some(StopReason::Cancellation);
                break;
            }
            match one.input() {
                "hold" | "holdpanic" => {
                    entered = true;
                    // Once the test has dropped its ends, neither blocks.
                    let _ = self.entered.send((request.id(), true));
                    let _ = self.gate.recv();
                    if one.input() == "holdpanic" {
                        panic!("the test runner panics after its gate opened");
                    }
                }
                "panic" => panic!("the test runner panics in a request"),
                _ => {}
            }
            reports.push(value(request.versions(), one.input()));
        }
        if holds && !entered {
            let _ = self.entered.send((request.id(), false));
        }
        if let Ok(mut seen) = self.seen.lock() {
            seen.push((request.id(), stop.is_some()));
        }
        RunReport::new(reports, stop)
    }
}

/// What the test keeps of the test runner.
struct Probe {
    /// Receives each job that entered a `hold` or `holdpanic` request, with
    /// `true`, and each job with such a request that ended without entering
    /// one, with `false`.
    entered: Receiver<(JobId, bool)>,
    /// Opens the gate of one `hold` or `holdpanic` request for each
    /// message.
    gate: Sender<()>,
    /// Shared by the preparing half and every cancel target, so its count
    /// is 2 while no cancel handle exists.
    targets: Arc<()>,
    /// How often a cancel handle set its job's cancellation.
    sets: Arc<AtomicUsize>,
    /// Shared by the runner, so its count is 1 once the runner is gone.
    runner_alive: Arc<()>,
    /// The names of the threads that ran jobs.
    threads: Arc<Mutex<Vec<Option<String>>>>,
    /// For each job that the runner ran without panicking, whether it saw
    /// the job's cancellation.
    seen: Arc<Mutex<Vec<(JobId, bool)>>>,
}

/// A host stub with the test runner, and what the test keeps of the
/// runner. A test binds the probe after the job layer, as in
/// `let (layer, probe) = recorded()?`, so that a failing test drops the gate
/// before the job layer joins a worker that waits at it.
fn probed() -> Checked<(HostStub<Prepare>, Probe)> {
    let (entered_by_runner, entered) = channel();
    let (gate, gate_of_runner) = channel();
    let probe = Probe {
        entered,
        gate,
        targets: Arc::new(()),
        sets: Arc::new(AtomicUsize::new(0)),
        runner_alive: Arc::new(()),
        threads: Arc::new(Mutex::new(Vec::new())),
        seen: Arc::new(Mutex::new(Vec::new())),
    };
    let run = Run {
        _runner_alive: Arc::clone(&probe.runner_alive),
        threads: Arc::clone(&probe.threads),
        seen: Arc::clone(&probe.seen),
        entered: entered_by_runner,
        gate: gate_of_runner,
    };
    let prepare = Prepare {
        targets: Arc::clone(&probe.targets),
        sets: Arc::clone(&probe.sets),
    };
    let stub = HostStub::new(prepare, run)?;
    assert_eq!(stub.live_threads(), 1);
    Ok((stub, probe))
}

/// A job layer on a host stub with the test runner, and what the test keeps
/// of the runner.
fn stubbed() -> Checked<(JobLayer<HostStub<Prepare>>, Probe)> {
    let (stub, probe) = probed()?;
    Ok((JobLayer::new(stub, Box::new(Still)), probe))
}

/// A backend that passes everything on to a host stub and notes each report
/// that it passes back.
struct Recording {
    stub: HostStub<Prepare>,
    reports: Vec<Report>,
}

/// The most reports that a recording backend passes back: no test makes
/// more, so more mean that the stub repeats its reports.
const MOST_REPORTS: usize = 4 * MAX_LIVE_JOBS;

impl Recording {
    /// Notes `report` and passes it back. A report beyond
    /// `MOST_REPORTS` fails the test instead of letting the job layer
    /// apply repeated reports for ever; while the test fails already, the
    /// reports end there.
    #[allow(
        clippy::panic,
        reason = "a stub that repeats its reports fails the test"
    )]
    fn noted(&mut self, report: Option<Report>) -> Option<Report> {
        if let Some(report) = &report {
            if self.reports.len() >= MOST_REPORTS {
                if std::thread::panicking() {
                    return None;
                }
                panic!("the stub passed back more than {MOST_REPORTS} reports");
            }
            self.reports.push(report.clone());
        }
        report
    }

    /// Asserts that each of `ids` got exactly one `Finished` and at most one
    /// `Started`.
    fn assert_finished_once(&self, ids: &[JobId]) {
        for id in ids {
            let started = self
                .reports
                .iter()
                .filter(|report| matches!(report, Report::Started(of) if of == id))
                .count();
            let finished = self
                .reports
                .iter()
                .filter(|report| matches!(report, Report::Finished(end) if end.id() == *id))
                .count();
            assert_eq!(finished, 1, "{id}: Finished reports");
            assert!(started <= 1, "{id}: {started} Started reports");
        }
    }

    /// The stops of the `Finished` reports, in order.
    fn stops(&self) -> Vec<Option<StopReason>> {
        self.reports
            .iter()
            .filter_map(|report| match report {
                Report::Finished(end) => Some(end.stop()),
                _ => None,
            })
            .collect()
    }
}

impl JobBackend for Recording {
    fn submit(&mut self, request: JobRequest) {
        self.stub.submit(request);
    }

    fn cancel(&mut self, id: JobId) {
        self.stub.cancel(id);
    }

    fn try_report(&mut self) -> Option<Report> {
        let report = self.stub.try_report();
        self.noted(report)
    }

    fn wait_report(&mut self) -> Option<Report> {
        let report = self.stub.wait_report();
        self.noted(report)
    }

    fn shutdown(&mut self) {
        self.stub.shutdown();
    }
}

/// A job layer on a recording host stub with the test runner, and what the
/// test keeps of the runner.
fn recorded() -> Checked<(JobLayer<Recording>, Probe)> {
    let (stub, probe) = probed()?;
    let backend = Recording {
        stub,
        reports: Vec::new(),
    };
    Ok((JobLayer::new(backend, Box::new(Still)), probe))
}

/// A job of one `eval` request for each of `texts`.
fn evals(session: SessionId, versions: Versions, texts: &[&str]) -> Checked<Submission> {
    let requests = texts
        .iter()
        .map(|text| Request::new(Operation::Eval, "", *text))
        .collect();
    Ok(Submission::several(session, versions, requests).ok_or("1 to 4 requests")?)
}

#[test]
fn shutdown_leaves_no_live_worker_and_one_terminal_state_per_job() -> Checked {
    let (mut layer, probe) = stubbed()?;
    let tagged = versions(1, 0, 0)?;
    let session = layer.open_session()?;
    let first = layer.submit(eval(session, tagged, "1"))?;
    let second = layer.submit(eval(session, tagged, "2"))?;

    layer.shutdown();
    assert_eq!(layer.backend().live_threads(), 0);
    assert_eq!(
        Arc::strong_count(&probe.runner_alive),
        1,
        "the worker's runner is gone"
    );
    let names = probe.threads.lock().map_err(|_| "poisoned")?.clone();
    assert_eq!(names, vec![Some(WORKER_NAME.to_string()); 2]);

    // Both jobs were still queued for the core, so both ended cancelled
    // when the shutdown cancelled them, and their reports were discarded.
    for id in [first, second] {
        let terminal = layer.take(id).ok_or("no terminal state")?;
        assert_eq!(terminal.record().state(), TerminalState::Cancelled);
        assert_eq!(layer.take(id), None);
    }
    assert!(layer.poll().is_empty());
    assert_eq!(layer.live_jobs(), 0);
    Ok(())
}

#[test]
fn a_runner_panic_fails_its_job_and_every_later_job() -> Checked {
    let (mut layer, _probe) = stubbed()?;
    let tagged = versions(1, 0, 0)?;
    let session = layer.open_session()?;
    let first = layer.submit(eval(session, tagged, "panic"))?;
    let terminal = layer.wait(first).ok_or("no terminal state")?;
    let record = terminal.record();
    assert_eq!(record.state(), TerminalState::Failed);
    assert_eq!(record.stop(), Some(StopReason::BackendInternalError));
    assert!(record.internal_error());
    assert!(record.lines().is_empty());

    let later = layer.submit(eval(session, tagged, "1"))?;
    let terminal = layer.wait(later).ok_or("no terminal state")?;
    let record = terminal.record();
    assert_eq!(record.state(), TerminalState::Failed);
    assert_eq!(record.stop(), Some(StopReason::BackendInternalError));
    assert!(record.internal_error());
    assert!(record.lines().is_empty());
    Ok(())
}

#[test]
fn dropping_the_layer_or_shutting_down_after_a_panic_leaves_no_live_worker() -> Checked {
    let tagged = versions(1, 0, 0)?;

    // Dropping the job layer sets the cancellation of the job that has not
    // ended, which is still queued for the core, and joins the worker.
    let (mut layer, probe) = stubbed()?;
    let session = layer.open_session()?;
    layer.submit(eval(session, tagged, "1"))?;
    drop(layer);
    assert_eq!(
        probe.sets.load(Ordering::SeqCst),
        1,
        "one cancellation set for the one job that had not ended"
    );
    assert_eq!(
        Arc::strong_count(&probe.runner_alive),
        1,
        "the worker's runner is gone"
    );

    // After the worker panicked, shutdown joins it without raising the
    // panic again.
    let (mut layer, probe) = stubbed()?;
    let session = layer.open_session()?;
    let id = layer.submit(eval(session, tagged, "panic"))?;
    let terminal = layer.wait(id).ok_or("no terminal state")?;
    assert_eq!(terminal.record().state(), TerminalState::Failed);
    layer.shutdown();
    assert_eq!(layer.backend().live_threads(), 0);
    assert_eq!(
        Arc::strong_count(&probe.runner_alive),
        1,
        "the worker's runner is gone"
    );
    Ok(())
}

#[test]
fn a_full_stub_refuses_nothing_blocks_nothing_and_finishes_each_job_once() -> Checked {
    let (mut layer, probe) = recorded()?;
    let tagged = versions(1, 0, 0)?;
    let session = layer.open_session()?;

    // The first job enters the runner and waits there.
    let first = layer.submit(eval(session, tagged, "hold"))?;
    assert!(layer.poll().is_empty());
    assert_eq!(probe.entered.recv()?, (first, true));

    // The other jobs fill the dispatch channel; none is refused, and none
    // ends while the worker is held.
    let mut ids = vec![first];
    for n in 2..=MAX_LIVE_JOBS {
        ids.push(layer.submit(eval(session, tagged, &n.to_string()))?);
    }
    assert!(
        layer.poll().is_empty(),
        "a job ended while the worker was held"
    );
    assert_eq!(layer.state(first), Some(JobState::Running));
    for id in &ids[1..] {
        assert_eq!(layer.state(*id), Some(JobState::Queued));
    }
    assert_eq!(layer.live_jobs(), MAX_LIVE_JOBS);

    // Released, the worker reports every job while the shutdown joins it,
    // and the job layer applies the reports before the shutdown returns.
    probe.gate.send(())?;
    layer.shutdown();
    for id in &ids {
        assert_eq!(
            layer.state(*id),
            Some(JobState::Terminal(TerminalState::Cancelled))
        );
    }
    let backend = layer.backend();
    backend.assert_finished_once(&ids);
    assert!(
        !backend
            .stops()
            .contains(&Some(StopReason::BackendInternalError)),
        "{:?}",
        backend.stops()
    );
    assert_eq!(backend.stub.live_threads(), 0);
    for id in &ids {
        let terminal = layer.take(*id).ok_or("no terminal state")?;
        assert!(terminal.record().lines().is_empty());
    }
    assert_eq!(layer.live_jobs(), 0);

    // A job submitted after the shutdown fails, with one Finished, and no
    // report follows.
    let late = layer.submit(eval(session, tagged, "late"))?;
    let terminal = layer.wait(late).ok_or("no terminal state")?;
    assert_eq!(terminal.record().state(), TerminalState::Failed);
    assert_eq!(
        terminal.record().stop(),
        Some(StopReason::BackendInternalError)
    );
    assert!(layer.poll().is_empty());
    layer.backend().assert_finished_once(&[late]);
    Ok(())
}

#[test]
fn a_panic_fails_the_held_job_and_the_jobs_in_the_dispatch_channel_once_each() -> Checked {
    let (mut layer, probe) = recorded()?;
    let tagged = versions(1, 0, 0)?;
    let session = layer.open_session()?;
    let no_handle = Arc::strong_count(&probe.targets);

    // The first job waits in its first request and panics in its second;
    // two more jobs wait in the dispatch channel meanwhile.
    let first = layer.submit(evals(session, tagged, &["hold", "panic"])?)?;
    assert!(layer.poll().is_empty());
    assert_eq!(probe.entered.recv()?, (first, true));
    let second = layer.submit(eval(session, tagged, "2"))?;
    let third = layer.submit(eval(session, tagged, "3"))?;
    assert!(
        layer.poll().is_empty(),
        "a job ended while the worker was held"
    );

    probe.gate.send(())?;
    for id in [first, second, third] {
        let terminal = layer.wait(id).ok_or("no terminal state")?;
        let record = terminal.record();
        assert_eq!(record.state(), TerminalState::Failed, "{id}");
        assert_eq!(record.stop(), Some(StopReason::BackendInternalError));
        assert!(record.internal_error());
        assert!(record.lines().is_empty());
    }

    // A job submitted after the worker ended fails at once, and no report
    // follows.
    let later = layer.submit(eval(session, tagged, "4"))?;
    let terminal = layer.wait(later).ok_or("no terminal state")?;
    assert_eq!(terminal.record().state(), TerminalState::Failed);
    assert!(layer.poll().is_empty());
    layer
        .backend()
        .assert_finished_once(&[first, second, third, later]);
    assert_eq!(layer.live_jobs(), 0);

    // Once the shutdown has joined the worker, nothing holds a cancel
    // handle: not the stub for the jobs that it finished itself or failed
    // at once, nor the dispatch channel for the jobs left in it.
    layer.shutdown();
    assert_eq!(
        Arc::strong_count(&probe.targets),
        no_handle,
        "the stub and the worker let go of every cancel handle"
    );
    Ok(())
}

#[test]
fn a_shutdown_after_a_worker_panic_finishes_the_jobs_left_in_the_dispatch_channel() -> Checked {
    let (mut layer, probe) = recorded()?;
    let tagged = versions(1, 0, 0)?;
    let session = layer.open_session()?;

    // The first job waits in its first request and panics in its second;
    // two more jobs wait in the dispatch channel meanwhile.
    let first = layer.submit(evals(session, tagged, &["hold", "panic"])?)?;
    assert!(layer.poll().is_empty());
    assert_eq!(probe.entered.recv()?, (first, true));
    let second = layer.submit(eval(session, tagged, "2"))?;
    let third = layer.submit(eval(session, tagged, "3"))?;
    probe.gate.send(())?;
    let terminal = layer.wait(first).ok_or("no terminal state")?;
    assert_eq!(terminal.record().state(), TerminalState::Failed);

    // Only the shutdown, without a poll or wait before it, takes the
    // reports that the stub makes itself for the two other jobs.
    layer.shutdown();
    for id in [second, third] {
        let terminal = layer.take(id).ok_or("no terminal state")?;
        assert_eq!(terminal.record().state(), TerminalState::Cancelled, "{id}");
    }
    let backend = layer.backend();
    backend.assert_finished_once(&[first, second, third]);
    assert_eq!(backend.stops(), [Some(StopReason::BackendInternalError); 3]);
    assert_eq!(layer.live_jobs(), 0);
    Ok(())
}

#[test]
fn a_worker_panic_during_a_shutdown_finishes_the_job_in_the_dispatch_channel() -> Checked {
    let (mut layer, probe) = recorded()?;
    let tagged = versions(1, 0, 0)?;
    let session = layer.open_session()?;

    // The first job runs and waits in a request that panics once its gate
    // opens, whatever its flag; the second waits in the dispatch channel.
    let first = layer.submit(eval(session, tagged, "holdpanic"))?;
    assert!(layer.poll().is_empty());
    assert_eq!(probe.entered.recv()?, (first, true));
    assert!(layer.poll().is_empty());
    assert_eq!(layer.state(first), Some(JobState::Running));
    let second = layer.submit(eval(session, tagged, "2"))?;

    // Without a poll, the worker panics while the job layer shuts down; the
    // shutdown applies the stub's own report for the second job too.
    probe.gate.send(())?;
    layer.shutdown();
    for id in [first, second] {
        let terminal = layer.take(id).ok_or("no terminal state")?;
        assert_eq!(terminal.record().state(), TerminalState::Cancelled, "{id}");
        assert!(terminal.record().lines().is_empty());
    }
    let backend = layer.backend();
    backend.assert_finished_once(&[first, second]);
    assert_eq!(backend.stops(), [Some(StopReason::BackendInternalError); 2]);
    assert_eq!(backend.stub.live_threads(), 0);
    assert_eq!(layer.live_jobs(), 0);
    Ok(())
}

#[test]
fn the_runner_sees_each_cancellation_and_the_stub_lets_go_of_each_handle() -> Checked {
    let (mut layer, probe) = recorded()?;
    let tagged = versions(1, 0, 0)?;
    let session = layer.open_session()?;
    let no_handle = Arc::strong_count(&probe.targets);

    // The first job waits in the first of its two requests, and the second
    // waits in the dispatch channel; both are cancelled meanwhile.
    let first = layer.submit(evals(session, tagged, &["hold", "1"])?)?;
    assert!(layer.poll().is_empty());
    assert_eq!(probe.entered.recv()?, (first, true));
    assert!(layer.poll().is_empty());
    assert_eq!(layer.state(first), Some(JobState::Running));
    let second = layer.submit(eval(session, tagged, "2"))?;
    assert_eq!(Arc::strong_count(&probe.targets), no_handle + 2);
    assert_eq!(layer.cancel(first), CancelAnswer::Requested);
    assert_eq!(layer.cancel(second), CancelAnswer::Cancelled);
    probe.gate.send(())?;

    let terminal = layer.wait(first).ok_or("no terminal state")?;
    assert_eq!(terminal.record().state(), TerminalState::Cancelled);
    let terminal = layer.take(second).ok_or("no terminal state")?;
    assert_eq!(terminal.record().state(), TerminalState::Cancelled);

    // The worker runs jobs in order, so once the third job has ended the
    // stub has the second job's Finished too.
    let third = layer.submit(eval(session, tagged, "3"))?;
    let terminal = layer.wait(third).ok_or("no terminal state")?;
    assert_eq!(terminal.record().state(), TerminalState::Completed);
    let seen = probe.seen.lock().map_err(|_| "poisoned")?.clone();
    assert_eq!(seen, [(first, true), (second, true), (third, false)]);
    assert_eq!(
        Arc::strong_count(&probe.targets),
        no_handle,
        "the stub and the worker let go of every cancel handle"
    );
    layer
        .backend()
        .assert_finished_once(&[first, second, third]);
    assert_eq!(layer.live_jobs(), 0);
    Ok(())
}

#[test]
fn cancelling_one_job_sets_only_its_own_cancellation() -> Checked {
    let (mut layer, probe) = recorded()?;
    let tagged = versions(1, 0, 0)?;
    let session = layer.open_session()?;

    // The first job waits in the first of its two requests; the second
    // waits in the dispatch channel and is the only one cancelled.
    let first = layer.submit(evals(session, tagged, &["hold", "1"])?)?;
    assert!(layer.poll().is_empty());
    assert_eq!(probe.entered.recv()?, (first, true));
    let second = layer.submit(eval(session, tagged, "2"))?;
    assert_eq!(layer.cancel(second), CancelAnswer::Cancelled);
    probe.gate.send(())?;

    let terminal = layer.wait(first).ok_or("no terminal state")?;
    assert_eq!(terminal.record().state(), TerminalState::Completed);
    assert_eq!(terminal.record().lines().len(), 2);
    let terminal = layer.take(second).ok_or("no terminal state")?;
    assert_eq!(terminal.record().state(), TerminalState::Cancelled);

    // The shutdown joins the worker after it ran the second job too.
    layer.shutdown();
    let seen = probe.seen.lock().map_err(|_| "poisoned")?.clone();
    assert_eq!(seen, [(first, false), (second, true)]);
    assert_eq!(probe.sets.load(Ordering::SeqCst), 1);
    layer.backend().assert_finished_once(&[first, second]);
    assert_eq!(layer.live_jobs(), 0);
    Ok(())
}
