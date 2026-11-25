//! The limits of a line: an edit past the item limit and a character
//! outside the character set are rejected and change nothing. The unit
//! tests of the buffer cover the byte limit of one insertion and the parse
//! depth.

mod support;

use kosmo_math_model::{EditBuffer, MAX_ITEMS, Rejected};
use support::{line, text};

/// Checks that `edit` is rejected with `reason` and leaves the buffer, its
/// items, cursor, selection and version, as it was.
fn assert_rejected(
    buffer: &mut EditBuffer,
    reason: Rejected,
    edit: impl FnOnce(&mut EditBuffer) -> Result<(), Rejected>,
) {
    let before = buffer.clone();
    assert_eq!(edit(buffer), Err(reason));
    assert_eq!(buffer.items(), before.items());
    assert_eq!(buffer.cursor(), before.cursor());
    assert_eq!(buffer.selection(), before.selection());
    assert_eq!(buffer.version(), before.version());
    assert_eq!(*buffer, before);
}

#[test]
fn inserting_into_a_full_line_is_rejected() -> Result<(), Rejected> {
    let mut buffer = line(&"1+".repeat(MAX_ITEMS / 2))?;
    assert_eq!(buffer.items().len(), MAX_ITEMS);
    let snapshot = line("x")?.snapshot();
    assert_rejected(&mut buffer, Rejected::TooManyItems, |b| b.insert_char('1'));
    assert_rejected(&mut buffer, Rejected::TooManyItems, |b| b.insert_text("1"));
    assert_rejected(&mut buffer, Rejected::TooManyItems, |b| {
        b.insert_fragment(&snapshot, snapshot.line())
    });
    buffer.move_to_start();
    assert_rejected(&mut buffer, Rejected::TooManyItems, |b| b.insert_char('-'));
    // Replacing a selected item keeps the line within the limit.
    buffer.extend_right();
    buffer.insert_char('2')?;
    assert!(text(buffer.items()).starts_with("2+1+"));
    assert_eq!(buffer.items().len(), MAX_ITEMS);
    Ok(())
}

#[test]
fn a_comma_is_rejected() -> Result<(), Rejected> {
    let mut buffer = line("f(x")?;
    assert_rejected(&mut buffer, Rejected::Character(','), |b| {
        b.insert_char(',')
    });
    assert_rejected(&mut buffer, Rejected::Character(','), |b| {
        b.insert_text(",y)")
    });
    assert_eq!(text(buffer.items()), "f(x");
    Ok(())
}
