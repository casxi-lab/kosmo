//! One event script run twice: the event handling records of the two runs
//! are the same byte for byte.

mod support;

use kosmo_types::{KeyAction, KeyMeaning, MonoTime, NamedKey};
use kosmo_ui::{EVALUATE_SELECTION, Event, INSERT_TEXT, MOVE_LEFT, Tagged};
use support::{Calc, Driver, Shape, focus_in, key, key_on, stamp, tagged};

/// The input of one round of the script: its platform events, and whether
/// the job report of submission 1 arrives in it.
struct Step {
    events: Vec<Event>,
    report: bool,
}

/// Twelve rounds: focus in; `Left` and `1` pressed, repeated and released;
/// `Execute`, whose submission is accepted in the next round and reported
/// in the round after; a round after the deadline of the timer that the
/// submission requested; and a key with another epoch.
fn script() -> Vec<Step> {
    let current = stamp(1, 1);
    let left = |action| key_on(current, key(KeyMeaning::Named(NamedKey::LEFT), action, 0));
    let one = |action| key_on(current, key(KeyMeaning::Char('1'), action, 0));
    let step = |events: Vec<Event>| Step {
        events,
        report: false,
    };
    vec![
        step(vec![focus_in(current)]),
        step(vec![left(KeyAction::Press)]),
        step(vec![left(KeyAction::Repeat)]),
        step(vec![left(KeyAction::Release)]),
        step(vec![one(KeyAction::Press)]),
        step(vec![one(KeyAction::Repeat)]),
        step(vec![one(KeyAction::Release)]),
        step(vec![key_on(
            current,
            key(KeyMeaning::Named(NamedKey::EXECUTE), KeyAction::Press, 0),
        )]),
        step(Vec::new()),
        Step {
            events: Vec::new(),
            report: true,
        },
        step(Vec::new()),
        step(vec![key_on(
            stamp(2, 1),
            key(KeyMeaning::Char('2'), KeyAction::Press, 0),
        )]),
    ]
}

/// Runs the script with a new calculator and returns the text of all the
/// records.
fn run() -> String {
    let mut calc = Calc::new(Shape::Line);
    calc.parts[3].handles = vec![MOVE_LEFT, INSERT_TEXT];
    calc.handles = vec![EVALUATE_SELECTION];
    calc.timer_on_submit = Some(MonoTime::from_nanos(10_500_000));
    let mut driver = Driver::new(calc);
    let mut text = String::new();
    for step in script() {
        let reports: Vec<Tagged<String, u64>> = if step.report {
            vec![tagged("1", 1, driver.app.versions())]
        } else {
            Vec::new()
        };
        text.push_str(&driver.round(step.events, reports).record.text());
    }
    text
}

#[test]
fn two_runs_of_one_script_write_the_same_record() {
    let first = run();
    let second = run();
    assert_eq!(first.as_bytes(), second.as_bytes());

    // The record holds effects, receipts, the outcomes of keys with their
    // handlers, drop reasons, and timers set and due.
    for expected in [
        "effect submit Request { text: \"11\", versions: Versions",
        "receipt accepted 1",
        "report Tagged { value: \"1\"",
        "named 1 press mods 0x0000 time 0 handled kosmo.MoveLeft by control 1:0",
        "named 1 repeat mods 0x0000 time 0 handled kosmo.MoveLeft by control 1:0",
        "named 1 release mods 0x0000 time 0 unhandled",
        "char '1' press mods 0x0000 time 0 handled kosmo.InsertText '1' by control 1:0",
        "named 4 press mods 0x0000 time 0 handled kosmo.EvaluateSelection by app",
        "epoch 2 generation 1 char '2' press mods 0x0000 time 0 dropped epoch",
        "timer set app deadline 10500000 label 1",
        "timer due app deadline 10500000 label 1",
    ] {
        assert!(first.contains(expected), "{expected} is not in:\n{first}");
    }
    assert_eq!(
        first
            .lines()
            .filter(|line| line.starts_with("round "))
            .count(),
        12
    );
    assert!(first.is_ascii());
}
