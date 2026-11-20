//! The host stub with a test runner: shutdown, a runner that panics, and
//! dropping the job layer. Each test waits only for terminal states and
//! for the worker thread to be joined, never for time.

mod support;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use kosmo_jobs::stub::{HostStub, WORKER_NAME};
use kosmo_jobs::{
    CancelHandle, CancelTarget, JobId, JobLayer, JobPreparer, JobRunner, RunReport, StopReason,
    TerminalState, WorkItem,
};
use support::{Checked, Still, eval, value, versions};

/// The cancel target of the test runner.
struct Flag(AtomicBool);

impl CancelTarget for Flag {
    fn set(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

/// The preparing half of the test runner.
struct Prepare;

impl JobPreparer for Prepare {
    fn prepare(&mut self, _id: JobId) -> CancelHandle {
        CancelHandle::new(Flag(AtomicBool::new(false)))
    }
}

/// The running half of the test runner: a request whose text is `panic`
/// panics, and every other request gives the value of its text. It holds a
/// token, and notes the name of the thread that runs each job.
struct Run {
    _token: Arc<()>,
    threads: Arc<Mutex<Vec<Option<String>>>>,
}

impl JobRunner for Run {
    #[allow(
        clippy::panic,
        reason = "the runner panics on purpose to end the worker thread"
    )]
    fn run(&mut self, item: WorkItem) -> RunReport {
        let name = std::thread::current().name().map(String::from);
        if let Ok(mut threads) = self.threads.lock() {
            threads.push(name);
        }
        let set = item
            .cancel_handle()
            .target::<Flag>()
            .is_none_or(|flag| flag.0.load(Ordering::SeqCst));
        if set {
            return RunReport::new(Vec::new(), Some(StopReason::Cancellation));
        }
        let request = item.request();
        let mut reports = Vec::new();
        for one in request.requests() {
            if one.input() == "panic" {
                panic!("the test runner panics in a request");
            }
            reports.push(value(request.versions(), one.input()));
        }
        RunReport::new(reports, None)
    }
}

/// A job layer on a host stub with the test runner, its token and the
/// names of the threads that ran jobs.
type Stubbed = (
    JobLayer<HostStub<Prepare>>,
    Arc<()>,
    Arc<Mutex<Vec<Option<String>>>>,
);

fn stubbed() -> Checked<Stubbed> {
    let token = Arc::new(());
    let threads = Arc::new(Mutex::new(Vec::new()));
    let run = Run {
        _token: Arc::clone(&token),
        threads: Arc::clone(&threads),
    };
    let stub = HostStub::new(Prepare, run)?;
    assert_eq!(stub.live_threads(), 1);
    Ok((JobLayer::new(stub, Box::new(Still)), token, threads))
}

#[test]
fn shutdown_leaves_no_live_worker_and_one_terminal_state_per_job() -> Checked {
    let (mut layer, token, threads) = stubbed()?;
    let tagged = versions(1, 0, 0)?;
    let session = layer.open_session()?;
    let first = layer.submit(eval(session, tagged, "1"))?;
    let second = layer.submit(eval(session, tagged, "2"))?;

    layer.shutdown();
    assert_eq!(layer.backend().live_threads(), 0);
    assert_eq!(Arc::strong_count(&token), 1, "the worker's runner is gone");
    let names = threads.lock().map_err(|_| "poisoned")?.clone();
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
    let (mut layer, _token, _threads) = stubbed()?;
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

    // Dropping the job layer joins the worker.
    let (mut layer, token, _threads) = stubbed()?;
    let session = layer.open_session()?;
    layer.submit(eval(session, tagged, "1"))?;
    drop(layer);
    assert_eq!(Arc::strong_count(&token), 1, "the worker's runner is gone");

    // After the worker panicked, shutdown joins it without raising the
    // panic again.
    let (mut layer, token, _threads) = stubbed()?;
    let session = layer.open_session()?;
    let id = layer.submit(eval(session, tagged, "panic"))?;
    let terminal = layer.wait(id).ok_or("no terminal state")?;
    assert_eq!(terminal.record().state(), TerminalState::Failed);
    layer.shutdown();
    assert_eq!(layer.backend().live_threads(), 0);
    assert_eq!(Arc::strong_count(&token), 1, "the worker's runner is gone");
    Ok(())
}
