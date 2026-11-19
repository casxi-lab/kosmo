//! Bounds: the display bound is the default output limit of kalyx 0.1.0,
//! text longer than the bound is cut on a UTF-8 character boundary and
//! states its length, and an output line with the wrong number of fields is
//! shown as an internal error instead of panicking.
//!
//! No value line under kalyx 0.1.0's default limits is longer than the
//! bound, so the long value is computed under raised limits. The test checks
//! the length of its line and compares its rendering with the whole expected
//! text in `data/long-value.txt`.

mod support;

use kalyx::command::serialize;
use kalyx::{ErrorKind, LimitField, LimitId, Limits, Request, execute};
use kosmo_math_kalyx::read_output_line;
use kosmo_math_model::EditBuffer;
use kosmo_math_model::display::{Freshness, MAX_DISPLAY_BYTES};
use kosmo_math_model::version::Versions;
use support::{Checked, context, line};

/// The request whose value is longer than the bound.
const LONG: (&str, &str, &str) = ("expand", "x", "(x+1)^600");

/// The default limits of kalyx 0.1.0 with the four raised so far that the
/// value of [`LONG`] is computed and written.
fn raised() -> Limits {
    Limits::new()
        .with_max_output_bytes(1_000_000)
        .with_max_work(1_000_000_000)
        .with_max_storage_bytes(100_000_000)
        .with_max_nodes(10_000_000)
}

/// The length of the line of [`LONG`], its line feed included.
const LINE_BYTES: usize = 82_971;

/// The length of the rendering of that line as the current result.
const RENDERED_BYTES: usize = 65_584;

/// The expected rendering, followed by a line feed; `data/README.md` gives
/// its source.
const RENDERED: &str = include_str!("data/long-value.txt");

#[test]
fn the_bound_is_the_default_output_limit_of_kalyx() -> Checked {
    assert_eq!(
        u64::try_from(MAX_DISPLAY_BYTES)?,
        Limits::new().max_output_bytes
    );
    Ok(())
}

#[test]
fn under_the_default_limits_the_long_value_reaches_the_work_limit() {
    let (operation, variable, input) = LONG;
    let mut context = context(Limits::new());
    let outcome = execute(&Request::new(operation, variable, input), &mut context);
    let error = outcome.result().as_ref().err();
    assert_eq!(
        error.map(kalyx::Error::kind),
        Some(ErrorKind::ResourceLimit)
    );
    let limit = error
        .and_then(kalyx::Error::exceeded)
        .map(|exceeded| exceeded.id().clone());
    assert_eq!(limit, Some(LimitId::Field(LimitField::Work)));
}

#[test]
fn a_value_longer_than_the_bound_is_cut_and_states_its_length() -> Checked {
    let (operation, variable, input) = LONG;
    let mut context = context(raised());
    let outcome = execute(&Request::new(operation, variable, input), &mut context);
    let written = serialize(&outcome, &mut context)?;
    assert_eq!(written.len(), LINE_BYTES);
    let text = written
        .strip_prefix(b"value\t0\t0\t0\t")
        .and_then(|rest| rest.strip_suffix(b"\n"))
        .ok_or("a value line of the versions 0")?;
    assert_eq!(text.len(), 82_958);
    assert!(text.len() > MAX_DISPLAY_BYTES);

    let snapshot = line(input)?.snapshot();
    let item = read_output_line(&written, Versions::default(), &snapshot);
    let rendered = item.render(Freshness::Current);
    assert_eq!(rendered.len(), RENDERED_BYTES);
    assert_eq!(Some(rendered.as_str()), RENDERED.strip_suffix('\n'));
    let expected = [
        b"value (source 0, settings 0): ".as_slice(),
        &text[..MAX_DISPLAY_BYTES],
        b" ... (82958 bytes)",
    ]
    .concat();
    assert_eq!(rendered.as_bytes(), expected);
    Ok(())
}

#[test]
fn a_line_with_the_wrong_number_of_fields_is_an_internal_error() {
    let empty = EditBuffer::new().snapshot();
    let item = read_output_line(b"value\t0\t0\t0\n", Versions::default(), &empty);
    assert_eq!(
        item.render(Freshness::Current),
        "internal error (source 0, settings 0): the output line has 4 fields; a value line has 5"
    );
}
