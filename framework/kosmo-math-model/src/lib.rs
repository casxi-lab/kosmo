//! The editing structure of formulas, apart from the expressions of the
//! computer algebra library: one line of input with its cursor, selection,
//! holes and source version.
//!
//! An [`EditBuffer`] holds one line as a root block of characters. Editing
//! operations insert and delete characters, move the cursor, select and copy
//! fragments; each checks its limits first and changes nothing when it is
//! rejected. Holes are not stored: [`EditBuffer::completeness`] derives them
//! by parsing the line with the grammar of kalyx 0.1.0, so incomplete input
//! such as `1/□` stays representable. A computation receives a
//! [`Snapshot`], never the buffer, so its result cannot change what is being
//! edited; [`Snapshot::tokens`] gives the tokens that the parser reads,
//! from which the line is converted for kalyx.
//!
//! ```
//! use kosmo_math_model::{Completeness, EditBuffer, HoleKind, Item};
//!
//! // `2*` misses its second operand, at index 2.
//! let mut line = EditBuffer::new();
//! line.insert_text("2*")?;
//! let holes = match line.completeness() {
//!     Completeness::Incomplete(holes) => holes,
//!     _ => Vec::new(),
//! };
//! assert_eq!(holes.len(), 1);
//! assert_eq!(holes[0].kind, HoleKind::MissingOperand);
//! assert_eq!(holes[0].position.index, 2);
//!
//! // Copying the line `x+1` into the hole adds brackets.
//! let mut other = EditBuffer::new();
//! other.insert_text("x+1")?;
//! let snapshot = other.snapshot();
//! line.insert_fragment(&snapshot, snapshot.line())?;
//! let expected: Vec<Item> = "2*(x+1)".chars().map(Item::Char).collect();
//! assert_eq!(line.items(), expected.as_slice());
//! assert_eq!(line.completeness(), Completeness::Complete);
//!
//! // Two edits changed the content, so the source version is 2.
//! assert_eq!(line.version().get(), 2);
//! # Ok::<(), kosmo_math_model::Rejected>(())
//! ```

#![forbid(unsafe_code)]

mod buffer;
mod parse;
mod structure;
pub mod version;

pub use buffer::{EditBuffer, Rejected, Snapshot};
pub use structure::{Fragment, Item, NodeId, Position, Selection, TemplateKind};

/// The most items a line holds. Every character is one item and one byte.
pub const MAX_ITEMS: usize = 4096;

/// The most bytes that one insertion of text or of a fragment holds.
pub const MAX_INSERT_BYTES: usize = 4096;

/// The deepest parse a line may have: each bracket pair and each unary minus
/// sign adds one level for what it encloses, so `((1))` and `--1` have the
/// depth 2. Exponents and chains of binary operators add none.
pub const MAX_PARSE_DEPTH: usize = 64;

/// Whether a line is complete, incomplete or invalid; every line is exactly
/// one of the three.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Completeness {
    /// The line follows the grammar.
    Complete,
    /// The line misses something; filling its holes can complete it. The
    /// holes are in reading order.
    Incomplete(Vec<Hole>),
    /// No filling of holes can complete the line. The first invalid part in
    /// reading order is given.
    Invalid(Invalid),
}

/// A place where the line misses something.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Hole {
    /// What is missing.
    pub kind: HoleKind,
    /// Where it is missing: at the start of the token after it, or at the
    /// end of the line when no token follows.
    pub position: Position,
}

/// What a hole misses.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HoleKind {
    /// The line holds no token, only spaces or nothing.
    EmptyLine,
    /// An operand is missing, as in `1/`, `+1` or `()`.
    MissingOperand,
    /// Two operands follow each other without an operator, as in `2x` or
    /// `2(x+1)`.
    MissingOperator,
    /// A left bracket is not closed; the hole is at the end of the line.
    MissingRightBracket,
}

/// The first part of a line that makes it invalid.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Invalid {
    /// Why it is invalid.
    pub kind: InvalidKind,
    /// Its items.
    pub fragment: Fragment,
}

/// Why a part of a line is invalid.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum InvalidKind {
    /// A run of digits and points that cannot form a number, such as
    /// `1.2.3` or a lone `.`.
    MalformedNumber,
    /// A right bracket without a left bracket before it.
    SurplusRightBracket,
}

/// A token of a line as the parser reads it, from
/// [`Snapshot::tokens`]. Spaces separate tokens and belong to none.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Token {
    /// What the token is.
    pub kind: TokenKind,
    /// Its items, never empty.
    pub fragment: Fragment,
}

/// What a token is.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TokenKind {
    /// A run of digits and points, such as `12`, `1.5`, `.5` or `1.`. A run
    /// that cannot form a number, such as `1.2.3`, is a number token too,
    /// which makes the line invalid.
    Number,
    /// A letter or `_`, followed by letters, digits and `_`.
    Name,
    /// One of `+`, `-`, `*`, `/` and `^`.
    Operator,
    /// `(`.
    LeftBracket,
    /// `)`.
    RightBracket,
}
