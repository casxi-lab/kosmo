use kosmo_math_model::{EditBuffer, HoleKind, InvalidKind};

use super::*;

fn snapshot(text: &str) -> Snapshot {
    let mut buffer = EditBuffer::new();
    buffer.insert_text(text).unwrap();
    buffer.snapshot()
}

fn linear(text: &str) -> Linear {
    Linear::of(&snapshot(text))
}

/// The items and bytes of each piece of the linear writing of `text`.
fn pieces(text: &str) -> Vec<(Range<usize>, Range<usize>)> {
    linear(text)
        .pieces
        .into_iter()
        .map(|piece| (piece.items, piece.bytes))
        .collect()
}

#[test]
fn numbers_with_a_point_at_either_end_are_normalized() {
    for (line, text) in [
        (".5", "0.5"),
        ("1.", "1"),
        ("1.5", "1.5"),
        ("12", "12"),
        ("0.25", "0.25"),
        ("-.5^2.", "-0.5^2"),
    ] {
        assert_eq!(convert(&snapshot(line)), Ok(text.to_string()), "{line}");
    }
}

#[test]
fn everything_but_numbers_is_written_as_typed() {
    for line in [
        "  x_1 +  y2 ",
        "f (x)*pi-i^undefined",
        "--2",
        "(1+2)*3",
        "Pi*e*diff",
    ] {
        assert_eq!(convert(&snapshot(line)), Ok(line.to_string()), "{line}");
    }
}

#[test]
fn incomplete_lines_give_their_holes_and_no_text() {
    let line = snapshot("1/");
    let holes = match line.completeness() {
        Completeness::Incomplete(holes) => holes,
        other => panic!("{other:?}"),
    };
    assert_eq!(holes[0].kind, HoleKind::MissingOperand);
    assert_eq!(convert(&line), Err(NotConverted::Incomplete(holes)));
    assert_eq!(
        convert(&snapshot("")).map_err(|error| error.to_string()),
        Err("the line is incomplete: it has a hole at position 0".to_string())
    );
    assert_eq!(
        convert(&snapshot("+")).map_err(|error| error.to_string()),
        Err("the line is incomplete: it has 2 holes".to_string())
    );
}

#[test]
fn invalid_lines_give_their_invalid_part_and_no_text() {
    for (line, kind, range) in [
        ("1.2.3", InvalidKind::MalformedNumber, 0..5),
        ("1)", InvalidKind::SurplusRightBracket, 1..2),
        ("x+.", InvalidKind::MalformedNumber, 2..3),
    ] {
        match convert(&snapshot(line)) {
            Err(NotConverted::Invalid(invalid)) => {
                assert_eq!(
                    (invalid.kind, invalid.fragment.range()),
                    (kind, range),
                    "{line}"
                );
            }
            other => panic!("{line}: {other:?}"),
        }
    }
    assert_eq!(
        convert(&snapshot("1)")).map_err(|error| error.to_string()),
        Err("the line is invalid at items 1..2".to_string())
    );
}

#[test]
fn each_token_and_each_space_is_one_piece() {
    assert_eq!(
        pieces(" .5 +f(1.)"),
        [
            (0..1, 0..1),
            (1..3, 1..4),
            (3..4, 4..5),
            (4..5, 5..6),
            (5..6, 6..7),
            (6..7, 7..8),
            (7..9, 8..9),
            (9..10, 9..10),
        ]
    );
    assert_eq!(linear(" .5 +f(1.)").text, " 0.5 +f(1)");
    assert_eq!(pieces(""), []);
}

#[test]
fn the_pieces_cover_every_item_and_byte_in_order() {
    for line in [
        "",
        "   ",
        ".5+1./0",
        "x^.5",
        "2/0.",
        " (1. +  .25) * f (x) ",
        "1.2.3)",
        "..5.",
        ".",
    ] {
        let linear = linear(line);
        let (mut item, mut byte) = (0, 0);
        for piece in &linear.pieces {
            assert_eq!(
                (piece.items.start, piece.bytes.start),
                (item, byte),
                "{line}"
            );
            assert!(piece.items.start < piece.items.end, "{line}");
            assert!(piece.bytes.start < piece.bytes.end, "{line}");
            (item, byte) = (piece.items.end, piece.bytes.end);
        }
        assert_eq!((item, byte), (line.len(), linear.text.len()), "{line}");
    }
}
