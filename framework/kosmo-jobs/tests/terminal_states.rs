//! Terminal states that only a controllable backend produces: a state line,
//! a request that saw the cancellation, the race between a finished job and
//! a cancel request, forced stops and CPU budgets, an echo of other
//! versions, and a cancellation seen between two requests.

mod support;

use kosmo_jobs::controllable::{ControllableBackend, Gate, Plan, Release};
use kosmo_jobs::{
    CancelAnswer, ErrorClass, JobState, Operation, OutcomeKind, Report, Request, RequestReport,
    SerializeFailure, StopReason, Submission, Terminal, TerminalState,
};
use support::{Checked, controllable, eval, line, texts, value, versions};

#[test]
fn a_state_line_completes_the_job_and_is_delivered() -> Checked {
    let tagged = versions(1, 2, 3)?;
    let state = line("state", tagged, "IterationLimit");
    let report = RequestReport::new(
        OutcomeKind::State,
        ErrorClass::NoError,
        Some(state.clone()),
        tagged,
    );
    let mut backend = ControllableBackend::new();
    backend.plan(0, Plan::new().request(report));
    let mut layer = controllable(backend);
    let session = layer.open_session()?;
    let id = layer.submit(eval(session, tagged, "f(x)"))?;
    let terminal = layer.wait(id).ok_or("no terminal state")?;
    assert_eq!(terminal.record().state(), TerminalState::Completed);
    assert_eq!(terminal.record().lines(), &[state]);
    assert_eq!(texts(&terminal), ["state\t1\t2\t3\tIterationLimit\n"]);
    assert_eq!(terminal.record().echoed(), Some(tagged));
    Ok(())
}

#[test]
fn a_request_cancelled_after_acceptance_ends_the_job_cancelled_without_lines() -> Checked {
    let tagged = versions(1, 0, 0)?;
    let cancelled = RequestReport::new(
        OutcomeKind::Cancelled,
        ErrorClass::Serialize(SerializeFailure::Cancelled),
        None,
        tagged,
    );
    let mut backend = ControllableBackend::new();
    backend.plan(
        0,
        Plan::new()
            .request(cancelled)
            .gate(Gate::Within(0), Release::OnCancel),
    );
    let mut layer = controllable(backend);
    let session = layer.open_session()?;
    let id = layer.submit(eval(session, tagged, "1/3"))?;
    assert!(layer.poll().is_empty());
    assert_eq!(layer.state(id), Some(JobState::Running));

    // Acceptance is answered at once; the flag is set only after it.
    assert!(layer.backend().cancels().is_empty());
    assert_eq!(layer.cancel(id), CancelAnswer::Requested);
    assert_eq!(layer.state(id), Some(JobState::CancelRequested));
    assert_eq!(layer.backend().cancels(), &[id]);

    // The terminal state follows the backend's report.
    assert_eq!(layer.poll(), vec![id]);
    let terminal = layer.take(id).ok_or("no terminal state")?;
    assert_eq!(terminal.record().state(), TerminalState::Cancelled);
    assert!(terminal.record().lines().is_empty());
    assert_eq!(layer.take(id), None);
    Ok(())
}

#[test]
fn a_cancel_accepted_before_the_finished_report_is_handled_discards_the_result() -> Checked {
    let tagged = versions(1, 0, 0)?;
    let mut backend = ControllableBackend::new();
    backend.plan(
        0,
        Plan::new()
            .request(value(tagged, "1/2"))
            .gate(Gate::Finished, Release::Manual),
    );
    let mut layer = controllable(backend);
    let session = layer.open_session()?;
    let id = layer.submit(eval(session, tagged, "1/3+1/6"))?;
    layer.poll();
    assert_eq!(layer.state(id), Some(JobState::Running));

    // The backend's end of the job with its value line is waiting, and the
    // core has not handled it yet.
    layer.backend_mut().release(0, Gate::Finished);
    assert!(matches!(
        layer.backend().sent().last(),
        Some(Report::Finished(finished)) if finished.requests().len() == 1
    ));
    assert_eq!(layer.state(id), Some(JobState::Running));

    assert_eq!(layer.cancel(id), CancelAnswer::Requested);
    assert_eq!(layer.poll(), vec![id]);
    let terminal = layer.take(id).ok_or("no terminal state")?;
    assert_eq!(terminal.record().state(), TerminalState::Cancelled);
    assert!(terminal.record().lines().is_empty());
    assert_eq!(layer.cancel(id), CancelAnswer::AlreadyTerminal);
    Ok(())
}

/// Runs one job that the backend ends with `stop` and no request report,
/// after cancelling it first when `cancel` is true.
fn stopped(stop: StopReason, cancel: bool) -> Checked<Terminal> {
    let tagged = versions(1, 0, 0)?;
    let mut backend = ControllableBackend::new();
    backend.plan(
        0,
        Plan::new().stop(stop).gate(Gate::Finished, Release::Manual),
    );
    let mut layer = controllable(backend);
    let session = layer.open_session()?;
    let id = layer.submit(eval(session, tagged, "1/3"))?;
    layer.poll();
    assert_eq!(layer.state(id), Some(JobState::Running));
    if cancel {
        assert_eq!(layer.cancel(id), CancelAnswer::Requested);
    }
    layer.backend_mut().release(0, Gate::Finished);
    assert_eq!(layer.poll(), vec![id]);
    Ok(layer.take(id).ok_or("no terminal state")?)
}

#[test]
fn an_exhausted_cpu_budget_without_a_cancel_request_ends_resource_exhausted_and_forced() -> Checked
{
    let terminal = stopped(StopReason::CpuBudget, false)?;
    let record = terminal.record();
    assert_eq!(record.state(), TerminalState::ResourceExhausted);
    assert_eq!(record.stop(), Some(StopReason::CpuBudget));
    assert!(record.forced());
    assert!(record.lines().is_empty());
    Ok(())
}

#[test]
fn an_exhausted_cpu_budget_after_a_cancel_request_ends_cancelled_and_forced() -> Checked {
    let terminal = stopped(StopReason::CpuBudget, true)?;
    let record = terminal.record();
    assert_eq!(record.state(), TerminalState::Cancelled);
    assert_eq!(record.stop(), Some(StopReason::CpuBudget));
    assert!(record.forced());
    assert!(record.lines().is_empty());
    Ok(())
}

#[test]
fn a_forced_stop_after_a_cancel_request_ends_cancelled_and_forced() -> Checked {
    let terminal = stopped(StopReason::ForcedStop, true)?;
    let record = terminal.record();
    assert_eq!(record.state(), TerminalState::Cancelled);
    assert_eq!(record.stop(), Some(StopReason::ForcedStop));
    assert!(record.forced());
    assert!(record.lines().is_empty());
    Ok(())
}

#[test]
fn a_forced_stop_in_the_second_request_delivers_the_line_of_the_first() -> Checked {
    let tagged = versions(1, 0, 0)?;
    let first = value(tagged, "1/2");
    let mut backend = ControllableBackend::new();
    backend.plan(
        0,
        Plan::new()
            .request(first.clone())
            .stop(StopReason::ForcedStop),
    );
    let mut layer = controllable(backend);
    let session = layer.open_session()?;
    let requests = vec![
        Request::new(Operation::Eval, "", "1/3+1/6"),
        Request::new(Operation::Expand, "x", "(x+1)^5"),
    ];
    let submission = Submission::several(session, tagged, requests).ok_or("two requests")?;
    let id = layer.submit(submission)?;
    let terminal = layer.wait(id).ok_or("no terminal state")?;
    let record = terminal.record();
    assert_eq!(record.state(), TerminalState::ResourceExhausted);
    assert_eq!(record.stop(), Some(StopReason::ForcedStop));
    assert!(record.forced());
    assert_eq!(texts(&terminal), ["value\t1\t0\t0\t1/2\n"]);
    assert!(matches!(
        layer.backend().sent().last(),
        Some(Report::Finished(finished)) if finished.requests() == [first]
    ));
    Ok(())
}

#[test]
fn an_echo_of_other_versions_fails_the_job_unless_it_was_cancelled() -> Checked {
    let tagged = versions(1, 0, 0)?;
    let other = versions(2, 0, 0)?;
    let echo = value(other, "1/2");

    // Outside cancel_requested: failed, an internal error, no line.
    let mut backend = ControllableBackend::new();
    backend.plan(0, Plan::new().request(echo.clone()));
    backend.plan(
        1,
        Plan::new()
            .request(echo)
            .gate(Gate::Finished, Release::Manual),
    );
    let mut layer = controllable(backend);
    let session = layer.open_session()?;
    let plain = layer.submit(eval(session, tagged, "1/3+1/6"))?;
    let terminal = layer.wait(plain).ok_or("no terminal state")?;
    let record = terminal.record();
    assert_eq!(record.state(), TerminalState::Failed);
    assert!(record.internal_error());
    assert!(record.lines().is_empty());
    assert_eq!(record.echoed(), Some(other));

    // In cancel_requested: cancelled.
    let cancelled = layer.submit(eval(session, tagged, "1/3+1/6"))?;
    layer.poll();
    assert_eq!(layer.cancel(cancelled), CancelAnswer::Requested);
    layer.backend_mut().release(1, Gate::Finished);
    let terminal = layer.wait(cancelled).ok_or("no terminal state")?;
    assert_eq!(terminal.record().state(), TerminalState::Cancelled);
    assert!(!terminal.record().internal_error());
    assert!(terminal.record().lines().is_empty());
    Ok(())
}

#[test]
fn a_cancellation_seen_between_two_requests_runs_no_second_request() -> Checked {
    let tagged = versions(1, 0, 0)?;
    let first = value(tagged, "1/2");
    let mut backend = ControllableBackend::new();
    backend.plan(
        0,
        Plan::new()
            .request(first.clone())
            .request(value(tagged, "x^5+5*x^4+10*x^3+10*x^2+5*x+1"))
            .gate(Gate::Between(1), Release::Manual),
    );
    let mut layer = controllable(backend);
    let session = layer.open_session()?;
    let requests = vec![
        Request::new(Operation::Eval, "", "1/3+1/6"),
        Request::new(Operation::Expand, "x", "(x+1)^5"),
    ];
    let submission = Submission::several(session, tagged, requests).ok_or("two requests")?;
    let id = layer.submit(submission)?;
    assert_eq!(layer.backend().waiting_at(0), Some(Gate::Between(1)));

    // In the gate: the core handles Started, then accepts the cancellation.
    assert!(layer.poll().is_empty());
    assert_eq!(layer.state(id), Some(JobState::Running));
    assert_eq!(layer.cancel(id), CancelAnswer::Requested);
    layer.backend_mut().release(0, Gate::Between(1));

    let Some(Report::Finished(finished)) = layer.backend().sent().last() else {
        return Err("no Finished report".into());
    };
    assert_eq!(finished.requests(), &[first]);
    assert_eq!(finished.stop(), Some(StopReason::Cancellation));
    assert_eq!(layer.poll(), vec![id]);
    let terminal = layer.take(id).ok_or("no terminal state")?;
    assert_eq!(terminal.record().state(), TerminalState::Cancelled);
    assert!(terminal.record().lines().is_empty());
    Ok(())
}
