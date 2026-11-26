use super::*;

fn line(text: &str) -> EditBuffer {
    let mut buffer = EditBuffer::new();
    buffer.insert_text(text).unwrap();
    buffer
}

fn text(buffer: &EditBuffer) -> String {
    buffer
        .items()
        .iter()
        .map(crate::structure::character)
        .collect()
}

/// Applies `edit` to a copy of `buffer`, expects it to be rejected with
/// `reason` and the copy to be unchanged.
fn assert_rejected(
    buffer: &EditBuffer,
    reason: Rejected,
    edit: impl FnOnce(&mut EditBuffer) -> Result<(), Rejected>,
) {
    let mut edited = buffer.clone();
    assert_eq!(edit(&mut edited), Err(reason));
    assert_eq!(&edited, buffer);
}

#[test]
fn the_character_set_has_72_characters() {
    let ascii: Vec<char> = (0u8..=127).map(char::from).collect();
    let set: String = ascii.iter().filter(|&&c| in_character_set(c)).collect();
    assert_eq!(set.len(), 72);
    assert_eq!(
        set,
        " ()*+-./0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ^_abcdefghijklmnopqrstuvwxyz"
    );
    for outside in ['\t', '\n', ',', '=', '□', 'λ', '\u{a0}'] {
        assert!(!in_character_set(outside), "{outside:?}");
    }
}

#[test]
fn text_of_the_byte_limit_is_accepted_and_longer_text_is_rejected() {
    let mut buffer = EditBuffer::new();
    let longest = "1".repeat(MAX_INSERT_BYTES);
    assert_rejected(&buffer, Rejected::InsertTooLong, |b| {
        b.insert_text(&format!("{longest}1"))
    });
    // The byte limit comes before the character set.
    assert_rejected(&buffer, Rejected::InsertTooLong, |b| {
        b.insert_text(&",".repeat(MAX_INSERT_BYTES + 1))
    });
    buffer.insert_text(&longest).unwrap();
    assert_eq!(buffer.items().len(), MAX_INSERT_BYTES);
}

#[test]
fn text_that_would_overfill_the_line_is_rejected_whole() {
    let buffer = line(&"1".repeat(MAX_ITEMS - 2));
    assert_rejected(&buffer, Rejected::TooManyItems, |b| b.insert_text("+x+"));
    let mut buffer = buffer;
    buffer.insert_text("+x").unwrap();
    assert_eq!(buffer.items().len(), MAX_ITEMS);
}

#[test]
fn replacing_a_selection_counts_the_items_that_remain() {
    let mut buffer = line(&"1".repeat(MAX_ITEMS));
    buffer.extend_left();
    buffer.extend_left();
    buffer.insert_text("+x").unwrap();
    assert_eq!(buffer.items().len(), MAX_ITEMS);
    assert!(text(&buffer).ends_with("1+x"));
}

#[test]
fn a_fragment_of_the_byte_limit_is_accepted() {
    let snapshot = line(&"1".repeat(MAX_INSERT_BYTES)).snapshot();
    let mut buffer = EditBuffer::new();
    buffer.insert_fragment(&snapshot, snapshot.line()).unwrap();
    assert_eq!(buffer.items().len(), MAX_INSERT_BYTES);
}

#[test]
fn a_fragment_longer_than_the_byte_limit_is_rejected() {
    // A snapshot never holds more than the item limit, so this one is built
    // directly.
    let mut structure = Structure::new();
    structure.set_root_items(vec![Item::Char('1'); MAX_INSERT_BYTES + 1]);
    let snapshot = Snapshot {
        structure,
        version: SourceVersion::default(),
    };
    assert_rejected(&EditBuffer::new(), Rejected::InsertTooLong, |b| {
        b.insert_fragment(&snapshot, snapshot.line())
    });
}

#[test]
fn added_brackets_count_toward_the_item_limit() {
    let fragment_source = line("x+1").snapshot();
    let buffer = line(&format!("{}*", "1".repeat(MAX_ITEMS - 5)));
    assert_rejected(&buffer, Rejected::TooManyItems, |b| {
        b.insert_fragment(&fragment_source, fragment_source.line())
    });
    let mut buffer = line(&format!("{}*", "1".repeat(MAX_ITEMS - 6)));
    buffer
        .insert_fragment(&fragment_source, fragment_source.line())
        .unwrap();
    assert_eq!(buffer.items().len(), MAX_ITEMS);
    assert!(text(&buffer).ends_with("*(x+1)"));
}

#[test]
fn brackets_nest_up_to_the_depth_limit() {
    let mut buffer = line(&"(".repeat(MAX_PARSE_DEPTH));
    assert_rejected(&buffer, Rejected::TooDeep, |b| b.insert_char('('));
    assert_rejected(&buffer, Rejected::TooDeep, |b| b.insert_char('-'));
    buffer.insert_char('1').unwrap();
    assert_eq!(buffer.items().len(), MAX_PARSE_DEPTH + 1);
}

#[test]
fn unary_minus_signs_nest_up_to_the_depth_limit() {
    let buffer = line(&"-".repeat(MAX_PARSE_DEPTH));
    assert_rejected(&buffer, Rejected::TooDeep, |b| b.insert_char('-'));
    assert_rejected(&buffer, Rejected::TooDeep, |b| b.insert_text("(1)"));
    // A binary minus after an operand ends the chain.
    let mut buffer = line(&format!("1{}", "-".repeat(MAX_PARSE_DEPTH + 1)));
    buffer.insert_char('1').unwrap();
}

#[test]
fn exponent_towers_and_long_sums_are_within_the_depth_limit() {
    let tower = format!("2{}", "^2".repeat((MAX_ITEMS - 1) / 2));
    assert_eq!(line(&tower).completeness(), Completeness::Complete);
    let sum = format!("1{}", "+1".repeat((MAX_ITEMS - 1) / 2));
    assert_eq!(line(&sum).completeness(), Completeness::Complete);
}

#[test]
fn a_deletion_that_would_deepen_the_line_past_the_limit_is_rejected() {
    // A binary minus and 64 unary ones; deleting the `1` turns the binary
    // minus into a unary one, which makes a chain of 65.
    let mut buffer = line(&format!("1{}", "-".repeat(MAX_PARSE_DEPTH + 1)));
    buffer.move_to_start();
    assert_rejected(&buffer, Rejected::TooDeep, EditBuffer::delete_forward);
    buffer.move_right();
    assert_rejected(&buffer, Rejected::TooDeep, EditBuffer::delete_backward);
    buffer.select_all();
    buffer.delete_backward().unwrap();
    assert!(buffer.items().is_empty());
}

#[test]
fn an_edit_past_the_maximum_version_is_rejected() {
    let mut buffer = line("1+");
    buffer.version = SourceVersion::at(u64::MAX - 1);
    buffer.insert_char('2').unwrap();
    assert_eq!(buffer.version().get(), u64::MAX);
    for edit in [
        (|b: &mut EditBuffer| b.insert_char('3')) as fn(&mut EditBuffer) -> Result<(), Rejected>,
        |b| b.insert_text("3"),
        EditBuffer::delete_backward,
        EditBuffer::clear,
        |b| {
            let snapshot = b.snapshot();
            b.insert_fragment(&snapshot, snapshot.line())
        },
    ] {
        assert_rejected(&buffer, Rejected::VersionExhausted, edit);
    }
    // Edits that change no content are not rejected.
    buffer.delete_forward().unwrap();
    buffer.insert_text("").unwrap();
    buffer.move_left();
    buffer.select_all();
    assert_eq!(buffer.version().get(), u64::MAX);
    assert_eq!(text(&buffer), "1+2");
}

#[test]
fn clearing_keeps_the_root_and_the_identity_counter() {
    let mut buffer = line("x+1");
    let before = (buffer.root(), buffer.structure.next_identity());
    buffer.clear().unwrap();
    assert!(buffer.items().is_empty());
    assert_eq!((buffer.root(), buffer.structure.next_identity()), before);
}

#[test]
fn rejections_have_messages() {
    assert_eq!(
        Rejected::Character(',').to_string(),
        "the character ',' cannot be typed"
    );
    assert_eq!(
        Rejected::TooManyItems.to_string(),
        "a line holds at most 4096 items"
    );
}
