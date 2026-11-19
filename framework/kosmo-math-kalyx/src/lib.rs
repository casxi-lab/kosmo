//! Formula conversion between kosmo's editing structure of formulas and
//! kalyx 0.1.0, the computer algebra library, and the reading of kalyx's
//! output lines into the display model of results. This is the only crate
//! of kosmo that depends on kalyx directly.
//!
//! - [`convert`] writes a complete line as the linear text that the grammar
//!   of kalyx 0.1.0 reads. For an incomplete or invalid line it writes no
//!   text and gives the holes or the invalid part instead, so nothing is
//!   computed from it.
//! - [`map_span`] maps the span of a kalyx error, a range of bytes of that
//!   text, back to the items of the line, to a position between them or to
//!   the whole line.
//! - [`read_back`] reads the display text of a kalyx value back into a
//!   separate line, from which a copy can be inserted into the line being
//!   edited, or says why the value cannot be reused.
//! - [`read_output_line`] reads an output line of kalyx into the display
//!   item of a result, with the versions of its request and its error
//!   placed in the submitted snapshot.
//!
//! ```
//! use kosmo_math_kalyx::{Place, convert, map_span};
//! use kosmo_math_model::{EditBuffer, Fragment};
//!
//! let mut line = EditBuffer::new();
//! line.insert_text(".5+1./0")?;
//! let snapshot = line.snapshot();
//! assert_eq!(convert(&snapshot)?, "0.5+1/0");
//!
//! // kalyx 0.1.0 reports the division by zero at the bytes 4..7 of the
//! // text, `1/0`, which come from the items 3..7 of the line, `1./0`.
//! let division = Fragment::new(snapshot.root(), 3..7).ok_or("a forward range")?;
//! assert_eq!(map_span(&snapshot, Some(4..7))?, Place::Fragment(division));
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! The output line of that error, read for the request of the snapshot,
//! is shown on those items:
//!
//! ```
//! use kosmo_math_kalyx::read_output_line;
//! use kosmo_math_model::EditBuffer;
//! use kosmo_math_model::display::Freshness;
//! use kosmo_math_model::version::Versions;
//!
//! let mut line = EditBuffer::new();
//! line.insert_text(".5+1./0")?;
//! let snapshot = line.snapshot();
//! let versions = Versions {
//!     source: snapshot.version(),
//!     ..Versions::default()
//! };
//! let output = b"error\t1\t0\t0\tDivisionByZero\t4..7\t-\t-\t-\t-\tdivision by zero\n";
//! let item = read_output_line(output, versions, &snapshot);
//! assert_eq!(
//!     item.render(Freshness::Current),
//!     "error (source 1, settings 0): DivisionByZero at items 3..7"
//! );
//! # Ok::<(), kosmo_math_model::Rejected>(())
//! ```

#![forbid(unsafe_code)]

mod conversion;
mod output;
mod read_back;
mod span;

pub use conversion::{NotConverted, convert};
pub use kosmo_math_model::Place;
pub use output::read_output_line;
pub use read_back::{NotReusable, read_back};
pub use span::{SpanOutside, map_span};
