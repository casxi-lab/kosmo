//! Deadlines through the injected clock: the job layer reads its clock once
//! when it accepts a job with a time limit, and the kalyx runner compares
//! readings of its fork of that clock with the job's deadline before each
//! request.

mod support;

use kosmo_jobs::{JobState, StopReason, TerminalState};
use support::Checked;
use support::jobs::{KalyxJobs, typed};

/// A time limit that no test reaches.
const FAR: u64 = 1 << 40;

#[test]
fn a_deadline_that_passed_while_the_job_was_queued_runs_no_request() -> Checked {
    let mut jobs = KalyxJobs::new()?;
    let session = jobs.layer.open_session()?;
    let line = typed("1/3+1/6")?;
    // The first job waits at its first reading, before its first request.
    jobs.clock.gate_at(1);
    let first = jobs.layer.submit(line.eval(session).with_time_limit(FAR))?;
    jobs.clock.wait_at_gate()?;

    let (start, limit) = (1_000, 500);
    jobs.clock.set(start);
    let second = jobs
        .layer
        .submit(line.eval(session).with_time_limit(limit))?;
    assert_eq!(
        jobs.clock.layer_readings(),
        2,
        "one reading for each accepted job with a time limit"
    );
    let received = jobs
        .layer
        .backend()
        .received(second)
        .ok_or("not received")?;
    assert_eq!(received.deadline(), Some(start + limit));
    assert_eq!(jobs.layer.state(second), Some(JobState::Queued));

    jobs.clock.set(start + limit);
    jobs.clock.open();
    let done = jobs.layer.wait(first).ok_or("no terminal state")?;
    assert_eq!(done.record().state(), TerminalState::Completed);

    // Taken from the dispatch channel, the second job finds before its first
    // request that a reading is at least its deadline.
    let terminal = jobs.layer.wait(second).ok_or("no terminal state")?;
    let record = terminal.record();
    assert_eq!(record.state(), TerminalState::ResourceExhausted);
    assert_eq!(record.stop(), Some(StopReason::Deadline));
    assert!(record.lines().is_empty());
    assert_eq!(record.echoed(), None);
    let finished = jobs.finished(second)?;
    assert!(finished.requests().is_empty(), "no request ran");
    assert_eq!(finished.stop(), Some(StopReason::Deadline));
    assert_eq!(jobs.clock.layer_readings(), 2);
    Ok(())
}
