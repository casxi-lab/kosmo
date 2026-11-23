//! Reuse: a range of an entry's input, or of its value read back, copied
//! into the current line, with brackets where an operand is missing; the
//! copy leaves the entry as it was and increases the source version by 1;
//! a result that cannot be reused says why and changes nothing.

mod support;

use kosmo_jobs::controllable::{Gate, Release};
use kosmo_jobs::{JobId, Operation};
use kosmo_math_kalyx::{NotConverted, convert};
use kosmo_math_model::HoleKind;
use kosmo_scratchpad::{Destination, MAX_HISTORY_ENTRIES, NotReused, Part, Scratchpad};
use support::{Checked, DIVISION_BY_ZERO, EXPANDED, Jobs, SUM, answer, cases, text};

#[test]
fn a_whole_input_is_reused_in_an_empty_line() -> Checked {
    cases::whole_input()
}

#[test]
fn part_of_a_result_is_reused() -> Checked {
    cases::part_of_a_result()
}

#[test]
fn a_result_fills_the_hole_of_one_over() -> Checked {
    cases::fill_one_over_a_hole()
}

#[test]
fn part_of_an_input_fills_the_hole_of_two_times() -> Checked {
    cases::fill_two_times_a_hole()
}

#[test]
fn editing_after_reuse_changes_no_entry() -> Checked {
    let (mut pad, c) = cases::two_times_a_hole()?;
    let copy = pad.entry(c).cloned().ok_or("no entry")?;
    pad.reuse(c, Part::Input, 1..4)?;
    let reused = pad.versions().source.get();

    pad.insert_char('-')?;
    pad.insert_char('1')?;
    pad.move_to_start();
    pad.delete_forward()?;
    assert_eq!(text(&pad), "*(x+1)-1");
    assert_eq!(pad.versions().source.get(), reused + 3);

    let entry = pad.entry(c).ok_or("no entry")?;
    assert_eq!(entry.input(), copy.input());
    assert_eq!(entry.versions(), copy.versions());
    assert_eq!(
        entry.outcome().and_then(|outcome| outcome.item()),
        copy.outcome().and_then(|outcome| outcome.item())
    );
    assert_eq!(entry, &copy);
    Ok(())
}

#[test]
fn reuse_increases_the_source_version_by_one() -> Checked {
    let (mut pad, b) = cases::one_over_a_hole()?;
    let before = pad.versions().source.get();
    pad.reuse(b, Part::Result, 0..3)?;
    let after = pad.versions().source.get();
    assert_eq!(after, before + 1);
    Ok(())
}

/// The reason why reusing the whole result of the entry of job `id` in the
/// line `1/` fails, after checking that it changed neither the line nor its
/// cursor and source version.
fn refused(pad: &mut Scratchpad, id: JobId) -> Checked<NotReused> {
    pad.clear()?;
    pad.insert_text("1/")?;
    let before = pad.clone();
    let reason = pad.reuse(id, Part::Result, 0..1).err().ok_or("reused")?;
    assert_eq!(text(pad), "1/");
    assert_eq!(pad.line().cursor(), before.line().cursor());
    assert_eq!(pad.versions(), before.versions());
    assert_eq!(*pad, before);
    Ok(reason)
}

#[test]
fn results_that_cannot_be_reused_say_why_and_change_nothing() -> Checked {
    let (mut pad, failed) = cases::with_entry(Operation::Eval, "", "1/0", DIVISION_BY_ZERO)?;
    assert_eq!(refused(&mut pad, failed)?, NotReused::Error);

    // A job that waits before its end has no result yet.
    let mut jobs = Jobs::new()?;
    let mut pad = Scratchpad::new();
    pad.insert_text("1/3+1/6")?;
    let plan = answer(SUM, pad.versions()).gate(Gate::Finished, Release::Manual);
    let waiting = jobs.submit(&mut pad, Operation::Eval, "", plan)?;
    assert_eq!(refused(&mut pad, waiting)?, NotReused::NoResultYet);

    // The first of one more submission than the history holds is evicted.
    let mut jobs = Jobs::new()?;
    let mut pad = Scratchpad::new();
    pad.insert_text("1/3+1/6")?;
    let mut ids = Vec::new();
    for _ in 0..=MAX_HISTORY_ENTRIES {
        let plan = answer(SUM, pad.versions());
        let id = jobs.submit(&mut pad, Operation::Eval, "", plan)?;
        assert_eq!(pad.deliver(&jobs.take(id)?).destination, Destination::Entry);
        ids.push(id);
    }
    let evicted = ids[0];
    assert_eq!(pad.entry(evicted), None);
    assert_eq!(
        refused(&mut pad, evicted)?,
        NotReused::NotInHistory(evicted)
    );
    Ok(())
}

#[test]
fn a_fragment_need_not_be_an_operand() -> Checked {
    let (mut pad, c) = cases::with_entry(Operation::Expand, "x", "(x+1)^2", EXPANDED)?;
    pad.insert_text("1/")?;
    pad.reuse(c, Part::Result, 0..3)?;
    assert_eq!(text(&pad), "1/(1 +)");
    let Err(NotConverted::Incomplete(holes)) = convert(&pad.line().snapshot()) else {
        return Err("the line was converted".into());
    };
    assert_eq!(holes.len(), 1);
    assert_eq!(holes[0].kind, HoleKind::MissingOperand);
    Ok(())
}
