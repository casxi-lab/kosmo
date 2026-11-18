use kosmo_math_model::display::Freshness;
use kosmo_math_model::version::{ContextVersion, SettingsVersion};
use kosmo_math_model::{EditBuffer, Fragment, Place};

use super::*;

fn snapshot(text: &str) -> Snapshot {
    let mut buffer = EditBuffer::new();
    buffer.insert_text(text).unwrap();
    buffer.snapshot()
}

/// The versions (1, 2, 3) of a request for the line `1+2`, typed as one
/// insertion, and that line.
fn request() -> (Versions, Snapshot) {
    let snapshot = snapshot("1+2");
    let versions = Versions {
        source: snapshot.version(),
        context: ContextVersion::new(2),
        settings: SettingsVersion::new(3),
    };
    (versions, snapshot)
}

/// What the line `line` shows for the request of [`request`].
fn read(line: &str) -> Content {
    read_bytes(line.as_bytes())
}

fn read_bytes(line: &[u8]) -> Content {
    let (versions, snapshot) = request();
    let item = read_output_line(line, versions, &snapshot);
    assert_eq!(item.versions, versions);
    item.content
}

fn unreadable(line: &str) -> Unreadable {
    match read(line) {
        Content::Unreadable(reason) => reason,
        other => panic!("{line:?} reads as {other:?}"),
    }
}

#[test]
fn a_value_line_gives_its_display_text() {
    assert_eq!(
        read("value\t1\t2\t3\t1 + 5*x + 10*x^2\n"),
        Content::Value("1 + 5*x + 10*x^2".to_string())
    );
}

#[test]
fn a_state_line_gives_the_name_of_any_state() {
    let names = [
        "NotConverged",
        "Unevaluated",
        "NotApplicable",
        "Undecided",
        "IterationLimit",
        "Diverged",
    ];
    for name in names {
        assert_eq!(
            read(&format!("state\t1\t2\t3\t{name}\n")),
            Content::State(name.to_string())
        );
    }
}

#[test]
fn the_other_four_states_and_a_later_one_render_by_name() {
    let (versions, snapshot) = request();
    for name in [
        "NotConverged",
        "Unevaluated",
        "NotApplicable",
        "Undecided",
        "Diverged",
    ] {
        let line = format!("state\t1\t2\t3\t{name}\n");
        let item = read_output_line(line.as_bytes(), versions, &snapshot);
        assert_eq!(
            item.render(Freshness::Current),
            format!("state (source 1, settings 3): {name}")
        );
    }
}

#[test]
fn an_error_line_gives_its_kind_and_place() {
    let (_, line) = request();
    let fragment = Fragment::new(line.root(), 0..1).unwrap();
    assert_eq!(
        read("error\t1\t2\t3\tDivisionByZero\t0..1\t-\t-\t-\t-\tdivision by zero\n"),
        Content::Error {
            kind: ErrorKind::DivisionByZero,
            place: Place::Fragment(fragment),
        }
    );
    assert_eq!(
        read("error\t1\t2\t3\tUnknownSymbol\t-\t-\t-\t-\t-\tx\n"),
        Content::Error {
            kind: ErrorKind::UnknownSymbol,
            place: Place::Line,
        }
    );
}

#[test]
fn a_resource_limit_gives_its_limit_scope_allowed_value_and_amount() {
    let cases = [
        (
            "Field(max_storage_bytes)\tTemporary\t18446744073709551615\tUsed(0)",
            LimitId::Field("max_storage_bytes".to_string()),
            Some("Temporary"),
            u64::MAX,
            LimitAmount::Used(0),
        ),
        (
            "Parameter(a\\tb\\nc\\\\d\\re\\x01\\x7f\u{e9})\t-\t0\tRequired(1)",
            LimitId::Parameter("a\\tb\\nc\\\\d\\re\\x01\\x7f\u{e9}".to_string()),
            None,
            0,
            LimitAmount::Required(1),
        ),
        (
            "Field(max_exponent)\t-\t4096\tBound(18446744073709551615)",
            LimitId::Field("max_exponent".to_string()),
            None,
            4096,
            LimitAmount::Bound(u64::MAX),
        ),
    ];
    for (fields, limit, scope, allowed, amount) in cases {
        let line = format!("error\t1\t2\t3\tResourceLimit\t-\t{fields}\tmessage\n");
        assert_eq!(
            read(&line),
            Content::Error {
                kind: ErrorKind::ResourceLimit(LimitExceeded {
                    limit,
                    scope: scope.map(str::to_string),
                    allowed,
                    amount,
                }),
                place: Place::Line,
            },
            "{line:?}"
        );
    }
}

#[test]
fn a_later_kind_is_kept_by_name_without_reading_its_limit_fields() {
    for fields in [
        "-\t-\t-\t-",
        "Field(max_x)\tValue\t1\tUsed(2)",
        "?\t?\t?\t?",
    ] {
        let line = format!("error\t1\t2\t3\tOutsideDomain\t-\t{fields}\tfault\n");
        assert_eq!(
            read(&line),
            Content::Error {
                kind: ErrorKind::Other("OutsideDomain".to_string()),
                place: Place::Line,
            }
        );
    }
}

#[test]
fn an_unknown_kind_of_line_is_unreadable() {
    for line in [
        "warning\t1\t2\t3\tx\n",
        "\n",
        "Value\t1\t2\t3\t1\n",
        "value 1 2 3 1\n",
    ] {
        assert_eq!(unreadable(line), Unreadable::UnknownKind, "{line:?}");
    }
}

#[test]
fn a_line_that_is_not_utf8_is_unreadable() {
    assert_eq!(
        read_bytes(b"value\t1\t2\t3\t\xff\n"),
        Content::Unreadable(Unreadable::NotUtf8)
    );
}

#[test]
fn a_line_without_its_line_feed_is_unreadable() {
    for line in ["value\t1\t2\t3\t1", "value\t1\t2\t3\t1\r", ""] {
        assert_eq!(unreadable(line), Unreadable::Unterminated, "{line:?}");
    }
}

#[test]
fn a_control_character_makes_a_line_unreadable() {
    for line in [
        "value\t1\t2\t3\t1\r\n",
        "value\t1\t2\t3\t1\n\n",
        "error\t1\t2\t3\tSyntax\t-\t-\t-\t-\t-\ta\u{1b}b\n",
        "state\t1\t2\t3\tA\u{7f}\n",
    ] {
        assert_eq!(unreadable(line), Unreadable::ControlCharacter, "{line:?}");
    }
}

#[test]
fn each_kind_of_line_has_its_number_of_fields() {
    let cases = [
        ("value\t1\t2\t3\n", LineKind::Value, 4, 5),
        ("value\t1\t2\t3\t1\t2\n", LineKind::Value, 6, 5),
        ("state\n", LineKind::State, 1, 5),
        (
            "error\t1\t2\t3\tSyntax\t-\t-\t-\t-\t-\n",
            LineKind::Error,
            10,
            11,
        ),
    ];
    for (line, kind, found, expected) in cases {
        assert_eq!(
            unreadable(line),
            Unreadable::FieldCount {
                kind,
                found,
                expected
            },
            "{line:?}"
        );
    }
}

#[test]
fn malformed_numbers_and_spans_are_unreadable() {
    let lines = [
        "value\t01\t2\t3\t1\n",
        "value\t+1\t2\t3\t1\n",
        "value\t1\t\t3\t1\n",
        "value\t1\t2\t18446744073709551616\t1\n",
        "error\t1\t2\t3\tSyntax\t1..\t-\t-\t-\t-\tm\n",
        "error\t1\t2\t3\tSyntax\t1\t-\t-\t-\t-\tm\n",
        "error\t1\t2\t3\tSyntax\t-1..2\t-\t-\t-\t-\tm\n",
        "error\t1\t2\t3\tSyntax\t0...2\t-\t-\t-\t-\tm\n",
        "error\t1\t2\t3\tResourceLimit\t-\tField(max_work)\t-\t07\tUsed(8)\tm\n",
        "error\t1\t2\t3\tResourceLimit\t-\tField(max_work)\t-\t7\tUsed(x)\tm\n",
    ];
    for line in lines {
        assert_eq!(unreadable(line), Unreadable::Malformed, "{line:?}");
    }
}

#[test]
fn malformed_fields_of_errors_values_and_states_are_unreadable() {
    let lines = [
        "value\t1\t2\t3\t\n",
        "value\t1\t2\t3\t\u{e9}\n",
        "state\t1\t2\t3\tNot Converged\n",
        "state\t1\t2\t3\t\n",
        "error\t1\t2\t3\t\t-\t-\t-\t-\t-\tm\n",
        "error\t1\t2\t3\tDivision-By-Zero\t-\t-\t-\t-\t-\tm\n",
        "error\t1\t2\t3\tSyntax\t-\tField(max_work)\t-\t-\t-\tm\n",
        "error\t1\t2\t3\tCancelled\t-\t-\t-\t1\t-\tm\n",
        "error\t1\t2\t3\tResourceLimit\t-\t-\t-\t-\t-\tm\n",
        "error\t1\t2\t3\tResourceLimit\t-\tField(Max)\t-\t1\tUsed(2)\tm\n",
        "error\t1\t2\t3\tResourceLimit\t-\tField()\t-\t1\tUsed(2)\tm\n",
        "error\t1\t2\t3\tResourceLimit\t-\tLimit(max_work)\t-\t1\tUsed(2)\tm\n",
        "error\t1\t2\t3\tResourceLimit\t-\tParameter(a\\qb)\t-\t1\tUsed(2)\tm\n",
        "error\t1\t2\t3\tResourceLimit\t-\tParameter(a\\x1)\t-\t1\tUsed(2)\tm\n",
        "error\t1\t2\t3\tResourceLimit\t-\tParameter(a\\x1B)\t-\t1\tUsed(2)\tm\n",
        "error\t1\t2\t3\tResourceLimit\t-\tField(max_nodes)\tIn Value\t1\tUsed(2)\tm\n",
        "error\t1\t2\t3\tResourceLimit\t-\tField(max_nodes)\t-\t1\tSpent(2)\tm\n",
    ];
    for line in lines {
        assert_eq!(unreadable(line), Unreadable::Malformed, "{line:?}");
    }
}

#[test]
fn a_span_outside_the_text_is_unreadable() {
    // Past the end of the text `1+2`, and backwards.
    for (start, end) in [(2, 4), (4, 4), (2, 1)] {
        let line = format!("error\t1\t2\t3\tSyntax\t{start}..{end}\t-\t-\t-\t-\tm\n");
        assert_eq!(
            unreadable(&line),
            Unreadable::SpanOutside {
                span: start..end,
                len: 3
            },
            "{line:?}"
        );
    }
}

#[test]
fn other_echoed_versions_are_unreadable() {
    for versions in ["0\t2\t3", "1\t0\t3", "1\t2\t0", "3\t2\t1"] {
        for rest in [
            "value\t{v}\t1/2",
            "state\t{v}\tUndecided",
            "error\t{v}\tInternal\t-\t-\t-\t-\t-\tm",
        ] {
            let line = format!("{}\n", rest.replace("{v}", versions));
            assert_eq!(unreadable(&line), Unreadable::OtherVersions, "{line:?}");
        }
    }
}

#[test]
fn an_unreadable_line_keeps_the_versions_of_the_request() {
    let (versions, snapshot) = request();
    let item = read_output_line(b"value\t9\t9\t9\t1\n", versions, &snapshot);
    assert_eq!(item.versions, versions);
    assert_eq!(
        item.render(Freshness::Current),
        "internal error (source 1, settings 3): the output line echoes other versions than the request"
    );
}

#[test]
fn no_cut_or_changed_line_panics() {
    let (versions, snapshot) = request();
    let lines: [&[u8]; 3] = [
        b"error\t1\t2\t3\tResourceLimit\t0..3\tParameter(a\\x7f)\tValue\t7\tRequired(8)\tm\n",
        b"value\t1\t2\t3\t-2/3 + t^2\n",
        b"state\t1\t2\t3\tIterationLimit\n",
    ];
    let replacements = [
        b'\t', b'\n', b'\\', b'(', b')', b'.', b'0', b'9', b'-', 0x80, 0xff,
    ];
    for line in lines {
        for end in 0..=line.len() {
            let _ = read_output_line(&line[..end], versions, &snapshot);
        }
        for at in 0..line.len() {
            for byte in replacements {
                let mut changed = line.to_vec();
                changed[at] = byte;
                let _ = read_output_line(&changed, versions, &snapshot);
            }
        }
    }
}
