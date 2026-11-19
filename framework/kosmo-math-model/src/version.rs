//! Versions of the content that results are computed from.
//!
//! A result is shown only against the versions it was computed from. There
//! are three distinct version types: the source version numbers the content
//! of one editing buffer, the context version the context of a region and
//! the settings version the semantic settings of a settings holder. Each
//! starts at 0, only increases and never wraps, and is compared only within
//! the buffer, region or holder that numbers it. [`Versions`] holds the
//! three for one computation request and its result.

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

/// The version of the context of a region, which kosmo's documentation
/// also calls the environment version.
///
/// No operation changes the context yet, so it stays at 0, kalyx's default;
/// it still takes part in every comparison of versions. Tests build other
/// values to stand for a region whose context changed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ContextVersion(u64);

impl ContextVersion {
    /// The version with the number `value`, as the region's state holds it.
    pub const fn new(value: u64) -> ContextVersion {
        ContextVersion(value)
    }

    /// The version as a number, as a computation request carries it.
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// The version of the semantic settings of a settings holder.
///
/// The holder increases it by 1 for every accepted operation that changes a
/// semantic setting; options that only change the presentation do not
/// count. kalyx 0.1.0 has no settings, so it stays at 0 outside tests,
/// which build other values to stand for changed settings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SettingsVersion(u64);

impl SettingsVersion {
    /// The version with the number `value`, as the holder's state holds it.
    pub const fn new(value: u64) -> SettingsVersion {
        SettingsVersion(value)
    }

    /// The version as a number, as a computation request carries it.
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// The three versions that a computation request carries and that its
/// result echoes: the source version of the snapshot it was converted
/// from, and the context and settings versions it was computed under.
/// The default is version 0 of each.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Versions {
    /// The source version.
    pub source: SourceVersion,
    /// The context version.
    pub context: ContextVersion,
    /// The settings version.
    pub settings: SettingsVersion,
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
    fn the_context_and_settings_versions_keep_their_numbers() {
        assert_eq!(ContextVersion::default().get(), 0);
        assert_eq!(SettingsVersion::default().get(), 0);
        assert_eq!(ContextVersion::new(u64::MAX).get(), u64::MAX);
        assert_eq!(SettingsVersion::new(3).get(), 3);
        assert!(SettingsVersion::new(0) < SettingsVersion::new(1));
    }

    #[test]
    fn the_default_versions_are_all_zero() {
        let versions = Versions::default();
        assert_eq!(
            (
                versions.source.get(),
                versions.context.get(),
                versions.settings.get()
            ),
            (0, 0, 0)
        );
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
