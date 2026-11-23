//! The checked-in session through the built `kosmo`: the script in
//! `tests/data/session-input.txt` goes to its standard input through a
//! pipe, and its standard output is the expected output in
//! `tests/data/session-output.txt`, byte for byte, in two runs.

use std::io::Write;
use std::process::{Command, Output, Stdio};

/// The script: two derivatives, an incomplete line, a line that is not in
/// the syntax and the history.
const SCRIPT: &[u8] = include_bytes!("data/session-input.txt");

/// What `kosmo` writes for it.
const EXPECTED: &[u8] = include_bytes!("data/session-output.txt");

/// One run of `kosmo` without arguments on the script.
fn session() -> Result<Output, Box<dyn std::error::Error>> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_kosmo"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    // The output is far smaller than a pipe holds, so the script is written
    // whole before the output is read; dropping the pipe ends the input.
    child
        .stdin
        .take()
        .ok_or("no standard input")?
        .write_all(SCRIPT)?;
    Ok(child.wait_with_output()?)
}

#[test]
fn the_session_writes_the_expected_output_in_two_runs() {
    let first = session().unwrap();
    let second = session().unwrap();
    for run in [&first, &second] {
        assert_eq!(run.status.code(), Some(0), "{run:?}");
        assert!(run.stderr.is_empty(), "{run:?}");
        assert_eq!(
            String::from_utf8_lossy(&run.stdout),
            String::from_utf8_lossy(EXPECTED)
        );
        assert_eq!(run.stdout, EXPECTED);
    }
    assert_eq!(first.stdout, second.stdout);
}
