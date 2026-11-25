use super::*;

fn items(text: &str) -> Vec<Item> {
    text.chars().map(Item::Char).collect()
}

fn kinds(text: &str) -> Vec<Kind> {
    tokens(&items(text))
        .iter()
        .map(|token| token.kind)
        .collect()
}

fn completeness(text: &str) -> Completeness {
    analyse(NodeId::ROOT, &items(text)).completeness
}

fn depth(text: &str) -> usize {
    analyse(NodeId::ROOT, &items(text)).depth
}

/// The holes of an incomplete line as (kind, index) pairs.
fn holes(text: &str) -> Vec<(HoleKind, usize)> {
    match completeness(text) {
        Completeness::Incomplete(holes) => holes
            .iter()
            .map(|hole| {
                assert_eq!(hole.position.block, NodeId::ROOT);
                (hole.kind, hole.position.index)
            })
            .collect(),
        other => panic!("{text:?} is not incomplete: {other:?}"),
    }
}

/// The kind and item range of the first invalid part of a line.
fn invalid(text: &str) -> (InvalidKind, std::ops::Range<usize>) {
    match completeness(text) {
        Completeness::Invalid(invalid) => (invalid.kind, invalid.fragment.range()),
        other => panic!("{text:?} is not invalid: {other:?}"),
    }
}

use HoleKind::{EmptyLine, MissingOperand, MissingOperator, MissingRightBracket};

const NUMBER: Kind = Kind::Number { valid: true };
const NOT_A_NUMBER: Kind = Kind::Number { valid: false };

#[test]
fn a_number_ends_where_a_letter_or_underscore_starts() {
    assert_eq!(kinds("2x"), [NUMBER, Kind::Name]);
    assert_eq!(kinds("1_000"), [NUMBER, Kind::Name]);
    assert_eq!(kinds("1.x"), [NUMBER, Kind::Name]);
    assert_eq!(kinds("x1_y"), [Kind::Name]);
    assert_eq!(kinds("x.5"), [Kind::Name, NUMBER]);
    assert_eq!(kinds("_"), [Kind::Name]);
}

#[test]
fn a_number_has_at_most_one_point_and_a_digit() {
    for valid in ["1", "12", "1.5", ".5", "1.", "0.25"] {
        assert_eq!(kinds(valid), [NUMBER], "{valid}");
    }
    for not_a_number in [".", "..", "1.2.3", "1..", "..5"] {
        assert_eq!(kinds(not_a_number), [NOT_A_NUMBER], "{not_a_number}");
    }
}

#[test]
fn spaces_only_separate_tokens() {
    let spaced = tokens(&items(" 1 +  x "));
    let ranges: Vec<_> = spaced.iter().map(|token| token.start..token.end).collect();
    assert_eq!(ranges, [1..2, 3..4, 6..7]);
    assert_eq!(kinds("1 2"), [NUMBER, NUMBER]);
    assert_eq!(kinds("   "), []);
}

#[test]
fn the_holes_of_the_documented_shapes() {
    assert_eq!(holes("1/"), [(MissingOperand, 2)]);
    assert_eq!(holes("+"), [(MissingOperand, 0), (MissingOperand, 1)]);
    assert_eq!(holes("x^"), [(MissingOperand, 2)]);
    assert_eq!(holes("("), [(MissingOperand, 1), (MissingRightBracket, 1)]);
    assert_eq!(holes(""), [(EmptyLine, 0)]);
    assert_eq!(invalid("1)"), (InvalidKind::SurplusRightBracket, 1..2));
}

#[test]
fn a_hole_sits_at_the_token_after_it_or_at_the_line_end() {
    assert_eq!(holes("1 + "), [(MissingOperand, 4)]);
    assert_eq!(holes(" +"), [(MissingOperand, 1), (MissingOperand, 2)]);
    assert_eq!(holes("( )"), [(MissingOperand, 2)]);
    assert_eq!(holes("2 x"), [(MissingOperator, 2)]);
    assert_eq!(holes("( 1 "), [(MissingRightBracket, 4)]);
    assert_eq!(
        holes("((x"),
        [(MissingRightBracket, 3), (MissingRightBracket, 3)]
    );
    assert_eq!(holes("1 ^ "), [(MissingOperand, 4)]);
}

#[test]
fn a_line_of_spaces_is_an_empty_line() {
    assert_eq!(holes("   "), [(EmptyLine, 3)]);
}

#[test]
fn juxtaposition_is_a_missing_operator() {
    assert_eq!(holes("2x"), [(MissingOperator, 1)]);
    assert_eq!(holes("2(x+1)"), [(MissingOperator, 1)]);
    assert_eq!(holes("(x)(y)"), [(MissingOperator, 3)]);
    assert_eq!(holes("1 .5"), [(MissingOperator, 2)]);
    assert_eq!(holes("2^3 4"), [(MissingOperator, 4)]);
}

#[test]
fn calls_and_reserved_names_are_complete_operands() {
    for complete in [
        "f(x)",
        "f (x)",
        "pi",
        "i",
        "undefined",
        "pi(x)",
        "2*sin(x+1)^2",
    ] {
        assert_eq!(completeness(complete), Completeness::Complete, "{complete}");
    }
    assert_eq!(holes("f()"), [(MissingOperand, 2)]);
    assert_eq!(holes("f("), [(MissingOperand, 2), (MissingRightBracket, 2)]);
    assert_eq!(holes("2f(x)"), [(MissingOperator, 1)]);
    assert_eq!(holes("f(x)(y)"), [(MissingOperator, 4)]);
}

#[test]
fn the_forms_of_the_grammar_are_complete() {
    for complete in [
        "1", "x", "1+2*3", "-2^2", "2^-2", "2^3^2", "--2", "1--1", "2*-3+4", "(1+2)*3", " 1 + 2 ",
        ".5", "1.", "1.+.5", "x_1/y",
    ] {
        assert_eq!(completeness(complete), Completeness::Complete, "{complete}");
    }
}

#[test]
fn unary_plus_and_repeated_operators_miss_an_operand() {
    assert_eq!(holes("+1"), [(MissingOperand, 0)]);
    assert_eq!(holes("1++2"), [(MissingOperand, 2)]);
    assert_eq!(holes("^2"), [(MissingOperand, 0)]);
    assert_eq!(holes("1*/2"), [(MissingOperand, 2)]);
}

#[test]
fn malformed_numbers_and_surplus_brackets_are_invalid() {
    assert_eq!(invalid("1.2.3"), (InvalidKind::MalformedNumber, 0..5));
    assert_eq!(invalid("."), (InvalidKind::MalformedNumber, 0..1));
    assert_eq!(invalid("x+.."), (InvalidKind::MalformedNumber, 2..4));
    assert_eq!(invalid(")("), (InvalidKind::SurplusRightBracket, 0..1));
    assert_eq!(invalid("(1))"), (InvalidKind::SurplusRightBracket, 3..4));
}

#[test]
fn an_invalid_part_outweighs_holes_and_the_first_one_is_reported() {
    assert_eq!(invalid("1)+"), (InvalidKind::SurplusRightBracket, 1..2));
    assert_eq!(invalid("(1.2.3"), (InvalidKind::MalformedNumber, 1..6));
    assert_eq!(
        invalid("1)+1.2.3"),
        (InvalidKind::SurplusRightBracket, 1..2)
    );
    assert_eq!(invalid("1.2.3)"), (InvalidKind::MalformedNumber, 0..5));
}

#[test]
fn the_parse_depth_counts_brackets_and_unary_minus_signs() {
    assert_eq!(depth(""), 0);
    assert_eq!(depth("1+2*3"), 0);
    assert_eq!(depth("(1)"), 1);
    assert_eq!(depth("((1)"), 2);
    assert_eq!(depth("(1)(2)"), 1);
    assert_eq!(depth("f(x)"), 1);
    assert_eq!(depth("-1"), 1);
    assert_eq!(depth("--1"), 2);
    assert_eq!(depth("-1-1"), 1);
    assert_eq!(depth("1------1"), 5);
    assert_eq!(depth("-(-(1))"), 4);
    assert_eq!(depth("f(-x)"), 2);
    assert_eq!(depth("-2*-3"), 1);
    assert_eq!(depth("-2x-3"), 1);
    // A missing operator, like `*`, ends the unary minus before it.
    assert_eq!(depth("-2(1)"), 1);
    assert_eq!(depth("-2(-3)"), 2);
    // A right bracket ends the unary minus signs inside it.
    assert_eq!(depth("(-1)+(-1)"), 2);
    assert_eq!(depth("(--1+-1+(((1))))"), 4);
}

#[test]
fn exponents_and_operator_chains_add_no_depth() {
    assert_eq!(depth("2^2^2^2^2"), 0);
    assert_eq!(depth("1+1+1+1"), 0);
    // An exponent lies inside the operand of a unary minus before its base.
    assert_eq!(depth("-2^-2^-2"), 3);
    assert_eq!(depth("-(1)^-2"), 2);
}

#[test]
fn surplus_brackets_do_not_lower_the_depth_below_zero() {
    assert_eq!(depth(")))(("), 2);
    assert_eq!(depth("-)-"), 1);
}

#[test]
fn single_operands_need_no_brackets() {
    for single in [
        "x", "12", "1.2.3", "(x+1)", "f(x)", "f()", "f (x+1)", " x ", "((x))", "", "  ",
    ] {
        assert!(!needs_grouping(&items(single)), "{single:?}");
    }
    for compound in [
        "x+1", "-x", "x^2", "(1)+(2)", "((x)", ")", "+", "f x", "2x", "f(x)(y)", "(x)y", "f)",
        "-(x)", "x-(1)", "2(x)",
    ] {
        assert!(needs_grouping(&items(compound)), "{compound:?}");
    }
}

#[test]
fn an_operand_is_needed_between_the_tokens_around_a_missing_one() {
    let needs = |text: &str, index| analyse(NodeId::ROOT, &items(text)).needs_operand_at(index);
    assert!(needs("2*", 2));
    assert!(!needs("2*", 1));
    assert!(needs("2* ", 2) && needs("2* ", 3));
    assert!(needs("*2", 0));
    assert!(!needs("*2", 1));
    assert!(needs("-", 1));
    assert!(needs("x^", 2));
    assert!(needs("( )", 1) && needs("( )", 2));
    assert!(!needs("()", 2));
    assert!(!needs("2*x", 2));
    assert!(!needs("", 0));
    assert!(!needs("2x", 1));
    // The gap starts at the end of the token before it.
    assert!(!needs("1++2", 1));
    assert!(needs("1++2", 2));
}
