//! Incomplete inputs produce no text: each line is built through editing
//! operations, and conversion returns exactly its holes instead, so there
//! is nothing to submit.

mod support;

use kosmo_math_kalyx::{NotConverted, convert};
use kosmo_math_model::HoleKind::{EmptyLine, MissingOperand, MissingOperator, MissingRightBracket};
use kosmo_math_model::{EditBuffer, HoleKind, Rejected};
use support::text;

/// Types each character of `keys` at the cursor.
fn type_keys(buffer: &mut EditBuffer, keys: &str) -> Result<(), Rejected> {
    keys.chars().try_for_each(|c| buffer.insert_char(c))
}

/// Puts the cursor of `buffer` at `index`, without a selection.
fn put_cursor(buffer: &mut EditBuffer, index: usize) {
    buffer.move_to_start();
    for _ in 0..index {
        buffer.move_right();
    }
}

/// Checks that `buffer` holds `items` and that conversion gives no text but
/// the holes `expected`, as (kind, index) pairs in the root block.
fn assert_not_converted(buffer: &EditBuffer, items: &str, expected: &[(HoleKind, usize)]) {
    assert_eq!(text(buffer.items()), items);
    let snapshot = buffer.snapshot();
    let converted = convert(&snapshot);
    let found: Option<Vec<(HoleKind, usize)>> = match &converted {
        Err(NotConverted::Incomplete(holes)) => holes
            .iter()
            .map(|hole| {
                (hole.position.block == snapshot.root()).then_some((hole.kind, hole.position.index))
            })
            .collect(),
        _ => None,
    };
    assert_eq!(found.as_deref(), Some(expected), "{items:?}: {converted:?}");
}

#[test]
fn one_over_a_hole() -> Result<(), Rejected> {
    let mut buffer = EditBuffer::new();
    type_keys(&mut buffer, "1/")?;
    assert_not_converted(&buffer, "1/", &[(MissingOperand, 2)]);
    Ok(())
}

#[test]
fn a_hole_plus_a_hole() -> Result<(), Rejected> {
    let mut buffer = EditBuffer::new();
    type_keys(&mut buffer, "+")?;
    assert_not_converted(&buffer, "+", &[(MissingOperand, 0), (MissingOperand, 1)]);
    Ok(())
}

#[test]
fn x_to_the_power_of_a_hole() -> Result<(), Rejected> {
    let mut buffer = EditBuffer::new();
    type_keys(&mut buffer, "x^")?;
    assert_not_converted(&buffer, "x^", &[(MissingOperand, 2)]);
    Ok(())
}

#[test]
fn a_left_bracket_before_a_hole() -> Result<(), Rejected> {
    let mut buffer = EditBuffer::new();
    type_keys(&mut buffer, "(")?;
    assert_not_converted(
        &buffer,
        "(",
        &[(MissingOperand, 1), (MissingRightBracket, 1)],
    );
    Ok(())
}

#[test]
fn an_unclosed_bracket_after_deleting_the_right_one() -> Result<(), Rejected> {
    let mut buffer = EditBuffer::new();
    type_keys(&mut buffer, "(1+2)")?;
    buffer.delete_backward()?;
    assert_not_converted(&buffer, "(1+2", &[(MissingRightBracket, 4)]);
    Ok(())
}

#[test]
fn an_empty_line_after_deleting_its_only_item() -> Result<(), Rejected> {
    let mut buffer = EditBuffer::new();
    type_keys(&mut buffer, "1")?;
    buffer.delete_backward()?;
    assert_not_converted(&buffer, "", &[(EmptyLine, 0)]);
    Ok(())
}

#[test]
fn a_number_before_a_name_misses_an_operator() -> Result<(), Rejected> {
    let mut buffer = EditBuffer::new();
    type_keys(&mut buffer, "x")?;
    buffer.move_to_start();
    type_keys(&mut buffer, "2")?;
    assert_not_converted(&buffer, "2x", &[(MissingOperator, 1)]);
    Ok(())
}

#[test]
fn a_number_before_a_bracket_misses_an_operator() -> Result<(), Rejected> {
    let mut buffer = EditBuffer::new();
    type_keys(&mut buffer, "2*(x+1)")?;
    put_cursor(&mut buffer, 2);
    buffer.delete_backward()?;
    assert_not_converted(&buffer, "2(x+1)", &[(MissingOperator, 1)]);
    Ok(())
}

#[test]
fn a_unary_minus_before_a_hole() -> Result<(), Rejected> {
    let mut buffer = EditBuffer::new();
    type_keys(&mut buffer, "-")?;
    assert_not_converted(&buffer, "-", &[(MissingOperand, 1)]);
    Ok(())
}

#[test]
fn one_plus_a_unary_minus_before_a_hole() -> Result<(), Rejected> {
    let mut buffer = EditBuffer::new();
    type_keys(&mut buffer, "1+-")?;
    assert_not_converted(&buffer, "1+-", &[(MissingOperand, 3)]);
    Ok(())
}

#[test]
fn a_left_bracket_typed_before_a_closed_pair() -> Result<(), Rejected> {
    let mut buffer = EditBuffer::new();
    type_keys(&mut buffer, "(1)")?;
    buffer.move_to_start();
    type_keys(&mut buffer, "(")?;
    assert_not_converted(&buffer, "((1)", &[(MissingRightBracket, 4)]);
    Ok(())
}
