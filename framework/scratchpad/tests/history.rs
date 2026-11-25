//! The history: one entry for each accepted submission, with the snapshot
//! and the versions it was prepared from; late results in their own entry;
//! refused submissions; and the bound with its eviction of the oldest
//! entry, whose job is not cancelled.

mod support;

use kosmo_jobs::controllable::{Gate, Release};
use kosmo_jobs::{JobId, Limits, MAX_LIVE_JOBS, Operation, Rejected, TerminalState};
use kosmo_math_model::version::Judgement;
use kosmo_scratchpad::{Destination, Entry, MAX_HISTORY_ENTRIES, NotAccepted, Scratchpad};
use support::{Checked, Jobs, SUM, answer, cases, numbers, ones};

#[test]
fn a_late_result_goes_only_into_the_entry_of_its_own_version() -> Checked {
    cases::late_result()
}

#[test]
fn the_oldest_entry_is_evicted_past_the_bound() -> Checked {
    let ids = cases::eviction()?;
    assert_eq!(ids.len(), MAX_HISTORY_ENTRIES + 1);
    Ok(())
}

#[test]
fn an_entry_keeps_the_line_as_it_was_prepared() -> Checked {
    let mut jobs = Jobs::new()?;
    let mut pad = Scratchpad::new();
    pad.insert_text("1/3+1/6")?;
    let prepared = pad.line().snapshot();
    let (submission, pending) =
        pad.prepare(jobs.session, Operation::Eval, "", Limits::new(), None)?;
    jobs.layer
        .controllable()
        .plan(0, answer(SUM, pending.versions()));
    let answer = jobs.layer.submit(submission);
    // An edit between the submission and its acceptance.
    pad.insert_char('0')?;
    let id = pad.accept(pending, answer)?;

    let entry = pad.entry(id).ok_or("no entry")?;
    assert_eq!(entry.input(), &prepared);
    assert_eq!(numbers(entry.versions()), (1, 0, 0));
    assert_eq!(pad.versions().source.get(), 2);
    // Its result is stale, since the line changed after it was prepared.
    let delivery = pad.deliver(&jobs.take(id)?);
    assert_eq!(delivery.judgement, Judgement::Stale);
    assert_eq!(delivery.destination, Destination::Entry);
    Ok(())
}

#[test]
fn a_refused_submission_appends_no_entry_and_is_not_the_latest() -> Checked {
    let mut jobs = Jobs::new()?;
    let mut pad = Scratchpad::new();
    let mut ids = Vec::new();
    // Eight jobs without a plan wait before they start.
    for _ in 0..MAX_LIVE_JOBS {
        pad.insert_char('1')?;
        let (submission, pending) =
            pad.prepare(jobs.session, Operation::Eval, "", Limits::new(), None)?;
        ids.push(pad.accept(pending, jobs.layer.submit(submission))?);
    }
    pad.insert_char('1')?;
    let (submission, pending) =
        pad.prepare(jobs.session, Operation::Eval, "", Limits::new(), None)?;
    let before = pad.clone();
    let refused = jobs.layer.submit(submission);
    assert_eq!(
        refused,
        Err(Rejected::Full {
            live: MAX_LIVE_JOBS
        })
    );
    assert_eq!(
        pad.accept(pending, refused),
        Err(NotAccepted::Refused(Rejected::Full {
            live: MAX_LIVE_JOBS
        }))
    );

    assert_eq!(pad, before);
    let held: Vec<JobId> = pad.history().iter().map(Entry::id).collect();
    assert_eq!(held, ids);
    assert_eq!(pad.latest(), ids.last().copied());
    Ok(())
}

#[test]
fn the_job_of_an_evicted_entry_runs_on_and_its_result_is_discarded() -> Checked {
    let mut jobs = Jobs::new()?;
    let mut pad = Scratchpad::new();
    let mut ids = Vec::new();
    for k in 1..=MAX_HISTORY_ENTRIES + 1 {
        pad.insert_char('1')?;
        let versions = pad.versions();
        let plan = answer(&ones(k), versions);
        // The first job waits before its end until the last acceptance.
        let plan = if k == 1 {
            plan.gate(Gate::Finished, Release::Manual)
        } else {
            plan
        };
        let id = jobs.submit(&mut pad, Operation::Eval, "", plan)?;
        ids.push(id);
        assert!(jobs.layer.live_jobs() <= 2);
        if k > 1 {
            assert_eq!(pad.deliver(&jobs.take(id)?).destination, Destination::Entry);
        }
    }
    let first = ids[0];
    assert_eq!(pad.entry(first), None);
    let before = pad.history().to_vec();

    jobs.release(0);
    let terminal = jobs.take(first)?;
    assert_eq!(terminal.record().state(), TerminalState::Completed);
    assert!(!jobs.layer.backend().was_cancelled(0));
    assert!(jobs.layer.backend().cancels().is_empty());
    let delivery = pad.deliver(&terminal);
    assert_eq!(delivery.destination, Destination::Discarded);
    assert_eq!(delivery.judgement, Judgement::Stale);
    assert_eq!(pad.discarded(), 1);
    assert_eq!(pad.history(), before.as_slice());
    assert_eq!(jobs.layer.live_jobs(), 0);
    Ok(())
}
