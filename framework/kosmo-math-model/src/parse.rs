//! The parser of a block: its tokens, holes, completeness and parse depth.
//!
//! The parser reads the grammar of kalyx 0.1.0, with two readings of
//! kosmo's own: function-call syntax is an operand, and a number is a run of
//! digits and points with at most one point and at least one digit, so `.5`
//! and `1.` are numbers (conversion writes them as `0.5` and `1`):
//!
//! ```text
//! expression = term , { ( "+" | "-" ) , term } ;
//! term       = unary , { ( "*" | "/" ) , unary } ;
//! unary      = "-" , unary | power ;
//! power      = primary , [ "^" , unary ] ;
//! primary    = number | name | call | "(" , expression , ")" ;
//! ```
//!
//! A call is a name followed by `(`, spaces allowed between, then an
//! expression and `)`. Every operator stands between two operands, and any
//! number of unary minus signs may start an operand, so the parser is a
//! single pass that alternates between expecting an operand and expecting
//! an operator, with a stack of open brackets. Where the expected token is
//! missing it notes a hole and goes on as if it were there, so one pass
//! finds every hole, the first invalid part and the parse depth of the
//! whole line, without recursion.

use crate::structure::{Fragment, Item, NodeId, Position, character};
use crate::{Completeness, Hole, HoleKind, Invalid, InvalidKind};

/// What a token is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// A run of digits and points; `valid` when it forms a number, with at
    /// most one point and at least one digit.
    Number {
        valid: bool,
    },
    /// A letter or `_`, then letters, digits and `_`.
    Name,
    /// `+`, `-`, `*`, `/` or `^`.
    Operator(char),
    Open,
    Close,
}

/// A token and the half-open range of items it covers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Token {
    kind: Kind,
    start: usize,
    end: usize,
}

const fn is_number_char(c: char) -> bool {
    c.is_ascii_digit() || c == '.'
}

const fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// The tokens of a block. Spaces only separate tokens. A number ends at the
/// first character that is neither a digit nor a point, so `2x` is a number
/// followed by a name.
fn tokens(items: &[Item]) -> Vec<Token> {
    let chars: Vec<char> = items.iter().map(character).collect();
    let mut tokens = Vec::new();
    let mut index = 0;
    while let Some(&c) = chars.get(index) {
        let start = index;
        index += 1;
        let kind = if is_number_char(c) {
            while chars.get(index).copied().is_some_and(is_number_char) {
                index += 1;
            }
            let run = chars.get(start..index).unwrap_or_default();
            let points = run.iter().filter(|&&d| d == '.').count();
            Kind::Number {
                valid: points <= 1 && points < run.len(),
            }
        } else if c.is_ascii_alphabetic() || c == '_' {
            while chars.get(index).copied().is_some_and(is_name_char) {
                index += 1;
            }
            Kind::Name
        } else {
            match c {
                '+' | '-' | '*' | '/' | '^' => Kind::Operator(c),
                '(' => Kind::Open,
                ')' => Kind::Close,
                // A space. Edits admit no other character; one would
                // separate tokens like a space.
                _ => continue,
            }
        };
        tokens.push(Token {
            kind,
            start,
            end: index,
        });
    }
    tokens
}

/// What the parser found in a block.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Analysis {
    pub(crate) completeness: Completeness,
    /// The parse depth: the deepest nesting of brackets and unary minus
    /// signs, where each bracket pair and each unary minus adds a level for
    /// what it encloses.
    pub(crate) depth: usize,
    /// For each missing operand, the items between the token before it, or
    /// the start of the line, and the token after it, or the end of the line.
    operand_gaps: Vec<(usize, usize)>,
}

impl Analysis {
    /// Whether an operand is missing at `index`: the place lies between the
    /// tokens around a missing operand.
    pub(crate) fn needs_operand_at(&self, index: usize) -> bool {
        self.operand_gaps
            .iter()
            .any(|&(from, to)| from <= index && index <= to)
    }
}

/// Parses the items of `block`.
pub(crate) fn analyse(block: NodeId, items: &[Item]) -> Analysis {
    let at = |index| Position { block, index };
    let tokens = tokens(items);
    let end = items.len();
    let mut holes = Vec::new();
    let mut operand_gaps = Vec::new();
    let mut invalid: Option<Invalid> = None;
    let mut note_invalid = |kind, token: &Token| {
        if invalid.is_none() {
            invalid = Fragment::new(block, token.start..token.end)
                .map(|fragment| Invalid { kind, fragment });
        }
    };
    // The number of unary minus signs whose operand is still being read,
    // for the line and for each open bracket.
    let mut levels: Vec<usize> = vec![0];
    let mut depth = 0usize;
    let mut deepest = 0usize;
    let mut expect_operand = true;
    let mut previous: Option<Token> = None;
    for token in &tokens {
        let gap_start = previous.map_or(0, |token| token.end);
        match token.kind {
            Kind::Number { .. } | Kind::Name | Kind::Open => {
                let call = token.kind == Kind::Open
                    && previous.is_some_and(|token| token.kind == Kind::Name);
                if !expect_operand && !call {
                    holes.push(Hole {
                        kind: HoleKind::MissingOperator,
                        position: at(token.start),
                    });
                    // A missing operator joins two operands like `*`.
                    close_unary(&mut levels, &mut depth);
                }
                match token.kind {
                    Kind::Number { valid: false } => {
                        note_invalid(InvalidKind::MalformedNumber, token);
                    }
                    Kind::Open => {
                        levels.push(0);
                        depth += 1;
                        deepest = deepest.max(depth);
                    }
                    _ => {}
                }
                expect_operand = token.kind == Kind::Open;
            }
            Kind::Operator('-') if expect_operand => {
                if let Some(open) = levels.last_mut() {
                    *open += 1;
                }
                depth += 1;
                deepest = deepest.max(depth);
            }
            Kind::Operator(operator) => {
                if expect_operand {
                    holes.push(Hole {
                        kind: HoleKind::MissingOperand,
                        position: at(token.start),
                    });
                    operand_gaps.push((gap_start, token.start));
                }
                // The exponent of `^` lies inside the operand of a unary
                // minus before its base; every other operator ends it.
                if operator != '^' {
                    close_unary(&mut levels, &mut depth);
                }
                expect_operand = true;
            }
            Kind::Close => {
                if expect_operand {
                    holes.push(Hole {
                        kind: HoleKind::MissingOperand,
                        position: at(token.start),
                    });
                    operand_gaps.push((gap_start, token.start));
                }
                if levels.len() > 1 {
                    let open = levels.pop().unwrap_or_default();
                    depth = depth.saturating_sub(open + 1);
                } else {
                    note_invalid(InvalidKind::SurplusRightBracket, token);
                }
                expect_operand = false;
            }
        }
        previous = Some(*token);
    }
    match previous {
        None => holes.push(Hole {
            kind: HoleKind::EmptyLine,
            position: at(end),
        }),
        Some(last) => {
            if expect_operand {
                holes.push(Hole {
                    kind: HoleKind::MissingOperand,
                    position: at(end),
                });
                operand_gaps.push((last.end, end));
            }
            for _ in 1..levels.len() {
                holes.push(Hole {
                    kind: HoleKind::MissingRightBracket,
                    position: at(end),
                });
            }
        }
    }
    let completeness = match invalid {
        Some(invalid) => Completeness::Invalid(invalid),
        None if holes.is_empty() => Completeness::Complete,
        None => Completeness::Incomplete(holes),
    };
    Analysis {
        completeness,
        depth: deepest,
        operand_gaps,
    }
}

/// Ends the operands of the unary minus signs at the innermost level.
fn close_unary(levels: &mut [usize], depth: &mut usize) {
    if let Some(open) = levels.last_mut() {
        *depth = depth.saturating_sub(*open);
        *open = 0;
    }
}

/// Whether inserted items need brackets to stay one operand: they hold a
/// token and are not a single operand, that is, a number, a name, a call or
/// a bracketed expression.
pub(crate) fn needs_grouping(items: &[Item]) -> bool {
    let tokens = tokens(items);
    let group = match tokens.as_slice() {
        [] => return false,
        [only] => return !matches!(only.kind, Kind::Number { .. } | Kind::Name),
        [first, rest @ ..] if first.kind == Kind::Name => rest,
        all => all,
    };
    // A single operand: a left bracket whose partner is the last token.
    let mut open = 0usize;
    for (index, token) in group.iter().enumerate() {
        match token.kind {
            Kind::Open => open += 1,
            Kind::Close if open > 0 => {
                open -= 1;
                if open == 0 {
                    return index + 1 != group.len();
                }
            }
            _ if open == 0 => return true,
            _ => {}
        }
    }
    true
}

#[cfg(test)]
mod tests;
