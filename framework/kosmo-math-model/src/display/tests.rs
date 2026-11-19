use crate::structure::{Fragment, NodeId, Position};
use crate::version::{ContextVersion, SettingsVersion, SourceVersion};

use super::*;

fn versions(source: u64, context: u64, settings: u64) -> Versions {
    Versions {
        source: SourceVersion::at(source),
        context: ContextVersion::new(context),
        settings: SettingsVersion::new(settings),
    }
}

fn item(content: Content) -> DisplayItem {
    DisplayItem {
        versions: versions(1, 2, 3),
        content,
    }
}

fn error(kind: ErrorKind, place: Place) -> DisplayItem {
    item(Content::Error { kind, place })
}

fn limit(limit: LimitId, scope: Option<&str>, allowed: u64, amount: LimitAmount) -> ErrorKind {
    ErrorKind::ResourceLimit(LimitExceeded {
        limit,
        scope: scope.map(str::to_string),
        allowed,
        amount,
    })
}

fn items(range: Range<usize>) -> Place {
    Place::Fragment(Fragment::new(NodeId::ROOT, range).unwrap())
}

#[test]
fn each_content_has_its_head() {
    let rendered = [
        item(Content::Value("-2/3 + t^2".to_string())),
        item(Content::State("NotConverged".to_string())),
        error(ErrorKind::Syntax, Place::Line),
        item(Content::Unreadable(Unreadable::UnknownKind)),
    ]
    .map(|item| item.render(Freshness::Current));
    assert_eq!(
        rendered,
        [
            "value (source 1, settings 3): -2/3 + t^2",
            "state (source 1, settings 3): NotConverged",
            "error (source 1, settings 3): Syntax on the whole line",
            "internal error (source 1, settings 3): the output line is not a value, state or error line",
        ]
    );
}

#[test]
fn the_context_version_is_not_written() {
    let content = Content::Value("1/2".to_string());
    let one = DisplayItem {
        versions: versions(4, 0, 5),
        content: content.clone(),
    };
    let other = DisplayItem {
        versions: versions(4, 9, 5),
        content,
    };
    assert_ne!(one, other);
    assert_eq!(
        one.render(Freshness::Current),
        other.render(Freshness::Current)
    );
    assert_eq!(
        one.render(Freshness::Current),
        "value (source 4, settings 5): 1/2"
    );
}

#[test]
fn a_stale_result_is_marked_after_the_versions() {
    let value = item(Content::Value("0".to_string()));
    assert_eq!(
        value.render(Freshness::Stale),
        "value (source 1, settings 3, stale): 0"
    );
    let state = item(Content::State("Undecided".to_string()));
    assert_eq!(
        state.render(Freshness::Stale),
        "state (source 1, settings 3, stale): Undecided"
    );
}

#[test]
fn rendering_twice_gives_the_same_text() {
    let value = item(Content::Value("1 + 2*x".to_string()));
    for freshness in [Freshness::Current, Freshness::Stale] {
        assert_eq!(value.render(freshness), value.render(freshness));
    }
}

#[test]
fn errors_name_their_kind_and_place() {
    let position = Place::Position(Position {
        block: NodeId::ROOT,
        index: 2,
    });
    let cases = [
        (ErrorKind::Cancelled, items(0..1), "Cancelled at items 0..1"),
        (ErrorKind::Syntax, position, "Syntax at position 2"),
        (
            ErrorKind::NotPolynomial,
            Place::Line,
            "NotPolynomial on the whole line",
        ),
        (
            ErrorKind::Other("OutsideDomain".to_string()),
            Place::Line,
            "OutsideDomain on the whole line",
        ),
    ];
    for (kind, place, text) in cases {
        assert_eq!(
            error(kind, place).render(Freshness::Current),
            format!("error (source 1, settings 3): {text}")
        );
    }
}

#[test]
fn every_kind_has_the_name_of_kalyx() {
    let kinds = [
        limit(
            LimitId::Field("max_work".to_string()),
            None,
            1,
            LimitAmount::Used(2),
        ),
        ErrorKind::Cancelled,
        ErrorKind::DeadlineExceeded,
        ErrorKind::Syntax,
        ErrorKind::UnknownSymbol,
        ErrorKind::DivisionByZero,
        ErrorKind::NonIntegerExponent,
        ErrorKind::NotPolynomial,
        ErrorKind::Unsupported,
        ErrorKind::Internal,
        ErrorKind::Other("Later".to_string()),
    ];
    let names: Vec<&str> = kinds.iter().map(ErrorKind::name).collect();
    assert_eq!(
        names,
        [
            "ResourceLimit",
            "Cancelled",
            "DeadlineExceeded",
            "Syntax",
            "UnknownSymbol",
            "DivisionByZero",
            "NonIntegerExponent",
            "NotPolynomial",
            "Unsupported",
            "Internal",
            "Later",
        ]
    );
}

#[test]
fn a_resource_limit_states_its_limit_scope_allowed_value_and_amount() {
    let field = |name: &str| LimitId::Field(name.to_string());
    let cases = [
        (
            limit(
                field("max_exponent"),
                None,
                4096,
                LimitAmount::Required(5000),
            ),
            "(max_exponent, allowed 4096, required 5000)",
        ),
        (
            limit(
                field("max_storage_bytes"),
                Some("Temporary"),
                u64::MAX,
                LimitAmount::Used(0),
            ),
            "(max_storage_bytes in Temporary, allowed 18446744073709551615, used 0)",
        ),
        (
            limit(field("max_exponent"), None, 4096, LimitAmount::Bound(7)),
            "(max_exponent, allowed 4096, required at most 7)",
        ),
        (
            limit(
                LimitId::Parameter("a\\tb".to_string()),
                None,
                3,
                LimitAmount::Required(4),
            ),
            "(parameter a\\tb, allowed 3, required 4)",
        ),
    ];
    for (kind, text) in cases {
        assert_eq!(
            error(kind, Place::Line).render(Freshness::Current),
            format!("error (source 1, settings 3): ResourceLimit {text} on the whole line")
        );
    }
}

#[test]
fn an_unreadable_line_says_why() {
    let cases = [
        (Unreadable::NotUtf8, "the output line is not UTF-8"),
        (
            Unreadable::Unterminated,
            "the output line does not end with a line feed",
        ),
        (
            Unreadable::ControlCharacter,
            "the output line holds a control character",
        ),
        (
            Unreadable::FieldCount {
                kind: LineKind::Value,
                found: 4,
                expected: 5,
            },
            "the output line has 4 fields; a value line has 5",
        ),
        (
            Unreadable::FieldCount {
                kind: LineKind::Error,
                found: 1,
                expected: 11,
            },
            "the output line has 1 field; an error line has 11",
        ),
        (
            Unreadable::Malformed,
            "a field of the output line is malformed",
        ),
        (
            Unreadable::SpanOutside { span: 2..9, len: 3 },
            "the span 2..9 does not lie in the 3 bytes of the text",
        ),
        (
            Unreadable::OtherVersions,
            "the output line echoes other versions than the request",
        ),
    ];
    for (reason, text) in cases {
        assert_eq!(
            item(Content::Unreadable(reason)).render(Freshness::Stale),
            format!("internal error (source 1, settings 3, stale): {text}")
        );
    }
}

#[test]
fn text_up_to_the_bound_is_shown_whole() {
    let text = "7".repeat(MAX_DISPLAY_BYTES);
    let rendered = item(Content::Value(text.clone())).render(Freshness::Current);
    assert_eq!(rendered, format!("value (source 1, settings 3): {text}"));
}

#[test]
fn longer_text_is_cut_and_states_its_length() {
    let text = "7".repeat(MAX_DISPLAY_BYTES + 1);
    let rendered = item(Content::Value(text)).render(Freshness::Current);
    let expected = format!(
        "value (source 1, settings 3): {} ... (65537 bytes)",
        "7".repeat(MAX_DISPLAY_BYTES)
    );
    assert_eq!(rendered, expected);
}

#[test]
fn text_is_cut_on_a_character_boundary() {
    // A two-byte character across the bound is left out whole, and so is a
    // four-byte character that ends one byte after it.
    for (tail, kept) in [("é", 1), ("\u{1f600}", 3)] {
        let before = MAX_DISPLAY_BYTES - kept;
        let text = format!("{}{tail}{}", "a".repeat(before), "z".repeat(10));
        let len = text.len();
        let rendered = item(Content::State(text)).render(Freshness::Current);
        assert_eq!(
            rendered,
            format!(
                "state (source 1, settings 3): {} ... ({len} bytes)",
                "a".repeat(before)
            )
        );
    }
}

#[test]
fn an_error_text_is_bounded_too() {
    let name = "p".repeat(MAX_DISPLAY_BYTES);
    let kind = limit(LimitId::Parameter(name), None, 1, LimitAmount::Required(2));
    let rendered = error(kind, Place::Line).render(Freshness::Current);
    let text = rendered
        .strip_prefix("error (source 1, settings 3): ")
        .unwrap();
    let length = "ResourceLimit (parameter , allowed 1, required 2) on the whole line".len()
        + MAX_DISPLAY_BYTES;
    assert_eq!(
        text,
        format!(
            "ResourceLimit (parameter {} ... ({length} bytes)",
            "p".repeat(MAX_DISPLAY_BYTES - "ResourceLimit (parameter ".len())
        )
    );
}
