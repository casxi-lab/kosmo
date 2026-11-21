//! The shell of the job layer on a controllable backend: what an accepted
//! submission hands to the backend, when the job layer reads its clock, a
//! shutdown while a job runs, a shutdown with jobs that have no plan, and
//! the cancellation of one job among several.

mod support;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use kosmo_jobs::controllable::{ControllableBackend, Gate, Plan, Release};
use kosmo_jobs::{
    CancelAnswer, JobClock, JobLayer, JobRequest, JobState, Limits, MAX_LIVE_JOBS, Rejected,
    Report, StopReason, TerminalState,
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

#[test]
fn a_shutdown_finishes_the_jobs_that_have_no_plan() -> Checked {
    let tagged = versions(1, 0, 0)?;
    let mut layer = controllable(ControllableBackend::new());
    let session = layer.open_session()?;

    // A job without a plan waits before it starts, so it is still queued.
    let queued = layer.submit(eval(session, tagged, "1"))?;
    assert!(layer.poll().is_empty());
    assert_eq!(layer.state(queued), Some(JobState::Queued));
    layer.shutdown();

    // The shutdown set the job's cancellation before it stopped the
    // backend, so the backend stopped the job before its first request.
    let stop = layer
        .backend()
        .sent()
        .iter()
        .find_map(|report| match report {
            Report::Finished(finished) if finished.id() == queued => Some(finished.stop()),
            _ => None,
        });
    assert_eq!(stop, Some(Some(StopReason::Cancellation)));
    let terminal = layer.take(queued).ok_or("no terminal state")?;
    assert_eq!(terminal.record().state(), TerminalState::Cancelled);
    assert_eq!(terminal.record().stop(), Some(StopReason::Cancellation));
    assert_eq!(layer.live_jobs(), 0, "the backend finished the job");

    // A job without a plan that arrives after the shutdown ends too.
    let late = layer.submit(eval(session, tagged, "2"))?;
    layer.wait(late).ok_or("no terminal state")?;
    assert_eq!(layer.live_jobs(), 0);
    Ok(())
}

#[test]
fn a_cancel_request_marks_only_its_job_in_the_controllable_backend() -> Checked {
    let tagged = versions(1, 0, 0)?;
    let mut backend = ControllableBackend::new();
    for order in 0..2 {
        backend.plan(
            order,
            Plan::new()
                .request(value(tagged, "1"))
                .gate(Gate::Between(0), Release::Manual),
        );
    }
    let mut layer = controllable(backend);
    let session = layer.open_session()?;
    let first = layer.submit(eval(session, tagged, "1"))?;
    let second = layer.submit(eval(session, tagged, "1"))?;
    assert!(layer.poll().is_empty());

    // Both jobs wait before their first request; only the first is
    // cancelled.
    assert_eq!(layer.cancel(first), CancelAnswer::Requested);
    assert!(layer.backend().was_cancelled(0));
    assert!(!layer.backend().was_cancelled(1));
    for order in 0..2 {
        layer.controllable().release(order, Gate::Between(0));
    }
    assert_eq!(layer.poll(), vec![first, second]);
    let first_end = layer.take(first).ok_or("no terminal state")?;
    assert_eq!(first_end.record().state(), TerminalState::Cancelled);
    let second_end = layer.take(second).ok_or("no terminal state")?;
    assert_eq!(second_end.record().state(), TerminalState::Completed);
    assert_eq!(second_end.record().lines().len(), 1);
    assert_eq!(layer.live_jobs(), 0);
    Ok(())
}
