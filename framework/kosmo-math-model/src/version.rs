//! Versions of the content that results are computed from.
//!
//! A result is shown only against the versions it was computed from. This
//! module holds the source version, which numbers the content of one editing
//! buffer.

/// The version of the source content of one editing buffer.
///
/// It starts at 0 and only increases: every accepted edit that changes the
/// content increases it by exactly 1, and nothing else changes it. It never
/// wraps; an edit that would increase it past `u64::MAX` is rejected.
/// Versions are compared only within the buffer that numbers them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SourceVersion(u64);

impl SourceVersion {
    /// The version as a number, as a computation request carries it.
    pub const fn get(self) -> u64 {
        self.0
    }

    /// The version after one more content change, or `None` at the maximum.
    pub(crate) const fn next(self) -> Option<SourceVersion> {
        match self.0.checked_add(1) {
            Some(value) => Some(SourceVersion(value)),
            None => None,
        }
    }

    /// A version with the given number, for tests that start near the
    /// maximum.
    #[cfg(test)]
    pub(crate) const fn at(value: u64) -> SourceVersion {
        SourceVersion(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_at_zero_and_steps_by_one() {
        let first = SourceVersion::default();
        assert_eq!(first.get(), 0);
        assert_eq!(first.next().map(SourceVersion::get), Some(1));
        assert!(first < first.next().unwrap());
    }

    #[test]
    fn has_no_version_after_the_maximum() {
        assert_eq!(SourceVersion::at(u64::MAX).next(), None);
        assert_eq!(
            SourceVersion::at(u64::MAX - 1).next(),
            Some(SourceVersion::at(u64::MAX))
        );
    }
}
