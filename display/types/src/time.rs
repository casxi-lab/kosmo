//! The logical time of input.

/// A reading of a monotonic clock in nanoseconds, the representation of
/// kalyx's `Clock::now()`.
///
/// Input events carry it as their time, and the framework's timers and
/// deadlines are computed in it. This type does not read a clock: whoever
/// holds a clock makes readings with [`MonoTime::from_nanos`]. Readings of
/// one input path never decrease, but readings from different clocks are
/// not comparable.
///
/// ```
/// use kosmo_types::MonoTime;
///
/// let now = MonoTime::from_nanos(1_000);
/// let deadline = now.saturating_add_nanos(16_000_000);
/// assert!(now < deadline);
/// assert_eq!(deadline.as_nanos(), 16_001_000);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MonoTime(u64);

impl MonoTime {
    /// The reading 0.
    pub const ZERO: MonoTime = MonoTime(0);

    /// The reading of `nanos` nanoseconds.
    pub const fn from_nanos(nanos: u64) -> MonoTime {
        MonoTime(nanos)
    }

    /// The reading in nanoseconds.
    pub const fn as_nanos(self) -> u64 {
        self.0
    }

    /// The reading `nanos` nanoseconds later, or the largest reading when
    /// that would not fit: a deadline that never comes.
    pub const fn saturating_add_nanos(self, nanos: u64) -> MonoTime {
        MonoTime(self.0.saturating_add(nanos))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readings_compare_and_add_without_overflow() {
        let early = MonoTime::from_nanos(5);
        let late = MonoTime::from_nanos(7);
        assert!(early < late);
        assert_eq!(early.saturating_add_nanos(2), late);
        assert_eq!(late.as_nanos(), 7);
        assert_eq!(
            MonoTime::from_nanos(u64::MAX - 1).saturating_add_nanos(5),
            MonoTime::from_nanos(u64::MAX)
        );
        assert_eq!(MonoTime::default(), MonoTime::ZERO);
    }
}
