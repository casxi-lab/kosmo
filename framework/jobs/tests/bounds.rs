//! The bound on live jobs: a submission beyond it is refused at once, and
//! taking a terminal state makes room again.

mod support;

use kosmo_jobs::controllable::{ControllableBackend, Gate, Plan, Release};
use kosmo_jobs::{JobState, MAX_LIVE_JOBS, Rejected, TerminalState};
use support::{Checked, controllable, eval, value, versions};

#[test]
fn a_submission_beyond_the_live_jobs_is_refused_as_full_until_one_is_taken() -> Checked {
    let tagged = versions(1, 0, 0)?;
    let mut backend = ControllableBackend::new();
    for order in 0..MAX_LIVE_JOBS {
        let plan = Plan::new().request(value(tagged, "1"));
        // The last four wait before they start.
        let plan = if order < 4 {
            plan
        } else {
            plan.gate(Gate::Started, Release::Manual)
        };
        backend.plan(order, plan);
    }
    let mut layer = controllable(backend);
    let session = layer.open_session()?;
    let mut ids = Vec::new();
    for _ in 0..MAX_LIVE_JOBS {
        ids.push(layer.submit(eval(session, tagged, "1"))?);
    }
    assert_eq!(
        ids.iter().map(|id| id.get()).collect::<Vec<_>>(),
        (1..=8).collect::<Vec<u64>>()
    );

    // Four jobs reached their terminal states and are not taken.
    assert_eq!(layer.poll(), ids[..4].to_vec());
    assert_eq!(layer.live_jobs(), MAX_LIVE_JOBS);
    let before: Vec<_> = ids.iter().map(|id| layer.state(*id)).collect();

    assert_eq!(
        layer.submit(eval(session, tagged, "1")),
        Err(Rejected::Full { live: 8 })
    );
    assert_eq!(layer.backend().received(), MAX_LIVE_JOBS, "no job created");
    assert_eq!(layer.live_jobs(), MAX_LIVE_JOBS);
    let after: Vec<_> = ids.iter().map(|id| layer.state(*id)).collect();
    assert_eq!(after, before, "nothing evicted");
    assert_eq!(after[0], Some(JobState::Terminal(TerminalState::Completed)));
    assert_eq!(after[7], Some(JobState::Queued));

    let taken = layer.take(ids[0]).ok_or("no terminal state")?;
    assert_eq!(taken.record().state(), TerminalState::Completed);
    assert_eq!(layer.live_jobs(), MAX_LIVE_JOBS - 1);
    let next = layer.submit(eval(session, tagged, "1"))?;
    assert_eq!(next.get(), 9, "the refusal used no identity");
    assert_eq!(layer.backend().received(), MAX_LIVE_JOBS + 1);
    Ok(())
}
