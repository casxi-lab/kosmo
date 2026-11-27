//! The script runner of `kosmo-testkit` on the headless backend, with the
//! line editor of `editor`: a key of an earlier focus generation is
//! dropped, a header must fit the capabilities, and a run stops with the
//! line number of each problem that only a run finds.

// The runner needs the test hooks: the check builds this package once
// more with them, and a build without them compiles this file empty.
#![cfg(feature = "test-hooks")]

mod editor;

use editor::{EditorDriver, MAIN, backend, driver, part};
use kosmo_backend_headless::HeadlessBackend;
use kosmo_platform::{
    EventSource, FaultPort, PlatformError, PlatformTestHooks, Stamp, SurfaceId, WaitEnd,
};
use kosmo_testkit::script::{Driver, MAX_ROUNDS_PER_LINE, Problem, Record, Script, ScriptError};
use kosmo_types::{Epoch, FocusGen, MonoTime};

/// Runs `text`, which reads as a script, with a new backend and `driver`.
#[allow(clippy::unwrap_used, reason = "the tests give scripts that read")]
fn run(text: &str, driver: &mut EditorDriver) -> Result<Record, ScriptError> {
    Script::parse(text.as_bytes())
        .unwrap()
        .run(&mut backend(), driver)
}

#[test]
fn a_key_with_the_stamp_of_an_earlier_focus_is_not_delivered() {
    let mut driver = driver(usize::MAX);
    let record = run(
        "kosmo-script 1 surface 64x8 xrgb8888\n\
         at 1000 press 7:4 char:a mods=0000\n\
         focus out 1 stamp=1/1\n\
         focus in 1 stamp=1/2\n\
         at 2000 press 7:5 char:b mods=0000 stamp=1/1\n\
         at 3000 press 7:6 char:c mods=0000\n\
         checkpoint\n",
        &mut driver,
    )
    .unwrap();
    assert_eq!(
        part(&record.checkpoints()[0], "view ")[2..],
        ["line \"ac\"", "cursor 2", "job none"]
    );
    let keys: Vec<&String> = driver
        .record()
        .iter()
        .filter(|line| line.starts_with("key "))
        .collect();
    assert_eq!(
        keys,
        [
            "key surface 1 epoch 1 generation 1 char 'a' press mods 0x0000 time 1000 handled kosmo.InsertText 'a' by app",
            "key surface 1 epoch 1 generation 1 char 'b' press mods 0x0000 time 2000 dropped generation",
            "key surface 1 epoch 1 generation 2 char 'c' press mods 0x0000 time 3000 handled kosmo.InsertText 'c' by app",
        ]
    );
}

#[test]
fn a_key_at_the_time_of_a_focus_line_takes_its_stamp() {
    // The focus lines and the key come at one time, so no round runs
    // between them: the key takes the stamp of the latest focus line that
    // entered the main surface, not the focus that the framework recorded.
    let mut driver = driver(usize::MAX);
    run(
        "kosmo-script 1 surface 64x8 xrgb8888\n\
         checkpoint\n\
         focus out 1 stamp=1/1\n\
         focus in 1 stamp=1/2\n\
         at 0 press 7:4 char:a mods=0000\n\
         checkpoint\n",
        &mut driver,
    )
    .unwrap();
    let keys: Vec<&String> = driver
        .record()
        .iter()
        .filter(|line| line.starts_with("key "))
        .collect();
    assert_eq!(
        keys,
        [
            "key surface 1 epoch 1 generation 2 char 'a' press mods 0x0000 time 0 handled kosmo.InsertText 'a' by app"
        ]
    );
}

#[test]
fn more_keys_than_one_wait_returns_are_all_handled_before_the_checkpoint() {
    // One wait returns at most 64 events, so the 65th key comes in a
    // round whose wait returns events, not `Idle`.
    let mut text = String::from("kosmo-script 1 surface 64x8 xrgb8888\n");
    for _ in 0..65 {
        text.push_str("at 5 press 7:50 named:left mods=0000\n");
    }
    text.push_str("checkpoint\n");
    let mut driver = driver(usize::MAX);
    run(&text, &mut driver).unwrap();
    let keys = driver
        .record()
        .iter()
        .filter(|line| line.starts_with("key "))
        .count();
    assert_eq!(keys, 65);
}

#[test]
fn lines_of_one_time_run_no_round_between_them() {
    let mut driver = driver(usize::MAX);
    run(
        "kosmo-script 1 surface 64x8 xrgb8888\n\
         at 5 press 7:4 char:a mods=0000\n\
         at 5 press 7:5 char:b mods=0000\n\
         checkpoint\n",
        &mut driver,
    )
    .unwrap();
    let lines: Vec<&String> = driver
        .record()
        .iter()
        .filter(|line| line.starts_with("key ") || line.starts_with("round "))
        .collect();
    let first = lines
        .iter()
        .position(|line| line.starts_with("key "))
        .unwrap();
    assert!(lines[first + 1].starts_with("key "), "{lines:?}");
    assert!(lines[first + 1].contains("char 'b'"), "{lines:?}");
}

#[test]
fn a_header_outside_the_capabilities_stops_the_run_at_line_1() {
    for header in [
        "kosmo-script 1 surface 65x8 xrgb8888",
        "kosmo-script 1 surface 64x9 xrgb8888",
        "kosmo-script 1 surface 64x8 rgb565",
    ] {
        let error = run(&format!("{header}\ncheckpoint\n"), &mut driver(0)).unwrap_err();
        assert_eq!(error.line, 1, "{header}");
        assert!(
            matches!(error.problem, Problem::OutsideCaps(_)),
            "{header}: {error}"
        );
    }
    let error = run("kosmo-script 1 surface 64x8 rgb565\n", &mut driver(0)).unwrap_err();
    assert_eq!(
        error.to_string(),
        "line 1: the main surface 64x8 rgb565 is outside the capabilities of the platform"
    );
    // A smaller surface in the other format fits.
    let record = run(
        "kosmo-script 1 surface 8x2 rgba8-premul\ncheckpoint\n",
        &mut driver(0),
    )
    .unwrap();
    assert!(
        record.checkpoints()[0]
            .lines()
            .last()
            .unwrap()
            .starts_with("pixels 1 8x2 stride=32 rgba8-premul sha256=")
    );
}

#[test]
fn a_focus_line_for_a_surface_that_the_script_did_not_create_stops_the_run() {
    let error = run(
        "kosmo-script 1 surface 64x8 xrgb8888\ncheckpoint\nfocus in 2 stamp=1/1\n",
        &mut driver(0),
    )
    .unwrap_err();
    assert_eq!((error.line, error.problem), (3, Problem::NoSurface(2)));
}

#[test]
fn platform_failures_come_from_the_test_hooks_and_stop_the_run() {
    let script = Script::parse(b"kosmo-script 1 surface 64x8 xrgb8888\ntime 50\n").unwrap();
    let mut backend = backend();
    backend
        .inject_fault(FaultPort::CreateSurface, PlatformError::Exhausted)
        .unwrap();
    let error = script.run(&mut backend, &mut driver(0)).unwrap_err();
    assert_eq!(
        (error.line, error.problem),
        (1, Problem::Platform(PlatformError::Exhausted))
    );
    // A clock that is later than the script's time.
    let mut backend = self::backend();
    backend.advance_to(MonoTime::from_nanos(100)).unwrap();
    let error = script.run(&mut backend, &mut driver(0)).unwrap_err();
    assert_eq!(
        (error.line, error.problem),
        (
            2,
            Problem::Earlier {
                time: MonoTime::from_nanos(50),
                clock: MonoTime::from_nanos(100)
            }
        )
    );
}

/// A driver without a framework, which waits once per round, counts its
/// rounds and writes what its fields say.
struct Fake {
    pending: bool,
    fails: bool,
    stamp: Option<Stamp>,
    view: Vec<String>,
    display: String,
    /// The rounds so far.
    rounds: usize,
    /// The first rounds whose next deadline is due: the deadline 0.
    due_rounds: usize,
    /// The rounds after each round whose wait returned events for which
    /// the driver has something pending.
    busy_after_events: usize,
    /// The rounds for which it still has something pending.
    busy: usize,
}

impl Fake {
    fn new() -> Fake {
        Fake {
            pending: false,
            fails: false,
            stamp: None,
            view: Vec::new(),
            display: String::new(),
            rounds: 0,
            due_rounds: 0,
            busy_after_events: 0,
            busy: 0,
        }
    }
}

impl Driver<HeadlessBackend> for Fake {
    type Error = &'static str;

    fn round(&mut self, platform: &mut HeadlessBackend) -> Result<WaitEnd, &'static str> {
        if self.fails {
            return Err("the round broke");
        }
        self.rounds += 1;
        let end = platform
            .wait(None, &mut Vec::new())
            .map_err(|_| "the wait failed")?;
        self.busy = if end == WaitEnd::Events {
            self.busy_after_events
        } else {
            self.busy.saturating_sub(1)
        };
        Ok(end)
    }

    fn pending(&self) -> bool {
        self.pending || self.busy > 0
    }

    fn next_deadline(&self) -> Option<MonoTime> {
        (self.rounds < self.due_rounds).then_some(MonoTime::ZERO)
    }

    fn focus_stamp(&self, _: SurfaceId) -> Option<Stamp> {
        self.stamp
    }

    fn view(&self, out: &mut Vec<String>) {
        out.extend(self.view.iter().cloned());
    }

    fn display(&self, surface: SurfaceId, out: &mut String) {
        if surface == MAIN {
            out.push_str(&self.display);
        }
    }
}

/// Runs `text`, which reads as a script, with a new backend and `driver`.
#[allow(clippy::unwrap_used, reason = "the tests give scripts that read")]
fn run_fake(text: &str, driver: &mut Fake) -> Result<Record, ScriptError> {
    Script::parse(text.as_bytes())
        .unwrap()
        .run(&mut backend(), driver)
}

#[test]
fn a_framework_that_never_becomes_idle_stops_the_run_after_the_bound() {
    let mut fake = Fake::new();
    fake.pending = true;
    let error = run_fake("kosmo-script 1 surface 64x8 xrgb8888\n", &mut fake).unwrap_err();
    assert_eq!((error.line, error.problem), (1, Problem::NotIdle));
    assert_eq!(
        Problem::NotIdle.to_string(),
        format!("the framework did not become idle within {MAX_ROUNDS_PER_LINE} rounds")
    );
}

#[test]
fn a_line_runs_at_most_max_rounds_per_line_rounds() {
    let mut fake = Fake::new();
    fake.pending = true;
    let error = run_fake("kosmo-script 1 surface 64x8 xrgb8888\n", &mut fake).unwrap_err();
    assert_eq!((error.line, error.problem), (1, Problem::NotIdle));
    assert_eq!(fake.rounds, MAX_ROUNDS_PER_LINE);
}

#[test]
fn a_due_deadline_keeps_the_framework_running() {
    // The first five rounds start with a due deadline, so the framework is
    // idle only after a sixth round, which starts quiet and waits `Idle`.
    let mut fake = Fake::new();
    fake.due_rounds = 5;
    run_fake("kosmo-script 1 surface 64x8 xrgb8888\n", &mut fake).unwrap();
    assert_eq!(fake.rounds, 6);
}

#[test]
fn the_bound_of_rounds_holds_for_each_line() {
    // Each round whose wait returns events keeps the driver busy for 40,000
    // rounds: the header, with the events of the new surface, and each key
    // line. Each line stays within the bound, all of them together do not.
    let mut fake = Fake::new();
    fake.busy_after_events = 40_000;
    fake.stamp = Some(Stamp::new(Epoch::new(1), FocusGen::new(1)));
    run_fake(
        "kosmo-script 1 surface 64x8 xrgb8888\n\
         at 1 press 7:4 char:a mods=0000\n\
         at 2 press 7:4 char:a mods=0000\n\
         checkpoint\n",
        &mut fake,
    )
    .unwrap();
    assert!(fake.rounds > MAX_ROUNDS_PER_LINE, "{}", fake.rounds);
}

#[test]
fn a_failing_round_and_a_key_without_a_stamp_stop_the_run() {
    let mut fake = Fake::new();
    fake.fails = true;
    let error = run_fake("kosmo-script 1 surface 64x8 xrgb8888\n", &mut fake).unwrap_err();
    assert_eq!(
        (error.line, error.problem),
        (1, Problem::Driver("the round broke".to_string()))
    );
    let error = run_fake(
        "kosmo-script 1 surface 64x8 xrgb8888\nat 5 press 7:4 char:a mods=0000\n",
        &mut Fake::new(),
    )
    .unwrap_err();
    assert_eq!((error.line, error.problem), (2, Problem::NoStamp));
}

#[test]
fn view_lines_and_display_text_keep_their_form() {
    let text = "kosmo-script 1 surface 64x8 xrgb8888\ncheckpoint\ncheckpoint\n";
    let mut fake = Fake::new();
    fake.view = vec!["a\tb".to_string()];
    let error = run_fake(text, &mut fake).unwrap_err();
    assert_eq!(
        (error.line, error.problem),
        (2, Problem::View("a\tb".to_string()))
    );
    for display in ["pop-clip", "pop-clip\r\n", "\n\u{1b}\n"] {
        let mut fake = Fake::new();
        fake.display = display.to_string();
        let error = run_fake(text, &mut fake).unwrap_err();
        assert_eq!((error.line, error.problem), (2, Problem::Display(1)));
    }
    // A surface that never presented has no display lines and no pixels.
    let mut fake = Fake::new();
    fake.view = vec!["state ready".to_string()];
    let record = run_fake(text, &mut fake).unwrap();
    assert_eq!(
        record.checkpoints(),
        [
            "checkpoint 1 at 0\nview 1\nstate ready\ndisplay 1 0\npixels 1 none\n",
            "checkpoint 2 at 0\nview 1\nstate ready\ndisplay 1 0\npixels 1 none\n"
        ]
    );
}
