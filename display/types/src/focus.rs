//! The focus generation and the epoch, which mark the key events of one
//! focus.
//!
//! Their values come from the display server; the framework only compares
//! them with the values of the latest focus in and drops a key event whose
//! generation or epoch differs. This crate only holds them: it neither makes
//! nor increases one.

/// A focus generation: which focus a key event was delivered for.
///
/// Any `u64` is a generation. The display server makes the values and
/// decides how they grow and what happens when they run out.
///
/// ```
/// use kosmo_types::FocusGen;
///
/// let first = FocusGen::new(0);
/// assert_eq!(FocusGen::new(first.get()), first);
/// assert_ne!(FocusGen::new(1), first);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FocusGen(u64);

impl FocusGen {
    /// The generation `value`.
    pub const fn new(value: u64) -> FocusGen {
        FocusGen(value)
    }

    /// The value of the generation.
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// An epoch: which instance of the display server a key event comes from.
///
/// 0 is no epoch: whoever starts a display server gives it an epoch of at
/// least 1, and this type does not check it.
///
/// ```
/// use kosmo_types::Epoch;
///
/// let epoch = Epoch::new(1);
/// assert_eq!(epoch.get(), 1);
/// assert_ne!(Epoch::new(2), epoch);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Epoch(u64);

impl Epoch {
    /// The epoch `value`, which is at least 1.
    pub const fn new(value: u64) -> Epoch {
        Epoch(value)
    }

    /// The value of the epoch.
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generations_and_epochs_keep_any_value_and_compare() {
        for value in [0, 1, u64::MAX - 2, u64::MAX] {
            assert_eq!(FocusGen::new(value).get(), value);
            assert_eq!(Epoch::new(value).get(), value);
        }
        assert!(FocusGen::new(u64::MAX - 1) < FocusGen::new(u64::MAX));
        assert!(Epoch::new(1) < Epoch::new(2));
        assert_ne!(FocusGen::new(3), FocusGen::new(4));
    }
}
