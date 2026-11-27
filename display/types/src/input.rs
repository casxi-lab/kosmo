//! The two layers of keyboard event records: the device record that an
//! input device produces, and the key event that kosmo's display server
//! delivers.
//!
//! All of kosmo's keyboard input uses these types, and no component defines
//! its own. Decoding them from bytes and their wire format belong to the
//! display protocol. Pointer, touch and input-method events will be new
//! kinds of record body.

use crate::time::MonoTime;

/// A HID usage: the page and the usage on the page of a key, such as page
/// 0x07, the keyboard, and usage 0x04, the key A. Modifier keys are usages
/// like any other.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HidUsage {
    /// The usage page.
    pub page: u16,
    /// The usage on the page.
    pub id: u16,
}

impl HidUsage {
    /// The usage `id` of the page `page`.
    pub const fn new(page: u16, id: u16) -> HidUsage {
        HidUsage { page, id }
    }
}

/// Whether a key in a device record went down or up. A device record has no
/// repeat and no long press.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum KeyState {
    /// The key went down.
    Press,
    /// The key went up.
    Release,
}

/// What a device record holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RecordBody {
    /// A key went down or up.
    Key {
        /// The key.
        usage: HidUsage,
        /// Down or up.
        state: KeyState,
    },
    /// The end of a packet: the records of one time before it belong
    /// together.
    PacketEnd,
    /// Records were lost; the display server reads the device's state
    /// again.
    Dropped,
}

/// A device record: what an input device reports, with the time of the
/// report. It has no device identity and no sequence number.
///
/// ```
/// use kosmo_types::{HidUsage, InputRecord, KeyState, MonoTime, RecordBody};
///
/// let press = InputRecord {
///     time: MonoTime::from_nanos(1_000),
///     body: RecordBody::Key { usage: HidUsage::new(0x07, 0x04), state: KeyState::Press },
/// };
/// let end = InputRecord { time: press.time, body: RecordBody::PacketEnd };
/// assert_ne!(press, end);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct InputRecord {
    /// When the device reported it, on the clock of its input path.
    pub time: MonoTime,
    /// What it reports.
    pub body: RecordBody,
}

/// What happened to a key in a delivered key event.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum KeyAction {
    /// The key went down.
    Press,
    /// The key went up.
    Release,
    /// The key is held and repeats; only the display server produces it.
    Repeat,
    /// The key has been held long; only the display server produces it.
    LongPress,
}

/// A key with a name rather than a character: a number in a table that
/// only grows, so a number never changes its meaning. 0 is not a key.
///
/// Event scripts name each of these keys, as `named:left` to
/// `named:mod-gui`, through the table of names in the script module of
/// `kosmo-testkit`, whose tests check that it names the keys 1 to 9; a key
/// added here gets a name there in the same change.
///
/// ```
/// use kosmo_types::NamedKey;
///
/// assert_eq!(NamedKey::LEFT.number(), 1);
/// assert_eq!(NamedKey::new(9), NamedKey::MOD_GUI);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NamedKey(u16);

impl NamedKey {
    /// Left, as the left arrow key gives it on a host.
    pub const LEFT: NamedKey = NamedKey(1);
    /// Right, as the right arrow key gives it on a host.
    pub const RIGHT: NamedKey = NamedKey(2);
    /// Delete backwards, as Backspace gives it on a host.
    pub const DELETE: NamedKey = NamedKey(3);
    /// Execute, as Return or keypad Enter gives it on a host.
    pub const EXECUTE: NamedKey = NamedKey(4);
    /// The application's cancel key, as Escape gives it on a host; not the
    /// stop key.
    pub const CANCEL: NamedKey = NamedKey(5);
    /// A Shift key of a PC keyboard, left or right.
    pub const MOD_SHIFT: NamedKey = NamedKey(6);
    /// A Ctrl key of a PC keyboard, left or right.
    pub const MOD_CTRL: NamedKey = NamedKey(7);
    /// An Alt key of a PC keyboard, left or right.
    pub const MOD_ALT: NamedKey = NamedKey(8);
    /// A GUI key of a PC keyboard, left or right, such as the Windows or
    /// Command key.
    pub const MOD_GUI: NamedKey = NamedKey(9);

    /// The named key with the number `number`, which may be one that this
    /// version does not name.
    pub const fn new(number: u16) -> NamedKey {
        NamedKey(number)
    }

    /// The number of the key.
    pub const fn number(self) -> u16 {
        self.0
    }
}

/// The meaning of a key in a delivered key event: a character or a named
/// key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum KeyMeaning {
    /// A character.
    Char(char),
    /// A named key.
    Named(NamedKey),
}

/// The modifier and layer state of a delivered key event, as 16 bits.
///
/// The low 8 bits are the HID modifier byte: bit `i` means that the usage
/// 0x07:0xE0 + `i` is down. The high 8 bits hold the layer state, which the
/// display server's key maps define; this type gives them only as part of
/// the raw value.
///
/// ```
/// use kosmo_types::ModSet;
///
/// let mods = ModSet::from_bits(0x0102);
/// assert_eq!(mods.hid_modifiers(), 0x02);
/// assert_eq!(mods.bits(), 0x0102);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ModSet(u16);

impl ModSet {
    /// No modifier and no layer.
    pub const EMPTY: ModSet = ModSet(0);

    /// The state with the raw value `bits`.
    pub const fn from_bits(bits: u16) -> ModSet {
        ModSet(bits)
    }

    /// The raw value.
    pub const fn bits(self) -> u16 {
        self.0
    }

    /// The HID modifier byte, the low 8 bits.
    pub const fn hid_modifiers(self) -> u8 {
        self.0.to_le_bytes()[0]
    }
}

/// A key event as kosmo's display server delivers it: the physical key and
/// its meaning, the modifiers, what happened, and when.
///
/// A synthetic event is a release that the display server made up, such as
/// when focus leaves while a key is down; the display protocol checks that a
/// synthetic event is a release, and this type does not. The focus
/// generation, the epoch and the trace number travel beside the event, in
/// the outer layers.
///
/// ```
/// use kosmo_types::{HidUsage, KeyAction, KeyEvent, KeyMeaning, ModSet, MonoTime, NamedKey};
///
/// let left = KeyEvent {
///     usage: HidUsage::new(0x07, 0x50),
///     meaning: KeyMeaning::Named(NamedKey::LEFT),
///     mods: ModSet::EMPTY,
///     action: KeyAction::Press,
///     time: MonoTime::from_nanos(2_000),
///     synthetic: false,
/// };
/// assert_eq!(left.meaning, KeyMeaning::Named(NamedKey::LEFT));
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct KeyEvent {
    /// The physical key.
    pub usage: HidUsage,
    /// The meaning of the key under the current key map.
    pub meaning: KeyMeaning,
    /// The modifier and layer state when the event was delivered.
    pub mods: ModSet,
    /// What happened to the key.
    pub action: KeyAction,
    /// When it happened, on the clock of its input path.
    pub time: MonoTime,
    /// Whether the display server made up this release.
    pub synthetic: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_keys_have_fixed_numbers() {
        let named = [
            NamedKey::LEFT,
            NamedKey::RIGHT,
            NamedKey::DELETE,
            NamedKey::EXECUTE,
            NamedKey::CANCEL,
            NamedKey::MOD_SHIFT,
            NamedKey::MOD_CTRL,
            NamedKey::MOD_ALT,
            NamedKey::MOD_GUI,
        ];
        for (number, key) in (1..).zip(named) {
            assert_eq!(key.number(), number);
            assert_eq!(NamedKey::new(number), key);
        }
        assert_eq!(NamedKey::new(0).number(), 0);
    }

    #[test]
    fn modifier_sets_give_the_raw_value_and_the_hid_byte() {
        assert_eq!(ModSet::from_bits(0xff00).hid_modifiers(), 0);
        assert_eq!(ModSet::from_bits(0x00ff).hid_modifiers(), 0xff);
        assert_eq!(ModSet::from_bits(0xabcd).bits(), 0xabcd);
        assert_eq!(ModSet::default(), ModSet::EMPTY);
    }

    #[test]
    fn records_hold_their_fields() {
        let usage = HidUsage::new(0x07, 0xe1);
        assert_eq!((usage.page, usage.id), (0x07, 0xe1));
        let record = InputRecord {
            time: MonoTime::from_nanos(3),
            body: RecordBody::Key {
                usage,
                state: KeyState::Release,
            },
        };
        assert!(matches!(
            record.body,
            RecordBody::Key {
                state: KeyState::Release,
                ..
            }
        ));
        let event = KeyEvent {
            usage,
            meaning: KeyMeaning::Named(NamedKey::MOD_SHIFT),
            mods: ModSet::from_bits(0x02),
            action: KeyAction::Release,
            time: record.time,
            synthetic: true,
        };
        let copy = event;
        assert_eq!(copy, event);
        assert_ne!(KeyMeaning::Char('a'), KeyMeaning::Char('b'));
    }
}
