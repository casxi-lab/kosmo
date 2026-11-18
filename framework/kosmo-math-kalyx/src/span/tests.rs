use kosmo_math_model::EditBuffer;

use super::*;

fn snapshot(text: &str) -> Snapshot {
    let mut buffer = EditBuffer::new();
    buffer.insert_text(text).unwrap();
    buffer.snapshot()
}

fn items(snapshot: &Snapshot, range: Range<usize>) -> Place {
    Place::Fragment(Fragment::new(snapshot.root(), range).unwrap())
}

fn at(snapshot: &Snapshot, index: usize) -> Place {
    Place::Position(Position {
        block: snapshot.root(),
        index,
    })
}

#[test]
fn an_error_without_a_span_is_about_the_whole_line() {
    let line = snapshot("x");
    assert_eq!(map_span(&line, None), Ok(Place::Line));
    assert_eq!(map_span(&snapshot(""), None), Ok(Place::Line));
}

#[test]
fn a_span_of_a_line_written_as_typed_covers_the_same_items() {
    let line = snapshot("a*b/(x-x)*c");
    assert_eq!(map_span(&line, Some(0..9)), Ok(items(&line, 0..9)));
    assert_eq!(map_span(&line, Some(4..9)), Ok(items(&line, 4..9)));
    assert_eq!(map_span(&line, Some(10..11)), Ok(items(&line, 10..11)));
}

#[test]
fn a_span_over_a_normalized_number_covers_all_its_items() {
    // `.5+1./0` is written `0.5+1/0`.
    let line = snapshot(".5+1./0");
    assert_eq!(map_span(&line, Some(4..7)), Ok(items(&line, 3..7)));
    assert_eq!(map_span(&line, Some(0..3)), Ok(items(&line, 0..2)));
    assert_eq!(map_span(&line, Some(4..5)), Ok(items(&line, 3..5)));
    // `2/0.` is written `2/0`: the span ends before the point it left out.
    let line = snapshot("2/0.");
    assert_eq!(map_span(&line, Some(0..3)), Ok(items(&line, 0..4)));
    // `x^.5` is written `x^0.5`: the span starts at the `0` it added.
    let line = snapshot("x^.5");
    assert_eq!(map_span(&line, Some(2..5)), Ok(items(&line, 2..4)));
}

#[test]
fn a_span_that_starts_or_ends_inside_a_piece_grows_to_its_edges() {
    // `0.5+12` comes from `.5+12`.
    let line = snapshot(".5+12");
    assert_eq!(map_span(&line, Some(1..2)), Ok(items(&line, 0..2)));
    assert_eq!(map_span(&line, Some(2..5)), Ok(items(&line, 0..5)));
    assert_eq!(map_span(&line, Some(4..5)), Ok(items(&line, 3..5)));
    let line = snapshot("abc+d");
    assert_eq!(map_span(&line, Some(1..2)), Ok(items(&line, 0..3)));
    // An empty span inside a piece covers the piece.
    assert_eq!(map_span(&line, Some(1..1)), Ok(items(&line, 0..3)));
}

#[test]
fn an_empty_span_marks_the_position_between_pieces() {
    let line = snapshot("1+");
    assert_eq!(map_span(&line, Some(2..2)), Ok(at(&line, 2)));
    assert_eq!(map_span(&line, Some(0..0)), Ok(at(&line, 0)));
    assert_eq!(map_span(&line, Some(1..1)), Ok(at(&line, 1)));
    // Between the pieces of `.5` and `+`, which are written `0.5+`.
    let line = snapshot(".5+");
    assert_eq!(map_span(&line, Some(3..3)), Ok(at(&line, 2)));
    assert_eq!(map_span(&line, Some(4..4)), Ok(at(&line, 3)));
    assert_eq!(
        map_span(&snapshot(""), Some(0..0)),
        Ok(at(&snapshot(""), 0))
    );
}

#[test]
fn spaces_are_pieces_of_their_own() {
    let line = snapshot("f (x)");
    assert_eq!(map_span(&line, Some(0..3)), Ok(items(&line, 0..3)));
    assert_eq!(map_span(&line, Some(1..2)), Ok(items(&line, 1..2)));
    let line = snapshot(" 1 ");
    assert_eq!(map_span(&line, Some(0..3)), Ok(items(&line, 0..3)));
    assert_eq!(map_span(&line, Some(3..3)), Ok(at(&line, 3)));
}

#[test]
fn a_span_outside_the_text_is_rejected() {
    // `0.5` is 3 bytes long.
    let line = snapshot(".5");
    assert_eq!(map_span(&line, Some(0..3)), Ok(items(&line, 0..2)));
    assert_eq!(
        map_span(&line, Some(0..4)),
        Err(SpanOutside { span: 0..4, len: 3 })
    );
    assert_eq!(
        map_span(&line, Some(4..4)),
        Err(SpanOutside { span: 4..4, len: 3 })
    );
    let (start, end) = (2, 1);
    let backwards = map_span(&line, Some(start..end));
    assert_eq!(
        backwards,
        Err(SpanOutside {
            span: start..end,
            len: 3
        })
    );
    assert_eq!(
        backwards.map_err(|error| error.to_string()),
        Err("the span 2..1 does not lie in the 3 bytes of the text".to_string())
    );
}
