//! The shell of the job layer on a controllable backend: what an accepted
//! submission hands to the backend, when the job layer reads its clock, and
//! a shutdown while a job runs.

mod support;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use kosmo_jobs::controllable::{ControllableBackend, Gate, Plan, Release};
use kosmo_jobs::{
    JobClock, JobLayer, JobRequest, JobState, Limits, MAX_LIVE_JOBS, Rejected, TerminalState,
};
use support::{Checked, controllable, eval, value, versions};

/// A clock that stands at one reading and counts how often it is read.
struct Counting {
    reading: u64,
    reads: Arc<AtomicUsize>,
}

impl JobClock for Counting {
    fn now(&mut self) -> u64 {
        self.reads.fetch_add(1, Ordering::SeqCst);
        self.reading
    }

    fn fork(&self) -> Box<dyn JobClock> {
        Box::new(Counting {
            reading: self.reading,
            reads: Arc::clone(&self.reads),
        })
    }
}

#[test]
fn an_accepted_job_carries_its_limits_and_a_deadline_read_once_from_the_clock() -> Checked {
    const NOW: u64 = 1_000;
    const TIME_LIMIT: u64 = 250;
    let reads = Arc::new(AtomicUsize::new(0));
    let clock = Counting {
        reading: NOW,
        reads: Arc::clone(&reads),
    };
    let mut layer = JobLayer::new(ControllableBackend::new(), Box::new(clock));
    let tagged = versions(1, 0, 0)?;
    let session = layer.open_session()?;
    let mut limits = Limits::new();
    limits.max_input_bytes = 4;
    limits.max_work = 7;

    let submission = eval(session, tagged, "1")
        .with_limits(limits)
        .with_time_limit(TIME_LIMIT);
    layer.submit(submission)?;
    let job = layer.backend().request(0).ok_or("not received")?;
    assert_eq!(job.limits(), limits);
    assert_eq!(job.deadline(), Some(NOW + TIME_LIMIT));
    assert_eq!(reads.load(Ordering::SeqCst), 1);

    // Without a time limit the job has no deadline and the clock is not
    // read; the default limits apply.
    layer.submit(eval(session, tagged, "2"))?;
    let job = layer.backend().request(1).ok_or("not received")?;
    assert_eq!(job.deadline(), None);
    assert_eq!(job.limits(), Limits::DEFAULT);
    assert_eq!(reads.load(Ordering::SeqCst), 1);

    // The deadline saturates.
    layer.submit(eval(session, tagged, "3").with_time_limit(u64::MAX))?;
    let deadline = layer.backend().request(2).and_then(JobRequest::deadline);
    assert_eq!(deadline, Some(u64::MAX));
    assert_eq!(reads.load(Ordering::SeqCst), 2);

    // Jobs without a plan wait before they start and stay live, so the job
    // layer fills up; a refused submission reads no clock.
    for n in 3..MAX_LIVE_JOBS {
        layer.submit(eval(session, tagged, &n.to_string()))?;
    }
    let refused = eval(session, tagged, "9").with_time_limit(TIME_LIMIT);
    assert_eq!(
        layer.submit(refused),
        Err(Rejected::Full {
            live: MAX_LIVE_JOBS
        })
    );
    assert_eq!(reads.load(Ordering::SeqCst), 2);
    Ok(())
}

#[test]
fn a_shutdown_ends_a_running_job_cancelled_before_it_returns() -> Checked {
    let tagged = versions(1, 0, 0)?;
    let mut backend = ControllableBackend::new();
    backend.plan(
        0,
        Plan::new()
            .request(value(tagged, "1"))
            .gate(Gate::Finished, Release::Manual),
    );
    let mut layer = controllable(backend);
    let session = layer.open_session()?;
    let id = layer.submit(eval(session, tagged, "1"))?;
    assert!(layer.poll().is_empty());
    assert_eq!(layer.state(id), Some(JobState::Running));

    // Without a poll: the shutdown applied the backend's last reports.
    layer.shutdown();
    assert_eq!(
        layer.state(id),
        Some(JobState::Terminal(TerminalState::Cancelled))
    );
    assert!(layer.backend().is_shut_down());
    assert_eq!(layer.backend().cancels(), &[id]);
    let terminal = layer.take(id).ok_or("no terminal state")?;
    assert_eq!(terminal.record().state(), TerminalState::Cancelled);
    assert!(terminal.record().lines().is_empty());
    assert_eq!(layer.live_jobs(), 0);
    Ok(())
}
