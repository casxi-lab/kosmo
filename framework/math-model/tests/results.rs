//! Results never change the editing structure: a computation receives a
//! snapshot of the line, and whatever comes back, a value, an error or a
//! cancellation, leaves the buffer as it was before the submission.

mod support;

use kosmo_math_model::{EditBuffer, Fragment, Rejected, Snapshot};
use support::{put_cursor, text};

/// What a computation hands back, built from the snapshot it received.
enum Outcome {
    /// The text of a value.
    Value(String),
    /// An error about a fragment of the snapshot.
    Error {
        message: &'static str,
        fragment: Fragment,
    },
    /// The computation was cancelled and has no result.
    Cancelled,
}

/// A line that was edited, with the cursor inside it and a selection.
fn edited_line() -> Result<EditBuffer, Rejected> {
    let mut buffer = EditBuffer::new();
    buffer.insert_text("1/(x-2)")?;
    buffer.delete_backward()?;
    buffer.insert_char(')')?;
    put_cursor(&mut buffer, 3);
    buffer.extend_right();
    Ok(buffer)
}

/// What a result view shows for an outcome. It reads the snapshot that was
/// submitted, never the buffer.
fn show(outcome: &Outcome, snapshot: &Snapshot) -> String {
    match outcome {
        Outcome::Value(value) => value.clone(),
        Outcome::Error { message, fragment } => {
            let part = snapshot.items_of(*fragment).map(text).unwrap_or_default();
            format!("{message}: {part}")
        }
        Outcome::Cancelled => "cancelled".to_string(),
    }
}

/// Submits the edited line, lets the outcome that `compute` builds from the
/// snapshot arrive, and checks that the buffer is equal, field by field, to
/// a clone taken before the submission. Returns what was shown.
fn submit_and_receive(compute: impl FnOnce(&Snapshot) -> Outcome) -> Result<String, Rejected> {
    let buffer = edited_line()?;
    let before = buffer.clone();
    let snapshot = buffer.snapshot();
    assert_eq!(snapshot.version(), buffer.version());
    assert_eq!(snapshot.items(), buffer.items());
    let outcome = compute(&snapshot);
    let shown = show(&outcome, &snapshot);
    assert_eq!(buffer.root(), before.root());
    assert_eq!(buffer.items(), before.items());
    assert_eq!(buffer.cursor(), before.cursor());
    assert_eq!(buffer.selection(), before.selection());
    assert_eq!(buffer.version(), before.version());
    assert_eq!(buffer.completeness(), before.completeness());
    assert_eq!(buffer, before);
    Ok(shown)
}

#[test]
fn a_value_leaves_the_buffer_as_it_was() -> Result<(), Rejected> {
    let shown = submit_and_receive(|snapshot| {
        Outcome::Value(format!("1/(x - 2) at version {}", snapshot.version().get()))
    })?;
    assert_eq!(shown, "1/(x - 2) at version 3");
    Ok(())
}

#[test]
fn an_error_about_a_fragment_leaves_the_buffer_as_it_was() -> Result<(), Rejected> {
    let shown = submit_and_receive(|snapshot| Outcome::Error {
        message: "the divisor may be zero",
        fragment: Fragment::new(snapshot.root(), 2..7).unwrap(),
    })?;
    assert_eq!(shown, "the divisor may be zero: (x-2)");
    Ok(())
}

#[test]
fn a_cancellation_leaves_the_buffer_as_it_was() -> Result<(), Rejected> {
    let shown = submit_and_receive(|_| Outcome::Cancelled)?;
    assert_eq!(shown, "cancelled");
    Ok(())
}

#[test]
fn a_snapshot_keeps_its_content_while_the_buffer_changes() -> Result<(), Rejected> {
    let mut buffer = edited_line()?;
    let snapshot = buffer.snapshot();
    let kept = snapshot.clone();
    buffer.select_all();
    buffer.insert_text("y")?;
    assert_eq!(text(buffer.items()), "y");
    assert_eq!(snapshot, kept);
    assert_eq!(text(snapshot.items()), "1/(x-2)");
    assert!(snapshot.version() < buffer.version());
    Ok(())
}
