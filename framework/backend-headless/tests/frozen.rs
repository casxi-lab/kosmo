//! The frozen script on the headless backend with the line editor of
//! `editor`: two runs with a new backend, editor and driver each write
//! byte-identical records at every checkpoint, and the records keep their
//! fixed order and hold only virtual times.

// The runner needs the test hooks: the check builds this package once
// more with them, and a build without them compiles this file empty.
#![cfg(feature = "test-hooks")]

mod editor;

use std::path::Path;

use editor::{EditorDriver, backend, driver, part};
use kosmo_testkit::script::{Record, Script};

/// The frozen script: it edits a line, moves focus away and back, submits
/// the line as a job and edits on while the job runs.
const FROZEN: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/scripts/editing-and-a-job.script"
);

/// The key events after which the release plan lets the job of the frozen
/// script finish: the 15th is the press of Delete, after the job started.
const KEYS_BEFORE_THE_RESULT: usize = 15;

/// Runs the frozen script with a new backend, editor and driver.
#[allow(clippy::unwrap_used, reason = "the frozen script reads and runs")]
fn run_frozen() -> (Record, EditorDriver) {
    let script = Script::read(Path::new(FROZEN)).unwrap();
    let mut driver = driver(KEYS_BEFORE_THE_RESULT);
    let record = script.run(&mut backend(), &mut driver).unwrap();
    (record, driver)
}

#[test]
fn two_runs_of_the_frozen_script_write_byte_identical_records() {
    let (first, _) = run_frozen();
    let (second, _) = run_frozen();
    assert_eq!(first.checkpoints().len(), 4);
    assert_eq!(first.checkpoints(), second.checkpoints());
    assert_eq!(first.text().as_bytes(), second.text().as_bytes());

    // The three layers of each checkpoint.
    let sections = first.checkpoints();
    let times = ["0", "6000000", "8000000", "41000000"];
    for (number, (section, time)) in (1..).zip(sections.iter().zip(times)) {
        assert!(
            section.starts_with(&format!("checkpoint {number} at {time}\nview 4\n")),
            "{section}"
        );
        let display = part(section, "display 1 ");
        assert_eq!(display[0], format!("display 1 {}", display.len() - 1));
        assert!(display.len() > 1, "{section}");
        assert!(
            section
                .lines()
                .last()
                .unwrap()
                .starts_with("pixels 1 64x8 stride=256 xrgb8888 sha256="),
            "{section}"
        );
    }
    // The view model: focus, the line, the cursor and the job. The focus
    // in that the backend queues with the main surface comes before the
    // first view, so it reaches no control: the editor learns of focus
    // from the focus lines.
    assert_eq!(
        part(&sections[0], "view "),
        [
            "view 4",
            "region main focus no",
            "line \"\"",
            "cursor 0",
            "job none"
        ]
    );
    assert_eq!(
        part(&sections[1], "view ")[2..],
        ["line \"1+2\"", "cursor 1", "job none"]
    );
    // The key of the old focus generation moved nothing: the cursor moved
    // once to the right, and the job runs, held by its plan.
    assert_eq!(
        part(&sections[2], "view ")[1..],
        [
            "region main focus yes",
            "line \"1+2\"",
            "cursor 2",
            "job 1 running"
        ]
    );
    // Delete was handled, the plan released the job, and its result came.
    assert_eq!(
        part(&sections[3], "view ")[2..],
        ["line \"12\"", "cursor 1", "job 1 Completed 3"]
    );
    // Each change of the view drew another frame.
    let pixels: Vec<&str> = sections
        .iter()
        .map(|section| section.lines().last().unwrap())
        .collect();
    for (one, next) in pixels.iter().zip(&pixels[1..]) {
        assert_ne!(one, next);
    }
}

#[test]
fn records_follow_their_fixed_order_and_hold_only_virtual_times() {
    let (record, driver) = run_frozen();
    for section in record.checkpoints() {
        let heads: Vec<&str> = section
            .lines()
            .filter(|line| {
                ["checkpoint ", "view ", "display ", "pixels "]
                    .iter()
                    .any(|head| line.starts_with(head))
            })
            .map(|line| line.split(' ').next().unwrap())
            .collect();
        assert_eq!(heads, ["checkpoint", "view", "display", "pixels"]);
        assert!(section.ends_with('\n'));
        assert!(!section.contains('\t') && !section.contains('\r'));
    }
    // The event handling record reads only the virtual clock: each round's
    // time is one of the script's times or a deadline on the way.
    let times: Vec<u64> = driver
        .record()
        .iter()
        .filter_map(|line| line.strip_prefix("round "))
        .map(|line| line.rsplit(' ').next().unwrap().parse().unwrap())
        .collect();
    let mut expected = vec![0, 1_000_000, 2_000_000, 3_000_000, 4_000_000, 4_500_000];
    expected.extend([5_000_000, 6_000_000, 7_000_000, 8_000_000]);
    // While the job runs, the clock stops at each polling deadline on the
    // way to 40 ms, 16 ms apart.
    expected.extend([24_000_000, 40_000_000, 41_000_000]);
    let mut seen = times.clone();
    seen.dedup();
    assert_eq!(seen, expected);
    assert!(times.windows(2).all(|pair| pair[0] <= pair[1]));
}
