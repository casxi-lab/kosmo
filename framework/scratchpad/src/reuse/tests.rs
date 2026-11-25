use std::ops::Range;

use kosmo_jobs::controllable::{Gate, Plan, Release};
use kosmo_jobs::{JobId, OutcomeKind, StopReason, TerminalState};
use kosmo_math_kalyx::{NotReusable, convert};
use kosmo_math_model::version::{Judgement, Versions};
use kosmo_math_model::{Item, MAX_ITEMS, Rejected};

use crate::test_jobs::{Checked, Jobs, report, value};
use crate::{NotReused, Part, Scratchpad};

/// A Scratchpad with one delivered entry: the line `line`, whose job is
/// planned with `plan` for the versions of the line.
fn delivered(
    jobs: &mut Jobs,
    line: &str,
    plan: impl FnOnce(Versions) -> Plan,
) -> Checked<(Scratchpad, JobId)> {
    let mut pad = Scratchpad::new();
    pad.insert_text(line)?;
    let plan = plan(pad.versions());
    let id = jobs.submit(&mut pad, plan)?;
    let terminal = jobs.wait(id)?;
    let _ = pad.deliver(&terminal);
    Ok((pad, id))
}

/// The reason why reusing the items `items` of `part` of the entry of job
/// `id` fails, after checking that it changed nothing.
fn refused(pad: &mut Scratchpad, id: JobId, part: Part, items: Range<usize>) -> Checked<NotReused> {
    let before = pad.clone();
    let reason = pad.reuse(id, part, items).err().ok_or("reused")?;
    assert_eq!(*pad, before);
    Ok(reason)
}

#[test]
fn a_result_without_a_value_is_not_reused_and_says_why() -> Checked {
    let mut jobs = Jobs::new()?;
    let (mut state, state_id) = delivered(&mut jobs, "1", |versions| {
        Plan::new().request(report(OutcomeKind::State, versions, "IterationLimit"))
    })?;
    assert_eq!(
        refused(&mut state, state_id, Part::Result, 0..1)?,
        NotReused::State
    );

    // A value line without its text cannot be read.
    let (mut unreadable, unreadable_id) = delivered(&mut jobs, "1", |versions| {
        Plan::new().request(report(OutcomeKind::Value, versions, "1\textra"))
    })?;
    assert_eq!(
        refused(&mut unreadable, unreadable_id, Part::Result, 0..1)?,
        NotReused::Unreadable
    );

    let (mut exhausted, exhausted_id) =
        delivered(&mut jobs, "1", |_| Plan::new().stop(StopReason::Deadline))?;
    assert_eq!(
        refused(&mut exhausted, exhausted_id, Part::Result, 0..1)?,
        NotReused::NoLine(TerminalState::ResourceExhausted)
    );
    Ok(())
}

#[test]
fn a_cancelled_job_has_no_line_to_reuse() -> Checked {
    let mut jobs = Jobs::new()?;
    let mut pad = Scratchpad::new();
    pad.insert_text("1")?;
    let id = jobs.submit(&mut pad, Plan::new().gate(Gate::Started, Release::Manual))?;
    jobs.layer.cancel(id);
    let _ = pad.deliver(&jobs.wait(id)?);
    assert_eq!(
        refused(&mut pad, id, Part::Result, 0..1)?,
        NotReused::NoLine(TerminalState::Cancelled)
    );
    // Its input can still be reused.
    pad.reuse(id, Part::Input, 0..1)?;
    assert_eq!(convert(&pad.line().snapshot())?, "11");
    Ok(())
}

#[test]
fn a_range_that_runs_backwards_or_past_the_line_is_rejected() -> Checked {
    let mut jobs = Jobs::new()?;
    let (mut pad, id) = delivered(&mut jobs, "x+1", |versions| value(versions, "1 + x"))?;
    let backwards = Range { start: 2, end: 1 };
    for part in [Part::Input, Part::Result] {
        assert_eq!(
            refused(&mut pad, id, part, backwards.clone())?,
            NotReused::Rejected(Rejected::NotInSnapshot)
        );
    }
    assert_eq!(
        refused(&mut pad, id, Part::Input, 1..4)?,
        NotReused::Rejected(Rejected::NotInSnapshot)
    );
    // The value `1 + x` has five items, the input three.
    assert_eq!(
        refused(&mut pad, id, Part::Result, 3..6)?,
        NotReused::Rejected(Rejected::NotInSnapshot)
    );
    pad.reuse(id, Part::Result, 3..5)?;
    assert_eq!(pad.line().items().len(), 5);
    Ok(())
}

#[test]
fn a_copy_that_the_line_cannot_hold_is_rejected() -> Checked {
    let mut jobs = Jobs::new()?;
    let (mut pad, id) = delivered(&mut jobs, "x+1", |versions| value(versions, "1 + x"))?;
    pad.clear()?;
    pad.insert_text(&"1".repeat(MAX_ITEMS - 2))?;
    assert_eq!(pad.line().items().len(), MAX_ITEMS - 2);
    assert_eq!(
        refused(&mut pad, id, Part::Input, 0..3)?,
        NotReused::Rejected(Rejected::TooManyItems)
    );
    Ok(())
}

#[test]
fn a_refusal_keeps_the_selection() -> Checked {
    let mut jobs = Jobs::new()?;
    let (mut state, state_id) = delivered(&mut jobs, "1", |versions| {
        Plan::new().request(report(OutcomeKind::State, versions, "IterationLimit"))
    })?;
    let (mut pad, id) = delivered(&mut jobs, "x+1", |versions| value(versions, "1 + x"))?;

    state.clear()?;
    state.insert_text("2*3")?;
    state.extend_left();
    assert!(state.line().selection().is_some());
    assert_eq!(
        refused(&mut state, state_id, Part::Result, 0..1)?,
        NotReused::State
    );

    // A selection of one item in a line one item short of its bound: the
    // copy of three items would still make the line too long, which the
    // line finds before it changes anything.
    pad.clear()?;
    pad.insert_text(&"1".repeat(MAX_ITEMS - 1))?;
    pad.extend_left();
    assert!(pad.line().selection().is_some());
    assert_eq!(
        refused(&mut pad, id, Part::Input, 0..3)?,
        NotReused::Rejected(Rejected::TooManyItems)
    );
    let backwards = Range { start: 2, end: 1 };
    assert_eq!(
        refused(&mut pad, id, Part::Input, backwards)?,
        NotReused::Rejected(Rejected::NotInSnapshot)
    );
    Ok(())
}

#[test]
fn a_copy_replaces_the_selection_and_an_empty_one_without_a_selection_changes_nothing() -> Checked {
    let mut jobs = Jobs::new()?;
    let (mut pad, id) = delivered(&mut jobs, "x+1", |versions| value(versions, "1 + x"))?;
    pad.clear()?;
    pad.insert_text("2*3")?;
    pad.extend_left();
    let version = pad.versions().source;
    pad.reuse(id, Part::Input, 0..3)?;
    assert_eq!(convert(&pad.line().snapshot())?, "2*(x+1)");
    assert_eq!(pad.versions().source.get(), version.get() + 1);

    // The copy replaced the selection, so the empty copies find none.
    assert_eq!(pad.line().selection(), None);
    let before = pad.clone();
    pad.reuse(id, Part::Input, 2..2)?;
    pad.reuse(id, Part::Result, 5..5)?;
    assert_eq!(pad, before);
    Ok(())
}

#[test]
fn an_empty_copy_deletes_the_selection() -> Checked {
    let mut jobs = Jobs::new()?;
    let (mut pad, id) = delivered(&mut jobs, "x+1", |versions| value(versions, "1 + x"))?;
    pad.clear()?;
    pad.insert_text("2*3")?;
    for part in [Part::Input, Part::Result] {
        pad.extend_left();
        assert!(pad.line().selection().is_some());
        let version = pad.versions().source;
        pad.reuse(id, part, 1..1)?;
        assert_eq!(pad.line().selection(), None);
        assert_eq!(pad.versions().source.get(), version.get() + 1);
    }
    // Each empty copy deleted the selected item before the cursor.
    let expected: Vec<Item> = "2".chars().map(Item::Char).collect();
    assert_eq!(pad.line().items(), expected.as_slice());
    Ok(())
}

#[test]
fn the_result_of_a_stale_entry_can_be_reused() -> Checked {
    let mut jobs = Jobs::new()?;
    let mut pad = Scratchpad::new();
    pad.insert_text("1/3+1/6")?;
    let versions = pad.versions();
    let id = jobs.submit(
        &mut pad,
        value(versions, "1/2").gate(Gate::Finished, Release::Manual),
    )?;
    pad.clear()?;
    pad.insert_text("x^")?;
    jobs.layer.controllable().release(0, Gate::Finished);
    let delivery = pad.deliver(&jobs.wait(id)?);
    assert_eq!(delivery.judgement, Judgement::Stale);
    pad.reuse(id, Part::Result, 0..3)?;
    assert_eq!(convert(&pad.line().snapshot())?, "x^(1/2)");
    let expected: Vec<Item> = "1/3+1/6".chars().map(Item::Char).collect();
    assert_eq!(
        pad.entry(id).ok_or("no entry")?.input().items(),
        expected.as_slice()
    );
    Ok(())
}

#[test]
fn reasons_name_what_could_not_be_reused() -> Checked {
    let mut jobs = Jobs::new()?;
    let (_, id) = delivered(&mut jobs, "1", |versions| value(versions, "1"))?;
    let reasons = [
        (NotReused::NotInHistory(id), "job 1 is not in the history"),
        (NotReused::NoResultYet, "the entry has no result yet"),
        (
            NotReused::NoLine(TerminalState::Cancelled),
            "the job ended without an output line",
        ),
        (NotReused::Error, "the result is an error, not a value"),
        (NotReused::State, "the result is a state, not a value"),
        (
            NotReused::Unreadable,
            "the output line of the result is unreadable",
        ),
        (
            NotReused::NotReadBack(NotReusable::Rejected(Rejected::InsertTooLong)),
            "the value cannot be reused: an insertion holds at most 4096 bytes",
        ),
        (
            NotReused::Rejected(Rejected::TooManyItems),
            "the line rejected the copy: a line holds at most 4096 items",
        ),
    ];
    for (reason, text) in reasons {
        assert_eq!(reason.to_string(), text);
    }
    Ok(())
}
