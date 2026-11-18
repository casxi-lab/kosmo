//! Error spans map back to the line. Each line is built through editing
//! operations and converted; kalyx 0.1.0 evaluates the text as
//! `kalyx eval TEXT` does, its error line is the one listed, and the span
//! of that line maps to the listed items, position or whole line. Lines
//! that are not complete are never converted: for the syntax errors, the
//! line itself is given to kalyx and its span is mapped the same way.

mod support;

use std::ops::Range;

use kosmo_math_kalyx::{NotConverted, Place, convert, map_span};
use kosmo_math_model::{Completeness, EditBuffer, Fragment, HoleKind, InvalidKind, Position};
use support::{Checked, eval_line, line, span_field, text};

/// Where an error is expected to lie in the root block.
#[derive(Debug, Clone)]
enum Expected {
    Line,
    Items(Range<usize>),
    At(usize),
}

/// An error case: the line, the text that conversion writes, or `None` for
/// a line that is not complete, the error line of kalyx 0.1.0 and where
/// the error lies.
struct Case<'a> {
    line: &'a str,
    text: Option<&'a str>,
    error_line: &'a str,
    expected: Expected,
}

/// Checks `case` on the line of `buffer` and returns the place to which the
/// span of the error line maps.
fn check(buffer: &EditBuffer, case: &Case) -> Checked<Place> {
    let line = case.line;
    assert_eq!(text(buffer.items()), line);
    let snapshot = buffer.snapshot();
    let converted = convert(&snapshot);
    let source = match case.text {
        Some(expected) => {
            assert_eq!(snapshot.completeness(), Completeness::Complete, "{line}");
            assert_eq!(converted.as_deref(), Ok(expected), "{line}");
            expected
        }
        None => {
            assert!(converted.is_err(), "{line}: {converted:?}");
            line
        }
    };
    let error_line = eval_line(source)?;
    assert_eq!(error_line, case.error_line, "{line}");
    let place = map_span(&snapshot, span_field(&error_line)?)?;
    let root = snapshot.root();
    let expected = match case.expected.clone() {
        Expected::Line => Place::Line,
        Expected::Items(range) => {
            Place::Fragment(Fragment::new(root, range).ok_or("a forward range")?)
        }
        Expected::At(index) => Place::Position(Position { block: root, index }),
    };
    assert_eq!(place, expected, "{line}");
    Ok(place)
}

/// Checks each case on its line typed as one insertion.
fn check_typed(cases: &[Case]) -> Checked {
    for case in cases {
        check(&line(case.line)?, case)?;
    }
    Ok(())
}

const DIVISION_BY_ZERO: &str = "\t-\t-\t-\t-\tdivision by zero";
const NOT_AN_INTEGER: &str = "\t-\t-\t-\t-\tan exponent is an integer constant";

#[test]
fn a_division_by_zero_maps_to_the_chain_up_to_the_divisor() -> Checked {
    let lines = [
        format!("error\t0\t0\t0\tDivisionByZero\t3..6{DIVISION_BY_ZERO}"),
        format!("error\t0\t0\t0\tDivisionByZero\t0..9{DIVISION_BY_ZERO}"),
        format!("error\t0\t0\t0\tDivisionByZero\t0..4{DIVISION_BY_ZERO}"),
    ];
    check_typed(&[
        // The division inside the brackets, without them.
        Case {
            line: "0*(1/0)",
            text: Some("0*(1/0)"),
            error_line: &lines[0],
            expected: Expected::Items(3..6),
        },
        // The chain from its first operand to the divisor and its brackets,
        // not the divisor alone.
        Case {
            line: "a*b/(x-x)*c",
            text: Some("a*b/(x-x)*c"),
            error_line: &lines[1],
            expected: Expected::Items(0..9),
        },
        // A chain whose first operand is a unary minus starts at the minus.
        Case {
            line: "-1/0",
            text: Some("-1/0"),
            error_line: &lines[2],
            expected: Expected::Items(0..4),
        },
    ])
}

#[test]
fn an_exponent_that_is_not_an_integer_maps_to_the_exponent() -> Checked {
    check_typed(&[Case {
        line: "2^(1/2)",
        text: Some("2^(1/2)"),
        error_line: &format!("error\t0\t0\t0\tNonIntegerExponent\t2..7{NOT_AN_INTEGER}"),
        expected: Expected::Items(2..7),
    }])
}

#[test]
fn reserved_names_and_calls_map_to_their_writing() -> Checked {
    check_typed(&[
        Case {
            line: "1+pi",
            text: Some("1+pi"),
            error_line: "error\t0\t0\t0\tUnsupported\t2..4\t-\t-\t-\t-\tthe name is reserved for a constant of a later release",
            expected: Expected::Items(2..4),
        },
        // The name, the space and the left bracket.
        Case {
            line: "f (x)",
            text: Some("f (x)"),
            error_line: "error\t0\t0\t0\tUnsupported\t0..3\t-\t-\t-\t-\tfunction calls are not supported in this release",
            expected: Expected::Items(0..3),
        },
    ])
}

#[test]
fn spans_over_normalized_numbers_cover_all_their_items() -> Checked {
    let lines = [
        format!("error\t0\t0\t0\tDivisionByZero\t4..7{DIVISION_BY_ZERO}"),
        format!("error\t0\t0\t0\tDivisionByZero\t0..3{DIVISION_BY_ZERO}"),
        format!("error\t0\t0\t0\tNonIntegerExponent\t2..5{NOT_AN_INTEGER}"),
    ];
    check_typed(&[
        // The written `0` shifts the bytes of the text by one.
        Case {
            line: ".5+1./0",
            text: Some("0.5+1/0"),
            error_line: &lines[0],
            expected: Expected::Items(3..7),
        },
        // The span ends before the point that conversion left out, and
        // covers it.
        Case {
            line: "2/0.",
            text: Some("2/0"),
            error_line: &lines[1],
            expected: Expected::Items(0..4),
        },
        // The span starts at the `0` that conversion wrote, and covers the
        // items of `.5`.
        Case {
            line: "x^.5",
            text: Some("x^0.5"),
            error_line: &lines[2],
            expected: Expected::Items(2..4),
        },
    ])
}

#[test]
fn a_tower_past_the_depth_limit_maps_to_the_operator_that_opens_the_level() -> Checked {
    // Exponents add no parse depth in kosmo, so the line is complete; kalyx
    // counts them and stops at the 128th `^`.
    let mut buffer = line("2")?;
    for _ in 0..128 {
        buffer.insert_text("^2")?;
    }
    let tower = format!("2{}", "^2".repeat(128));
    let place = check(
        &buffer,
        &Case {
            line: &tower,
            text: Some(&tower),
            error_line: "error\t0\t0\t0\tResourceLimit\t255..256\tField(max_depth)\t-\t128\tRequired(129)\tresource limit exceeded: max_depth",
            expected: Expected::Items(255..256),
        },
    )?;
    assert_eq!(buffer.items().len(), 257);
    assert_ne!(place, Place::Line);
    Ok(())
}

#[test]
fn an_error_without_a_span_maps_to_the_whole_line() -> Checked {
    check_typed(&[Case {
        line: "x",
        text: Some("x"),
        error_line: "error\t0\t0\t0\tUnknownSymbol\t-\t-\t-\t-\t-\teval needs a number, and the input holds a symbol, which has no value",
        expected: Expected::Line,
    }])
}

#[test]
fn syntax_errors_of_the_line_itself_map_back_to_the_line() -> Checked {
    // An empty span at the end of `1+`: the position of its missing operand.
    let missing_operand = line("1+")?;
    let place = check(
        &missing_operand,
        &Case {
            line: "1+",
            text: None,
            error_line: "error\t0\t0\t0\tSyntax\t2..2\t-\t-\t-\t-\texpected a number, an identifier, `-` or `(`",
            expected: Expected::At(2),
        },
    )?;
    let holes = |buffer: &EditBuffer| match convert(&buffer.snapshot()) {
        Err(NotConverted::Incomplete(holes)) => holes
            .iter()
            .map(|hole| (hole.kind, Place::Position(hole.position)))
            .collect(),
        _ => Vec::new(),
    };
    assert_eq!(holes(&missing_operand), [(HoleKind::MissingOperand, place)]);

    // `2(x+1)`: the left bracket, the first token of the second operand,
    // which starts where the operator is missing but is not that position.
    let missing_operator = line("2(x+1)")?;
    let place = check(
        &missing_operator,
        &Case {
            line: "2(x+1)",
            text: None,
            error_line: "error\t0\t0\t0\tSyntax\t1..2\t-\t-\t-\t-\texpected an operator or the end of the input",
            expected: Expected::Items(1..2),
        },
    )?;
    let at_the_hole = Place::Position(Position {
        block: missing_operator.root(),
        index: 1,
    });
    assert_eq!(
        holes(&missing_operator),
        [(HoleKind::MissingOperator, at_the_hole)]
    );
    assert_ne!(place, at_the_hole);

    // `1)`: the right bracket without a left one, the invalid part.
    let surplus = line("1)")?;
    let place = check(
        &surplus,
        &Case {
            line: "1)",
            text: None,
            error_line: "error\t0\t0\t0\tSyntax\t1..2\t-\t-\t-\t-\ta `)` without a matching `(`",
            expected: Expected::Items(1..2),
        },
    )?;
    match convert(&surplus.snapshot()) {
        Err(NotConverted::Invalid(invalid)) => {
            assert_eq!(invalid.kind, InvalidKind::SurplusRightBracket);
            assert_eq!(place, Place::Fragment(invalid.fragment));
        }
        other => panic!("{other:?}"),
    }
    Ok(())
}
