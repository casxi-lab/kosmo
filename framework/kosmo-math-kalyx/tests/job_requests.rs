//! Jobs of several requests that the kalyx runner executes on the job
//! layer's host stub: the requests run in order, each with its own copy of
//! the job's limits; a fault does not stop the job, and a deadline seen
//! between two requests keeps the line of the request that ran.

mod support;

use kosmo_jobs::{
    ErrorClass, Limits, Operation, OutcomeKind, Request, StopReason, Submission, TerminalState,
};
use support::Checked;
use support::jobs::{KalyxJobs, typed};

/// The value line of `1/3+1/6` at the versions 1, 0 and 0: 16 bytes.
const VALUE: &[u8] = b"value\t1\t0\t0\t1/2\n";

/// The error line of `12345` under `max_input_bytes = 4`.
const OVER_INPUT: &[u8] =
    b"error\t1\t0\t0\tResourceLimit\t0..5\tField(max_input_bytes)\t-\t4\tRequired(5)\t\
                            resource limit exceeded: max_input_bytes\n";

#[test]
fn each_request_of_a_job_gets_its_own_copy_of_the_limits() -> Checked {
    let mut jobs = KalyxJobs::new()?;
    let session = jobs.layer.open_session()?;
    let line = typed("1/3+1/6")?;
    // The line of each request fits; the lines of both together would not.
    let mut limits = Limits::new();
    limits.max_output_bytes = 16;
    let requests = vec![
        Request::new(Operation::Eval, "", line.text.as_str()),
        Request::new(Operation::Eval, "", line.text.as_str()),
    ];
    let submission = Submission::several(session, line.versions, requests)
        .ok_or("two requests")?
        .with_limits(limits);
    let id = jobs.layer.submit(submission)?;
    let terminal = jobs.layer.wait(id).ok_or("no terminal state")?;
    let record = terminal.record();
    assert_eq!(record.state(), TerminalState::Completed);
    assert_eq!(record.stop(), None);
    assert_eq!(record.lines(), &[VALUE.to_vec(), VALUE.to_vec()]);

    let finished = jobs.finished(id)?;
    assert_eq!(finished.requests().len(), 2);
    assert_eq!(finished.stop(), None);
    Ok(())
}

#[test]
fn the_second_request_runs_under_the_limits_of_the_job() -> Checked {
    let mut jobs = KalyxJobs::new()?;
    let session = jobs.layer.open_session()?;
    let line = typed("12345")?;
    // Under kalyx's default limits the second request would write a value
    // line; under the job's limits it fails as the first does.
    let mut limits = Limits::new();
    limits.max_input_bytes = 4;
    let requests = vec![
        Request::new(Operation::Eval, "", line.text.as_str()),
        Request::new(Operation::Eval, "", line.text.as_str()),
    ];
    let submission = Submission::several(session, line.versions, requests)
        .ok_or("two requests")?
        .with_limits(limits);
    let id = jobs.layer.submit(submission)?;
    let terminal = jobs.layer.wait(id).ok_or("no terminal state")?;
    let record = terminal.record();
    assert_eq!(record.state(), TerminalState::Failed);
    assert_eq!(record.stop(), None);
    assert_eq!(record.lines(), &[OVER_INPUT.to_vec(), OVER_INPUT.to_vec()]);
    assert_eq!(jobs.finished(id)?.requests().len(), 2);
    Ok(())
}

#[test]
fn a_fault_in_the_first_request_does_not_stop_the_second() -> Checked {
    let mut jobs = KalyxJobs::new()?;
    let session = jobs.layer.open_session()?;
    let line = typed("1/0")?;
    let requests = vec![
        Request::new(Operation::Eval, "", line.text.as_str()),
        Request::new(Operation::Eval, "", "1/3+1/6"),
    ];
    let submission = Submission::several(session, line.versions, requests).ok_or("two requests")?;
    let id = jobs.layer.submit(submission)?;
    let terminal = jobs.layer.wait(id).ok_or("no terminal state")?;
    let record = terminal.record();
    assert_eq!(record.state(), TerminalState::Failed);
    assert_eq!(record.stop(), None);
    assert!(!record.internal_error());
    let [error, value] = record.lines() else {
        return Err("not two lines".into());
    };
    assert!(error.starts_with(b"error\t1\t0\t0\tDivisionByZero\t"));
    assert_eq!(value, VALUE);

    let finished = jobs.finished(id)?;
    assert_eq!(finished.stop(), None);
    let [first, second] = finished.requests() else {
        return Err("not two request reports".into());
    };
    assert_eq!(first.kind(), OutcomeKind::Fault);
    assert_eq!(second.kind(), OutcomeKind::Value);
    Ok(())
}

#[test]
fn a_deadline_seen_between_two_requests_delivers_the_line_of_the_first() -> Checked {
    let mut jobs = KalyxJobs::new()?;
    let session = jobs.layer.open_session()?;
    let (start, limit) = (1_000, 500);
    let deadline = start + limit;
    jobs.clock.set(start);
    // The runner reads the clock before the first request, whose input is
    // over `max_input_bytes`, so it fails before its first charge point;
    // kalyx reads it at the first charge of the error line, and the runner
    // again before the second request.
    jobs.clock.script(&[deadline - 1, deadline - 1, deadline]);
    let line = typed("12345")?;
    let mut limits = Limits::new();
    limits.max_input_bytes = 4;
    let requests = vec![
        Request::new(Operation::Eval, "", line.text.as_str()),
        Request::new(Operation::Eval, "", "1/3+1/6"),
    ];
    let submission = Submission::several(session, line.versions, requests)
        .ok_or("two requests")?
        .with_limits(limits)
        .with_time_limit(limit);
    let id = jobs.layer.submit(submission)?;
    let terminal = jobs.layer.wait(id).ok_or("no terminal state")?;
    let record = terminal.record();
    assert_eq!(record.state(), TerminalState::ResourceExhausted);
    assert_eq!(record.stop(), Some(StopReason::Deadline));
    let [error] = record.lines() else {
        return Err("not one line".into());
    };
    // The line of the first request, whose input is 5 bytes long; the
    // second's is 7.
    assert_eq!(
        std::str::from_utf8(error)?,
        "error\t1\t0\t0\tResourceLimit\t0..5\tField(max_input_bytes)\t-\t4\tRequired(5)\t\
         resource limit exceeded: max_input_bytes\n"
    );

    let finished = jobs.finished(id)?;
    assert_eq!(finished.stop(), Some(StopReason::Deadline));
    let [report] = finished.requests() else {
        return Err("not one request report".into());
    };
    assert_eq!(report.kind(), OutcomeKind::ResourceLimit);
    assert_eq!(report.error(), ErrorClass::Outcome);
    assert_eq!(jobs.clock.worker_readings(), 3);
    Ok(())
}
