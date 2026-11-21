//! The terminal states of jobs that the kalyx runner executes on the job
//! layer's host stub: a value, a fault, a limit of the request, a deadline
//! that kalyx sees, a value line that does not fit, and a deadline that
//! kalyx sees after the job's cancellation was accepted.
//!
//! Each job's report, as the runner made it, is read from a backend that
//! records it on its way to the job layer.

mod support;

use kosmo_jobs::{
    CancelAnswer, EchoedVersions, ErrorClass, JobState, Limits, OutcomeKind, SerializeFailure,
    StopReason, TerminalState,
};
use kosmo_math_kalyx::read_output_line;
use kosmo_math_model::display::{Content, ErrorKind, LimitId};
use support::Checked;
use support::jobs::{KalyxJobs, typed};

/// The text of `line`.
fn text(line: &[u8]) -> Checked<&str> {
    Ok(std::str::from_utf8(line)?)
}

#[test]
fn a_value_completes_the_job_and_delivers_its_value_line() -> Checked {
    let mut jobs = KalyxJobs::new()?;
    let session = jobs.layer.open_session()?;
    let line = typed("1/3+1/6")?;
    assert_eq!(line.text, "1/3+1/6");
    let id = jobs.layer.submit(line.eval(session))?;
    let terminal = jobs.layer.wait(id).ok_or("no terminal state")?;
    let record = terminal.record();
    assert_eq!(record.state(), TerminalState::Completed);
    assert_eq!(record.stop(), None);
    assert_eq!(record.lines(), &[b"value\t1\t0\t0\t1/2\n".to_vec()]);
    assert_eq!(record.echoed(), Some(EchoedVersions::new(1, 0, 0)));
    assert_eq!(terminal.versions(), line.versions);

    let received = jobs.layer.backend().received(id).ok_or("not received")?;
    assert_eq!(received.limits(), Limits::new());
    let finished = jobs.finished(id)?;
    let [report] = finished.requests() else {
        return Err("not one request report".into());
    };
    assert_eq!(report.kind(), OutcomeKind::Value);
    assert_eq!(report.error(), ErrorClass::NoError);
    assert_eq!(report.echoed(), EchoedVersions::new(1, 0, 0));
    Ok(())
}

#[test]
fn a_fault_fails_the_job_with_its_error_line_without_stopping_it() -> Checked {
    let mut jobs = KalyxJobs::new()?;
    let session = jobs.layer.open_session()?;
    let line = typed("1/0")?;
    let id = jobs.layer.submit(line.eval(session))?;
    let terminal = jobs.layer.wait(id).ok_or("no terminal state")?;
    let record = terminal.record();
    assert_eq!(record.state(), TerminalState::Failed);
    assert_eq!(record.stop(), None, "a fault does not stop the job");
    assert!(!record.internal_error());
    let [error] = record.lines() else {
        return Err("not one line".into());
    };
    assert!(text(error)?.starts_with("error\t1\t0\t0\tDivisionByZero\t"));
    let item = read_output_line(error, line.versions, &line.snapshot);
    let Content::Error { kind, .. } = item.content else {
        return Err(format!("not an error: {item:?}").into());
    };
    assert_eq!(kind, ErrorKind::DivisionByZero);

    let finished = jobs.finished(id)?;
    assert_eq!(finished.stop(), None);
    let [report] = finished.requests() else {
        return Err("not one request report".into());
    };
    assert_eq!(report.kind(), OutcomeKind::Fault);
    assert_eq!(report.error(), ErrorClass::Outcome);
    Ok(())
}

#[test]
fn a_limit_of_the_request_fails_the_job_with_its_error_line_without_stopping_it() -> Checked {
    let mut jobs = KalyxJobs::new()?;
    let session = jobs.layer.open_session()?;
    let line = typed("12345")?;
    let mut limits = Limits::new();
    limits.max_input_bytes = 4;
    let id = jobs.layer.submit(line.eval(session).with_limits(limits))?;
    let terminal = jobs.layer.wait(id).ok_or("no terminal state")?;
    let record = terminal.record();
    assert_eq!(record.state(), TerminalState::Failed);
    assert_eq!(record.stop(), None, "a limit does not stop the job");
    let [error] = record.lines() else {
        return Err("not one line".into());
    };
    assert!(text(error)?.starts_with("error\t1\t0\t0\tResourceLimit\t"));
    let item = read_output_line(error, line.versions, &line.snapshot);
    let Content::Error {
        kind: ErrorKind::ResourceLimit(exceeded),
        ..
    } = item.content
    else {
        return Err(format!("not a limit: {item:?}").into());
    };
    assert_eq!(
        exceeded.limit,
        LimitId::Field("max_input_bytes".to_string())
    );
    assert_eq!(exceeded.allowed, 4);

    let received = jobs.layer.backend().received(id).ok_or("not received")?;
    assert_eq!(received.limits(), limits);
    let finished = jobs.finished(id)?;
    assert_eq!(finished.stop(), None);
    let [report] = finished.requests() else {
        return Err("not one request report".into());
    };
    assert_eq!(report.kind(), OutcomeKind::ResourceLimit);
    assert_eq!(report.error(), ErrorClass::Outcome);
    Ok(())
}

#[test]
fn a_deadline_that_kalyx_sees_exhausts_the_job_with_the_fallback_line() -> Checked {
    let mut jobs = KalyxJobs::new()?;
    let session = jobs.layer.open_session()?;
    let (start, limit) = (1_000, 500);
    let deadline = start + limit;
    jobs.clock.set(start);
    // The runner reads the clock before the request, and kalyx at its
    // first charge point.
    jobs.clock.script(&[deadline - 1, deadline]);
    let line = typed("1/3+1/6")?;
    let id = jobs
        .layer
        .submit(line.eval(session).with_time_limit(limit))?;
    let terminal = jobs.layer.wait(id).ok_or("no terminal state")?;
    let record = terminal.record();
    assert_eq!(record.state(), TerminalState::ResourceExhausted);
    assert_eq!(record.stop(), Some(StopReason::Deadline));
    assert!(!record.forced());
    let [fallback] = record.lines() else {
        return Err("not one line".into());
    };
    assert!(text(fallback)?.starts_with("error\t1\t0\t0\tDeadlineExceeded\t"));
    let item = read_output_line(fallback, line.versions, &line.snapshot);
    let Content::Error { kind, .. } = item.content else {
        return Err(format!("not an error: {item:?}").into());
    };
    assert_eq!(kind, ErrorKind::DeadlineExceeded);

    let received = jobs.layer.backend().received(id).ok_or("not received")?;
    assert_eq!(received.deadline(), Some(deadline));
    let finished = jobs.finished(id)?;
    let [report] = finished.requests() else {
        return Err("not one request report".into());
    };
    assert_eq!(report.kind(), OutcomeKind::DeadlineExceeded);
    // The expiry stays observed, so the line fails under the request's own
    // context and is written again as the fallback line.
    assert_eq!(
        report.error(),
        ErrorClass::Serialize(SerializeFailure::DeadlineExceeded)
    );
    assert_eq!(jobs.clock.worker_readings(), 2);
    Ok(())
}

#[test]
fn a_value_line_that_does_not_fit_fails_the_job_without_a_line() -> Checked {
    let mut jobs = KalyxJobs::new()?;
    let session = jobs.layer.open_session()?;
    let line = typed("1/3+1/6")?;
    let mut limits = Limits::new();
    limits.max_output_bytes = 12;
    let id = jobs.layer.submit(line.eval(session).with_limits(limits))?;
    let terminal = jobs.layer.wait(id).ok_or("no terminal state")?;
    let record = terminal.record();
    assert_eq!(record.state(), TerminalState::Failed);
    assert_eq!(record.stop(), None);
    assert!(record.lines().is_empty());
    assert!(!record.internal_error());

    let finished = jobs.finished(id)?;
    let [report] = finished.requests() else {
        return Err("not one request report".into());
    };
    assert_eq!(report.kind(), OutcomeKind::Value);
    assert_eq!(
        report.error(),
        ErrorClass::Serialize(SerializeFailure::ResourceLimit)
    );
    assert_eq!(report.line(), None, "a value is not written again");
    Ok(())
}

#[test]
fn a_deadline_that_kalyx_sees_after_a_cancel_request_ends_the_job_cancelled() -> Checked {
    let mut jobs = KalyxJobs::new()?;
    let session = jobs.layer.open_session()?;
    let (start, limit) = (1_000, 500);
    let deadline = start + limit;
    jobs.clock.set(start);
    // The first reading is the runner's, before the request; the second is
    // kalyx's at its first charge point, after it found no cancellation.
    jobs.clock.gate_at(2);
    let line = typed("1/3+1/6")?;
    let id = jobs
        .layer
        .submit(line.eval(session).with_time_limit(limit))?;
    jobs.clock.wait_at_gate()?;
    assert!(jobs.layer.poll().is_empty());
    assert_eq!(jobs.layer.state(id), Some(JobState::Running));
    assert_eq!(jobs.layer.cancel(id), CancelAnswer::Requested);
    assert_eq!(jobs.layer.state(id), Some(JobState::CancelRequested));
    jobs.clock.set(deadline);
    jobs.clock.open();

    let terminal = jobs.layer.wait(id).ok_or("no terminal state")?;
    let record = terminal.record();
    assert_eq!(record.state(), TerminalState::Cancelled);
    assert!(record.lines().is_empty(), "the fallback line is discarded");
    assert_eq!(record.stop(), Some(StopReason::Cancellation));

    let finished = jobs.finished(id)?;
    let [report] = finished.requests() else {
        return Err("not one request report".into());
    };
    assert_eq!(report.kind(), OutcomeKind::DeadlineExceeded);
    let fallback = report.line().ok_or("no fallback line")?;
    assert!(text(fallback)?.starts_with("error\t1\t0\t0\tDeadlineExceeded\t"));
    let item = read_output_line(fallback, line.versions, &line.snapshot);
    let Content::Error { kind, .. } = item.content else {
        return Err(format!("not an error: {item:?}").into());
    };
    assert_eq!(kind, ErrorKind::DeadlineExceeded);
    assert_eq!(jobs.clock.worker_readings(), 2);
    Ok(())
}
