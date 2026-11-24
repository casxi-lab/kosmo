//! A resubmitted input on the host: the derivative of `x^2` with respect to
//! `x` is submitted, the line is changed to `x^3` and submitted again,
//! through the Scratchpad and the job layer with kalyx 0.1.0 on the host
//! stub's worker thread. In one order the old job is cancelled while it
//! waits at a gate before the kalyx runner; in the other, its `Finished` is
//! held until the new job's result has been delivered, so the old result
//! arrives late. The old job never shows a result, and its late result goes
//! only into the entry of `x^2`.
//!
//! Each run of an order writes a record of its deliveries. The records of
//! the two orders, one after the other, equal the checked-in record
//! `tests/data/resubmitted-input.txt` byte for byte. The gate and the hold
//! are in `support::gates`; nothing sleeps or reads the wall clock.

mod support;

use kosmo_jobs::{
    CancelAnswer, JobId, JobState, Limits, Operation, StopReason, Terminal, TerminalState,
};
use kosmo_math_kalyx::convert;
use kosmo_math_model::display::Freshness;
use kosmo_math_model::version::Judgement;
use kosmo_scratchpad::{Delivery, Destination, Entry, Scratchpad};
use support::Checked;
use support::gates::HostJobs;

/// The checked-in record of both orders: first the cancelled old job, then
/// the late old result.
const EXPECTED: &str = include_str!("data/resubmitted-input.txt");

/// How many times the repeated test runs each order.
const REPETITIONS: usize = 100;

/// The output line of the old job, `diff` of `x^2` with respect to `x` at
/// the source version 3, without its line feed.
const OLD_LINE: &str = "value\t3\t0\t0\t2*x";

/// The output line of the new job, `diff` of `x^3` with respect to `x` at
/// the source version 5, without its line feed.
const NEW_LINE: &str = "value\t5\t0\t0\t3*x^2";

/// One run of the example: its job layer, its Scratchpad, the result that
/// it shows and the record of its deliveries.
struct Run {
    jobs: HostJobs,
    pad: Scratchpad,
    /// The name of the order, which starts each line of the record.
    order: &'static str,
    /// The result of the last delivery judged current, rendered as current.
    shown: Option<String>,
    record: String,
}

impl Run {
    /// A run of the order `order` on a new Scratchpad and a new job layer,
    /// with the gate and the hold of [`HostJobs::new`].
    fn new(order: &'static str, gate: Option<usize>, hold: Option<usize>) -> Checked<Run> {
        Ok(Run {
            jobs: HostJobs::new(gate, hold)?,
            pad: Scratchpad::new(),
            order,
            shown: None,
            record: String::new(),
        })
    }

    /// Types `text` into the line one character at a time.
    fn type_text(&mut self, text: &str) -> Checked {
        for c in text.chars() {
            self.pad.insert_char(c)?;
        }
        Ok(())
    }

    /// Replaces the last character of the line with `c`.
    fn change_last(&mut self, c: char) -> Checked {
        self.pad.delete_backward()?;
        self.pad.insert_char(c)?;
        Ok(())
    }

    /// Submits the derivative of the line with respect to `x`, with the
    /// default limits and no time limit.
    fn submit(&mut self) -> Checked<JobId> {
        let session = self.jobs.session;
        let (submission, pending) =
            self.pad
                .prepare(session, Operation::Diff, "x", Limits::new(), None)?;
        let answer = self.jobs.layer.submit(submission);
        Ok(self.pad.accept(pending, answer)?)
    }

    /// Delivers `terminal` to the Scratchpad, shows its result when the
    /// delivery is judged current, and writes its line of the record.
    fn deliver(&mut self, terminal: &Terminal) -> Checked<Delivery> {
        let delivery = self.pad.deliver(terminal);
        let id = terminal.id();
        let judgement = judgement_name(delivery.judgement);
        let line = match delivery.destination {
            Destination::Entry => {
                let entry = self.pad.entry(id).ok_or("no entry")?;
                let result = result_of(entry, delivery.judgement)?;
                let line = format!(
                    "{id} {} {} {}: {judgement}: {result}",
                    entry.operation().name(),
                    entry.variable(),
                    convert(entry.input())?
                );
                if delivery.judgement == Judgement::Current {
                    self.shown = Some(result);
                }
                line
            }
            Destination::Discarded => format!("{id} discarded: {judgement}"),
            other => return Err(format!("{id} went into no entry: {other:?}").into()),
        };
        self.record.push_str(&format!("{}: {line}\n", self.order));
        Ok(delivery)
    }

    /// Writes the shown result as the last line of the record, shuts the
    /// job layer down, checks that the host stub's worker thread has ended
    /// and gives the record.
    fn finish(mut self) -> Checked<String> {
        let shown = self.shown.as_deref().unwrap_or("-");
        let line = format!("{}: shown {shown}\n", self.order);
        self.record.push_str(&line);
        assert_eq!(self.jobs.shut_down(), 0, "a worker thread is still live");
        Ok(std::mem::take(&mut self.record))
    }
}

/// The judgement as the record writes it.
fn judgement_name(judgement: Judgement) -> &'static str {
    match judgement {
        Judgement::Current => "current",
        Judgement::Superseded => "superseded",
        Judgement::Stale => "stale",
    }
}

/// The terminal state as the record writes it.
fn state_name(state: TerminalState) -> Checked<&'static str> {
    Ok(match state {
        TerminalState::Completed => "completed",
        TerminalState::Failed => "failed",
        TerminalState::Cancelled => "cancelled",
        TerminalState::ResourceExhausted => "resource_exhausted",
        other => return Err(format!("a terminal state without a name: {other:?}").into()),
    })
}

/// The result of `entry` as the record writes it: its display item,
/// rendered as current for a current delivery and as stale otherwise, or,
/// without one, its terminal state with the versions of its submission.
fn result_of(entry: &Entry, judgement: Judgement) -> Checked<String> {
    let outcome = entry.outcome().ok_or("no outcome")?;
    let freshness = match judgement {
        Judgement::Current => Freshness::Current,
        Judgement::Superseded | Judgement::Stale => Freshness::Stale,
    };
    Ok(match outcome.item() {
        Some(item) => item.render(freshness),
        None => format!(
            "{} (source {}, settings {})",
            state_name(outcome.state())?,
            entry.versions().source.get(),
            entry.versions().settings.get()
        ),
    })
}

/// The only output line of `terminal`, without its line feed.
fn line_of(terminal: &Terminal) -> Checked<String> {
    let [line] = terminal.record().lines() else {
        return Err("not one line".into());
    };
    let line = std::str::from_utf8(line)?
        .strip_suffix('\n')
        .ok_or("no final line feed")?;
    Ok(line.to_string())
}

/// The expected record of each order: the cancelled old job, and the late
/// old result.
fn expected_orders() -> Checked<(&'static str, &'static str)> {
    let late = EXPECTED.find("late: ").ok_or("no late order")?;
    Ok(EXPECTED.split_at(late))
}

/// When the old job is cancelled while it waits at the gate.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Cancel {
    /// After the job layer handled its `Started`: the job is running.
    Running,
    /// Before the job layer handled its `Started`: the job is queued as far
    /// as the job layer knows.
    Queued,
}

/// The old job waits at the gate before the kalyx runner; the line is
/// changed and submitted again, the old job is cancelled, and the gate
/// opens. Gives the record of the run.
fn cancelled_before_kalyx(cancel: Cancel) -> Checked<String> {
    let mut run = Run::new("cancel", Some(0), None)?;
    run.type_text("x^2")?;
    let old = run.submit()?;
    run.jobs.gate.wait_at_gate()?;
    if cancel == Cancel::Running {
        run.jobs.layer.poll();
        assert_eq!(run.jobs.layer.state(old), Some(JobState::Running));
    }
    run.change_last('3')?;
    let new = run.submit()?;
    assert_eq!(run.jobs.layer.live_jobs(), 2);
    let answer = match cancel {
        Cancel::Running => CancelAnswer::Requested,
        Cancel::Queued => CancelAnswer::Cancelled,
    };
    assert_eq!(run.jobs.layer.cancel(old), answer);
    run.jobs.gate.open();

    let terminal = run.jobs.layer.wait(old).ok_or("old: no terminal state")?;
    assert_eq!(terminal.record().state(), TerminalState::Cancelled);
    assert!(terminal.record().lines().is_empty());
    let delivery = run.deliver(&terminal)?;
    assert_eq!(delivery.judgement, Judgement::Stale);
    assert_eq!(delivery.destination, Destination::Entry);
    assert_eq!(run.shown, None);

    let terminal = run.jobs.layer.wait(new).ok_or("new: no terminal state")?;
    assert_eq!(terminal.record().state(), TerminalState::Completed);
    assert_eq!(line_of(&terminal)?, NEW_LINE);
    let delivery = run.deliver(&terminal)?;
    assert_eq!(delivery.judgement, Judgement::Current);
    assert_eq!(delivery.destination, Destination::Entry);

    // The kalyx runner found the cancellation before the first request, so
    // kalyx never ran for the old job.
    let finished = run.jobs.finished(old)?;
    assert!(finished.requests().is_empty(), "kalyx ran the old job");
    assert_eq!(finished.stop(), Some(StopReason::Cancellation));
    let entry = run.pad.entry(old).ok_or("old: no entry")?;
    let outcome = entry.outcome().ok_or("old: no outcome")?;
    assert_eq!(outcome.state(), TerminalState::Cancelled);
    assert_eq!(outcome.item(), None);
    assert_eq!(run.pad.latest(), Some(new));
    assert_eq!(run.pad.discarded(), 0);
    run.finish()
}

/// Neither job is cancelled; the old job's `Finished` is held until the new
/// job's result has been delivered, and then released. Gives the record of
/// the run.
fn late_old_result() -> Checked<String> {
    let mut run = Run::new("late", None, Some(0))?;
    run.type_text("x^2")?;
    let old = run.submit()?;
    run.change_last('3')?;
    let new = run.submit()?;
    assert_eq!(run.jobs.layer.live_jobs(), 2);

    // The new job reaches its terminal state first.
    let terminal = run.jobs.layer.wait(new).ok_or("new: no terminal state")?;
    assert_eq!(run.jobs.layer.state(old), Some(JobState::Running));
    assert_eq!(terminal.record().state(), TerminalState::Completed);
    assert_eq!(line_of(&terminal)?, NEW_LINE);
    let delivery = run.deliver(&terminal)?;
    assert_eq!(delivery.judgement, Judgement::Current);
    assert_eq!(delivery.destination, Destination::Entry);
    let newer = run.pad.entry(new).cloned();
    let shown = run.shown.clone();
    assert!(shown.is_some());

    // The old result arrives late: stale, and only into the entry of `x^2`.
    run.jobs.hold.release();
    run.jobs.layer.poll();
    let terminal = run.jobs.layer.take(old).ok_or("old: no terminal state")?;
    assert_eq!(terminal.record().state(), TerminalState::Completed);
    assert_eq!(line_of(&terminal)?, OLD_LINE);
    let delivery = run.deliver(&terminal)?;
    assert_eq!(delivery.judgement, Judgement::Stale);
    assert_eq!(delivery.destination, Destination::Entry);

    let entry = run.pad.entry(old).ok_or("old: no entry")?;
    assert_eq!(convert(entry.input())?, "x^2");
    assert_eq!(entry.versions().source.get(), 3);
    assert_eq!(run.pad.entry(new).cloned(), newer);
    assert_eq!(run.pad.latest(), Some(new));
    assert_eq!(run.pad.discarded(), 0);
    assert_eq!(run.shown, shown);
    run.finish()
}

/// The record of the cancelled old job and then of the late old result,
/// each on a new Scratchpad and job layer.
fn record() -> Checked<String> {
    Ok(cancelled_before_kalyx(Cancel::Running)? + &late_old_result()?)
}

#[test]
fn the_old_job_cancelled_at_the_gate_ends_cancelled_and_shows_no_result() -> Checked {
    cancelled_before_kalyx(Cancel::Running)?;
    Ok(())
}

#[test]
fn a_cancel_before_the_start_is_handled_also_stops_the_old_job_before_kalyx() -> Checked {
    let (cancel, _) = expected_orders()?;
    assert_eq!(cancelled_before_kalyx(Cancel::Queued)?, cancel);
    Ok(())
}

#[test]
fn the_late_old_result_goes_only_into_the_entry_of_its_own_version() -> Checked {
    late_old_result()?;
    Ok(())
}

#[test]
fn the_record_equals_the_checked_in_record_in_two_runs() -> Checked {
    let first = record()?;
    let second = record()?;
    assert_eq!(first, EXPECTED);
    assert_eq!(second, EXPECTED);
    Ok(())
}

#[test]
fn each_order_gives_the_same_record_in_every_repetition() -> Checked {
    let (cancel, late) = expected_orders()?;
    for repetition in 0..REPETITIONS {
        assert_eq!(
            cancelled_before_kalyx(Cancel::Running)?,
            cancel,
            "{repetition}"
        );
        assert_eq!(late_old_result()?, late, "{repetition}");
    }
    Ok(())
}
