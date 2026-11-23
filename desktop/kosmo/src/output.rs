//! The lines that the terminal writes.

use std::fmt::Write as _;
use std::io::{self, Write};

use kosmo_sdk::kosmo_jobs::{JobId, Operation, StopReason, TerminalState};
use kosmo_sdk::kosmo_math_kalyx::NotConverted;
use kosmo_sdk::kosmo_math_model::display::{Content, DisplayItem, ErrorKind, Freshness};
use kosmo_sdk::kosmo_math_model::version::Versions;
use kosmo_sdk::kosmo_math_model::{HoleKind, InvalidKind, Item, Place};
use kosmo_sdk::kosmo_scratchpad::{Entry, NotPrepared};

use crate::enter::NotEntered;

/// Writes `line` and a line feed to `output`, with every character outside
/// 0x20 to 0x7E written as `\u{h}`, `h` its code point in lower-case
/// hexadecimal, so the bytes written are only 0x20 to 0x7E and the line
/// feed. Every line that the terminal writes goes through it.
pub(crate) fn write_line(output: &mut dyn Write, line: &str) -> io::Result<()> {
    output.write_all(escape(line).as_bytes())?;
    output.write_all(b"\n")
}

/// `line` with every character outside 0x20 to 0x7E written as `\u{h}`.
fn escape(line: &str) -> String {
    let mut escaped = String::with_capacity(line.len());
    for c in line.chars() {
        if (' '..='~').contains(&c) {
            escaped.push(c);
        } else {
            // Writing into a `String` does not fail.
            let _ = write!(escaped, "\\u{{{:x}}}", u32::from(c));
        }
    }
    escaped
}

/// The line for an expression that was not entered whole.
pub(crate) fn not_entered(not: NotEntered) -> String {
    format!("input error: {} at position {}", not.name(), not.position)
}

/// The line for a line that was not prepared: every hole of an incomplete
/// line in reading order, or the first invalid part of an invalid line.
pub(crate) fn not_prepared(not: &NotPrepared) -> String {
    match not {
        NotPrepared::NotConverted(NotConverted::Incomplete(holes)) => {
            let holes: Vec<String> = holes
                .iter()
                .map(|hole| {
                    format!(
                        "{} at position {}",
                        hole_name(hole.kind),
                        hole.position.index
                    )
                })
                .collect();
            format!("input error: {}", holes.join(", "))
        }
        NotPrepared::NotConverted(NotConverted::Invalid(invalid)) => {
            let items = invalid.fragment.range();
            format!(
                "input error: {} at items {}..{}",
                invalid_name(invalid.kind),
                items.start,
                items.end
            )
        }
        NotPrepared::VariableTooLong { .. } => "input error: VariableTooLong".to_string(),
        other => format!("internal error: {other}"),
    }
}

fn hole_name(kind: HoleKind) -> &'static str {
    match kind {
        HoleKind::EmptyLine => "EmptyLine",
        HoleKind::MissingOperand => "MissingOperand",
        HoleKind::MissingOperator => "MissingOperator",
        HoleKind::MissingRightBracket => "MissingRightBracket",
        _ => "Hole",
    }
}

fn invalid_name(kind: InvalidKind) -> &'static str {
    match kind {
        InvalidKind::MalformedNumber => "MalformedNumber",
        InvalidKind::SurplusRightBracket => "SurplusRightBracket",
        _ => "Invalid",
    }
}

/// The line for job `id` when the job layer gives no terminal state for it,
/// at submission and in the history alike.
pub(crate) fn undelivered(id: JobId) -> String {
    format!("internal error: the job layer gave no terminal state for {id}")
}

/// The line for the outcome of `entry`, shown with `freshness`.
///
/// A job with an output line writes the rendering of its display item. A
/// job whose deadline passed before its request ran has no line and writes
/// the error `DeadlineExceeded` on the whole line. Any other job without a
/// line writes an internal error with its versions and the reason, and a
/// job without a terminal state the line of [`undelivered`].
pub(crate) fn outcome(entry: &Entry, freshness: Freshness) -> String {
    let versions = entry.versions();
    let Some(outcome) = entry.outcome() else {
        return undelivered(entry.id());
    };
    if let Some(item) = outcome.item() {
        return item.render(freshness);
    }
    if outcome.stop() == Some(StopReason::Deadline) {
        let item = DisplayItem {
            versions,
            content: Content::Error {
                kind: ErrorKind::DeadlineExceeded,
                place: Place::Line,
            },
        };
        return item.render(freshness);
    }
    let reason = if outcome.internal_error() {
        "the job failed with an internal error"
    } else {
        match outcome.state() {
            TerminalState::Cancelled => "the job was cancelled",
            TerminalState::Failed => "the result does not fit the limits of the job",
            TerminalState::ResourceExhausted => "the job was stopped by force",
            _ => "the job ended without a result",
        }
    };
    internal(versions, reason)
}

fn internal(versions: Versions, reason: &str) -> String {
    format!(
        "internal error (source {}, settings {}): {reason}",
        versions.source.get(),
        versions.settings.get()
    )
}

/// The line for `entry` in the history: its job, the line as it was typed
/// and its outcome, which is current for its own versions.
pub(crate) fn history_entry(entry: &Entry) -> String {
    let expression: String = entry
        .input()
        .items()
        .iter()
        .map(|item| match item {
            Item::Char(c) => *c,
            _ => char::REPLACEMENT_CHARACTER,
        })
        .collect();
    let typed = match entry.operation() {
        Operation::Eval => expression,
        operation => format!(":{} {} {expression}", operation.name(), entry.variable()),
    };
    format!(
        "{}: {typed} => {}",
        entry.id(),
        outcome(entry, Freshness::Current)
    )
}

#[cfg(test)]
mod tests {
    use kosmo_sdk::kosmo_math_model::display::{LimitAmount, LimitExceeded, LimitId};

    use super::*;

    fn written(line: &str) -> Vec<u8> {
        let mut output = Vec::new();
        write_line(&mut output, line).unwrap();
        output
    }

    #[test]
    fn printable_ascii_is_written_as_it_is() {
        let all: String = (' '..='~').collect();
        assert_eq!(written(&all), format!("{all}\n").into_bytes());
    }

    #[test]
    fn every_other_character_is_escaped() {
        assert_eq!(
            written("\t\r\u{1b}[31m\u{7f}\u{85}\u{9b}\u{2028}\u{2029}"),
            b"\\u{9}\\u{d}\\u{1b}[31m\\u{7f}\\u{85}\\u{9b}\\u{2028}\\u{2029}\n"
        );
        assert_eq!(
            written("\u{61c}\u{200e}\u{200f}\u{202a}\u{202e}\u{2066}\u{2069}"),
            b"\\u{61c}\\u{200e}\\u{200f}\\u{202a}\\u{202e}\\u{2066}\\u{2069}\n"
        );
        assert_eq!(written("x\u{b2} \u{3b1}"), b"x\\u{b2} \\u{3b1}\n");
        assert_eq!(written("\n"), b"\\u{a}\n");
    }

    #[test]
    fn a_parameter_name_with_control_characters_is_written_escaped() {
        let item = DisplayItem {
            versions: Versions::default(),
            content: Content::Error {
                kind: ErrorKind::ResourceLimit(LimitExceeded {
                    limit: LimitId::Parameter("n\u{9b}31m\u{202e}x".to_string()),
                    scope: None,
                    allowed: 10,
                    amount: LimitAmount::Used(11),
                }),
                place: Place::Line,
            },
        };
        let rendered = item.render(Freshness::Current);
        assert_eq!(
            rendered,
            "error (source 0, settings 0): ResourceLimit (parameter n\u{9b}31m\u{202e}x, allowed 10, used 11) on the whole line",
            "the rendering itself keeps the characters"
        );
        let output = written(&rendered);
        assert_eq!(
            output,
            b"error (source 0, settings 0): ResourceLimit (parameter n\\u{9b}31m\\u{202e}x, allowed 10, used 11) on the whole line\n"
        );
        assert!(
            output
                .iter()
                .all(|&byte| (0x20..=0x7E).contains(&byte) || byte == b'\n')
        );
    }

    #[test]
    fn holes_and_invalid_parts_are_written_by_their_kind() {
        assert_eq!(hole_name(HoleKind::EmptyLine), "EmptyLine");
        assert_eq!(hole_name(HoleKind::MissingOperator), "MissingOperator");
        assert_eq!(
            invalid_name(InvalidKind::MalformedNumber),
            "MalformedNumber"
        );
        assert_eq!(
            not_prepared(&NotPrepared::VariableTooLong { bytes: 4097 }),
            "input error: VariableTooLong"
        );
    }
}
