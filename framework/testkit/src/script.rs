//! Event scripts: the input of the script runner, one record per line.
//!
//! A script is UTF-8 text with a line feed after every line, the last one
//! included, at most [`MAX_LINES`] lines of at most [`MAX_LINE_BYTES`]
//! bytes each, line feed not counted. Fields are separated by one space;
//! no line is empty, starts or ends with a space or holds a control
//! character. The first line is the header and the only one; each later
//! line is a key line, a time line, a focus line or a checkpoint line:
//!
//! ```text
//! kosmo-script 1 surface <width>x<height> <format>
//! at <time> <action> <page>:<id> <meaning> mods=<bits> [synthetic] [stamp=<epoch>/<generation>]
//! time <time>
//! focus in <surface> stamp=<epoch>/<generation>
//! focus out <surface> stamp=<epoch>/<generation>
//! checkpoint
//! ```
//!
//! - The header gives the format version, 1, and the size and pixel format
//!   of the main surface: the width and height in pixels, each 1 to
//!   `MAX_BUFFER_SIDE` of `kosmo-types`, and `rgba8-premul`, `xrgb8888`,
//!   `rgb565` or `argb8888-premul`.
//! - A key line is a delivered key event for the main surface. The time is
//!   in nanoseconds; the action is `press`, `release`, `repeat` or
//!   `long-press`; the HID usage page and the usage on it are 1 to 4
//!   lowercase hexadecimal digits each; the meaning is `char:<c>` with one
//!   character, `char:U+<code>` with its code point in 4 to 6 uppercase
//!   hexadecimal digits, or `named:<name>` with the name of a named key of
//!   `kosmo-types` (`left`, `right`, `delete`, `execute`, `cancel`,
//!   `mod-shift`, `mod-ctrl`, `mod-alt` or `mod-gui`); the modifiers are
//!   the 16 bits of `ModSet` in 4 lowercase hexadecimal digits. `synthetic`
//!   marks a release that the display server made up, and the outer focus
//!   stamp, epoch and focus generation in decimal, overrides the stamp of
//!   the surface's focus. Optional fields come in this order.
//! - A time line advances the virtual clock to its time.
//! - A focus line enters or leaves focus of a surface with its stamp.
//! - A checkpoint line runs the framework until it is idle and writes a
//!   record.
//!
//! Times and the other decimal numbers have no sign and no leading zero,
//! and fit their types: `u64` for times, epochs and generations and `u32`
//! for surfaces. The time of a key line or a time line is never earlier
//! than the time of the key line or time line before it.
//!
//! [`Script::parse`] checks all of this and returns the first problem
//! with its line number, without panicking; [`Script::read`] reads a
//! script file of at most [`MAX_SCRIPT_BYTES`] bytes first, and its
//! errors of reading come back as text, with the line number 0.
//!
//! ```
//! use kosmo_testkit::script::{Line, Problem, Script};
//!
//! let script = Script::parse(b"kosmo-script 1 surface 64x8 xrgb8888\n\
//!     at 5 press 7:4 char:a mods=0000\n\
//!     checkpoint\n").unwrap();
//! assert_eq!(script.header().size.width(), 64);
//! assert!(matches!(script.lines()[0], (2, Line::Key { stamp: None, .. })));
//!
//! let error = Script::parse(b"kosmo-script 1 surface 64x8 xrgb8888\ntime 9\ntime 5\n").unwrap_err();
//! assert_eq!(error.line, 3);
//! assert!(matches!(error.problem, Problem::Earlier { .. }));
//! ```

use std::fmt;
use std::path::Path;

use kosmo_platform::{PixelSize, PlatformError, Stamp, SurfaceId};
use kosmo_types::{
    Epoch, FocusGen, HidUsage, KeyAction, KeyEvent, KeyMeaning, ModSet, MonoTime, NamedKey,
    PixelFormat,
};

use crate::pixels::format_named;

mod run;

pub use run::{Driver, MAX_ROUNDS_PER_LINE, Record};

/// The version of the format that this runner reads.
pub const VERSION: &str = "1";

/// The most lines of a script: 65,536.
pub const MAX_LINES: usize = 65_536;

/// The most bytes of one line of a script, its line feed not counted: 256.
pub const MAX_LINE_BYTES: usize = 256;

/// The most bytes of a script file: [`MAX_LINES`] lines of
/// [`MAX_LINE_BYTES`] bytes and their line feeds, 16,842,752. A longer
/// file breaks one of those bounds, so it is not read.
pub const MAX_SCRIPT_BYTES: u64 = 65_536 * 257;

const _: () = assert!(MAX_SCRIPT_BYTES == (MAX_LINES * (MAX_LINE_BYTES + 1)) as u64);

/// The names of the named keys in scripts.
const NAMED_KEYS: [(&str, NamedKey); 9] = [
    ("left", NamedKey::LEFT),
    ("right", NamedKey::RIGHT),
    ("delete", NamedKey::DELETE),
    ("execute", NamedKey::EXECUTE),
    ("cancel", NamedKey::CANCEL),
    ("mod-shift", NamedKey::MOD_SHIFT),
    ("mod-ctrl", NamedKey::MOD_CTRL),
    ("mod-alt", NamedKey::MOD_ALT),
    ("mod-gui", NamedKey::MOD_GUI),
];

/// The header of a script: the size and the pixel format of the main
/// surface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    /// The size of the main surface.
    pub size: PixelSize,
    /// The pixel format of its buffer.
    pub format: PixelFormat,
}

/// A line of a script after the header.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Line {
    /// A key line: a delivered key event for the main surface, with the
    /// outer focus stamp that the line states, if it states one.
    Key {
        /// The key event, whose time is the line's time.
        event: KeyEvent,
        /// The stamp that the line states.
        stamp: Option<Stamp>,
    },
    /// A time line: the time to advance the virtual clock to.
    Time(MonoTime),
    /// A focus line: the surface enters focus with the stamp when `enter`
    /// is true, and leaves it otherwise.
    Focus {
        /// Whether the surface enters focus or leaves it.
        enter: bool,
        /// The surface.
        surface: SurfaceId,
        /// The focus stamp.
        stamp: Stamp,
    },
    /// A checkpoint line.
    Checkpoint,
}

/// A script that [`Script::parse`] read: its header and its other lines,
/// each with its line number.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Script {
    header: Header,
    lines: Vec<(usize, Line)>,
}

impl Script {
    /// Reads a script from its bytes, as the module documentation states.
    ///
    /// # Errors
    ///
    /// The first problem of the script, with its line number.
    pub fn parse(bytes: &[u8]) -> Result<Script, ScriptError> {
        let mut header = None;
        let mut lines = Vec::new();
        let mut clock = MonoTime::ZERO;
        let mut rest = bytes;
        let mut number = 0;
        while !rest.is_empty() {
            number += 1;
            let fail = |problem| ScriptError {
                line: number,
                problem,
            };
            if number > MAX_LINES {
                return Err(fail(Problem::TooManyLines));
            }
            let (raw, next) = match rest.iter().position(|&byte| byte == b'\n') {
                Some(end) => (&rest[..end], &rest[end + 1..]),
                None => (rest, &[][..]),
            };
            if raw.len() > MAX_LINE_BYTES {
                return Err(fail(Problem::TooLong(raw.len())));
            }
            if next.is_empty() && rest.last() != Some(&b'\n') {
                return Err(fail(Problem::NoLineFeed));
            }
            let text = std::str::from_utf8(raw).map_err(|_| fail(Problem::NotUtf8))?;
            if text.chars().any(char::is_control) {
                return Err(fail(Problem::Control));
            }
            let fields: Vec<&str> = text.split(' ').collect();
            if fields.iter().any(|field| field.is_empty()) {
                return Err(fail(Problem::EmptyField));
            }
            let kind = fields.first().copied().unwrap_or_default();
            if number == 1 {
                if kind != "kosmo-script" {
                    return Err(fail(Problem::NoHeader));
                }
                header = Some(parse_header(&fields).map_err(fail)?);
            } else {
                let line = parse_line(kind, &fields).map_err(fail)?;
                if let Line::Key {
                    event: KeyEvent { time, .. },
                    ..
                }
                | Line::Time(time) = line
                {
                    if time < clock {
                        return Err(fail(Problem::Earlier { time, clock }));
                    }
                    clock = time;
                }
                lines.push((number, line));
            }
            rest = next;
        }
        let header = header.ok_or(ScriptError {
            line: 1,
            problem: Problem::NoHeader,
        })?;
        Ok(Script { header, lines })
    }

    /// Reads the script file at `path`, as [`Script::parse`] reads its
    /// bytes. Only the file's metadata is read when it has more than
    /// [`MAX_SCRIPT_BYTES`] bytes.
    ///
    /// # Errors
    ///
    /// [`Problem::Unreadable`] or [`Problem::TooLarge`], with the line
    /// number 0, or the first problem of the script.
    pub fn read(path: &Path) -> Result<Script, ScriptError> {
        let unreadable = |reason: String| ScriptError {
            line: 0,
            problem: Problem::Unreadable(format!("{}: {reason}", path.display())),
        };
        let too_large = |bytes| ScriptError {
            line: 0,
            problem: Problem::TooLarge(bytes),
        };
        let bytes = std::fs::metadata(path)
            .map_err(|error| unreadable(error.to_string()))?
            .len();
        if bytes > MAX_SCRIPT_BYTES {
            return Err(too_large(bytes));
        }
        let text = std::fs::read(path).map_err(|error| unreadable(error.to_string()))?;
        let read = u64::try_from(text.len()).unwrap_or(u64::MAX);
        if read > MAX_SCRIPT_BYTES {
            return Err(too_large(read));
        }
        Script::parse(&text)
    }

    /// The header.
    pub fn header(&self) -> Header {
        self.header
    }

    /// The lines after the header, in order, each with its line number.
    pub fn lines(&self) -> &[(usize, Line)] {
        &self.lines
    }
}

/// A problem of a script, with the number of its line, counted from 1.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScriptError {
    /// The line number, or 0 for a problem of the whole file.
    pub line: usize,
    /// The problem.
    pub problem: Problem,
}

impl fmt::Display for ScriptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.line == 0 {
            write!(f, "{}", self.problem)
        } else {
            write!(f, "line {}: {}", self.line, self.problem)
        }
    }
}

impl std::error::Error for ScriptError {}

/// What is wrong with a line of a script.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Problem {
    /// The file cannot be read: its path and why.
    Unreadable(String),
    /// The file has this many bytes, more than [`MAX_SCRIPT_BYTES`].
    TooLarge(u64),
    /// The line comes after [`MAX_LINES`] lines.
    TooManyLines,
    /// The line has this many bytes, more than [`MAX_LINE_BYTES`].
    TooLong(usize),
    /// The last line has no line feed.
    NoLineFeed,
    /// The line is not UTF-8.
    NotUtf8,
    /// The line holds a control character, such as a tab or a carriage
    /// return.
    Control,
    /// The line is empty, starts or ends with a space, or has two spaces
    /// in a row.
    EmptyField,
    /// The first line is not a header, or the script is empty.
    NoHeader,
    /// A header on a line after the first.
    SecondHeader,
    /// The header gives this version, not [`VERSION`].
    Version(String),
    /// The line starts with this word, which starts no kind of line.
    Unknown(String),
    /// The line ends before this field.
    Missing(&'static str),
    /// The line has this field after its last one.
    Extra(String),
    /// This field has a value that it cannot have.
    Invalid {
        /// The field.
        field: &'static str,
        /// Its value.
        value: String,
    },
    /// The time is earlier than the time of the key line or time line
    /// before it, or than the platform's clock.
    Earlier {
        /// The time of the line.
        time: MonoTime,
        /// The time of the line before it.
        clock: MonoTime,
    },
    /// The main surface of the header, as `<width>x<height> <format>`,
    /// does not fit the output size or has a format outside the
    /// capabilities of the platform.
    OutsideCaps(String),
    /// A port of the platform returned this error.
    Platform(PlatformError),
    /// A round of the driver failed with this error.
    Driver(String),
    /// A focus line names this surface, which the script did not create.
    NoSurface(u32),
    /// A key line states no stamp, and there is none to take: no focus line
    /// entered the main surface, and the driver reports no focus of it.
    NoStamp,
    /// The framework did not become idle within
    /// [`MAX_ROUNDS_PER_LINE`] rounds.
    NotIdle,
    /// The driver wrote this view line, which holds a control character.
    View(String),
    /// The driver wrote display text for this surface that is not lines,
    /// each followed by a line feed, without other control characters.
    Display(u32),
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Problem::Unreadable(reason) => write!(f, "the script cannot be read: {reason}"),
            Problem::TooLarge(bytes) => write!(
                f,
                "the script has {bytes} bytes, more than {MAX_SCRIPT_BYTES}"
            ),
            Problem::TooManyLines => write!(f, "the script has more than {MAX_LINES} lines"),
            Problem::TooLong(bytes) => {
                write!(f, "the line has {bytes} bytes, more than {MAX_LINE_BYTES}")
            }
            Problem::NoLineFeed => f.write_str("the line has no line feed"),
            Problem::NotUtf8 => f.write_str("the line is not UTF-8"),
            Problem::Control => f.write_str("the line holds a control character"),
            Problem::EmptyField => f.write_str("the line has an empty field"),
            Problem::NoHeader => f.write_str("the script does not start with a header"),
            Problem::SecondHeader => f.write_str("a header after the first line"),
            Problem::Version(version) => {
                write!(f, "the format version is {version}, not {VERSION}")
            }
            Problem::Unknown(word) => write!(f, "no kind of line starts with {word}"),
            Problem::Missing(field) => write!(f, "the line ends before its {field}"),
            Problem::Extra(field) => write!(f, "the field {field} after the last one"),
            Problem::Invalid { field, value } => write!(f, "the {field} {value} is not valid"),
            Problem::Earlier { time, clock } => write!(
                f,
                "the time {} is earlier than the time {} before it",
                time.as_nanos(),
                clock.as_nanos()
            ),
            Problem::OutsideCaps(surface) => write!(
                f,
                "the main surface {surface} is outside the capabilities of the platform"
            ),
            Problem::Platform(error) => write!(f, "the platform returned an error: {error}"),
            Problem::Driver(error) => write!(f, "the round failed: {error}"),
            Problem::NoSurface(surface) => {
                write!(f, "the script did not create the surface {surface}")
            }
            Problem::NoStamp => f.write_str("the key has no focus stamp to take"),
            Problem::NotIdle => write!(
                f,
                "the framework did not become idle within {MAX_ROUNDS_PER_LINE} rounds"
            ),
            Problem::View(line) => write!(f, "the view line {line:?} holds a control character"),
            Problem::Display(surface) => {
                write!(f, "the display text of the surface {surface} is not lines")
            }
        }
    }
}

/// The fields of a line after its first word, which the parsers take in
/// order.
struct Fields<'a> {
    rest: std::slice::Iter<'a, &'a str>,
}

impl<'a> Fields<'a> {
    fn new(fields: &'a [&'a str]) -> Fields<'a> {
        let mut rest = fields.iter();
        rest.next();
        Fields { rest }
    }

    /// The next field, which the line must have.
    fn next(&mut self, name: &'static str) -> Result<&'a str, Problem> {
        self.rest.next().copied().ok_or(Problem::Missing(name))
    }

    /// The next field when it is `word`.
    fn word(&mut self, word: &str) -> bool {
        let found = self.rest.as_slice().first() == Some(&word);
        if found {
            self.rest.next();
        }
        found
    }

    /// The next field when it starts with `prefix`, without the prefix.
    fn prefixed(&mut self, prefix: &str) -> Option<&'a str> {
        let value = self.rest.as_slice().first()?.strip_prefix(prefix)?;
        self.rest.next();
        Some(value)
    }

    /// The end of the line.
    fn end(mut self) -> Result<(), Problem> {
        match self.rest.next() {
            Some(extra) => Err(Problem::Extra((*extra).to_string())),
            None => Ok(()),
        }
    }
}

fn invalid(field: &'static str, value: &str) -> Problem {
    Problem::Invalid {
        field,
        value: value.to_string(),
    }
}

fn parse_header(fields: &[&str]) -> Result<Header, Problem> {
    let mut fields = Fields::new(fields);
    let version = fields.next("version")?;
    if version != VERSION {
        return Err(Problem::Version(version.to_string()));
    }
    let keyword = fields.next("surface")?;
    if keyword != "surface" {
        return Err(invalid("surface", keyword));
    }
    let size = fields.next("size")?;
    let parsed = size.split_once('x').and_then(|(width, height)| {
        PixelSize::new(
            decimal(width)?.try_into().ok()?,
            decimal(height)?.try_into().ok()?,
        )
    });
    let size = parsed.ok_or_else(|| invalid("size", size))?;
    let format = fields.next("format")?;
    let format = format_named(format).ok_or_else(|| invalid("format", format))?;
    fields.end()?;
    Ok(Header { size, format })
}

fn parse_line(kind: &str, fields: &[&str]) -> Result<Line, Problem> {
    let mut fields = Fields::new(fields);
    let line = match kind {
        "kosmo-script" => return Err(Problem::SecondHeader),
        "at" => {
            let time = time(fields.next("time")?)?;
            let action = fields.next("action")?;
            let action = match action {
                "press" => KeyAction::Press,
                "release" => KeyAction::Release,
                "repeat" => KeyAction::Repeat,
                "long-press" => KeyAction::LongPress,
                _ => return Err(invalid("action", action)),
            };
            let usage = usage(fields.next("usage")?)?;
            let meaning = meaning(fields.next("meaning")?)?;
            let mods = fields.next("mods")?;
            let bits = mods
                .strip_prefix("mods=")
                .filter(|bits| bits.len() == 4)
                .and_then(hex);
            let mods = ModSet::from_bits(bits.ok_or_else(|| invalid("mods", mods))?);
            let synthetic = fields.word("synthetic");
            let stamp = fields.prefixed("stamp=").map(stamp).transpose()?;
            let event = KeyEvent {
                usage,
                meaning,
                mods,
                action,
                time,
                synthetic,
            };
            Line::Key { event, stamp }
        }
        "time" => Line::Time(time(fields.next("time")?)?),
        "focus" => {
            let direction = fields.next("direction")?;
            let enter = match direction {
                "in" => true,
                "out" => false,
                _ => return Err(invalid("direction", direction)),
            };
            let surface = fields.next("surface")?;
            let number = decimal(surface).and_then(|number| u32::try_from(number).ok());
            let surface = SurfaceId::new(number.ok_or_else(|| invalid("surface", surface))?);
            let given = fields.next("stamp")?;
            let value = given
                .strip_prefix("stamp=")
                .ok_or_else(|| invalid("stamp", given))?;
            Line::Focus {
                enter,
                surface,
                stamp: stamp(value)?,
            }
        }
        "checkpoint" => Line::Checkpoint,
        _ => return Err(Problem::Unknown(kind.to_string())),
    };
    fields.end()?;
    Ok(line)
}

/// A decimal number without sign and leading zeros that fits a `u64`.
fn decimal(text: &str) -> Option<u64> {
    let canonical = !text.is_empty()
        && text.bytes().all(|byte| byte.is_ascii_digit())
        && (text == "0" || !text.starts_with('0'));
    canonical.then(|| text.parse().ok()).flatten()
}

/// 1 to 4 lowercase hexadecimal digits.
fn hex(text: &str) -> Option<u16> {
    let digits = (1..=4).contains(&text.len())
        && text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
    digits.then(|| u16::from_str_radix(text, 16).ok()).flatten()
}

fn time(text: &str) -> Result<MonoTime, Problem> {
    decimal(text)
        .map(MonoTime::from_nanos)
        .ok_or_else(|| invalid("time", text))
}

fn usage(text: &str) -> Result<HidUsage, Problem> {
    text.split_once(':')
        .and_then(|(page, id)| Some(HidUsage::new(hex(page)?, hex(id)?)))
        .ok_or_else(|| invalid("usage", text))
}

fn meaning(text: &str) -> Result<KeyMeaning, Problem> {
    if let Some(character) = text.strip_prefix("char:") {
        let mut chars = character.chars();
        if let (Some(one), None) = (chars.next(), chars.next()) {
            return Ok(KeyMeaning::Char(one));
        }
        let code = character.strip_prefix("U+").filter(|code| {
            (4..=6).contains(&code.len())
                && code
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'A'..=b'F').contains(&byte))
        });
        code.and_then(|code| u32::from_str_radix(code, 16).ok())
            .and_then(char::from_u32)
            .map(KeyMeaning::Char)
            .ok_or_else(|| invalid("meaning", text))
    } else if let Some(name) = text.strip_prefix("named:") {
        NAMED_KEYS
            .iter()
            .find(|&&(known, _)| known == name)
            .map(|&(_, key)| KeyMeaning::Named(key))
            .ok_or_else(|| invalid("meaning", text))
    } else {
        Err(invalid("meaning", text))
    }
}

fn stamp(text: &str) -> Result<Stamp, Problem> {
    text.split_once('/')
        .and_then(|(epoch, generation)| {
            Some(Stamp::new(
                Epoch::new(decimal(epoch)?),
                FocusGen::new(decimal(generation)?),
            ))
        })
        .ok_or_else(|| invalid("stamp", text))
}

#[cfg(test)]
mod tests;
