//! Complete inputs: each line is built through editing operations, must be
//! complete, converts to the expected text byte for byte, and that text
//! reads in kalyx 0.1.0 as the same term as a fully bracketed writing of
//! the intended reading. Where a misreading is listed, its term differs.
//! Calls and reserved names give no term: kalyx reports them as
//! unsupported, with the span of the call's name and left bracket or of
//! the reserved name.

mod support;

use std::ops::Range;

use kalyx::{ErrorKind, FaultKind, Limits, parse};
use kosmo_math_kalyx::convert;
use kosmo_math_model::{Completeness, EditBuffer};
use support::{Checked, context, equal_terms, line, text};

/// A complete input: the line as typed, the text that conversion writes,
/// the intended reading and a misreading that it must not have.
struct Input {
    line: &'static str,
    text: &'static str,
    expected: &'static str,
    rules_out: Option<&'static str>,
}

const fn input(
    line: &'static str,
    text: &'static str,
    expected: &'static str,
    rules_out: Option<&'static str>,
) -> Input {
    Input {
        line,
        text,
        expected,
        rules_out,
    }
}

/// Checks that `buffer` holds the complete line of `input`, that it
/// converts to the text of `input`, and that kalyx 0.1.0 reads the text as
/// the term of the expected reading, and the misreading as another term.
fn check(buffer: &EditBuffer, input: &Input) -> Checked {
    let line = input.line;
    assert_eq!(text(buffer.items()), line);
    let snapshot = buffer.snapshot();
    assert_eq!(snapshot.completeness(), Completeness::Complete, "{line}");
    assert_eq!(convert(&snapshot)?, input.text, "{line}");
    assert!(
        equal_terms(input.text, input.expected, Limits::new())?,
        "{line}: {} and {} differ",
        input.text,
        input.expected
    );
    if let Some(misreading) = input.rules_out {
        assert!(
            !equal_terms(input.expected, misreading, Limits::new())?,
            "{line}: {misreading} reads as {}",
            input.expected
        );
    }
    Ok(())
}

/// Checks each input typed as one insertion.
fn check_typed(inputs: &[Input]) -> Checked {
    for input in inputs {
        check(&line(input.line)?, input)?;
    }
    Ok(())
}

#[test]
fn numbers_are_written_as_kalyx_reads_them() -> Checked {
    check_typed(&[
        // Integers and numbers with a point stay; spaces stay as typed.
        input("1.5 +  12", "1.5 +  12", "27/2", None),
        // `.5` gains a `0` and `1.` loses its point.
        input(".5+1.", "0.5+1", "3/2", Some("5+1")),
    ])
}

#[test]
fn names_are_split_as_text() -> Checked {
    check_typed(&[
        // Adjacent letters form one name.
        input("xy-x*y", "xy-x*y", "xy-(x*y)", Some("x*y-x*y")),
        // Digits and `_` continue a name, which may start with `_`.
        input("x2+x_1+_a", "x2+x_1+_a", "x2+x_1+_a", Some("x*2+x_1+_a")),
        // Names are case-sensitive.
        input("x-X", "x-X", "x-X", Some("0")),
        // Names that resemble reserved names, `e` and command words are
        // ordinary names.
        input(
            "Pi*ii*pi2*e*diff",
            "Pi*ii*pi2*e*diff",
            "Pi*ii*pi2*e*diff",
            None,
        ),
    ])
}

#[test]
fn reserved_names_and_calls_are_passed_on_unchanged() -> Checked {
    // The line, and the span of the unsupported writing in its text.
    let cases: [(&str, Range<usize>); 4] = [
        // A reserved name followed by a bracket is reported as the name.
        ("pi(x)", 0..2),
        ("2*i", 2..3),
        ("-undefined", 1..10),
        // A call is reported from its name to its left bracket.
        ("f(x)", 0..2),
    ];
    for (typed, span) in cases {
        let buffer = line(typed)?;
        let snapshot = buffer.snapshot();
        assert_eq!(snapshot.completeness(), Completeness::Complete, "{typed}");
        let converted = convert(&snapshot)?;
        assert_eq!(converted, typed);
        let mut context = context(Limits::new());
        let error = parse(&converted, &mut context)
            .err()
            .ok_or(format!("{typed} has a term"))?;
        assert_eq!(
            error.kind(),
            ErrorKind::Fault(FaultKind::Unsupported),
            "{typed}"
        );
        let found = error.span().map(|span| span.start()..span.end());
        assert_eq!(found, Some(span), "{typed}");
    }
    Ok(())
}

#[test]
fn precedence_and_associativity_follow_the_grammar() -> Checked {
    check_typed(&[
        // `*` before `+`.
        input("1+2*3", "1+2*3", "1+(2*3)", Some("(1+2)*3")),
        // `+` and `-` share a level and associate to the left.
        input("5-2-1+3", "5-2-1+3", "((5-2)-1)+3", Some("5-(2-(1+3))")),
        // `*` and `/` share a level and associate to the left.
        input("8/4/2*3", "8/4/2*3", "((8/4)/2)*3", Some("8/(4/(2*3))")),
        // `/` before a binary `-`.
        input("8-4/2", "8-4/2", "8-(4/2)", Some("(8-4)/2")),
        // `^` before `*`.
        input("2*3^2", "2*3^2", "2*(3^2)", Some("(2*3)^2")),
        // `^` associates to the right.
        input("2^3^2", "2^3^2", "2^(3^2)", Some("(2^3)^2")),
        // `^` before a unary minus.
        input("-2^2", "-2^2", "-(2^2)", Some("(-2)^2")),
        // A unary minus before `+`.
        input("-2+3", "-2+3", "(-2)+3", Some("-(2+3)")),
        // A unary minus after a binary operator.
        input("2*-3+4", "2*-3+4", "(2*(-3))+4", Some("2*(-(3+4))")),
        // Unary minus signs after a binary minus.
        input("1---2", "1---2", "1-(-(-2))", Some("1-(-2)")),
        // A unary minus after `^`, whose operand is itself a power.
        input("2^-1^2", "2^-1^2", "2^(-(1^2))", Some("(2^(-1))^2")),
        // Brackets group and are written as typed.
        input(
            "(-2)^2*(1+2)",
            "(-2)^2*(1+2)",
            "((-2)^2)*(1+2)",
            Some("-2^2*1+2"),
        ),
    ])
}

#[test]
fn a_unary_minus_and_a_tower_of_powers_in_one_line() -> Checked {
    check_typed(&[input(
        "-2^2 + 2^3^2",
        "-2^2 + 2^3^2",
        "(-(2^2))+(2^(3^2))",
        Some("((-2)^2)+((2^3)^2)"),
    )])?;
    assert!(equal_terms("-2^2 + 2^3^2", "508", Limits::new())?);
    Ok(())
}

#[test]
fn brackets_added_by_fragment_insertion_are_written() -> Checked {
    let copied = line("x+1")?.snapshot();
    let mut buffer = line("2*")?;
    buffer.insert_fragment(&copied, copied.line())?;
    check(
        &buffer,
        &input("2*(x+1)", "2*(x+1)", "2*(x+1)", Some("2*x+1")),
    )
}
