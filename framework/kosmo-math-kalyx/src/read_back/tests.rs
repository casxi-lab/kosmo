use kosmo_math_model::{HoleKind, InvalidKind, Item, MAX_INSERT_BYTES, MAX_PARSE_DEPTH};

use super::*;
use crate::convert;

fn text(snapshot: &Snapshot) -> String {
    snapshot
        .items()
        .iter()
        .map(|item| match item {
            Item::Char(c) => *c,
            _ => char::REPLACEMENT_CHARACTER,
        })
        .collect()
}

#[test]
fn display_text_reads_back_into_a_complete_line() {
    // Display forms of kalyx 0.1.0: fractions, a constant first, negative
    // exponents and bases in brackets, and a witness monomial.
    for display in [
        "-3/4 + x - 1/2*y",
        "1/2*x^(-1)*y^(-1)",
        "(x + y)^(-2)",
        "-z*(x + y)",
        "5 + 0*x^(-1)",
        "340282366920938463463374607431768211456",
    ] {
        let snapshot = read_back(display).unwrap();
        assert_eq!(text(&snapshot), display);
        assert_eq!(snapshot.completeness(), Completeness::Complete);
        assert_eq!(snapshot.version().get(), 1, "one insertion");
        assert_eq!(convert(&snapshot), Ok(display.to_string()));
    }
}

#[test]
fn text_longer_than_one_insertion_is_not_reusable() {
    let longest = "1".repeat(MAX_INSERT_BYTES);
    assert!(read_back(&longest).is_ok());
    let longer = "1".repeat(MAX_INSERT_BYTES + 1);
    let reason = read_back(&longer).unwrap_err();
    assert_eq!(reason, NotReusable::Rejected(Rejected::InsertTooLong));
    assert_eq!(
        reason.to_string(),
        "the value cannot be reused: an insertion holds at most 4096 bytes"
    );
}

#[test]
fn text_that_the_editing_structure_rejects_is_not_reusable() {
    // The display grammar reserves `f(a, b)` for calls; no value of kalyx
    // 0.1.0 is a call, and a comma cannot be typed.
    assert_eq!(
        read_back("f(a, b)"),
        Err(NotReusable::Rejected(Rejected::Character(',')))
    );
    let deep = format!(
        "{}1{}",
        "(".repeat(MAX_PARSE_DEPTH + 1),
        ")".repeat(MAX_PARSE_DEPTH + 1)
    );
    assert_eq!(
        read_back(&deep),
        Err(NotReusable::Rejected(Rejected::TooDeep))
    );
}

#[test]
fn text_that_is_not_a_complete_line_is_not_reusable() {
    match read_back("1 +") {
        Err(NotReusable::NotComplete(NotConverted::Incomplete(holes))) => {
            assert_eq!(holes.len(), 1);
            assert_eq!(
                (holes[0].kind, holes[0].position.index),
                (HoleKind::MissingOperand, 3)
            );
        }
        other => panic!("{other:?}"),
    }
    match read_back("1)") {
        Err(NotReusable::NotComplete(NotConverted::Invalid(invalid))) => {
            assert_eq!(invalid.kind, InvalidKind::SurplusRightBracket);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        read_back("").map_err(|reason| reason.to_string()),
        Err(
            "the value cannot be reused: the line is incomplete: it has a hole at position 0"
                .to_string()
        )
    );
}
