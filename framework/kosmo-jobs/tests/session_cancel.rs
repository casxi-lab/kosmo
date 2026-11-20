//! Cancelling a session: one step cancels every job of the session that
//! has not ended, and leaves the jobs of other sessions alone.

mod support;

use kosmo_jobs::controllable::{ControllableBackend, Gate, Plan, Release};
use kosmo_jobs::{CancelCount, JobState, Report, SessionCancel, StopReason, TerminalState};
use support::{Checked, controllable, eval, value, versions};

#[test]
fn cancelling_a_session_cancels_its_queued_and_running_jobs_once() -> Checked {
    let tagged = versions(1, 0, 0)?;
    let mut backend = ControllableBackend::new();
    // Session A: one job that runs and one that waits before it starts.
    backend.plan(
        0,
        Plan::new()
            .request(value(tagged, "1"))
            .gate(Gate::Finished, Release::Manual),
    );
    backend.plan(
        1,
        Plan::new()
            .request(value(tagged, "2"))
            .gate(Gate::Started, Release::Manual),
    );
    // Session B: one job that runs.
    backend.plan(
        2,
        Plan::new()
            .request(value(tagged, "3"))
            .gate(Gate::Finished, Release::Manual),
    );
    let mut layer = controllable(backend);
    let a = layer.open_session()?;
    let b = layer.open_session()?;
    let a_running = layer.submit(eval(a, tagged, "1"))?;
    let a_queued = layer.submit(eval(a, tagged, "2"))?;
    let b_running = layer.submit(eval(b, tagged, "3"))?;
    assert!(layer.poll().is_empty());
    assert_eq!(
        [a_running, a_queued, b_running].map(|id| layer.state(id)),
        [
            Some(JobState::Running),
            Some(JobState::Queued),
            Some(JobState::Running)
        ]
    );

    let first = SessionCancel::cancel_session(&mut layer, a);
    assert_eq!(
        (first.queued, first.requested),
        (1, 1),
        "one queued job cancelled, one running job in cancel_requested"
    );
    assert_eq!(
        [a_running, a_queued, b_running].map(|id| layer.state(id)),
        [
            Some(JobState::CancelRequested),
            Some(JobState::Terminal(TerminalState::Cancelled)),
            Some(JobState::Running)
        ]
    );
    assert_eq!(layer.backend().cancels(), &[a_running, a_queued]);
    let second = SessionCancel::cancel_session(&mut layer, a);
    assert_eq!(second, CancelCount::default());
    assert_eq!(layer.backend().cancels(), &[a_running, a_queued]);

    // The queued job stops before its first request; the running ones end.
    for order in 0..3 {
        let gate = layer.backend().waiting_at(order).ok_or("no gate")?;
        layer.controllable().release(order, gate);
    }
    assert_eq!(layer.poll(), vec![a_running, a_queued, b_running]);
    let ends: Vec<_> = [a_running, a_queued, b_running]
        .into_iter()
        .map(|id| layer.take(id).map(|t| t.record().state()))
        .collect();
    assert_eq!(
        ends,
        [
            Some(TerminalState::Cancelled),
            Some(TerminalState::Cancelled),
            Some(TerminalState::Completed)
        ]
    );
    assert!(layer.poll().is_empty());
    assert_eq!(layer.live_jobs(), 0);
    let queued_end = layer.backend().sent().iter().any(|report| {
        matches!(report, Report::Finished(finished)
            if finished.id() == a_queued && finished.stop() == Some(StopReason::Cancellation)
                && finished.requests().is_empty())
    });
    assert!(queued_end, "the queued job ran no request");
    Ok(())
}
