//! The cases of the history and of reuse, each on a Scratchpad of its own
//! with a job layer on the controllable backend. The test files of the
//! history and of reuse run them, and the test of files runs them all
//! again between its two listings of the working directory.

use kosmo_jobs::controllable::{Gate, Release};
use kosmo_jobs::{JobId, MAX_LIVE_JOBS, Operation};
use kosmo_math_model::Position;
use kosmo_math_model::version::{ContextVersion, Judgement, SettingsVersion};
use kosmo_scratchpad::{Destination, Entry, MAX_HISTORY_ENTRIES, Part, Scratchpad};

use super::{
    Checked, DERIVATIVE, EXPANDED, Jobs, NEWER, OLDER, SUM, answer, linear, numbers, ones, text,
    value_of,
};

/// A new Scratchpad whose history holds one delivered entry: the line
/// `line`, typed as one insertion and submitted with `operation` and
/// `variable`, at the versions 1, 0 and 0, answered with the output line
/// `output`. The current line is cleared afterwards.
pub fn with_entry(
    operation: Operation,
    variable: &str,
    line: &str,
    output: &str,
) -> Checked<(Scratchpad, JobId)> {
    let mut jobs = Jobs::new()?;
    let mut pad = Scratchpad::new();
    pad.insert_text(line)?;
    let versions = pad.versions();
    assert_eq!(numbers(versions), (1, 0, 0));
    let id = jobs.submit(&mut pad, operation, variable, answer(output, versions))?;
    let delivery = pad.deliver(&jobs.take(id)?);
    assert_eq!(delivery.judgement, Judgement::Current);
    assert_eq!(delivery.destination, Destination::Entry);
    pad.clear()?;
    Ok((pad, id))
}

/// Two submissions at the settings version 1, the second after one more
/// character: the older result arrives last and goes only into its own
/// entry.
pub fn late_result() -> Checked {
    let mut jobs = Jobs::new()?;
    let mut pad = Scratchpad::with_versions(ContextVersion::new(0), SettingsVersion::new(1));
    pad.insert_text("1/3+1/6")?;
    let older = pad.versions();
    let gated = |line, versions| answer(line, versions).gate(Gate::Finished, Release::Manual);
    let first = jobs.submit(&mut pad, Operation::Eval, "", gated(OLDER, older))?;
    pad.insert_char('0')?;
    assert_eq!(text(&pad), "1/3+1/60");
    let newer = pad.versions();
    let second = jobs.submit(&mut pad, Operation::Eval, "", gated(NEWER, newer))?;
    let tagged: Vec<_> = pad
        .history()
        .iter()
        .map(|entry| numbers(entry.versions()))
        .collect();
    assert_eq!(tagged, [(1, 0, 1), (2, 0, 1)]);

    // The newer job finishes first and is current.
    jobs.release(1);
    let delivery = pad.deliver(&jobs.take(second)?);
    assert_eq!(delivery.judgement, Judgement::Current);
    assert_eq!(delivery.destination, Destination::Entry);
    assert_eq!(value_of(&pad, second)?, "7/20");
    let newer_entry = pad.entry(second).cloned();

    // The older one arrives late: stale, only into its own entry.
    jobs.release(0);
    let delivery = pad.deliver(&jobs.take(first)?);
    assert_eq!(delivery.judgement, Judgement::Stale);
    assert_eq!(delivery.destination, Destination::Entry);
    assert_eq!(value_of(&pad, first)?, "1/2");
    assert_eq!(pad.entry(second).cloned(), newer_entry);
    assert_eq!(pad.latest(), Some(second));
    assert_eq!(pad.discarded(), 0);
    Ok(())
}

/// One more submission than the history holds, each delivered as soon as
/// it was accepted: the oldest entry is evicted, the others stay as they
/// were. Gives the job identities in order of acceptance.
pub fn eviction() -> Checked<Vec<JobId>> {
    let mut jobs = Jobs::new()?;
    let mut pad = Scratchpad::new();
    let mut ids = Vec::new();
    let mut kept = Vec::new();
    for k in 1..=MAX_HISTORY_ENTRIES + 1 {
        pad.insert_char('1')?;
        if k == MAX_HISTORY_ENTRIES + 1 {
            kept = pad.history().iter().skip(1).cloned().collect();
        }
        let versions = pad.versions();
        assert_eq!(numbers(versions), (k as u64, 0, 0));
        let id = jobs.submit(&mut pad, Operation::Eval, "", answer(&ones(k), versions))?;
        assert!(jobs.layer.live_jobs() <= MAX_LIVE_JOBS);
        let delivery = pad.deliver(&jobs.take(id)?);
        assert_eq!(delivery.destination, Destination::Entry);
        ids.push(id);
    }

    let held: Vec<JobId> = pad.history().iter().map(Entry::id).collect();
    assert_eq!(held.len(), MAX_HISTORY_ENTRIES);
    assert_eq!(held, ids[1..]);
    assert_eq!(pad.entry(ids[0]), None);
    assert_eq!(pad.history()[..MAX_HISTORY_ENTRIES - 1], kept[..]);
    for (k, entry) in (2..).zip(pad.history()) {
        assert_eq!(entry.input().items().len(), k);
        assert_eq!(numbers(entry.versions()), (k as u64, 0, 0));
        assert_eq!(value_of(&pad, entry.id())?, "1".repeat(k));
    }
    assert_eq!(pad.discarded(), 0);
    Ok(ids)
}

/// The whole input of an entry reused in the empty line.
pub fn whole_input() -> Checked {
    let (mut pad, a) = with_entry(Operation::Eval, "", "1/3+1/6", SUM)?;
    let before = pad.versions().source.get();
    pad.reuse(a, Part::Input, 0..7)?;
    assert_eq!(text(&pad), "1/3+1/6");
    assert_eq!(linear(&pad)?, "1/3+1/6");
    assert_eq!(pad.versions().source.get(), before + 1);
    Ok(())
}

/// The term `x^2` of an expanded result reused in the empty line.
pub fn part_of_a_result() -> Checked {
    let (mut pad, c) = with_entry(Operation::Expand, "x", "(x+1)^2", EXPANDED)?;
    assert_eq!(value_of(&pad, c)?, "1 + 2*x + x^2");
    let before = pad.versions().source.get();
    pad.reuse(c, Part::Result, 10..13)?;
    assert_eq!(text(&pad), "x^2");
    assert_eq!(linear(&pad)?, "x^2");
    assert_eq!(pad.versions().source.get(), before + 1);
    Ok(())
}

/// A Scratchpad with the entry of `diff x x^2` and the current line `1/`,
/// whose cursor is at its hole.
pub fn one_over_a_hole() -> Checked<(Scratchpad, JobId)> {
    let (mut pad, b) = with_entry(Operation::Diff, "x", "x^2", DERIVATIVE)?;
    pad.insert_text("1/")?;
    assert_eq!(
        pad.line().cursor(),
        Position {
            block: pad.line().root(),
            index: 2
        }
    );
    Ok((pad, b))
}

/// The result `2*x` filling the hole of `1/`.
pub fn fill_one_over_a_hole() -> Checked {
    let (mut pad, b) = one_over_a_hole()?;
    let before = pad.versions().source.get();
    pad.reuse(b, Part::Result, 0..3)?;
    assert_eq!(text(&pad), "1/(2*x)");
    assert_eq!(linear(&pad)?, "1/(2*x)");
    assert_eq!(pad.versions().source.get(), before + 1);
    Ok(())
}

/// A Scratchpad with the entry of `expand x (x+1)^2` and the current line
/// `2*`, whose cursor is at its hole.
pub fn two_times_a_hole() -> Checked<(Scratchpad, JobId)> {
    let (mut pad, c) = with_entry(Operation::Expand, "x", "(x+1)^2", EXPANDED)?;
    pad.insert_text("2*")?;
    Ok((pad, c))
}

/// The part `x+1` of an input filling the hole of `2*`.
pub fn fill_two_times_a_hole() -> Checked {
    let (mut pad, c) = two_times_a_hole()?;
    let before = pad.versions().source.get();
    pad.reuse(c, Part::Input, 1..4)?;
    assert_eq!(text(&pad), "2*(x+1)");
    assert_eq!(linear(&pad)?, "2*(x+1)");
    assert_eq!(pad.versions().source.get(), before + 1);
    Ok(())
}
