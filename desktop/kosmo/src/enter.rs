//! Entering an expression into the current line.

use kosmo_sdk::kosmo_math_model::Rejected;
use kosmo_sdk::kosmo_scratchpad::Scratchpad;

/// Why an expression was not entered whole: where the first byte that was
/// not entered lies, and why it was not.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NotEntered {
    /// The position of that byte in the expression, counted from 0. The
    /// characters before it were entered, one item each, so it is also the
    /// position in the current line after them when the line was empty.
    pub position: usize,
    /// How the current line rejected the character there, or `None` when
    /// the bytes there are not UTF-8.
    pub rejected: Option<Rejected>,
}

impl NotEntered {
    /// The name that the terminal writes for it: `OutsideCharacterSet` for
    /// a character outside the character set and for bytes that are not
    /// UTF-8, and the name of any other rejection, such as `TooDeep`.
    pub fn name(&self) -> &'static str {
        match self.rejected {
            None | Some(Rejected::Character(_)) => "OutsideCharacterSet",
            Some(Rejected::InsertTooLong) => "InsertTooLong",
            Some(Rejected::TooManyItems) => "TooManyItems",
            Some(Rejected::TooDeep) => "TooDeep",
            Some(Rejected::NotInSnapshot) => "NotInSnapshot",
            Some(Rejected::VersionExhausted) => "VersionExhausted",
            Some(_) => "Rejected",
        }
    }
}

/// Enters `expression` into the current line of `pad` one character at a
/// time through the insert operation, as the terminal does with the
/// expression of every line it submits.
///
/// Each character is an edit of its own, so the source version of the line
/// increases by one for each. Entering stops at the first character that the
/// line rejects, which changes nothing, or at the first bytes that are not
/// UTF-8: the characters before stay in the line, and the error gives the
/// position. Every character outside ASCII is outside the character set.
pub fn enter(pad: &mut Scratchpad, expression: &[u8]) -> Result<(), NotEntered> {
    let mut position = 0;
    for chunk in expression.utf8_chunks() {
        for c in chunk.valid().chars() {
            pad.insert_char(c).map_err(|rejected| NotEntered {
                position,
                rejected: Some(rejected),
            })?;
            position += c.len_utf8();
        }
        if !chunk.invalid().is_empty() {
            return Err(NotEntered {
                position,
                rejected: None,
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entering_stops_at_the_first_rejected_character_and_keeps_those_before() {
        let mut pad = Scratchpad::new();
        let not = enter(&mut pad, "x\u{b2}+1".as_bytes()).unwrap_err();
        assert_eq!(
            not,
            NotEntered {
                position: 1,
                rejected: Some(Rejected::Character('\u{b2}'))
            }
        );
        assert_eq!(not.name(), "OutsideCharacterSet");
        assert_eq!(pad.line().items().len(), 1);
        assert_eq!(pad.line().version().get(), 1);
    }

    #[test]
    fn bytes_that_are_not_utf8_are_outside_the_character_set() {
        let mut pad = Scratchpad::new();
        let not = enter(&mut pad, b"12\xff3").unwrap_err();
        assert_eq!(
            not,
            NotEntered {
                position: 2,
                rejected: None
            }
        );
        assert_eq!(not.name(), "OutsideCharacterSet");
        assert_eq!(pad.line().items().len(), 2);
    }

    #[test]
    fn other_rejections_are_named_by_their_kind() {
        let mut pad = Scratchpad::new();
        let not = enter(&mut pad, "(".repeat(70).as_bytes()).unwrap_err();
        assert_eq!(not.position, 64);
        assert_eq!(not.name(), "TooDeep");
        for (rejected, name) in [
            (Rejected::InsertTooLong, "InsertTooLong"),
            (Rejected::TooManyItems, "TooManyItems"),
            (Rejected::NotInSnapshot, "NotInSnapshot"),
            (Rejected::VersionExhausted, "VersionExhausted"),
        ] {
            let not = NotEntered {
                position: 0,
                rejected: Some(rejected),
            };
            assert_eq!(not.name(), name);
        }
    }
}
