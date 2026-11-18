//! Random lines from a fixed seed. Random complete lines are judged
//! complete, convert to the text that the generator expects, and kalyx
//! 0.1.0 reads that text without a syntax error under its default limits;
//! the span of any error it reports maps back into the line. Random
//! incomplete lines convert to no text, only to their holes. Nothing
//! panics.
//!
//! The generator reports its seed and how many values it had drawn when a
//! check fails, which identifies the failing line.

mod support;

use std::collections::BTreeMap;

use kalyx::{ErrorKind, FaultKind, Limits, parse};
use kosmo_math_kalyx::{NotConverted, Place, convert, map_span};
use kosmo_math_model::{Completeness, Fragment, Position};
use support::{Checked, context, line};

/// The seed of the first line; line `n` uses `SEED + n`.
const SEED: u64 = 0x6b6f_736d_6f00_0102;

/// The number of complete lines, and of incomplete lines.
const LINES: u64 = 1_000;

/// The most mutations tried for incomplete lines, per line wanted.
const ATTEMPTS_PER_LINE: u64 = 20;

/// The most operands of one generated line.
const MAX_OPERANDS: usize = 24;

/// The deepest nesting of brackets, calls and unary minus signs that the
/// generator writes, well within the parse depth limit of the line.
const MAX_NESTING: usize = 8;

/// The reserved names of kalyx 0.1.0.
const RESERVED: [&str; 3] = ["pi", "i", "undefined"];

/// The SplitMix64 generator. When a test panics while a generator is alive,
/// it reports its seed and how many values it produced.
struct SplitMix64 {
    seed: u64,
    state: u64,
    drawn: u64,
}

impl SplitMix64 {
    fn new(seed: u64) -> SplitMix64 {
        SplitMix64 {
            seed,
            state: seed,
            drawn: 0,
        }
    }

    fn next(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        self.drawn += 1;
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// A value in `0..bound`, up to the generator's bias; 0 when `bound` is 0.
    fn below(&mut self, bound: usize) -> usize {
        let bound = bound as u64;
        if bound == 0 {
            0
        } else {
            (self.next() % bound) as usize
        }
    }

    /// Whether a one-in-`n` chance came up.
    fn one_in(&mut self, n: usize) -> bool {
        self.below(n) == 0
    }

    /// One of `chars`.
    fn pick(&mut self, chars: &str) -> char {
        let chars: Vec<char> = chars.chars().collect();
        chars[self.below(chars.len())]
    }
}

impl Drop for SplitMix64 {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!(
                "random line: seed {:#x}, failure after {} values were drawn",
                self.seed, self.drawn
            );
        }
    }
}

const DIGITS: &str = "0123456789";
const NAME_START: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ_";
const NAME_REST: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ_0123456789";

/// A complete line as it is typed, the text that conversion is expected to
/// write for it, and what it holds.
#[derive(Default)]
struct Written {
    line: String,
    text: String,
    /// Whether it holds a call or a reserved name.
    unsupported: bool,
    /// Whether a number of it starts or ends with a point.
    normalized: bool,
}

impl Written {
    /// Appends the same characters to the line and the text.
    fn both(&mut self, s: &str) {
        self.line.push_str(s);
        self.text.push_str(s);
    }
}

/// Writes random complete lines from the grammar of kalyx 0.1.0 as kosmo
/// reads it: calls are operands, and numbers may start or end with a point.
struct Generator<'r> {
    rng: &'r mut SplitMix64,
    written: Written,
    /// The operands still to write; when none are left, every production
    /// takes its shortest form.
    operands: usize,
}

impl Generator<'_> {
    fn line(rng: &mut SplitMix64) -> Written {
        let operands = 1 + rng.below(MAX_OPERANDS);
        let mut generator = Generator {
            rng,
            written: Written::default(),
            operands,
        };
        generator.space();
        generator.expression(MAX_NESTING);
        generator.space();
        generator.written
    }

    /// Mostly nothing, sometimes one or two spaces.
    fn space(&mut self) {
        if self.rng.one_in(6) {
            let spaces = if self.rng.one_in(4) { "  " } else { " " };
            self.written.both(spaces);
        }
    }

    fn more(&mut self) -> bool {
        self.operands > 0
    }

    /// A chain of terms joined by `+` and `-`.
    fn expression(&mut self, nesting: usize) {
        self.term(nesting);
        while self.more() && self.rng.one_in(2) {
            self.space();
            let operator = self.rng.pick("+-");
            self.written.both(&operator.to_string());
            self.space();
            self.term(nesting);
        }
    }

    /// A chain of unary operands joined by `*` and `/`.
    fn term(&mut self, nesting: usize) {
        self.unary(nesting);
        while self.more() && self.rng.one_in(2) {
            self.space();
            let operator = self.rng.pick("*/");
            self.written.both(&operator.to_string());
            self.space();
            self.unary(nesting);
        }
    }

    /// A unary minus before a unary operand, or a power.
    fn unary(&mut self, nesting: usize) {
        if nesting > 0 && self.rng.one_in(5) {
            self.written.both("-");
            self.space();
            self.unary(nesting - 1);
        } else {
            self.power(nesting);
        }
    }

    /// A primary, with an exponent sometimes.
    fn power(&mut self, nesting: usize) {
        self.primary(nesting);
        if self.more() && self.rng.one_in(4) {
            self.space();
            self.written.both("^");
            self.space();
            self.unary(nesting);
        }
    }

    /// A number, a name, a call or an expression in brackets.
    fn primary(&mut self, nesting: usize) {
        self.operands = self.operands.saturating_sub(1);
        let nested = nesting > 0 && self.more();
        if nested && self.rng.one_in(6) {
            self.written.both("(");
            self.space();
            self.expression(nesting - 1);
            self.space();
            self.written.both(")");
        } else if nested && self.rng.one_in(20) {
            self.name();
            self.space();
            self.written.both("(");
            self.expression(nesting - 1);
            self.written.both(")");
            self.written.unsupported = true;
        } else if self.rng.one_in(2) {
            self.number();
        } else {
            self.name();
        }
    }

    fn digits(&mut self) -> String {
        let count = 1 + self.rng.below(4);
        (0..count).map(|_| self.rng.pick(DIGITS)).collect()
    }

    /// An integer, a number with a point between digits, or one that starts
    /// or ends with a point, which conversion writes with a `0` before it or
    /// without the point. Sometimes the digits are a zero, so that some
    /// divisions are by zero.
    fn number(&mut self) {
        let digits = if self.rng.one_in(8) {
            "0".to_string()
        } else {
            self.digits()
        };
        match self.rng.below(4) {
            0 => self.written.both(&digits),
            1 => {
                let fraction = self.digits();
                self.written.both(&format!("{digits}.{fraction}"));
            }
            2 => {
                self.written.line.push_str(&format!(".{digits}"));
                self.written.text.push_str(&format!("0.{digits}"));
                self.written.normalized = true;
            }
            _ => {
                self.written.line.push_str(&format!("{digits}."));
                self.written.text.push_str(&digits);
                self.written.normalized = true;
            }
        }
    }

    /// A name, sometimes a reserved one.
    fn name(&mut self) {
        let name = if self.rng.one_in(30) {
            RESERVED[self.rng.below(RESERVED.len())].to_string()
        } else {
            let mut name = self.rng.pick(NAME_START).to_string();
            for _ in 0..self.rng.below(4) {
                name.push(self.rng.pick(NAME_REST));
            }
            name
        };
        if RESERVED.contains(&name.as_str()) {
            self.written.unsupported = true;
        }
        self.written.both(&name);
    }
}

/// What kalyx 0.1.0 made of a complete line, for the counts of outcomes.
fn outcome_name(kind: Option<ErrorKind>) -> String {
    match kind {
        None => "term".to_string(),
        Some(kind) => format!("{kind:?}"),
    }
}

#[test]
fn random_complete_lines_convert_to_text_that_kalyx_reads() -> Checked {
    let mut outcomes: BTreeMap<String, u64> = BTreeMap::new();
    let mut normalized = 0;
    for n in 0..LINES {
        let mut rng = SplitMix64::new(SEED.wrapping_add(n));
        let written = Generator::line(&mut rng);
        let typed = &written.line;
        let snapshot = line(typed)?.snapshot();
        assert_eq!(snapshot.completeness(), Completeness::Complete, "{typed:?}");
        let text = convert(&snapshot)?;
        assert_eq!(text, written.text, "{typed:?}");
        normalized += u64::from(written.normalized);
        let mut context = context(Limits::new());
        let error = parse(&text, &mut context).err();
        let kind = error.as_ref().map(kalyx::Error::kind);
        assert_ne!(kind, Some(ErrorKind::Fault(FaultKind::Syntax)), "{text:?}");
        if !written.unsupported {
            assert_ne!(
                kind,
                Some(ErrorKind::Fault(FaultKind::Unsupported)),
                "{text:?}"
            );
        }
        if let Some(span) = error.as_ref().and_then(kalyx::Error::span) {
            let place = map_span(&snapshot, Some(span.start()..span.end()))?;
            // A line written as typed: the span is already at the edges of
            // tokens and maps to the same items.
            if !written.normalized {
                let root = snapshot.root();
                let expected = if span.start() == span.end() {
                    Place::Position(Position {
                        block: root,
                        index: span.start(),
                    })
                } else {
                    Place::Fragment(
                        Fragment::new(root, span.start()..span.end()).ok_or("a forward span")?,
                    )
                };
                assert_eq!(place, expected, "{text:?}");
            }
        }
        *outcomes.entry(outcome_name(kind)).or_default() += 1;
    }
    // Each outcome came up, so a change of the generator that stops
    // reaching one fails.
    for name in [
        "term",
        "Fault(Unsupported)",
        "Fault(DivisionByZero)",
        "Fault(NonIntegerExponent)",
    ] {
        assert!(outcomes.contains_key(name), "{name}: {outcomes:?}");
    }
    assert!(normalized > LINES / 4, "{normalized} normalized lines");
    Ok(())
}

/// Turns a complete line into one that is likely incomplete: deletes a
/// character, inserts an operator, a bracket or an operand, or cuts the
/// line short.
fn mutate(rng: &mut SplitMix64, typed: &str) -> String {
    let mut chars: Vec<char> = typed.chars().collect();
    let at = rng.below(chars.len() + 1);
    match rng.below(4) {
        0 if !chars.is_empty() => {
            chars.remove(at.min(chars.len() - 1));
        }
        1 => chars.insert(at, rng.pick("+-*/^()")),
        2 => {
            let operand = if rng.one_in(2) { "x" } else { "2" };
            chars.splice(at..at, operand.chars());
        }
        _ => chars.truncate(at),
    }
    chars.into_iter().collect()
}

#[test]
fn random_incomplete_lines_convert_to_their_holes_only() -> Checked {
    let mut incomplete = 0;
    let mut invalid = 0;
    let mut attempt = 0;
    while incomplete < LINES {
        assert!(attempt < LINES * ATTEMPTS_PER_LINE, "{incomplete} found");
        let mut rng = SplitMix64::new(SEED.wrapping_add(LINES + attempt));
        attempt += 1;
        let written = Generator::line(&mut rng);
        let mutated = mutate(&mut rng, &written.line);
        let Ok(buffer) = line(&mutated) else {
            continue;
        };
        let snapshot = buffer.snapshot();
        match (snapshot.completeness(), convert(&snapshot)) {
            (Completeness::Incomplete(holes), converted) => {
                assert!(!holes.is_empty(), "{mutated:?}");
                assert_eq!(
                    converted,
                    Err(NotConverted::Incomplete(holes)),
                    "{mutated:?}"
                );
                incomplete += 1;
            }
            (Completeness::Invalid(found), converted) => {
                assert_eq!(converted, Err(NotConverted::Invalid(found)), "{mutated:?}");
                invalid += 1;
            }
            (Completeness::Complete, converted) => {
                assert!(converted.is_ok(), "{mutated:?}");
            }
        }
    }
    assert!(invalid > 0, "no invalid line came up");
    Ok(())
}
