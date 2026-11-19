//! Helpers shared by the integration tests: building lines, reading text
//! with kalyx 0.1.0 as its command-line program does, and the answer
//! vectors of kalyx 0.1.0.

#![allow(dead_code, reason = "each test file uses some of the helpers")]

use std::error::Error;
use std::ops::Range;

use kalyx::command::serialize;
use kalyx::context::CancelFlag;
use kalyx::{Context, Limits, Request, execute, parse};
use kosmo_math_model::{EditBuffer, Item};

/// The result of a helper or a test.
pub type Checked<T = ()> = Result<T, Box<dyn Error>>;

/// A context under `limits`, with a cancellation flag that nothing sets and
/// no deadline, as the command-line program of kalyx builds one for each
/// request.
pub fn context(limits: Limits) -> Context {
    Context::new(limits, CancelFlag::new())
}

/// Limits far above every input of these tests, as kalyx's own tests use
/// them to read answers.
pub fn wide() -> Limits {
    Limits::new()
        .with_max_work(u64::MAX)
        .with_max_depth(1 << 20)
        .with_max_nodes(1 << 40)
        .with_max_storage_bytes(1 << 40)
        .with_max_output_bytes(u64::MAX)
        .with_max_exponent(1 << 20)
        .with_max_integer_bits(1 << 20)
}

/// A line with `text` typed into it as one insertion.
pub fn line(text: &str) -> Checked<EditBuffer> {
    let mut buffer = EditBuffer::new();
    buffer.insert_text(text)?;
    Ok(buffer)
}

/// The characters of `items`. No template can exist yet; one would show as
/// U+FFFD, which no line can hold.
pub fn text(items: &[Item]) -> String {
    items
        .iter()
        .map(|item| match item {
            Item::Char(c) => *c,
            _ => char::REPLACEMENT_CHARACTER,
        })
        .collect()
}

/// Whether kalyx 0.1.0 reads `one` and `other` under `limits` as terms that
/// are equal by structure.
pub fn equal_terms(one: &str, other: &str, limits: Limits) -> Checked<bool> {
    let mut context = context(limits);
    let one = parse(one, &mut context)?;
    let other = parse(other, &mut context)?;
    Ok(one.equals(&other, &mut context)?)
}

/// The output line of `eval` of `input` under the default limits, without
/// its line feed: the line that `kalyx eval INPUT` writes.
pub fn eval_line(input: &str) -> Checked<String> {
    let request = Request::new("eval", "", input);
    let mut context = context(Limits::new());
    let outcome = execute(&request, &mut context);
    let line = String::from_utf8(serialize(&outcome, &mut context)?)?;
    let line = line
        .strip_suffix('\n')
        .ok_or("a line ends with a line feed")?;
    Ok(line.to_string())
}

/// The span of an error line, the sixth of its eleven fields: `start..end`
/// as a range, or `None` for `-`.
pub fn span_field(line: &str) -> Checked<Option<Range<usize>>> {
    let fields: Vec<&str> = line.split('\t').collect();
    if fields.len() != 11 || fields.first() != Some(&"error") {
        return Err(format!("not an error line: {line:?}").into());
    }
    match fields.get(5).copied() {
        Some("-") => Ok(None),
        Some(span) => {
            let (start, end) = span.split_once("..").ok_or("a span is start..end")?;
            Ok(Some(start.parse()?..end.parse()?))
        }
        None => Err("an error line has a span".into()),
    }
}

/// The answer vectors of kalyx 0.1.0; `data/README.md` gives their source.
const VECTORS: &str = include_str!("../data/r02.tsv");

/// The answers that are longer than one insertion may be.
pub const TOO_LONG: [&str; 2] = ["dense_rational_128", "derivative_128"];

/// A case of the answer vectors whose result is `ok`.
pub struct Answer<'a> {
    pub id: &'a str,
    pub operation: &'a str,
    pub variable: &'a str,
    pub input: &'a str,
    pub answer: &'a str,
}

/// The cases of the answer vectors whose result is `ok`, in order. Every
/// line has the seven columns of the header.
pub fn answers() -> Checked<Vec<Answer<'static>>> {
    let mut lines = VECTORS.lines();
    assert_eq!(
        lines.next(),
        Some("id\toperation\tvariable\tinput\tresult\tanswer\tlimits")
    );
    let mut cases = 0;
    let mut answers = Vec::new();
    for line in lines {
        cases += 1;
        let columns: Vec<&str> = line.split('\t').collect();
        let &[id, operation, variable, input, result, answer, limits] = columns.as_slice() else {
            return Err(format!("not seven columns: {line:?}").into());
        };
        if result == "ok" {
            // Every `ok` case uses the default limits.
            assert_eq!(limits, "-", "{id}");
            answers.push(Answer {
                id,
                operation,
                variable,
                input,
                answer,
            });
        }
    }
    assert_eq!(cases, 58);
    Ok(answers)
}
