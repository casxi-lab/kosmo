//! The source version: each accepted edit that changes the content
//! increases it by exactly 1, and nothing else changes it. The rejection of
//! an edit past the maximum version is a unit test of the buffer, which can
//! start a buffer near the maximum.

mod support;

use kosmo_math_model::{EditBuffer, Fragment, Rejected};
use support::{line, put_cursor, text};

/// Checks that `edit` leaves the version as it was.
fn assert_same_version(buffer: &mut EditBuffer, edit: impl FnOnce(&mut EditBuffer)) {
    let before = buffer.version();
    edit(buffer);
    assert_eq!(buffer.version(), before);
}

#[test]
fn content_edits_increase_the_version_by_one() -> Result<(), Rejected> {
    let snapshot = line("x+1")?.snapshot();
    let mut buffer = EditBuffer::new();
    assert_eq!(buffer.version().get(), 0);
    let mut seen = Vec::new();
    buffer.insert_char('2')?;
    seen.push(buffer.version().get());
    buffer.insert_text("*3")?;
    seen.push(buffer.version().get());
    buffer.delete_backward()?;
    seen.push(buffer.version().get());
    buffer.insert_fragment(&snapshot, snapshot.line())?;
    seen.push(buffer.version().get());
    buffer.move_to_start();
    buffer.delete_forward()?;
    seen.push(buffer.version().get());
    buffer.clear()?;
    seen.push(buffer.version().get());
    assert_eq!(seen, [1, 2, 3, 4, 5, 6]);
    Ok(())
}

#[test]
fn cursor_and_selection_commands_keep_the_version() -> Result<(), Rejected> {
    let mut buffer = line("1+2")?;
    let version = buffer.version();
    buffer.move_left();
    buffer.move_right();
    buffer.move_to_start();
    buffer.move_to_end();
    buffer.extend_left();
    buffer.extend_right();
    buffer.select_all();
    let _snapshot = buffer.snapshot();
    assert_eq!(buffer.version(), version);
    assert_eq!(text(buffer.items()), "1+2");
    Ok(())
}

#[test]
fn rejected_edits_keep_the_version() -> Result<(), Rejected> {
    let mut buffer = line("1+2")?;
    let snapshot = buffer.snapshot();
    let outside = Fragment::new(snapshot.root(), 0..9).unwrap();
    assert_same_version(&mut buffer, |b| {
        assert_eq!(b.insert_char('='), Err(Rejected::Character('=')));
        assert_eq!(b.insert_text("x\t"), Err(Rejected::Character('\t')));
        assert_eq!(
            b.insert_fragment(&snapshot, outside),
            Err(Rejected::NotInSnapshot)
        );
    });
    Ok(())
}

#[test]
fn edits_that_change_no_content_keep_the_version() -> Result<(), Rejected> {
    let mut buffer = line("1+2")?;
    assert_same_version(&mut buffer, |b| {
        assert_eq!(b.delete_forward(), Ok(()));
        assert_eq!(b.insert_text(""), Ok(()));
        b.move_to_start();
        assert_eq!(b.delete_backward(), Ok(()));
    });
    // Replacing a selection with what it holds changes no content.
    put_cursor(&mut buffer, 1);
    buffer.extend_right();
    assert_same_version(&mut buffer, |b| assert_eq!(b.insert_char('+'), Ok(())));
    assert_eq!(text(buffer.items()), "1+2");
    let mut empty = EditBuffer::new();
    assert_same_version(&mut empty, |b| assert_eq!(b.clear(), Ok(())));
    Ok(())
}

/// What a buffer holds after one step of a list of edits.
#[derive(Debug, PartialEq, Eq)]
struct Step {
    /// The items, then the outcome of the step.
    content: String,
    root: u64,
    cursor: usize,
    selection: Option<std::ops::Range<usize>>,
    version: u64,
}

/// One step of a list of edits.
type Edit = dyn Fn(&mut EditBuffer) -> Result<(), Rejected>;

/// The state after each step of a fixed list of edits on a new buffer.
fn run_edits() -> Result<Vec<Step>, Rejected> {
    let mut buffer = EditBuffer::new();
    let mut record = Vec::new();
    let steps: [&Edit; 9] = [
        &|b| b.insert_text("x+1"),
        &|b| {
            b.select_all();
            Ok(())
        },
        &|b| {
            let snapshot = b.snapshot();
            b.move_to_start();
            b.insert_text("2*")?;
            b.move_to_end();
            b.insert_char('^')?;
            b.insert_fragment(&snapshot, snapshot.line())
        },
        &|b| b.insert_char(','),
        &EditBuffer::delete_backward,
        &|b| {
            b.extend_left();
            b.extend_left();
            Ok(())
        },
        &EditBuffer::delete_forward,
        &EditBuffer::clear,
        &|b| b.insert_text("(1"),
    ];
    for step in steps {
        let outcome = step(&mut buffer);
        record.push(Step {
            content: format!("{} {outcome:?}", text(buffer.items())),
            root: buffer.root().get(),
            cursor: buffer.cursor().index,
            selection: buffer
                .selection()
                .map(|selection| selection.fragment().range()),
            version: buffer.version().get(),
        });
    }
    Ok(record)
}

#[test]
fn the_same_edits_twice_give_the_same_structure_identities_and_versions() -> Result<(), Rejected> {
    let first = run_edits()?;
    let second = run_edits()?;
    assert_eq!(first, second);
    let versions: Vec<u64> = first.iter().map(|step| step.version).collect();
    assert_eq!(versions, [1, 1, 4, 4, 5, 5, 6, 7, 8]);
    assert!(first.iter().all(|step| step.root == 0));
    assert_eq!(first[2].content, "2*x+1^(x+1) Ok(())");
    assert_eq!(first[3].content, "2*x+1^(x+1) Err(Character(','))");
    Ok(())
}
