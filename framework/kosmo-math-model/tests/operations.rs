//! The editing operations, each with a case where it acts, a case where it
//! is rejected or changes nothing, and a case at the start or end of the
//! line.

mod support;

use kosmo_math_model::{Completeness, EditBuffer, Fragment, Position, Rejected};
use support::{line, put_cursor, text};

/// The cursor index and the selected item range of a buffer, with its root
/// checked.
fn cursor_and_selection(buffer: &EditBuffer) -> (usize, Option<std::ops::Range<usize>>) {
    let cursor = buffer.cursor();
    assert_eq!(cursor.block, buffer.root());
    let selection = buffer.selection().map(|selection| {
        assert_eq!(selection.active(), cursor);
        selection.fragment().range()
    });
    (cursor.index, selection)
}

/// Checks that `edit` leaves `buffer` as it was.
fn assert_unchanged(buffer: &mut EditBuffer, edit: impl FnOnce(&mut EditBuffer)) {
    let before = buffer.clone();
    edit(buffer);
    assert_eq!(*buffer, before);
}

// Inserting a character.

#[test]
fn insert_char_adds_a_digit_after_plus() -> Result<(), Rejected> {
    let mut buffer = line("1+")?;
    buffer.insert_char('2')?;
    assert_eq!(text(buffer.items()), "1+2");
    assert_eq!(cursor_and_selection(&buffer), (3, None));
    Ok(())
}

#[test]
fn insert_char_rejects_a_comma() -> Result<(), Rejected> {
    let mut buffer = line("1")?;
    assert_unchanged(&mut buffer, |b| {
        assert_eq!(b.insert_char(','), Err(Rejected::Character(',')));
    });
    Ok(())
}

#[test]
fn insert_char_at_line_start_adds_a_unary_minus() -> Result<(), Rejected> {
    let mut buffer = line("x")?;
    buffer.move_to_start();
    buffer.insert_char('-')?;
    assert_eq!(text(buffer.items()), "-x");
    assert_eq!(cursor_and_selection(&buffer), (1, None));
    Ok(())
}

#[test]
fn insert_char_replaces_the_selection() -> Result<(), Rejected> {
    let mut buffer = line("1+2")?;
    buffer.extend_left();
    buffer.extend_left();
    buffer.insert_char('*')?;
    assert_eq!(text(buffer.items()), "1*");
    assert_eq!(cursor_and_selection(&buffer), (2, None));
    Ok(())
}

// Inserting text.

#[test]
fn insert_text_into_an_empty_line() -> Result<(), Rejected> {
    let mut buffer = EditBuffer::new();
    buffer.insert_text("x+1")?;
    assert_eq!(text(buffer.items()), "x+1");
    assert_eq!(cursor_and_selection(&buffer), (3, None));
    Ok(())
}

#[test]
fn insert_text_rejects_text_with_a_comma_as_a_whole() -> Result<(), Rejected> {
    let mut buffer = line("x")?;
    assert_unchanged(&mut buffer, |b| {
        assert_eq!(b.insert_text("1,2"), Err(Rejected::Character(',')));
    });
    Ok(())
}

#[test]
fn insert_text_at_line_end_completes_a_product() -> Result<(), Rejected> {
    let mut buffer = line("2*")?;
    buffer.insert_text("3")?;
    assert_eq!(text(buffer.items()), "2*3");
    assert_eq!(cursor_and_selection(&buffer), (3, None));
    Ok(())
}

// Deleting backward.

#[test]
fn delete_backward_removes_the_item_before_the_cursor() -> Result<(), Rejected> {
    let mut buffer = line("12")?;
    put_cursor(&mut buffer, 1);
    buffer.delete_backward()?;
    assert_eq!(text(buffer.items()), "2");
    assert_eq!(cursor_and_selection(&buffer), (0, None));
    Ok(())
}

#[test]
fn delete_backward_at_line_start_changes_nothing() -> Result<(), Rejected> {
    let mut buffer = line("12")?;
    buffer.move_to_start();
    assert_unchanged(&mut buffer, |b| assert_eq!(b.delete_backward(), Ok(())));
    Ok(())
}

#[test]
fn delete_backward_at_line_end() -> Result<(), Rejected> {
    let mut buffer = line("12")?;
    buffer.delete_backward()?;
    assert_eq!(text(buffer.items()), "1");
    assert_eq!(cursor_and_selection(&buffer), (1, None));
    Ok(())
}

#[test]
fn delete_backward_removes_the_selection() -> Result<(), Rejected> {
    let mut buffer = line("1+2")?;
    put_cursor(&mut buffer, 1);
    buffer.extend_right();
    buffer.delete_backward()?;
    assert_eq!(text(buffer.items()), "12");
    assert_eq!(cursor_and_selection(&buffer), (1, None));
    Ok(())
}

// Deleting forward.

#[test]
fn delete_forward_removes_the_item_after_the_cursor() -> Result<(), Rejected> {
    let mut buffer = line("12")?;
    put_cursor(&mut buffer, 1);
    buffer.delete_forward()?;
    assert_eq!(text(buffer.items()), "1");
    assert_eq!(cursor_and_selection(&buffer), (1, None));
    Ok(())
}

#[test]
fn delete_forward_at_line_end_changes_nothing() -> Result<(), Rejected> {
    let mut buffer = line("12")?;
    assert_unchanged(&mut buffer, |b| assert_eq!(b.delete_forward(), Ok(())));
    Ok(())
}

#[test]
fn delete_forward_at_line_start() -> Result<(), Rejected> {
    let mut buffer = line("12")?;
    buffer.move_to_start();
    buffer.delete_forward()?;
    assert_eq!(text(buffer.items()), "2");
    assert_eq!(cursor_and_selection(&buffer), (0, None));
    Ok(())
}

#[test]
fn delete_forward_removes_the_selection() -> Result<(), Rejected> {
    let mut buffer = line("1+2")?;
    buffer.extend_left();
    buffer.extend_left();
    buffer.delete_forward()?;
    assert_eq!(text(buffer.items()), "1");
    assert_eq!(cursor_and_selection(&buffer), (1, None));
    Ok(())
}

// Moving left.

#[test]
fn move_left_steps_over_one_item() -> Result<(), Rejected> {
    let mut buffer = line("12")?;
    buffer.move_left();
    assert_eq!(cursor_and_selection(&buffer), (1, None));
    Ok(())
}

#[test]
fn move_left_stops_at_line_start() -> Result<(), Rejected> {
    let mut buffer = line("12")?;
    buffer.move_to_start();
    assert_unchanged(&mut buffer, EditBuffer::move_left);
    Ok(())
}

#[test]
fn move_left_from_line_end() -> Result<(), Rejected> {
    let mut buffer = line("1+2")?;
    buffer.move_left();
    assert_eq!(cursor_and_selection(&buffer), (2, None));
    Ok(())
}

#[test]
fn move_left_goes_to_the_start_of_the_selection() -> Result<(), Rejected> {
    let mut buffer = line("1+2")?;
    put_cursor(&mut buffer, 1);
    buffer.extend_right();
    buffer.extend_right();
    buffer.move_left();
    assert_eq!(cursor_and_selection(&buffer), (1, None));
    Ok(())
}

// Moving right.

#[test]
fn move_right_steps_over_one_item() -> Result<(), Rejected> {
    let mut buffer = line("12")?;
    put_cursor(&mut buffer, 1);
    buffer.move_right();
    assert_eq!(cursor_and_selection(&buffer), (2, None));
    Ok(())
}

#[test]
fn move_right_stops_at_line_end() -> Result<(), Rejected> {
    let mut buffer = line("12")?;
    assert_unchanged(&mut buffer, EditBuffer::move_right);
    Ok(())
}

#[test]
fn move_right_from_line_start() -> Result<(), Rejected> {
    let mut buffer = line("12")?;
    buffer.move_to_start();
    buffer.move_right();
    assert_eq!(cursor_and_selection(&buffer), (1, None));
    Ok(())
}

#[test]
fn move_right_goes_to_the_end_of_the_selection() -> Result<(), Rejected> {
    let mut buffer = line("1+2")?;
    put_cursor(&mut buffer, 2);
    buffer.extend_left();
    buffer.extend_left();
    buffer.move_right();
    assert_eq!(cursor_and_selection(&buffer), (2, None));
    Ok(())
}

// Moving to the start of the line.

#[test]
fn move_to_start_from_the_middle() -> Result<(), Rejected> {
    let mut buffer = line("1+2")?;
    put_cursor(&mut buffer, 2);
    buffer.extend_left();
    buffer.move_to_start();
    assert_eq!(cursor_and_selection(&buffer), (0, None));
    Ok(())
}

#[test]
fn move_to_start_at_line_start_changes_nothing() -> Result<(), Rejected> {
    let mut buffer = line("1+2")?;
    buffer.move_to_start();
    assert_unchanged(&mut buffer, EditBuffer::move_to_start);
    Ok(())
}

#[test]
fn move_to_start_from_line_end() -> Result<(), Rejected> {
    let mut buffer = line("1+2")?;
    buffer.move_to_start();
    assert_eq!(cursor_and_selection(&buffer), (0, None));
    Ok(())
}

// Moving to the end of the line.

#[test]
fn move_to_end_from_the_middle() -> Result<(), Rejected> {
    let mut buffer = line("1+2")?;
    put_cursor(&mut buffer, 1);
    buffer.extend_right();
    buffer.move_to_end();
    assert_eq!(cursor_and_selection(&buffer), (3, None));
    Ok(())
}

#[test]
fn move_to_end_at_line_end_changes_nothing() -> Result<(), Rejected> {
    let mut buffer = line("1+2")?;
    assert_unchanged(&mut buffer, EditBuffer::move_to_end);
    Ok(())
}

#[test]
fn move_to_end_from_line_start() -> Result<(), Rejected> {
    let mut buffer = line("1+2")?;
    buffer.move_to_start();
    buffer.move_to_end();
    assert_eq!(cursor_and_selection(&buffer), (3, None));
    Ok(())
}

// Extending the selection to the left.

#[test]
fn extend_left_selects_the_item_before_the_cursor() -> Result<(), Rejected> {
    let mut buffer = line("12")?;
    buffer.extend_left();
    assert_eq!(cursor_and_selection(&buffer), (1, Some(1..2)));
    let selection = buffer.selection().map(|selection| selection.anchor());
    let anchor = Position {
        block: buffer.root(),
        index: 2,
    };
    assert_eq!(selection, Some(anchor));
    Ok(())
}

#[test]
fn extend_left_at_line_start_selects_nothing() -> Result<(), Rejected> {
    let mut buffer = line("12")?;
    buffer.move_to_start();
    assert_unchanged(&mut buffer, EditBuffer::extend_left);
    assert_eq!(buffer.selection(), None);
    Ok(())
}

#[test]
fn extend_left_from_line_end_to_line_start() -> Result<(), Rejected> {
    let mut buffer = line("1")?;
    buffer.extend_left();
    assert_eq!(cursor_and_selection(&buffer), (0, Some(0..1)));
    assert_unchanged(&mut buffer, EditBuffer::extend_left);
    Ok(())
}

#[test]
fn extending_back_to_the_anchor_ends_the_selection() -> Result<(), Rejected> {
    let mut buffer = line("12")?;
    buffer.extend_left();
    buffer.extend_right();
    assert_eq!(cursor_and_selection(&buffer), (2, None));
    Ok(())
}

// Extending the selection to the right.

#[test]
fn extend_right_selects_the_item_after_the_cursor() -> Result<(), Rejected> {
    let mut buffer = line("12")?;
    buffer.move_to_start();
    buffer.extend_right();
    assert_eq!(cursor_and_selection(&buffer), (1, Some(0..1)));
    Ok(())
}

#[test]
fn extend_right_at_line_end_selects_nothing() -> Result<(), Rejected> {
    let mut buffer = line("12")?;
    assert_unchanged(&mut buffer, EditBuffer::extend_right);
    assert_eq!(buffer.selection(), None);
    Ok(())
}

#[test]
fn extend_right_from_line_start_to_line_end() -> Result<(), Rejected> {
    let mut buffer = line("1")?;
    buffer.move_to_start();
    buffer.extend_right();
    assert_eq!(cursor_and_selection(&buffer), (1, Some(0..1)));
    assert_unchanged(&mut buffer, EditBuffer::extend_right);
    Ok(())
}

// Selecting the whole line.

#[test]
fn select_all_selects_the_whole_line() -> Result<(), Rejected> {
    let mut buffer = line("1+2")?;
    put_cursor(&mut buffer, 1);
    buffer.select_all();
    assert_eq!(cursor_and_selection(&buffer), (3, Some(0..3)));
    let anchor = buffer.selection().map(|selection| selection.anchor().index);
    assert_eq!(anchor, Some(0));
    Ok(())
}

#[test]
fn select_all_on_an_empty_line_selects_nothing() {
    let mut buffer = EditBuffer::new();
    assert_unchanged(&mut buffer, EditBuffer::select_all);
    assert_eq!(buffer.selection(), None);
}

#[test]
fn select_all_with_the_cursor_at_line_end() -> Result<(), Rejected> {
    let mut buffer = line("1+2")?;
    buffer.select_all();
    assert_eq!(cursor_and_selection(&buffer), (3, Some(0..3)));
    Ok(())
}

// Clearing the line.

#[test]
fn clear_empties_the_line() -> Result<(), Rejected> {
    let mut buffer = line("1+2")?;
    let root = buffer.root();
    buffer.select_all();
    buffer.clear()?;
    assert_eq!(text(buffer.items()), "");
    assert_eq!(cursor_and_selection(&buffer), (0, None));
    assert_eq!(buffer.root(), root);
    Ok(())
}

#[test]
fn clear_on_an_empty_line_changes_nothing() {
    let mut buffer = EditBuffer::new();
    assert_unchanged(&mut buffer, |b| assert_eq!(b.clear(), Ok(())));
}

#[test]
fn clear_with_the_cursor_at_line_start() -> Result<(), Rejected> {
    let mut buffer = line("1+2")?;
    buffer.move_to_start();
    buffer.clear()?;
    assert_eq!(text(buffer.items()), "");
    assert_eq!(cursor_and_selection(&buffer), (0, None));
    Ok(())
}

// Inserting a fragment of a snapshot.

#[test]
fn insert_fragment_brackets_a_sum_after_times() -> Result<(), Rejected> {
    let snapshot = line("x+1")?.snapshot();
    let mut buffer = line("2*")?;
    buffer.insert_fragment(&snapshot, snapshot.line())?;
    assert_eq!(text(buffer.items()), "2*(x+1)");
    assert_eq!(cursor_and_selection(&buffer), (7, None));
    Ok(())
}

#[test]
fn insert_fragment_brackets_a_fragment_whose_brackets_do_not_pair() -> Result<(), Rejected> {
    // The added brackets pair with the copy's own left bracket.
    let snapshot = line("(1")?.snapshot();
    let mut buffer = line("2*")?;
    buffer.insert_fragment(&snapshot, snapshot.line())?;
    assert_eq!(text(buffer.items()), "2*((1)");
    assert!(matches!(buffer.completeness(), Completeness::Incomplete(_)));
    // Here they pair with the line's left bracket, and the line is complete
    // with a grouping that typing the same characters would not give.
    let snapshot = line("1)+(1")?.snapshot();
    let mut buffer = line("(2*)")?;
    put_cursor(&mut buffer, 3);
    buffer.insert_fragment(&snapshot, snapshot.line())?;
    assert_eq!(text(buffer.items()), "(2*(1)+(1))");
    assert_eq!(buffer.completeness(), Completeness::Complete);
    Ok(())
}

#[test]
fn insert_fragment_past_the_item_limit_is_rejected() -> Result<(), Rejected> {
    let snapshot = line("x+1")?.snapshot();
    let mut buffer = line(&"1".repeat(kosmo_math_model::MAX_ITEMS - 2))?;
    assert_unchanged(&mut buffer, |b| {
        assert_eq!(
            b.insert_fragment(&snapshot, snapshot.line()),
            Err(Rejected::TooManyItems)
        );
    });
    Ok(())
}

#[test]
fn insert_fragment_at_line_start_brackets_a_sum() -> Result<(), Rejected> {
    let snapshot = line("x+1")?.snapshot();
    let mut buffer = line("*2")?;
    buffer.move_to_start();
    buffer.insert_fragment(&snapshot, snapshot.line())?;
    assert_eq!(text(buffer.items()), "(x+1)*2");
    assert_eq!(cursor_and_selection(&buffer), (5, None));
    Ok(())
}

#[test]
fn insert_fragment_keeps_a_single_operand_as_it_is() -> Result<(), Rejected> {
    let snapshot = line("f(x)+12")?.snapshot();
    let mut buffer = line("2*")?;
    let number = Fragment::new(snapshot.root(), 5..7).unwrap();
    let call = Fragment::new(snapshot.root(), 0..4).unwrap();
    for fragment in [number, call] {
        buffer.insert_fragment(&snapshot, fragment)?;
        buffer.insert_char('*')?;
    }
    assert_eq!(text(buffer.items()), "2*12*f(x)*");
    Ok(())
}

#[test]
fn insert_fragment_needs_no_brackets_where_no_operand_is_missing() -> Result<(), Rejected> {
    let snapshot = line("x+1")?.snapshot();
    let mut buffer = line("2*")?;
    buffer.insert_fragment(&snapshot, snapshot.line())?;
    buffer.insert_fragment(&snapshot, snapshot.line())?;
    assert_eq!(text(buffer.items()), "2*(x+1)x+1");
    let mut empty = EditBuffer::new();
    empty.insert_fragment(&snapshot, snapshot.line())?;
    assert_eq!(text(empty.items()), "x+1");
    Ok(())
}

#[test]
fn insert_fragment_replaces_the_selection_and_brackets_where_it_was() -> Result<(), Rejected> {
    let mut buffer = line("2*y")?;
    buffer.extend_left();
    let snapshot = line("x+1")?.snapshot();
    buffer.insert_fragment(&snapshot, snapshot.line())?;
    assert_eq!(text(buffer.items()), "2*(x+1)");
    Ok(())
}

#[test]
fn insert_fragment_copies_part_of_the_line_itself() -> Result<(), Rejected> {
    let mut buffer = line("a-b")?;
    buffer.select_all();
    let snapshot = buffer.snapshot();
    let selected = buffer.selection().unwrap().fragment();
    buffer.move_to_end();
    buffer.insert_char('^')?;
    buffer.insert_fragment(&snapshot, selected)?;
    assert_eq!(text(buffer.items()), "a-b^(a-b)");
    Ok(())
}

#[test]
fn insert_fragment_outside_the_snapshot_is_rejected() -> Result<(), Rejected> {
    let snapshot = line("x+1")?.snapshot();
    let mut buffer = line("2*")?;
    let past_the_end = Fragment::new(snapshot.root(), 2..4).unwrap();
    assert_unchanged(&mut buffer, |b| {
        assert_eq!(
            b.insert_fragment(&snapshot, past_the_end),
            Err(Rejected::NotInSnapshot)
        );
    });
    Ok(())
}
