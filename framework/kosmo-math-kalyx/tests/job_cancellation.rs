//! Cancellation of jobs that the kalyx runner executes on the job layer's
//! host stub: before a job starts, while kalyx runs its request, and
//! between two requests.
//!
//! A gate at a reading of the worker thread's clock holds the worker while
//! the test acts on the job layer: the core handles the job's `Started`,
//! accepts the cancel request and sets the job's flag, and opening the gate
//! makes that visible to the worker. A job with a deadline reads the clock
//! before each request, and kalyx reads it at the first charge point of
//! each request, so the readings count what ran after the gate.

mod support;

use kosmo_jobs::{
    CancelAnswer, ErrorClass, JobState, Limits, Operation, OutcomeKind, Report, Request,
    SerializeFailure, StopReason, Submission, TerminalState,
};
use support::Checked;
use support::jobs::{KalyxJobs, typed};

/// A time limit that no test reaches.
const FAR: u64 = 1 << 40;

#[test]
fn a_job_cancelled_before_it_starts_runs_no_request() -> Checked {
    let mut jobs = KalyxJobs::new()?;
    let session = jobs.layer.open_session()?;
    let line = typed("1/3+1/6")?;
    // The first job waits at its first reading, before its first request,
    // so the second stays in the dispatch channel.
    jobs.clock.gate_at(1);
    let first = jobs.layer.submit(line.eval(session).with_time_limit(FAR))?;
    jobs.clock.wait_at_gate();
    let second = jobs.layer.submit(line.eval(session))?;
    assert!(jobs.layer.poll().is_empty());
    assert_eq!(jobs.layer.state(first), Some(JobState::Running));
    assert_eq!(jobs.layer.state(second), Some(JobState::Queued));

    assert_eq!(jobs.layer.cancel(second), CancelAnswer::Cancelled);
    let cancelled = jobs.layer.take(second).ok_or("not terminal")?;
    assert_eq!(cancelled.record().state(), TerminalState::Cancelled);
    assert_eq!(cancelled.record().stop(), Some(StopReason::Cancellation));
    assert!(cancelled.record().lines().is_empty());
    assert_eq!(cancelled.record().echoed(), None);

    jobs.clock.open();
    let done = jobs.layer.wait(first).ok_or("no terminal state")?;
    assert_eq!(done.record().state(), TerminalState::Completed);
    assert_eq!(done.record().lines(), &[b"value\t1\t0\t0\t1/2\n".to_vec()]);

    // The worker took the second job, reported it started, and the runner
    // found its flag before its first request; the core discards both
    // reports.
    jobs.layer.shutdown();
    let recorded = jobs.layer.backend();
    assert!(recorded.reports().contains(&Report::Started(second)));
    let finished = jobs.finished(second)?;
    assert!(finished.requests().is_empty(), "no request ran");
    assert_eq!(finished.stop(), Some(StopReason::Cancellation));
    assert_eq!(jobs.layer.state(second), None);
    assert!(jobs.layer.take(second).is_none());
    assert_eq!(jobs.layer.live_jobs(), 0);
    Ok(())
}

#[test]
fn a_running_job_stops_at_the_next_charge_point_after_its_cancellation() -> Checked {
    let mut jobs = KalyxJobs::new()?;
    let session = jobs.layer.open_session()?;
    let line = typed("1")?;
    // The first reading is the runner's, before the request; the second is
    // kalyx's at its first charge point, after it found no cancellation.
    jobs.clock.gate_at(2);
    let expand = Request::new(Operation::Expand, "x", "(x+1)^5");
    let submission = Submission::new(session, line.versions, expand).with_time_limit(FAR);
    let id = jobs.layer.submit(submission)?;
    jobs.clock.wait_at_gate();
    assert!(jobs.layer.poll().is_empty());
    assert_eq!(jobs.layer.state(id), Some(JobState::Running));
    assert_eq!(jobs.layer.cancel(id), CancelAnswer::Requested);
    jobs.clock.open();

    let terminal = jobs.layer.wait(id).ok_or("no terminal state")?;
    let record = terminal.record();
    assert_eq!(record.state(), TerminalState::Cancelled);
    assert_eq!(record.stop(), Some(StopReason::Cancellation));
    assert!(record.lines().is_empty());

    let finished = jobs.finished(id)?;
    let [report] = finished.requests() else {
        return Err("not one request report".into());
    };
    // kalyx found the cancellation at the charge point after the gate, which
    // checks it before any reading of the clock, so the clock was not read
    // again and the expansion gave no value.
    assert_eq!(report.kind(), OutcomeKind::Cancelled);
    assert_eq!(
        report.error(),
        ErrorClass::Serialize(SerializeFailure::Cancelled)
    );
    assert_eq!(finished.stop(), None);
    assert_eq!(jobs.clock.worker_readings(), 2);
    Ok(())
}

#[test]
fn a_cancellation_seen_between_two_requests_runs_no_second_request() -> Checked {
    let mut jobs = KalyxJobs::new()?;
    let session = jobs.layer.open_session()?;
    let line = typed("12345")?;
    let mut limits = Limits::new();
    limits.max_input_bytes = 4;
    limits.max_output_bytes = 4;
    let requests = vec![
        Request::new(Operation::Eval, "", line.text.as_str()),
        Request::new(Operation::Eval, "", "1/3+1/6"),
    ];
    let submission = Submission::several(session, line.versions, requests)
        .ok_or("two requests")?
        .with_limits(limits)
        .with_time_limit(FAR);
    // The first reading is the runner's, before the first request. That
    // request's input is over `max_input_bytes`, so it fails before its
    // first charge point; the second reading is the first charge of its
    // error line, after the last check for cancellation in the request.
    jobs.clock.gate_at(2);
    let id = jobs.layer.submit(submission)?;
    jobs.clock.wait_at_gate();
    assert!(jobs.layer.poll().is_empty());
    assert_eq!(jobs.layer.state(id), Some(JobState::Running));
    assert_eq!(jobs.layer.cancel(id), CancelAnswer::Requested);
    jobs.clock.open();

    let terminal = jobs.layer.wait(id).ok_or("no terminal state")?;
    let record = terminal.record();
    assert_eq!(record.state(), TerminalState::Cancelled);
    assert_eq!(record.stop(), Some(StopReason::Cancellation));
    assert!(record.lines().is_empty());

    let finished = jobs.finished(id)?;
    assert_eq!(finished.stop(), Some(StopReason::Cancellation));
    let [report] = finished.requests() else {
        return Err("not one request report".into());
    };
    // The error line did not fit `max_output_bytes` and was written again
    // as the fallback line.
    assert_eq!(report.kind(), OutcomeKind::ResourceLimit);
    assert_eq!(
        report.error(),
        ErrorClass::Serialize(SerializeFailure::ResourceLimit)
    );
    let fallback = report.line().ok_or("no fallback line")?;
    assert!(fallback.starts_with(b"error\t1\t0\t0\tResourceLimit\t"));
    // No reading after the gate: the runner found the flag before the
    // second request and did not run it.
    assert_eq!(jobs.clock.worker_readings(), 2);
    Ok(())
}
