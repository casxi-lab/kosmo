//! The tokens of a snapshot: the numbers, names, operators and brackets
//! that the parser reads, each with its items, in reading order.

mod support;

use kosmo_math_model::TokenKind::{LeftBracket, Name, Number, Operator, RightBracket};
use kosmo_math_model::{Rejected, Snapshot, TokenKind};
use support::line;

/// The kind and item range of each token of `snapshot`; every token lies
/// in the root block and holds at least one item.
fn tokens(snapshot: &Snapshot) -> Vec<(TokenKind, std::ops::Range<usize>)> {
    snapshot
        .tokens()
        .iter()
        .map(|token| {
            assert_eq!(token.fragment.block(), snapshot.root());
            assert!(!token.fragment.is_empty());
            (token.kind, token.fragment.range())
        })
        .collect()
}

#[test]
fn a_snapshot_gives_the_tokens_of_its_line() -> Result<(), Rejected> {
    let snapshot = line("-2^x_1*(.5+f(1.))")?.snapshot();
    assert_eq!(
        tokens(&snapshot),
        [
            (Operator, 0..1),
            (Number, 1..2),
            (Operator, 2..3),
            (Name, 3..6),
            (Operator, 6..7),
            (LeftBracket, 7..8),
            (Number, 8..10),
            (Operator, 10..11),
            (Name, 11..12),
            (LeftBracket, 12..13),
            (Number, 13..15),
            (RightBracket, 15..16),
            (RightBracket, 16..17),
        ]
    );
    Ok(())
}

#[test]
fn spaces_belong_to_no_token() -> Result<(), Rejected> {
    let snapshot = line(" 12 +  f (x) ")?.snapshot();
    assert_eq!(
        tokens(&snapshot),
        [
            (Number, 1..3),
            (Operator, 4..5),
            (Name, 7..8),
            (LeftBracket, 9..10),
            (Name, 10..11),
            (RightBracket, 11..12),
        ]
    );
    assert_eq!(tokens(&line("   ")?.snapshot()), []);
    assert_eq!(tokens(&line("")?.snapshot()), []);
    Ok(())
}

#[test]
fn incomplete_and_invalid_lines_have_tokens_too() -> Result<(), Rejected> {
    // A number ends where a letter starts, as in the juxtaposition `2x`.
    assert_eq!(
        tokens(&line("2x")?.snapshot()),
        [(Number, 0..1), (Name, 1..2)]
    );
    // Runs of digits and points that form no number are number tokens.
    assert_eq!(
        tokens(&line("1.2.3+.)")?.snapshot()),
        [
            (Number, 0..5),
            (Operator, 5..6),
            (Number, 6..7),
            (RightBracket, 7..8)
        ]
    );
    Ok(())
}

#[test]
fn the_tokens_of_a_snapshot_stay_with_it() -> Result<(), Rejected> {
    let mut buffer = line("x+1")?;
    let snapshot = buffer.snapshot();
    buffer.insert_text("0")?;
    assert_eq!(
        tokens(&snapshot),
        [(Name, 0..1), (Operator, 1..2), (Number, 2..3)]
    );
    assert_eq!(
        tokens(&buffer.snapshot()),
        [(Name, 0..1), (Operator, 1..2), (Number, 2..4)]
    );
    Ok(())
}
