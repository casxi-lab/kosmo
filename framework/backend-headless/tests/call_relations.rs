//! The call relations of a script run: running the frozen script on the
//! headless backend records the call probes of the script runner's entry
//! in `kosmo-testkit` and of the single-round entry in `kosmo-ui`.
//!
//! The record of call probes holds the calls of the whole process, so this
//! test owns its test binary: it is the only test in this file, and the
//! calls of other tests do not mix in.

// The test needs the call probes and the test hooks, which only the
// command line turns on, as `--features test-hooks,call-probes`; without
// them this file compiles empty.
#![cfg(all(feature = "test-hooks", feature = "call-probes"))]

mod editor;

use std::path::Path;

use editor::{backend, driver};
use kosmo_testkit::probes;
use kosmo_testkit::script::Script;

/// The entries that a script run reaches.
const ENTRIES: [&str; 2] = ["kosmo-testkit::run", "kosmo-ui::run_round"];

#[test]
fn a_script_run_reaches_the_runner_and_the_single_round_entry() {
    let before = probes::check(&ENTRIES).unwrap_err();
    assert_eq!(before.entries, ENTRIES);
    let frozen =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/scripts/editing-and-a-job.script");
    let script = Script::read(&frozen).unwrap();
    script.run(&mut backend(), &mut driver(15)).unwrap();
    assert_eq!(probes::check(&ENTRIES), Ok(()));
    assert_eq!(probes::recorded(), ENTRIES);
}
