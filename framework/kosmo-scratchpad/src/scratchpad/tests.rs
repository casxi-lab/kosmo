use kosmo_jobs::controllable::{Gate, Plan, Release};
use kosmo_jobs::{
    JobState, Limits, Operation, OutcomeKind, Rejected as Refusal, StopReason, TerminalState,
};
use kosmo_math_model::display::{Content, ErrorKind};
use kosmo_math_model::version::{ContextVersion, Judgement, SettingsVersion};
use kosmo_math_model::{
    Completeness, EditBuffer, Fragment, HoleKind, InvalidKind, Place, Rejected,
};

use super::*;
use crate::history::Outcome;
use crate::test_jobs::{Checked, Jobs, report, value};

#[test]
fn a_new_scratchpad_is_empty_at_version_zero() {
    let pad = Scratchpad::new();
    assert_eq!(pad.line(), &EditBuffer::new());
    assert_eq!(pad.versions(), Versions::default());
    assert_eq!(pad.latest(), None);
    assert!(pad.history().is_empty());
    assert_eq!(pad.discarded(), 0);
    assert_eq!(MAX_HISTORY_ENTRIES, 32);

    let built = Scratchpad::with_versions(ContextVersion::new(2), SettingsVersion::new(3));
    assert_eq!(built.versions().source.get(), 0);
    assert_eq!(built.versions().context.get(), 2);
    assert_eq!(built.versions().settings.get(), 3);
    assert!(built.history().is_empty());
}

#[test]
fn editing_operations_reach_the_current_line_as_they_are() -> Checked {
    type Edit = fn(&mut Scratchpad) -> Result<(), Rejected>;
    type Same = fn(&mut EditBuffer) -> Result<(), Rejected>;
    let edits: [(Edit, Same); 14] = [
        (
            |pad| pad.insert_text("2*x+1"),
            |line| line.insert_text("2*x+1"),
        ),
        (|pad| pad.insert_char('0'), |line| line.insert_char('0')),
        (|pad| pad.insert_char('#'), |line| line.insert_char('#')),
        (
            |pad| {
                pad.move_left();
                Ok(())
            },
            |line| {
                line.move_left();
                Ok(())
            },
        ),
        (
            |pad| {
                pad.extend_left();
                Ok(())
            },
            |line| {
                line.extend_left();
                Ok(())
            },
        ),
        (|pad| pad.delete_backward(), |line| line.delete_backward()),
        (
            |pad| {
                pad.move_to_start();
                Ok(())
            },
            |line| {
                line.move_to_start();
                Ok(())
            },
        ),
        (|pad| pad.delete_forward(), |line| line.delete_forward()),
        (
            |pad| {
                pad.extend_right();
                Ok(())
            },
            |line| {
                line.extend_right();
                Ok(())
            },
        ),
        (
            |pad| {
                pad.move_right();
                Ok(())
            },
            |line| {
                line.move_right();
                Ok(())
            },
        ),
        (
            |pad| {
                pad.move_to_end();
                Ok(())
            },
            |line| {
                line.move_to_end();
                Ok(())
            },
        ),
        (
            |pad| {
                pad.select_all();
                Ok(())
            },
            |line| {
                line.select_all();
                Ok(())
            },
        ),
        (|pad| pad.clear(), |line| line.clear()),
        (|pad| pad.clear(), |line| line.clear()),
    ];
    let mut pad = Scratchpad::new();
    let mut line = EditBuffer::new();
    for (edit, same) in edits {
        assert_eq!(edit(&mut pad), same(&mut line));
        assert_eq!(pad.line(), &line);
        assert_eq!(pad.versions().source, line.version());
    }
    // Five edits changed the items: the text, the character, the two
    // deletions and the first clearing.
    assert_eq!(pad.versions().source.get(), 5);
    Ok(())
}

#[test]
fn preparing_changes_neither_the_line_nor_its_version() -> Checked {
    let jobs = Jobs::new()?;
    let mut pad = Scratchpad::new();
    pad.insert_text("1/3+1/6")?;
    pad.move_left();
    pad.extend_left();
    let before = pad.clone();
    for _ in 0..2 {
        pad.prepare(jobs.session, Operation::Eval, "", Limits::new(), None)?;
        assert_eq!(pad, before);
    }
    Ok(())
}

#[test]
fn a_prepared_job_carries_the_line_its_versions_limits_and_time_limit() -> Checked {
    let jobs = Jobs::new()?;
    let mut pad = Scratchpad::with_versions(ContextVersion::new(0), SettingsVersion::new(4));
    pad.insert_text(".5*x^2")?;
    let mut limits = Limits::new();
    limits.max_work = 1_000;
    let (submission, pending) =
        pad.prepare(jobs.session, Operation::Diff, "x", limits, Some(25))?;

    assert_eq!(submission.session(), jobs.session);
    assert_eq!(submission.versions(), pad.versions());
    assert_eq!(submission.limits(), limits);
    assert_eq!(submission.time_limit(), Some(25));
    let [request] = submission.requests() else {
        return Err("not one request".into());
    };
    assert_eq!(request.operation(), Operation::Diff);
    assert_eq!(request.variable(), "x");
    assert_eq!(request.input(), "0.5*x^2");

    assert_eq!(pending.input(), &pad.line().snapshot());
    assert_eq!(pending.versions(), pad.versions());
    assert_eq!(pending.versions().settings.get(), 4);
    assert_eq!(pending.operation(), Operation::Diff);
    assert_eq!(pending.variable(), "x");

    let (untimed, _) = pad.prepare(jobs.session, Operation::Eval, "", Limits::new(), None)?;
    assert_eq!(untimed.time_limit(), None);
    assert_eq!(untimed.limits(), Limits::new());
    Ok(())
}

#[test]
fn lines_that_are_not_complete_give_their_holes_or_invalid_part() -> Checked {
    let jobs = Jobs::new()?;
    let prepare = |pad: &Scratchpad| {
        pad.prepare(jobs.session, Operation::Eval, "", Limits::new(), None)
            .map(|_| ())
    };
    let mut pad = Scratchpad::new();
    let Err(NotConverted::Incomplete(holes)) = prepare(&pad) else {
        return Err("an empty line was prepared".into());
    };
    assert_eq!(holes.len(), 1);
    assert_eq!(holes[0].kind, HoleKind::EmptyLine);

    pad.insert_text("1/")?;
    let Err(NotConverted::Incomplete(holes)) = prepare(&pad) else {
        return Err("an incomplete line was prepared".into());
    };
    assert_eq!(holes.len(), 1);
    assert_eq!(holes[0].kind, HoleKind::MissingOperand);
    assert_eq!(holes[0].position.index, 2);

    pad.clear()?;
    pad.insert_text("1)")?;
    let Err(NotConverted::Invalid(invalid)) = prepare(&pad) else {
        return Err("an invalid line was prepared".into());
    };
    assert_eq!(invalid.kind, InvalidKind::SurplusRightBracket);
    assert!(matches!(
        pad.line().completeness(),
        Completeness::Invalid(_)
    ));
    Ok(())
}

#[test]
fn an_accepted_submission_becomes_the_latest_and_appends_its_entry() -> Checked {
    let mut jobs = Jobs::new()?;
    let mut pad = Scratchpad::new();
    pad.insert_text("x^2")?;
    let (submission, pending) =
        pad.prepare(jobs.session, Operation::Diff, "x", Limits::new(), None)?;
    let id = pad.accept(pending.clone(), jobs.layer.submit(submission))?;

    assert_eq!(pad.latest(), Some(id));
    let [entry] = pad.history() else {
        return Err("not one entry".into());
    };
    assert_eq!(pad.entry(id), Some(entry));
    assert_eq!(entry.id(), id);
    assert_eq!(entry.input(), pending.input());
    assert_eq!(entry.versions(), pending.versions());
    assert_eq!(entry.operation(), Operation::Diff);
    assert_eq!(entry.variable(), "x");
    assert_eq!(entry.outcome(), None);
    // Accepting does not change the line either.
    assert_eq!(pad.line().snapshot(), *pending.input());
    Ok(())
}

#[test]
fn a_refused_submission_changes_nothing() -> Checked {
    let mut jobs = Jobs::new()?;
    let mut pad = Scratchpad::new();
    pad.insert_text("1")?;
    let (submission, pending) =
        pad.prepare(jobs.session, Operation::Eval, "", Limits::new(), None)?;
    pad.accept(pending, jobs.layer.submit(submission))?;
    let before = pad.clone();

    for refusal in [Refusal::Full { live: 8 }, Refusal::Exhausted] {
        let (_, pending) = pad.prepare(jobs.session, Operation::Eval, "", Limits::new(), None)?;
        assert_eq!(
            pad.accept(pending, Err(refusal)),
            Err(NotAccepted::Refused(refusal))
        );
        assert_eq!(pad, before);
    }
    Ok(())
}

#[test]
fn an_identity_that_does_not_come_after_the_latest_is_not_accepted() -> Checked {
    let mut jobs = Jobs::new()?;
    let mut pad = Scratchpad::new();
    pad.insert_text("1")?;
    let (submission, pending) =
        pad.prepare(jobs.session, Operation::Eval, "", Limits::new(), None)?;
    let first = jobs.layer.submit(submission.clone())?;
    let second = jobs.layer.submit(submission)?;
    pad.accept(pending.clone(), Ok(second))?;
    let before = pad.clone();

    for id in [second, first] {
        assert_eq!(
            pad.accept(pending.clone(), Ok(id)),
            Err(NotAccepted::NotAfterLatest { id, latest: second })
        );
        assert_eq!(pad, before);
    }
    Ok(())
}

#[test]
fn a_result_goes_into_its_own_entry_whatever_its_judgement() -> Checked {
    let mut jobs = Jobs::new()?;
    let mut pad = Scratchpad::new();
    pad.insert_text("1/3+1/6")?;
    let versions = pad.versions();
    let plan = |versions| value(versions, "1/2").gate(Gate::Finished, Release::Manual);
    let first = jobs.submit(&mut pad, plan(versions))?;
    // The same line again: the same versions, a later submission.
    let second = jobs.submit(&mut pad, plan(versions))?;

    jobs.layer.controllable().release(0, Gate::Finished);
    let delivery = pad.deliver(&jobs.wait(first)?);
    assert_eq!(delivery.judgement, Judgement::Superseded);
    assert_eq!(delivery.destination, Destination::Entry);
    jobs.layer.controllable().release(1, Gate::Finished);
    let delivery = pad.deliver(&jobs.wait(second)?);
    assert_eq!(delivery.judgement, Judgement::Current);
    assert_eq!(delivery.destination, Destination::Entry);

    for id in [first, second] {
        let item = pad
            .entry(id)
            .and_then(Entry::outcome)
            .and_then(Outcome::item)
            .ok_or("no item")?;
        assert_eq!(item.content, Content::Value("1/2".to_string()));
        assert_eq!(item.versions, versions);
    }
    assert_eq!(pad.discarded(), 0);
    Ok(())
}

#[test]
fn a_second_delivery_of_a_terminal_state_leaves_the_entry_as_it_was() -> Checked {
    let mut jobs = Jobs::new()?;
    let mut pad = Scratchpad::new();
    pad.insert_text("1/3+1/6")?;
    let versions = pad.versions();
    let id = jobs.submit(&mut pad, value(versions, "1/2"))?;
    let terminal = jobs.wait(id)?;
    assert_eq!(pad.deliver(&terminal).destination, Destination::Entry);
    let before = pad.clone();

    let delivery = pad.deliver(&terminal);
    assert_eq!(delivery.destination, Destination::AlreadyFinished);
    assert_eq!(delivery.judgement, Judgement::Current);
    assert_eq!(pad, before);
    Ok(())
}

#[test]
fn a_terminal_state_without_an_entry_counts_toward_the_discard_count() -> Checked {
    let mut jobs = Jobs::new()?;
    let mut pad = Scratchpad::new();
    pad.insert_text("2")?;
    let versions = pad.versions();
    // A job that reached the job layer but was never accepted here.
    let (submission, _) = pad.prepare(jobs.session, Operation::Eval, "", Limits::new(), None)?;
    jobs.layer.controllable().plan(0, value(versions, "2"));
    let stranger = jobs.layer.submit(submission)?;
    let before = pad.clone();

    let delivery = pad.deliver(&jobs.wait(stranger)?);
    assert_eq!(delivery.destination, Destination::Discarded);
    assert_eq!(delivery.judgement, Judgement::Superseded);
    assert_eq!(pad.discarded(), 1);
    assert_eq!(pad.history(), before.history());
    assert_eq!(pad.latest(), None);
    Ok(())
}

#[test]
fn jobs_that_end_without_a_line_keep_their_state_and_stop() -> Checked {
    let mut jobs = Jobs::new()?;
    let mut pad = Scratchpad::new();
    pad.insert_text("1")?;
    let versions = pad.versions();

    // Cancelled while it waits before it starts.
    let (submission, pending) =
        pad.prepare(jobs.session, Operation::Eval, "", Limits::new(), None)?;
    let cancelled = pad.accept(pending, jobs.layer.submit(submission))?;
    jobs.layer.cancel(cancelled);
    assert_eq!(
        jobs.layer.state(cancelled),
        Some(JobState::Terminal(TerminalState::Cancelled))
    );
    assert_eq!(
        pad.deliver(&jobs.wait(cancelled)?).destination,
        Destination::Entry
    );

    // A report whose echo differs from the versions fails the job.
    let mut other = versions;
    other.settings = SettingsVersion::new(9);
    let failed = jobs.submit(
        &mut pad,
        Plan::new().request(report(OutcomeKind::Value, other, "1")),
    )?;
    assert_eq!(
        pad.deliver(&jobs.wait(failed)?).destination,
        Destination::Entry
    );

    // The deadline passed between requests: the backend stops the job
    // before its request ran.
    let exhausted = jobs.submit(&mut pad, Plan::new().stop(StopReason::Deadline))?;
    assert_eq!(
        pad.deliver(&jobs.wait(exhausted)?).destination,
        Destination::Entry
    );

    let expected = [
        (
            cancelled,
            TerminalState::Cancelled,
            Some(StopReason::Cancellation),
            false,
        ),
        (failed, TerminalState::Failed, None, true),
        (
            exhausted,
            TerminalState::ResourceExhausted,
            Some(StopReason::Deadline),
            false,
        ),
    ];
    for (id, state, stop, internal_error) in expected {
        let outcome = pad.entry(id).and_then(Entry::outcome).ok_or("no outcome")?;
        assert_eq!(outcome.state(), state);
        assert_eq!(outcome.stop(), stop);
        assert_eq!(outcome.internal_error(), internal_error);
        assert_eq!(outcome.item(), None);
    }
    Ok(())
}

#[test]
fn an_error_lies_in_the_submitted_snapshot_after_the_line_changed() -> Checked {
    let mut jobs = Jobs::new()?;
    let mut pad = Scratchpad::new();
    pad.insert_text("1/0")?;
    let versions = pad.versions();
    let plan = Plan::new()
        .request(report(
            OutcomeKind::Fault,
            versions,
            "DivisionByZero\t0..3\t-\t-\t-\t-\tdivision by zero",
        ))
        .gate(Gate::Finished, Release::Manual);
    let id = jobs.submit(&mut pad, plan)?;
    pad.clear()?;
    pad.insert_text("x")?;
    jobs.layer.controllable().release(0, Gate::Finished);

    let delivery = pad.deliver(&jobs.wait(id)?);
    assert_eq!(delivery.judgement, Judgement::Stale);
    let entry = pad.entry(id).ok_or("no entry")?;
    let outcome = entry.outcome().ok_or("no outcome")?;
    assert_eq!(outcome.state(), TerminalState::Failed);
    let item = outcome.item().ok_or("no item")?;
    let division = Fragment::new(entry.input().root(), 0..3).ok_or("a forward range")?;
    assert_eq!(
        item.content,
        Content::Error {
            kind: ErrorKind::DivisionByZero,
            place: Place::Fragment(division),
        }
    );
    assert_eq!(entry.input().items().len(), 3);
    Ok(())
}

#[test]
fn reasons_name_what_was_not_accepted() -> Checked {
    let mut jobs = Jobs::new()?;
    let mut pad = Scratchpad::new();
    pad.insert_text("1")?;
    let (submission, _) = pad.prepare(jobs.session, Operation::Eval, "", Limits::new(), None)?;
    let first = jobs.layer.submit(submission.clone())?;
    let second = jobs.layer.submit(submission)?;
    assert_eq!(
        NotAccepted::Refused(Refusal::Full { live: 8 }).to_string(),
        "the job layer refused the submission: full: 8 jobs are live"
    );
    assert_eq!(
        NotAccepted::NotAfterLatest {
            id: first,
            latest: second
        }
        .to_string(),
        "job 1 does not come after the latest accepted submission, job 2"
    );
    Ok(())
}
