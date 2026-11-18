//! Reading the output lines of kalyx 0.1.0 into the display model.

use std::ops::Range;

use kosmo_math_model::Snapshot;
use kosmo_math_model::display::{
    Content, DisplayItem, ErrorKind, LimitAmount, LimitExceeded, LimitId, LineKind, Unreadable,
};
use kosmo_math_model::version::Versions;

use crate::span::map_span;

/// The number of fields of a value line, a state line and an error line.
const VALUE_FIELDS: usize = 5;
const STATE_FIELDS: usize = 5;
const ERROR_FIELDS: usize = 11;

/// Reads an output line of kalyx 0.1.0 into the display item of a result.
///
/// `line` holds the bytes that `kalyx::command::serialize` wrote for a
/// request, its final line feed included. `versions` are the versions of
/// that request, and `snapshot` is the snapshot whose linear text it
/// carried. The item always carries `versions`, whatever the line holds.
///
/// The line is read as the output lines of kalyx 0.1.0 are written: UTF-8,
/// fields separated by tabs and one final line feed, with no other control
/// character; decimal numbers without a sign or leading zeros; and the
/// versions of the request echoed in the second to fourth fields.
///
/// - A value line gives its display text, which is printable ASCII, and a
///   state line the name of its state, which may be a state that kalyx
///   0.1.0 does not have.
/// - An error line gives its kind. A `ResourceLimit` also gives its limit,
///   scope, allowed value and amount; the other kinds of kalyx 0.1.0 write
///   `-` in these fields. A kind that kalyx 0.1.0 does not have, as a later
///   version writes its new kinds, is kept by its name, and these fields
///   are not read. The span is mapped into `snapshot` by [`map_span`], so
///   the error lies on the items of the line that kalyx read, never on a
///   later state of the line being edited. The message is not read: its
///   wording is not part of kalyx's interface.
/// - Any other line gives [`Content::Unreadable`] with the reason, among
///   them a line that echoes other versions and an error whose span does
///   not lie in the linear text of `snapshot`.
///
/// It never panics.
pub fn read_output_line(line: &[u8], versions: Versions, snapshot: &Snapshot) -> DisplayItem {
    let content = content_of(line, versions, snapshot).unwrap_or_else(Content::Unreadable);
    DisplayItem { versions, content }
}

/// What a line shows, or why it cannot be read.
fn content_of(line: &[u8], versions: Versions, snapshot: &Snapshot) -> Result<Content, Unreadable> {
    let line = std::str::from_utf8(line).map_err(|_| Unreadable::NotUtf8)?;
    let line = line.strip_suffix('\n').ok_or(Unreadable::Unterminated)?;
    if line
        .bytes()
        .any(|byte| byte != b'\t' && byte.is_ascii_control())
    {
        return Err(Unreadable::ControlCharacter);
    }
    let fields: Vec<&str> = line.split('\t').collect();
    let (kind, expected) = match fields.first().copied() {
        Some("value") => (LineKind::Value, VALUE_FIELDS),
        Some("state") => (LineKind::State, STATE_FIELDS),
        Some("error") => (LineKind::Error, ERROR_FIELDS),
        _ => return Err(Unreadable::UnknownKind),
    };
    if fields.len() != expected {
        return Err(Unreadable::FieldCount {
            kind,
            found: fields.len(),
            expected,
        });
    }
    let [_, source, context, settings, rest @ ..] = fields.as_slice() else {
        return Err(Unreadable::Malformed);
    };
    let echoed = [number(source)?, number(context)?, number(settings)?];
    let requested = [
        versions.source.get(),
        versions.context.get(),
        versions.settings.get(),
    ];
    if echoed != requested {
        return Err(Unreadable::OtherVersions);
    }
    match (kind, rest) {
        (LineKind::Value, &[text]) if is_display_text(text) => Ok(Content::Value(text.to_string())),
        (LineKind::State, &[name]) if is_name(name) => Ok(Content::State(name.to_string())),
        (LineKind::Error, &[kind, span, limit, scope, allowed, amount, _message]) => {
            let span = span_of(span)?;
            let kind = match kind {
                "ResourceLimit" => {
                    ErrorKind::ResourceLimit(limit_exceeded(limit, scope, allowed, amount)?)
                }
                name => {
                    let kind = fault(name)?;
                    let known = !matches!(kind, ErrorKind::Other(_));
                    if known && [limit, scope, allowed, amount] != ["-"; 4] {
                        return Err(Unreadable::Malformed);
                    }
                    kind
                }
            };
            let place = map_span(snapshot, span).map_err(|outside| Unreadable::SpanOutside {
                span: outside.span,
                len: outside.len,
            })?;
            Ok(Content::Error { kind, place })
        }
        _ => Err(Unreadable::Malformed),
    }
}

/// The kind of an error other than `ResourceLimit`, by its name.
fn fault(name: &str) -> Result<ErrorKind, Unreadable> {
    Ok(match name {
        "Cancelled" => ErrorKind::Cancelled,
        "DeadlineExceeded" => ErrorKind::DeadlineExceeded,
        "Syntax" => ErrorKind::Syntax,
        "UnknownSymbol" => ErrorKind::UnknownSymbol,
        "DivisionByZero" => ErrorKind::DivisionByZero,
        "NonIntegerExponent" => ErrorKind::NonIntegerExponent,
        "NotPolynomial" => ErrorKind::NotPolynomial,
        "Unsupported" => ErrorKind::Unsupported,
        "Internal" => ErrorKind::Internal,
        other if is_name(other) => ErrorKind::Other(other.to_string()),
        _ => return Err(Unreadable::Malformed),
    })
}

/// The limit, scope, allowed value and amount of a `ResourceLimit`.
fn limit_exceeded(
    limit: &str,
    scope: &str,
    allowed: &str,
    amount: &str,
) -> Result<LimitExceeded, Unreadable> {
    let limit = if let Some(name) = inside(limit, "Field") {
        is_field_name(name).then(|| LimitId::Field(name.to_string()))
    } else if let Some(name) = inside(limit, "Parameter") {
        is_escaped(name).then(|| LimitId::Parameter(name.to_string()))
    } else {
        None
    };
    let scope = match scope {
        "-" => None,
        name if is_name(name) => Some(name.to_string()),
        _ => return Err(Unreadable::Malformed),
    };
    let amount = if let Some(n) = inside(amount, "Used") {
        LimitAmount::Used(number(n)?)
    } else if let Some(n) = inside(amount, "Required") {
        LimitAmount::Required(number(n)?)
    } else if let Some(n) = inside(amount, "Bound") {
        LimitAmount::Bound(number(n)?)
    } else {
        return Err(Unreadable::Malformed);
    };
    Ok(LimitExceeded {
        limit: limit.ok_or(Unreadable::Malformed)?,
        scope,
        allowed: number(allowed)?,
        amount,
    })
}

/// What `field` holds in brackets after `name`, as in `Field(max_work)`.
fn inside<'f>(field: &'f str, name: &str) -> Option<&'f str> {
    field
        .strip_prefix(name)?
        .strip_prefix('(')?
        .strip_suffix(')')
}

/// A `u64` written in decimal digits, without a sign or leading zeros.
fn number(field: &str) -> Result<u64, Unreadable> {
    let digits = !field.is_empty() && field.bytes().all(|byte| byte.is_ascii_digit());
    if !digits || (field.len() > 1 && field.starts_with('0')) {
        return Err(Unreadable::Malformed);
    }
    field.parse().map_err(|_| Unreadable::Malformed)
}

/// A span, `start..end`, or `None` for `-`.
fn span_of(field: &str) -> Result<Option<Range<usize>>, Unreadable> {
    if field == "-" {
        return Ok(None);
    }
    let (start, end) = field.split_once("..").ok_or(Unreadable::Malformed)?;
    let offset = |text: &str| usize::try_from(number(text)?).map_err(|_| Unreadable::Malformed);
    Ok(Some(offset(start)?..offset(end)?))
}

/// Whether `text` is display text: printable ASCII, not empty.
fn is_display_text(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|byte| matches!(byte, b' '..=b'~'))
}

/// Whether `name` is the name of a kind, a state or a scope: ASCII letters
/// and digits, not empty.
fn is_name(name: &str) -> bool {
    !name.is_empty() && name.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

/// Whether `name` is the name of a field of the limits, such as `max_work`.
fn is_field_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

/// Whether `name` is escaped as kalyx escapes the name of a parameter:
/// every backslash starts `\\`, `\t`, `\n`, `\r` or `\x` with two lowercase
/// hexadecimal digits. The line holds no control character, which the
/// escapes stand for.
fn is_escaped(name: &str) -> bool {
    let mut bytes = name.bytes();
    while let Some(byte) = bytes.next() {
        if byte != b'\\' {
            continue;
        }
        match bytes.next() {
            Some(b'\\' | b't' | b'n' | b'r') => {}
            Some(b'x') => {
                let hex = |byte: Option<u8>| matches!(byte, Some(b'0'..=b'9' | b'a'..=b'f'));
                if !(hex(bytes.next()) && hex(bytes.next())) {
                    return false;
                }
            }
            _ => return false,
        }
    }
    true
}

#[cfg(test)]
mod tests;
