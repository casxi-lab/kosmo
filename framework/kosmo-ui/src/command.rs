//! Commands, binding tables and the minimal key map.

use std::fmt;

use kosmo_types::{KeyAction, KeyEvent, KeyMeaning, ModSet, NamedKey};

/// The identity of a command: a constant name in a namespace, with whether
/// the command repeats. The framework's shared commands are in the
/// namespace `kosmo`; an application's own commands carry the
/// application's namespace.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CommandId {
    namespace: &'static str,
    name: &'static str,
    repeats: bool,
}

impl CommandId {
    /// The command `name` of `namespace`. A command that `repeats` is
    /// triggered by repeat events too; moves and deletions repeat,
    /// submitting and cancelling do not.
    pub const fn new(namespace: &'static str, name: &'static str, repeats: bool) -> CommandId {
        CommandId {
            namespace,
            name,
            repeats,
        }
    }

    /// The namespace.
    pub const fn namespace(self) -> &'static str {
        self.namespace
    }

    /// The name.
    pub const fn name(self) -> &'static str {
        self.name
    }

    /// Whether repeat events trigger the command.
    pub const fn repeats(self) -> bool {
        self.repeats
    }
}

impl fmt::Display for CommandId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.namespace, self.name)
    }
}

/// Move the cursor left.
pub const MOVE_LEFT: CommandId = CommandId::new("kosmo", "MoveLeft", true);
/// Move the cursor right.
pub const MOVE_RIGHT: CommandId = CommandId::new("kosmo", "MoveRight", true);
/// Delete backwards.
pub const DELETE_BACKWARD: CommandId = CommandId::new("kosmo", "DeleteBackward", true);
/// Submit the current line.
pub const EVALUATE_SELECTION: CommandId = CommandId::new("kosmo", "EvaluateSelection", false);
/// Cancel the current job.
pub const CANCEL_JOB: CommandId = CommandId::new("kosmo", "CancelJob", false);
/// Insert the character that the command carries.
pub const INSERT_TEXT: CommandId = CommandId::new("kosmo", "InsertText", true);

/// A command: its identity and its parameter, the character of a binding
/// for any character.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Command {
    /// The command's identity.
    pub id: CommandId,
    /// The character that a binding for any character passes on.
    pub text: Option<char>,
}

impl fmt::Display for Command {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.text {
            Some(text) => write!(f, "{} {text:?}", self.id),
            None => write!(f, "{}", self.id),
        }
    }
}

/// The key meaning that a binding names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Trigger {
    /// A named key.
    Named(NamedKey),
    /// One character.
    Char(char),
    /// Any character, which becomes the command's parameter.
    AnyChar,
}

/// The modifiers that a binding needs: Shift, Ctrl, Alt and GUI, without
/// telling left and right apart.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Mods(u8);

impl Mods {
    /// No modifier.
    pub const NONE: Mods = Mods(0);
    /// Shift.
    pub const SHIFT: Mods = Mods(1);
    /// Ctrl.
    pub const CTRL: Mods = Mods(2);
    /// Alt.
    pub const ALT: Mods = Mods(4);
    /// GUI, such as the Windows or Command key.
    pub const GUI: Mods = Mods(8);

    /// The modifiers of both sets.
    pub const fn union(self, other: Mods) -> Mods {
        Mods(self.0 | other.0)
    }

    /// The modifiers of a key event: its HID modifier byte, the low 8 bits
    /// of `mods`, with left and right folded together. The high 8 bits, the
    /// layer state, take no part.
    pub const fn of(mods: ModSet) -> Mods {
        // Bits 0 to 3 of the HID byte are the left Ctrl, Shift, Alt and GUI
        // keys, bits 4 to 7 the right ones.
        let byte = mods.hid_modifiers();
        let folded = (byte | (byte >> 4)) & 0x0f;
        let ctrl = folded & 1;
        let shift = (folded >> 1) & 1;
        let alt = (folded >> 2) & 1;
        let gui = (folded >> 3) & 1;
        Mods(shift | (ctrl << 1) | (alt << 2) | (gui << 3))
    }

    const fn without_shift(self) -> Mods {
        Mods(self.0 & !Mods::SHIFT.0)
    }
}

/// A binding: a key meaning with the modifiers it needs, and the command
/// it gives.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Binding {
    /// The key meaning.
    pub trigger: Trigger,
    /// The modifiers, which must equal those of the event; a binding of a
    /// character ignores Shift, which the display server has already
    /// applied to the character.
    pub mods: Mods,
    /// The command.
    pub command: CommandId,
}

impl Binding {
    /// The command parameter if `meaning` with `mods` matches the binding:
    /// `Some(None)` for a match without a character, `Some(Some(c))` for a
    /// binding of any character, `None` for no match.
    fn matches(&self, meaning: KeyMeaning, mods: Mods) -> Option<Option<char>> {
        let same_mods = self.mods == mods;
        let same_mods_but_shift = self.mods.without_shift() == mods.without_shift();
        match (self.trigger, meaning) {
            (Trigger::Named(named), KeyMeaning::Named(key)) if named == key && same_mods => {
                Some(None)
            }
            (Trigger::Char(c), KeyMeaning::Char(key)) if c == key && same_mods_but_shift => {
                Some(None)
            }
            (Trigger::AnyChar, KeyMeaning::Char(key)) if same_mods_but_shift => Some(Some(key)),
            _ => None,
        }
    }

    /// Whether some key meaning with some modifiers matches both bindings.
    fn overlaps(&self, other: &Binding) -> bool {
        match (self.trigger, other.trigger) {
            (Trigger::Named(a), Trigger::Named(b)) => a == b && self.mods == other.mods,
            (Trigger::Char(a), Trigger::Char(b)) => {
                a == b && self.mods.without_shift() == other.mods.without_shift()
            }
            (Trigger::Char(_) | Trigger::AnyChar, Trigger::Char(_) | Trigger::AnyChar) => {
                self.mods.without_shift() == other.mods.without_shift()
            }
            _ => false,
        }
    }
}

/// The minimal key map: the outermost binding table, after the tables of
/// the focus chain and of the application.
pub const MINIMAL_KEY_MAP: &[Binding] = &[
    Binding {
        trigger: Trigger::Named(NamedKey::LEFT),
        mods: Mods::NONE,
        command: MOVE_LEFT,
    },
    Binding {
        trigger: Trigger::Named(NamedKey::RIGHT),
        mods: Mods::NONE,
        command: MOVE_RIGHT,
    },
    Binding {
        trigger: Trigger::Named(NamedKey::DELETE),
        mods: Mods::NONE,
        command: DELETE_BACKWARD,
    },
    Binding {
        trigger: Trigger::Named(NamedKey::EXECUTE),
        mods: Mods::NONE,
        command: EVALUATE_SELECTION,
    },
    Binding {
        trigger: Trigger::Named(NamedKey::CANCEL),
        mods: Mods::NONE,
        command: CANCEL_JOB,
    },
    Binding {
        trigger: Trigger::AnyChar,
        mods: Mods::NONE,
        command: INSERT_TEXT,
    },
];

/// Resolves a key event to a command with the binding tables of `tables`,
/// from the innermost to the outermost.
///
/// The first binding whose key meaning and modifiers match gives the
/// command. A press triggers it; a repeat triggers it only if the command
/// repeats; a release, synthetic or not, and a long press trigger nothing.
/// The result is `None` when no binding matches or the action does not
/// trigger the command.
pub fn resolve<'t>(
    event: &KeyEvent,
    tables: impl IntoIterator<Item = &'t [Binding]>,
) -> Option<Command> {
    let mods = Mods::of(event.mods);
    let (binding, text) = tables.into_iter().find_map(|table| {
        table.iter().find_map(|binding| {
            binding
                .matches(event.meaning, mods)
                .map(|text| (binding, text))
        })
    })?;
    let triggers = match event.action {
        KeyAction::Press => true,
        KeyAction::Repeat => binding.command.repeats(),
        _ => false,
    };
    triggers.then_some(Command {
        id: binding.command,
        text,
    })
}

/// Two bindings of one table that map a key meaning with the same
/// modifiers to different commands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Conflict {
    /// The earlier binding.
    pub first: Binding,
    /// The later binding.
    pub second: Binding,
}

/// Checks that no key meaning with any modifiers maps to two commands in
/// `table`, as a unit test of each table does, and gives the first
/// conflict.
pub fn check_table(table: &[Binding]) -> Result<(), Conflict> {
    for (index, first) in table.iter().enumerate() {
        for second in table.iter().skip(index + 1) {
            if first.command != second.command && first.overlaps(second) {
                return Err(Conflict {
                    first: *first,
                    second: *second,
                });
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use kosmo_types::{HidUsage, MonoTime};

    fn key(meaning: KeyMeaning, mods: u16, action: KeyAction) -> KeyEvent {
        KeyEvent {
            usage: HidUsage::new(0x07, 0x04),
            meaning,
            mods: ModSet::from_bits(mods),
            action,
            time: MonoTime::ZERO,
            synthetic: false,
        }
    }

    #[test]
    fn the_minimal_key_map_has_no_conflict() {
        assert_eq!(check_table(MINIMAL_KEY_MAP), Ok(()));
    }

    #[test]
    fn a_table_that_maps_one_key_to_two_commands_is_reported() {
        let left = Binding {
            trigger: Trigger::Named(NamedKey::LEFT),
            mods: Mods::CTRL,
            command: MOVE_LEFT,
        };
        let conflicting = Binding {
            command: DELETE_BACKWARD,
            ..left
        };
        let other_mods = Binding {
            mods: Mods::NONE,
            ..conflicting
        };
        let table = [left, other_mods, conflicting];
        assert_eq!(
            check_table(&table),
            Err(Conflict {
                first: left,
                second: conflicting
            })
        );
        assert_eq!(check_table(&[left, other_mods, left]), Ok(()));

        let any = Binding {
            trigger: Trigger::AnyChar,
            mods: Mods::NONE,
            command: INSERT_TEXT,
        };
        let shifted_a = Binding {
            trigger: Trigger::Char('a'),
            mods: Mods::SHIFT,
            command: MOVE_LEFT,
        };
        assert!(check_table(&[any, shifted_a]).is_err());
    }

    #[test]
    fn modifiers_fold_left_and_right_and_ignore_the_layer_state() {
        assert_eq!(Mods::of(ModSet::from_bits(0x01)), Mods::CTRL);
        assert_eq!(Mods::of(ModSet::from_bits(0x10)), Mods::CTRL);
        assert_eq!(Mods::of(ModSet::from_bits(0x22)), Mods::SHIFT);
        assert_eq!(Mods::of(ModSet::from_bits(0x04)), Mods::ALT);
        assert_eq!(Mods::of(ModSet::from_bits(0x80)), Mods::GUI);
        assert_eq!(Mods::of(ModSet::from_bits(0xff00)), Mods::NONE);
        assert_eq!(
            Mods::of(ModSet::from_bits(0x0f)),
            Mods::SHIFT
                .union(Mods::CTRL)
                .union(Mods::ALT)
                .union(Mods::GUI)
        );
    }

    #[test]
    fn modifiers_must_equal_and_the_layer_state_takes_no_part() {
        let left = KeyMeaning::Named(NamedKey::LEFT);
        let tables = || [MINIMAL_KEY_MAP];
        assert_eq!(resolve(&key(left, 0x01, KeyAction::Press), tables()), None);
        assert_eq!(
            resolve(&key(left, 0xab00, KeyAction::Press), tables()),
            Some(Command {
                id: MOVE_LEFT,
                text: None
            })
        );
    }

    #[test]
    fn long_press_and_synthetic_releases_trigger_nothing() {
        let left = KeyMeaning::Named(NamedKey::LEFT);
        assert_eq!(
            resolve(&key(left, 0, KeyAction::LongPress), [MINIMAL_KEY_MAP]),
            None
        );
        let mut release = key(left, 0, KeyAction::Release);
        release.synthetic = true;
        assert_eq!(resolve(&release, [MINIMAL_KEY_MAP]), None);
    }

    #[test]
    fn the_first_table_with_a_match_gives_the_command() {
        const INNER: &[Binding] = &[Binding {
            trigger: Trigger::Char('x'),
            mods: Mods::NONE,
            command: CANCEL_JOB,
        }];
        let x = key(KeyMeaning::Char('x'), 0, KeyAction::Press);
        let y = key(KeyMeaning::Char('y'), 0, KeyAction::Press);
        let tables = || [INNER, MINIMAL_KEY_MAP];
        assert_eq!(resolve(&x, tables()).map(|c| c.id), Some(CANCEL_JOB));
        assert_eq!(
            resolve(&y, tables()),
            Some(Command {
                id: INSERT_TEXT,
                text: Some('y')
            })
        );
        assert_eq!(check_table(INNER), Ok(()));
    }
}
