//! Errors: one of each of the ten error kinds of kalyx 0.1.0 and one of a
//! kind that a later version adds. Each error line is the one that kalyx
//! 0.1.0 writes for its request, or the one its own tests assert for a kind
//! that no request of this version can give. Read for its request, it is
//! placed in the submitted line, on the items or at the position that its
//! span covers, or on the whole line, and renders with its kind and place.

mod support;

use std::ops::Range;

use kalyx::command::serialize;
use kalyx::context::{CancelFlag, Clock};
use kalyx::{Context, ErrorKind, Limits, Outcome, Request, execute};
use kosmo_math_kalyx::read_output_line;
use kosmo_math_model::display::{Content, Freshness};
use kosmo_math_model::version::{ContextVersion, SettingsVersion, SourceVersion, Versions};
use kosmo_math_model::{Fragment, Place, Position, Snapshot};
use support::{Checked, context, line, text};

/// A clock that always reads the same time, in nanoseconds.
struct FixedClock(u64);

impl Clock for FixedClock {
    fn now(&mut self) -> u64 {
        self.0
    }

    fn fork(&self) -> Box<dyn Clock> {
        Box::new(FixedClock(self.0))
    }
}

/// How the error line of a case is obtained.
enum Obtained {
    /// kalyx 0.1.0 runs the request (operation, variable, input) under the
    /// default limits, as its command-line program does.
    Run(&'static str, &'static str, &'static str),
    /// `eval` of the input with the cancellation flag set before it runs.
    Cancelled(&'static str),
    /// `eval` of the input with a deadline that has passed when it runs.
    Expired(&'static str),
    /// The line of the case itself, which kalyx 0.1.0's own tests assert
    /// for a kind that no request of this version gives, or a later kind
    /// written in its place.
    Written,
}

/// Where in the submitted line an error is placed.
enum Expected {
    /// The items of a range, which hold the given text.
    Items(Range<usize>, &'static str),
    /// A position.
    Position(usize),
    /// The whole line.
    Line,
}

/// One error case: its kind, how its line is obtained, the line typed and
/// submitted, the versions of the request, the line without its line
/// feed, its place and its rendering as the current result.
struct Case {
    kind: &'static str,
    obtained: Obtained,
    typed: &'static str,
    versions: [u64; 3],
    line: &'static str,
    place: Expected,
    rendered: &'static str,
}

const CASES: [Case; 11] = [
    Case {
        kind: "ResourceLimit",
        obtained: Obtained::Run("eval", "", "x^5000"),
        typed: "x^5000",
        versions: [0, 0, 0],
        line: "error\t0\t0\t0\tResourceLimit\t2..6\tField(max_exponent)\t-\t4096\tRequired(5000)\tresource limit exceeded: max_exponent",
        place: Expected::Items(2..6, "5000"),
        rendered: "error (source 0, settings 0): ResourceLimit (max_exponent, allowed 4096, required 5000) at items 2..6",
    },
    Case {
        kind: "Cancelled",
        obtained: Obtained::Cancelled("1+2"),
        typed: "1+2",
        versions: [0, 0, 0],
        line: "error\t0\t0\t0\tCancelled\t0..1\t-\t-\t-\t-\tcomputation cancelled",
        place: Expected::Items(0..1, "1"),
        rendered: "error (source 0, settings 0): Cancelled at items 0..1",
    },
    Case {
        kind: "DeadlineExceeded",
        obtained: Obtained::Expired("1+2"),
        typed: "1+2",
        versions: [0, 0, 0],
        line: "error\t0\t0\t0\tDeadlineExceeded\t0..1\t-\t-\t-\t-\tdeadline exceeded",
        place: Expected::Items(0..1, "1"),
        rendered: "error (source 0, settings 0): DeadlineExceeded at items 0..1",
    },
    Case {
        kind: "Syntax",
        obtained: Obtained::Run("eval", "", "1+"),
        typed: "1+",
        versions: [0, 0, 0],
        line: "error\t0\t0\t0\tSyntax\t2..2\t-\t-\t-\t-\texpected a number, an identifier, `-` or `(`",
        place: Expected::Position(2),
        rendered: "error (source 0, settings 0): Syntax at position 2",
    },
    Case {
        kind: "UnknownSymbol",
        obtained: Obtained::Run("expand", "x", "x+y"),
        typed: "x+y",
        versions: [0, 0, 0],
        line: "error\t0\t0\t0\tUnknownSymbol\t-\t-\t-\t-\t-\ta symbol other than the variable of the polynomial ring",
        place: Expected::Line,
        rendered: "error (source 0, settings 0): UnknownSymbol on the whole line",
    },
    Case {
        kind: "DivisionByZero",
        obtained: Obtained::Run("eval", "", "0*(1/0)"),
        typed: "0*(1/0)",
        versions: [0, 0, 0],
        line: "error\t0\t0\t0\tDivisionByZero\t3..6\t-\t-\t-\t-\tdivision by zero",
        place: Expected::Items(3..6, "1/0"),
        rendered: "error (source 0, settings 0): DivisionByZero at items 3..6",
    },
    Case {
        kind: "NonIntegerExponent",
        obtained: Obtained::Run("eval", "", "2^(1/2)"),
        typed: "2^(1/2)",
        versions: [0, 0, 0],
        line: "error\t0\t0\t0\tNonIntegerExponent\t2..7\t-\t-\t-\t-\tan exponent is an integer constant",
        place: Expected::Items(2..7, "(1/2)"),
        rendered: "error (source 0, settings 0): NonIntegerExponent at items 2..7",
    },
    Case {
        kind: "NotPolynomial",
        obtained: Obtained::Run("expand", "x", "(x^2-1)/(x-1)"),
        typed: "(x^2-1)/(x-1)",
        versions: [0, 0, 0],
        line: "error\t0\t0\t0\tNotPolynomial\t-\t-\t-\t-\t-\ta negative power of a base that holds the variable",
        place: Expected::Line,
        rendered: "error (source 0, settings 0): NotPolynomial on the whole line",
    },
    Case {
        kind: "Unsupported",
        obtained: Obtained::Run("eval", "", "2*f(x)"),
        typed: "2*f(x)",
        versions: [0, 0, 0],
        line: "error\t0\t0\t0\tUnsupported\t2..4\t-\t-\t-\t-\tfunction calls are not supported in this release",
        place: Expected::Items(2..4, "f("),
        rendered: "error (source 0, settings 0): Unsupported at items 2..4",
    },
    Case {
        kind: "Internal",
        obtained: Obtained::Written,
        typed: "1+2",
        versions: [1, 2, 3],
        line: "error\t1\t2\t3\tInternal\t-\t-\t-\t-\t-\tfault",
        place: Expected::Line,
        rendered: "error (source 1, settings 3): Internal on the whole line",
    },
    Case {
        kind: "OutsideDomain",
        obtained: Obtained::Written,
        typed: "1+2",
        versions: [1, 2, 3],
        line: "error\t1\t2\t3\tOutsideDomain\t-\t-\t-\t-\t-\tfault",
        place: Expected::Line,
        rendered: "error (source 1, settings 3): OutsideDomain on the whole line",
    },
];

/// The error kinds of kalyx 0.1.0, in the order of the cases.
const KINDS: [&str; 10] = [
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
];

/// The fallback line of an outcome whose line cannot be written under the
/// context of its request: kalyx's command contract writes it under a new
/// context with the default limits, a new flag and no deadline. kosmo
/// builds that context with an injected clock, which gives the same bytes.
fn fallback(outcome: &Outcome) -> Checked<Vec<u8>> {
    let mut contract = Context::new(Limits::new(), CancelFlag::new());
    let line = serialize(outcome, &mut contract)?;
    let mut injected = Context::with_observers(
        Limits::new(),
        Box::new(FixedClock(5)),
        Box::new(CancelFlag::new()),
    );
    assert_eq!(serialize(outcome, &mut injected)?, line);
    Ok(line)
}

/// The output line of `eval` of `input` under `context`, which stops the
/// request with the error `kind` and cannot write the line itself.
fn stopped(input: &str, mut context: Context, kind: ErrorKind) -> Checked<Vec<u8>> {
    let outcome = execute(&Request::new("eval", "", input), &mut context);
    let error = outcome.result().as_ref().err().map(kalyx::Error::kind);
    assert_eq!(error, Some(kind));
    let own = serialize(&outcome, &mut context)
        .err()
        .map(|error| error.kind());
    assert_eq!(own, Some(kind));
    fallback(&outcome)
}

/// The error line of a case, obtained as the case says.
fn obtain(case: &Case) -> Checked<Vec<u8>> {
    match case.obtained {
        Obtained::Run(operation, variable, input) => {
            let mut context = context(Limits::new());
            let outcome = execute(&Request::new(operation, variable, input), &mut context);
            Ok(serialize(&outcome, &mut context)?)
        }
        Obtained::Cancelled(input) => {
            let flag = CancelFlag::new();
            flag.cancel();
            stopped(
                input,
                Context::new(Limits::new(), flag),
                ErrorKind::Cancelled,
            )
        }
        Obtained::Expired(input) => {
            let expired = Context::with_observers(
                Limits::new(),
                Box::new(FixedClock(5)),
                Box::new(CancelFlag::new()),
            )
            .with_deadline(0);
            stopped(input, expired, ErrorKind::DeadlineExceeded)
        }
        Obtained::Written => Ok(format!("{}\n", case.line).into_bytes()),
    }
}

/// The versions of a request for `snapshot` whose numbers are `numbers`:
/// the source version is 0, before any edit, or that of the snapshot.
fn versions(numbers: [u64; 3], snapshot: &Snapshot) -> Checked<Versions> {
    let source = [SourceVersion::default(), snapshot.version()]
        .into_iter()
        .find(|source| source.get() == numbers[0])
        .ok_or("the source version is 0 or that of the snapshot")?;
    Ok(Versions {
        source,
        context: ContextVersion::new(numbers[1]),
        settings: SettingsVersion::new(numbers[2]),
    })
}

/// The place that `expected` names in `snapshot`.
fn place(expected: &Expected, snapshot: &Snapshot) -> Checked<Place> {
    let root = snapshot.root();
    Ok(match expected {
        Expected::Items(range, items) => {
            let fragment = Fragment::new(root, range.clone()).ok_or("a forward range")?;
            let held = snapshot.items_of(fragment).ok_or("a range of the line")?;
            assert_eq!(text(held), *items);
            Place::Fragment(fragment)
        }
        Expected::Position(index) => Place::Position(Position {
            block: root,
            index: *index,
        }),
        Expected::Line => Place::Line,
    })
}

#[test]
fn each_kind_of_error_is_placed_in_the_line_and_rendered() -> Checked {
    let kinds: Vec<&str> = CASES.iter().map(|case| case.kind).collect();
    assert_eq!(kinds[..10], KINDS);
    assert!(!KINDS.contains(&kinds[10]));
    for case in &CASES {
        let kind = case.kind;
        let written = obtain(case)?;
        assert_eq!(written, format!("{}\n", case.line).into_bytes(), "{kind}");
        let snapshot = line(case.typed)?.snapshot();
        let versions = versions(case.versions, &snapshot)?;
        let item = read_output_line(&written, versions, &snapshot);
        assert_eq!(item.versions, versions, "{kind}");
        let Content::Error {
            kind: read,
            place: placed,
        } = &item.content
        else {
            panic!("{kind}: not an error: {:?}", item.content);
        };
        assert_eq!(read.name(), kind);
        assert_eq!(*placed, place(&case.place, &snapshot)?, "{kind}");
        assert_eq!(item.render(Freshness::Current), case.rendered, "{kind}");
    }
    Ok(())
}

#[test]
fn a_bound_on_the_amount_is_rendered_as_required_at_most() -> Checked {
    let typed = "x^100000000000000000000";
    let mut context = context(Limits::new());
    let outcome = execute(&Request::new("eval", "", typed), &mut context);
    let written = serialize(&outcome, &mut context)?;
    assert_eq!(
        written,
        b"error\t0\t0\t0\tResourceLimit\t2..23\tField(max_exponent)\t-\t4096\tBound(18446744073709551615)\tresource limit exceeded: max_exponent\n"
    );
    let snapshot = line(typed)?.snapshot();
    let item = read_output_line(&written, Versions::default(), &snapshot);
    assert_eq!(
        item.render(Freshness::Current),
        "error (source 0, settings 0): ResourceLimit (max_exponent, allowed 4096, required at most 18446744073709551615) at items 2..23"
    );
    Ok(())
}

#[test]
fn a_limit_reached_in_a_scope_names_the_scope() -> Checked {
    let snapshot = line("1+2")?.snapshot();
    let versions = versions([1, 2, 3], &snapshot)?;
    let written = b"error\t1\t2\t3\tResourceLimit\t-\tField(max_storage_bytes)\tTemporary\t18446744073709551615\tUsed(0)\tresource limit exceeded: max_storage_bytes\n";
    let item = read_output_line(written, versions, &snapshot);
    assert_eq!(
        item.render(Freshness::Current),
        "error (source 1, settings 3): ResourceLimit (max_storage_bytes in Temporary, allowed 18446744073709551615, used 0) on the whole line"
    );
    Ok(())
}

#[test]
fn a_limit_of_a_parameter_names_the_parameter() -> Checked {
    let error = context(Limits::new())
        .check_parameter("max_terms", 3, 4)
        .err()
        .ok_or("4 is above the limit 3")?;
    let written = format!(
        "error\t1\t2\t3\tResourceLimit\t-\tParameter(max_terms)\t-\t3\tRequired(4)\t{}\n",
        error.message()
    );
    let snapshot = line("1+2")?.snapshot();
    let versions = versions([1, 2, 3], &snapshot)?;
    let item = read_output_line(written.as_bytes(), versions, &snapshot);
    assert_eq!(
        item.render(Freshness::Current),
        "error (source 1, settings 3): ResourceLimit (parameter max_terms, allowed 3, required 4) on the whole line"
    );
    Ok(())
}

#[test]
fn a_stale_error_keeps_its_place_in_the_submitted_line() -> Checked {
    let mut buffer = line("0*(1/0)")?;
    let snapshot = buffer.snapshot();
    let versions = versions([1, 0, 0], &snapshot)?;
    let request = Request::new("eval", "", "0*(1/0)").with_input_version(versions.source.get());
    let mut context = context(Limits::new());
    let outcome = execute(&request, &mut context);
    let written = serialize(&outcome, &mut context)?;
    let item = read_output_line(&written, versions, &snapshot);
    // The line changes after the submission, so the result is stale.
    buffer.select_all();
    buffer.insert_text("2")?;
    assert_ne!(buffer.version(), versions.source);
    assert_eq!(
        item.render(Freshness::Stale),
        "error (source 1, settings 0, stale): DivisionByZero at items 3..6"
    );
    // The place is in the submitted snapshot, not in the changed line.
    let fragment = Fragment::new(snapshot.root(), 3..6).ok_or("a forward range")?;
    assert_eq!(
        item.content,
        Content::Error {
            kind: kosmo_math_model::display::ErrorKind::DivisionByZero,
            place: Place::Fragment(fragment),
        }
    );
    assert_eq!(
        snapshot.items_of(fragment).map(text).as_deref(),
        Some("1/0")
    );
    assert_eq!(buffer.snapshot().items_of(fragment), None);
    Ok(())
}
