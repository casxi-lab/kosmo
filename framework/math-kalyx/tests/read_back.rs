//! Reading answers back: the answers of the answer vectors of kalyx 0.1.0
//! whose result is `ok` are read back into lines, converted, and read by
//! kalyx 0.1.0 as the same terms as the answers. The two answers longer
//! than one insertion are not reusable. The display text of the value of
//! every such case, as kalyx 0.1.0 computes it, reads back the same way.

mod support;

use kalyx::command::serialize;
use kalyx::{Limits, Request, execute};
use kosmo_math_kalyx::{NotReusable, convert, read_back};
use kosmo_math_model::{Completeness, MAX_INSERT_BYTES, Rejected};
use support::{Checked, TOO_LONG, answers, context, equal_terms, line, text, wide};

#[test]
fn the_answer_vectors_hold_forty_nine_ok_answers() -> Checked {
    let answers = answers()?;
    assert_eq!(answers.len(), 49);
    let long: Vec<&str> = answers
        .iter()
        .filter(|answer| answer.answer.len() > MAX_INSERT_BYTES)
        .map(|answer| answer.id)
        .collect();
    assert_eq!(long, TOO_LONG);
    Ok(())
}

#[test]
fn answers_within_one_insertion_read_back_and_convert_to_the_same_terms() -> Checked {
    let mut read = 0;
    for answer in answers()?
        .iter()
        .filter(|answer| !TOO_LONG.contains(&answer.id))
    {
        let id = answer.id;
        let snapshot = read_back(answer.answer)?;
        assert_eq!(text(snapshot.items()), answer.answer, "{id}");
        assert_eq!(snapshot.completeness(), Completeness::Complete, "{id}");
        let converted = convert(&snapshot)?;
        assert_eq!(converted, answer.answer, "{id}");
        assert!(
            equal_terms(&converted, answer.answer, wide())?,
            "{id}: the terms differ"
        );
        read += 1;
    }
    assert_eq!(read, 47);
    Ok(())
}

#[test]
fn answers_longer_than_one_insertion_are_not_reusable() -> Checked {
    let mut edited = line("2*")?;
    let before = edited.clone();
    let mut rejected = Vec::new();
    for answer in answers()?
        .iter()
        .filter(|answer| TOO_LONG.contains(&answer.id))
    {
        let reason = match read_back(answer.answer) {
            Ok(snapshot) => {
                edited.insert_fragment(&snapshot, snapshot.line())?;
                None
            }
            Err(reason) => Some(reason),
        };
        assert_eq!(
            reason,
            Some(NotReusable::Rejected(Rejected::InsertTooLong)),
            "{}",
            answer.id
        );
        assert_eq!(
            reason.map(|reason| reason.to_string()).as_deref(),
            Some("the value cannot be reused: an insertion holds at most 4096 bytes")
        );
        rejected.push(answer.id);
    }
    assert_eq!(rejected, TOO_LONG);
    // Nothing was inserted into the line being edited.
    assert_eq!(edited, before);
    Ok(())
}

#[test]
fn the_display_text_of_each_value_reads_back() -> Checked {
    for answer in answers()? {
        let id = answer.id;
        let request = Request::new(answer.operation, answer.variable, answer.input);
        let mut context = context(Limits::new());
        let outcome = execute(&request, &mut context);
        let value_line = String::from_utf8(serialize(&outcome, &mut context)?)?;
        let display = value_line
            .strip_prefix("value\t0\t0\t0\t")
            .and_then(|rest| rest.strip_suffix('\n'))
            .ok_or(format!("{id}: not a value line: {value_line:?}"))?;
        match read_back(display) {
            Ok(snapshot) => {
                let converted = convert(&snapshot)?;
                assert_eq!(converted, display, "{id}");
                assert!(equal_terms(&converted, answer.answer, wide())?, "{id}");
            }
            Err(reason) => {
                assert!(display.len() > MAX_INSERT_BYTES, "{id}: {reason}");
                assert_eq!(reason, NotReusable::Rejected(Rejected::InsertTooLong));
            }
        }
    }
    Ok(())
}
