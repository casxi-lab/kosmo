use kosmo_types::{MonoTime, NamedKey};

use super::*;
use crate::command::{EVALUATE_SELECTION, MOVE_LEFT};
use crate::port::execute;
use crate::testing::{Logged, Note, Reply, SURFACE, TestApp, TestPort, Widget, node, press, stamp};
use crate::tree::{Key, View};

mod draw;
mod layout;

/// A view of a root, which is the region's root, and one focused control
/// that handles `MoveLeft` with `reply`.
fn focused_view(reply: Reply) -> View<Widget> {
    let mut focused = node(1, "line", 0);
    focused = View::new(
        Key::new(1),
        Widget {
            handles: Some(MOVE_LEFT),
            reply,
            ..focused.control().clone()
        },
    )
    .focused();
    node(0, "root", 0).region().child(focused)
}

fn input(time: u64) -> Input<TestPort> {
    Input::new(MonoTime::from_nanos(time))
}

/// Runs a round at `time` with `events` and returns its output.
fn round(
    runtime: &mut Runtime<TestApp>,
    app: &mut TestApp,
    time: u64,
    events: Vec<Event>,
) -> Output {
    let mut input = input(time);
    input.events = events;
    runtime.round(app, input).finish(app)
}

#[test]
fn a_timer_of_a_removed_control_is_dropped_and_counted() {
    let mut app = TestApp {
        view: Some(focused_view(Reply::Timer(100))),
        ..TestApp::default()
    };
    let mut runtime = Runtime::new();
    round(
        &mut runtime,
        &mut app,
        0,
        vec![Event::FocusIn {
            surface: SURFACE,
            stamp: stamp(1, 1),
        }],
    );
    let focused = runtime.tree().focused().unwrap();
    let set = round(
        &mut runtime,
        &mut app,
        10,
        vec![press(NamedKey::LEFT, stamp(1, 1))],
    );
    assert!(set.record.lines().contains(&format!(
        "timer set control {focused} deadline 100 label 100"
    )));
    assert_eq!(set.next_deadline, Some(MonoTime::from_nanos(100)));

    // A receipt is a message, so the round rebuilds the view without the
    // control.
    app.view = Some(node(0, "root", 0));
    let mut removal = input(50);
    removal.receipts.push(Receipt::Accepted(1));
    let removed = runtime.round(&mut app, removal).finish(&app);
    assert_eq!(removed.view, ViewPass::Rebuilt);
    assert!(!runtime.tree().contains(focused));

    let due = round(&mut runtime, &mut app, 100, Vec::new());
    assert!(due.record.lines().contains(&format!(
        "timer dropped missing control {focused} deadline 100 label 100"
    )));
    assert_eq!(runtime.counts().dropped_missing, 1);
    assert!(!app.log.contains(&Logged::Own(Note::ControlTimer(100))));
}

#[test]
fn a_full_timer_table_refuses_the_next_timer_distinctly() {
    let mut app = TestApp {
        view: Some(focused_view(Reply::Timer(5))),
        ..TestApp::default()
    };
    let mut runtime = Runtime::new();
    round(
        &mut runtime,
        &mut app,
        0,
        vec![Event::FocusIn {
            surface: SURFACE,
            stamp: stamp(1, 1),
        }],
    );
    let mut out: Out<TestApp> = Out::new();
    for label in 0..=crate::TIMER_CAPACITY as u64 {
        out.timer(
            MonoTime::from_nanos(1_000 + label),
            crate::TimerLabel::new(label),
        );
    }
    for request in out.timers {
        let _ = runtime.timers.register(request);
    }
    assert_eq!(runtime.timers.len(), crate::TIMER_CAPACITY);

    // A control's request in a round goes to the full table.
    let refused = round(
        &mut runtime,
        &mut app,
        2,
        vec![press(NamedKey::LEFT, stamp(1, 1))],
    );
    let focused = runtime.tree().focused().unwrap();
    assert!(refused.record.lines().contains(&format!(
        "timer refused full control {focused} deadline 5 label 5"
    )));
    assert_eq!(runtime.counts().timers_refused, 1);
    assert_eq!(runtime.timers.len(), crate::TIMER_CAPACITY);
    assert_eq!(refused.next_deadline, Some(MonoTime::from_nanos(1_000)));
}

#[test]
fn the_next_deadline_is_the_earlier_of_polling_and_the_timers() {
    let mut app = TestApp {
        view: Some(focused_view(Reply::Timer(500))),
        ..TestApp::default()
    };
    let mut runtime = Runtime::new();
    let mut polling = input(10);
    polling.unfinished_jobs = 1;
    let first = runtime.round(&mut app, polling).finish(&app);
    assert_eq!(
        first.next_deadline,
        Some(MonoTime::from_nanos(10 + crate::JOB_POLL_INTERVAL))
    );

    round(
        &mut runtime,
        &mut app,
        20,
        vec![Event::FocusIn {
            surface: SURFACE,
            stamp: stamp(1, 1),
        }],
    );
    let mut timer = input(30);
    timer.events = vec![press(NamedKey::LEFT, stamp(1, 1))];
    timer.unfinished_jobs = 1;
    let timer = runtime.round(&mut app, timer).finish(&app);
    // The timer is earlier than the polling deadline.
    assert_eq!(timer.next_deadline, Some(MonoTime::from_nanos(500)));
    let timer = round(&mut runtime, &mut app, 40, Vec::new());
    assert_eq!(timer.next_deadline, Some(MonoTime::from_nanos(500)));
    assert_eq!(timer.record.lines().last().unwrap(), "deadline 500");

    let due = round(&mut runtime, &mut app, 600, Vec::new());
    assert_eq!(due.next_deadline, None);
    assert_eq!(due.record.lines().last().unwrap(), "deadline none");
    assert!(app.log.contains(&Logged::Own(Note::ControlTimer(500))));

    // The polling deadline is earlier than a distant timer.
    let mut out: Out<TestApp> = Out::new();
    out.timer(
        MonoTime::from_nanos(1_000_000_000),
        crate::TimerLabel::new(9),
    );
    for request in out.timers {
        let _ = runtime.timers.register(request);
    }
    assert_eq!(runtime.timers.len(), 1);
    let mut both = input(700);
    both.unfinished_jobs = 1;
    let both = runtime.round(&mut app, both).finish(&app);
    let polling = 700 + crate::JOB_POLL_INTERVAL;
    assert_eq!(both.next_deadline, Some(MonoTime::from_nanos(polling)));
    assert_eq!(
        *both.record.lines().last().unwrap(),
        format!("deadline {polling}")
    );
}

#[test]
fn effects_keep_their_order_and_receipts_come_first_in_the_next_round() {
    let mut app = TestApp {
        handles: Some(EVALUATE_SELECTION),
        submits: vec!["first", "second"],
        ..TestApp::default()
    };
    let mut runtime = Runtime::new();
    let mut port = TestPort::default();
    let mut first = input(0);
    first.events = vec![
        Event::FocusIn {
            surface: SURFACE,
            stamp: stamp(1, 1),
        },
        press(NamedKey::EXECUTE, stamp(1, 1)),
    ];
    let mut step = runtime.round(&mut app, first);
    let requests: Vec<&str> = step
        .effects()
        .iter()
        .map(|effect| match effect {
            Effect::Submit(request) => *request,
            Effect::Cancel(_) => "cancel",
        })
        .collect();
    assert_eq!(requests, ["first", "second"]);
    let receipts = execute(&mut port, step.take_effects());
    let output = step.finish(&app);
    assert!(output.record.lines().ends_with(&[
        "effect submit \"first\"".to_string(),
        "effect submit \"second\"".to_string(),
        "view rebuilt controls 1".to_string(),
        "deadline none".to_string(),
    ]));

    app.log.clear();
    app.submits.clear();
    let mut second = input(1);
    second.receipts = receipts;
    second.events = vec![press(NamedKey::EXECUTE, stamp(1, 1))];
    let output = runtime.round(&mut app, second).finish(&app);
    assert_eq!(
        app.log,
        [
            Logged::Accepted(1),
            Logged::Accepted(2),
            Logged::Own(Note::App(EVALUATE_SELECTION)),
        ]
    );
    assert_eq!(output.record.lines()[1], "receipt accepted 1");
    assert_eq!(output.record.lines()[2], "receipt accepted 2");
    assert!(output.record.lines()[3].starts_with("key "));
}

#[test]
fn a_round_that_changes_only_presentation_state_skips_the_view() {
    let mut app = TestApp {
        view: Some(focused_view(Reply::StateOnly)),
        ..TestApp::default()
    };
    let mut runtime = Runtime::new();
    round(
        &mut runtime,
        &mut app,
        0,
        vec![Event::FocusIn {
            surface: SURFACE,
            stamp: stamp(1, 1),
        }],
    );
    let views = app.views.get();
    let before = runtime.tree().preorder();
    let focused = runtime.tree().focused().unwrap();
    assert_eq!(runtime.tree().state(focused), Some(&0));

    let output = round(
        &mut runtime,
        &mut app,
        1,
        vec![press(NamedKey::LEFT, stamp(1, 1))],
    );
    assert_eq!(output.view, ViewPass::Skipped);
    assert_eq!(app.views.get(), views);
    assert_eq!(runtime.tree().preorder(), before);
    assert_eq!(runtime.tree().state(focused), Some(&1));
    assert!(app.log.is_empty());
}

#[test]
fn a_focus_change_before_the_first_view_reaches_no_control() {
    let mut app = TestApp::default();
    let mut runtime = Runtime::new();
    let first = round(
        &mut runtime,
        &mut app,
        0,
        vec![Event::FocusIn {
            surface: SURFACE,
            stamp: stamp(1, 1),
        }],
    );
    assert_eq!(
        first.record.lines()[1],
        "focus-in surface 1 epoch 1 generation 1 no-region"
    );
    assert_eq!(first.view, ViewPass::Rebuilt);

    // Once a view has been applied, the region's root receives the change.
    let second = round(
        &mut runtime,
        &mut app,
        1,
        vec![Event::FocusOut {
            surface: SURFACE,
            stamp: stamp(1, 1),
        }],
    );
    assert_eq!(
        second.record.lines()[1],
        "focus-out surface 1 epoch 1 generation 1"
    );
}

#[test]
fn a_rejected_view_leaves_the_tree_and_is_recorded() {
    let mut app = TestApp::default();
    let mut runtime = Runtime::new();
    round(&mut runtime, &mut app, 0, Vec::new());
    let before = runtime.tree().preorder();
    app.view = Some(
        node(0, "root", 0)
            .child(node(1, "a", 0))
            .child(node(1, "b", 0)),
    );
    let mut again = input(1);
    again.receipts.push(Receipt::Rejected("busy"));
    let output = runtime.round(&mut app, again).finish(&app);
    assert_eq!(
        output.view,
        ViewPass::Rejected(RebuildError::DuplicateKey(Key::new(1)))
    );
    assert!(
        output
            .record
            .lines()
            .contains(&"view rejected duplicate-key 1".to_string())
    );
    assert_eq!(runtime.tree().preorder(), before);
    assert_eq!(app.log, [Logged::Rejected("busy")]);
}

#[test]
fn the_runtime_reports_the_latest_focus_of_a_surface() {
    let mut app = TestApp::default();
    let mut runtime = Runtime::new();
    assert_eq!(runtime.focus(SURFACE), None);
    round(
        &mut runtime,
        &mut app,
        0,
        vec![Event::FocusIn {
            surface: SURFACE,
            stamp: stamp(1, 1),
        }],
    );
    assert_eq!(runtime.focus(SURFACE), Some((stamp(1, 1), true)));
    round(
        &mut runtime,
        &mut app,
        1,
        vec![
            Event::FocusOut {
                surface: SURFACE,
                stamp: stamp(1, 1),
            },
            Event::FocusIn {
                surface: SURFACE,
                stamp: stamp(1, 2),
            },
            Event::FocusOut {
                surface: SURFACE,
                stamp: stamp(1, 2),
            },
        ],
    );
    assert_eq!(runtime.focus(SURFACE), Some((stamp(1, 2), false)));
    // A focus out with another stamp than the latest focus in changes
    // nothing.
    round(
        &mut runtime,
        &mut app,
        2,
        vec![Event::FocusOut {
            surface: SURFACE,
            stamp: stamp(1, 1),
        }],
    );
    assert_eq!(runtime.focus(SURFACE), Some((stamp(1, 2), false)));
}
