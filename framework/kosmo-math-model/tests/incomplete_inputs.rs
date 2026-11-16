//! Incomplete input stays representable: each line is built through editing
//! operations, and its items, holes and completeness are checked.

mod support;

use kosmo_math_model::HoleKind::{EmptyLine, MissingOperand, MissingOperator, MissingRightBracket};
use kosmo_math_model::{Completeness, EditBuffer, HoleKind, Rejected};
use support::{holes, put_cursor, text};

/// Types each character of `keys` at the cursor.
fn type_keys(buffer: &mut EditBuffer, keys: &str) -> Result<(), Rejected> {
    keys.chars().try_for_each(|c| buffer.insert_char(c))
}

/// Checks the root items and the holes of an incomplete line; every hole is
/// in the root block.
fn assert_incomplete(buffer: &EditBuffer, items: &str, expected: &[(HoleKind, usize)]) {
    assert_eq!(text(buffer.items()), items);
    let completeness = buffer.completeness();
    assert!(
        matches!(&completeness, Completeness::Incomplete(found)
            if found.iter().all(|hole| hole.position.block == buffer.root())),
        "{completeness:?}"
    );
    assert_eq!(holes(&completeness).as_deref(), Some(expected));
}

#[test]
fn one_over_a_hole() -> Result<(), Rejected> {
    let mut buffer = EditBuffer::new();
    type_keys(&mut buffer, "1/")?;
    assert_incomplete(&buffer, "1/", &[(MissingOperand, 2)]);
    Ok(())
}

#[test]
fn a_hole_plus_a_hole() -> Result<(), Rejected> {
    let mut buffer = EditBuffer::new();
    type_keys(&mut buffer, "+")?;
    assert_incomplete(&buffer, "+", &[(MissingOperand, 0), (MissingOperand, 1)]);
    Ok(())
}

#[test]
fn x_to_the_power_of_a_hole() -> Result<(), Rejected> {
    let mut buffer = EditBuffer::new();
    type_keys(&mut buffer, "x^")?;
    assert_incomplete(&buffer, "x^", &[(MissingOperand, 2)]);
    Ok(())
}

#[test]
fn a_left_bracket_before_a_hole() -> Result<(), Rejected> {
    let mut buffer = EditBuffer::new();
    type_keys(&mut buffer, "(")?;
    assert_incomplete(
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
    assert_incomplete(&buffer, "(1+2", &[(MissingRightBracket, 4)]);
    Ok(())
}

#[test]
fn an_empty_line_after_deleting_its_only_item() -> Result<(), Rejected> {
    let mut buffer = EditBuffer::new();
    type_keys(&mut buffer, "1")?;
    buffer.delete_backward()?;
    assert_incomplete(&buffer, "", &[(EmptyLine, 0)]);
    Ok(())
}

#[test]
fn a_number_before_a_name_misses_an_operator() -> Result<(), Rejected> {
    let mut buffer = EditBuffer::new();
    type_keys(&mut buffer, "x")?;
    buffer.move_to_start();
    type_keys(&mut buffer, "2")?;
    assert_incomplete(&buffer, "2x", &[(MissingOperator, 1)]);
    Ok(())
}

#[test]
fn a_number_before_a_bracket_misses_an_operator() -> Result<(), Rejected> {
    let mut buffer = EditBuffer::new();
    type_keys(&mut buffer, "2*(x+1)")?;
    put_cursor(&mut buffer, 2);
    buffer.delete_backward()?;
    assert_incomplete(&buffer, "2(x+1)", &[(MissingOperator, 1)]);
    Ok(())
}

#[test]
fn a_unary_minus_before_a_hole() -> Result<(), Rejected> {
    let mut buffer = EditBuffer::new();
    type_keys(&mut buffer, "-")?;
    assert_incomplete(&buffer, "-", &[(MissingOperand, 1)]);
    Ok(())
}

#[test]
fn one_plus_a_unary_minus_before_a_hole() -> Result<(), Rejected> {
    let mut buffer = EditBuffer::new();
    type_keys(&mut buffer, "1+-")?;
    assert_incomplete(&buffer, "1+-", &[(MissingOperand, 3)]);
    Ok(())
}

#[test]
fn a_left_bracket_typed_before_a_closed_pair() -> Result<(), Rejected> {
    let mut buffer = EditBuffer::new();
    type_keys(&mut buffer, "(1)")?;
    buffer.move_to_start();
    type_keys(&mut buffer, "(")?;
    assert_incomplete(&buffer, "((1)", &[(MissingRightBracket, 4)]);
    Ok(())
}
