//! Versions of the content that results are computed from.
//!
//! A result is shown only against the versions it was computed from. There
//! are three distinct version types: the source version numbers the content
//! of one editing buffer, the context version the context of a region and
//! the settings version the semantic settings of a settings holder. Each
//! starts at 0, only increases and never wraps, and is compared only within
//! the buffer, region or holder that numbers it. [`Versions`] holds the
//! three for one computation request and its result.
//!
//! [`judge`] decides whether a result is current: the job layer and the
//! result slot of a region call the same function. A region records the
//! latest submission that the job layer accepted, and every result carries
//! the identity of its submission with its three versions:
//!
//! ```
//! use kosmo_math_model::EditBuffer;
//! use kosmo_math_model::version::{judge, Judgement, Versions};
//!
//! let mut line = EditBuffer::new();
//! line.insert_text("1/3")?;
//! let first = Versions { source: line.version(), ..Versions::default() };
//!
//! // Submission 1 is the latest, and nothing changed since.
//! assert_eq!(judge(first, &1, first, Some(&1)), Judgement::Current);
//!
//! // Submission 2 of the same versions became the latest.
//! assert_eq!(judge(first, &1, first, Some(&2)), Judgement::Superseded);
//!
//! // An edit changed the source after submission 1.
//! line.insert_char('+')?;
//! let now = Versions { source: line.version(), ..Versions::default() };
//! assert_eq!(judge(first, &1, now, Some(&1)), Judgement::Stale);
//! # Ok::<(), kosmo_math_model::Rejected>(())
//! ```

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

/// How a result relates to its region, as [`judge`] decides it.
///
/// Only a current result is written into the region's current result slot.
/// Superseded and stale results go into the record of their own
/// submission, and the first release handles the two the same way.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Judgement {
    /// All three versions equal the region's current versions, and the
    /// result belongs to the latest submission.
    Current,
    /// All three versions equal the region's current versions, but the
    /// result belongs to another, earlier submission.
    Superseded,
    /// At least one of the three versions differs from the region's
    /// current versions, whatever the submission.
    Stale,
}

/// Judges a result against the state of its region.
///
/// `result` and `submission` are the versions and the submission identity
/// that the result carries; `current` and `latest` are the region's current
/// versions and the latest submission that the job layer accepted for it,
/// `None` before the first. The function is generic over the submission
/// identity: the job layer identifies a submission by its job identity.
///
/// A result is [`Judgement::Current`] when all three versions equal
/// `current` and it belongs to `latest`, [`Judgement::Superseded`] when the
/// versions are equal but it belongs to another submission, and
/// [`Judgement::Stale`] when any version differs. The region keeps its
/// latest submission when a later one is rejected, and does not go back to
/// an earlier one when the latest is cancelled or fails; the function only
/// compares what it is given.
pub fn judge<S: PartialEq>(
    result: Versions,
    submission: &S,
    current: Versions,
    latest: Option<&S>,
) -> Judgement {
    if result != current {
        Judgement::Stale
    } else if latest == Some(submission) {
        Judgement::Current
    } else {
        Judgement::Superseded
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Versions with the given numbers of source, context and settings.
    fn versions(source: u64, context: u64, settings: u64) -> Versions {
        Versions {
            source: SourceVersion::at(source),
            context: ContextVersion::new(context),
            settings: SettingsVersion::new(settings),
        }
    }

    #[test]
    fn a_result_of_the_latest_submission_at_the_current_versions_is_current() {
        let now = versions(4, 0, 2);
        assert_eq!(judge(now, &7_u64, now, Some(&7)), Judgement::Current);
        assert_eq!(
            judge(versions(0, 0, 0), &"a", versions(0, 0, 0), Some(&"a")),
            Judgement::Current
        );
    }

    #[test]
    fn a_result_of_another_submission_at_the_current_versions_is_superseded() {
        let now = versions(4, 0, 2);
        assert_eq!(judge(now, &6_u64, now, Some(&7)), Judgement::Superseded);
        assert_eq!(judge(now, &8_u64, now, Some(&7)), Judgement::Superseded);
        assert_eq!(judge(now, &6_u64, now, None), Judgement::Superseded);
    }

    #[test]
    fn a_result_with_any_other_version_is_stale_whatever_its_submission() {
        let now = versions(4, 1, 2);
        let others = [
            versions(3, 1, 2),
            versions(5, 1, 2),
            versions(4, 0, 2),
            versions(4, 2, 2),
            versions(4, 1, 1),
            versions(4, 1, 3),
            versions(3, 0, 1),
        ];
        for result in others {
            assert_eq!(judge(result, &7_u64, now, Some(&7)), Judgement::Stale);
            assert_eq!(judge(result, &6_u64, now, Some(&7)), Judgement::Stale);
            assert_eq!(judge(result, &6_u64, now, None), Judgement::Stale);
        }
    }

    #[test]
    fn the_judgement_compares_submissions_only_for_equality() {
        #[derive(PartialEq)]
        struct Submission(&'static str);
        let now = versions(1, 0, 0);
        let latest = Submission("second");
        assert_eq!(
            judge(now, &Submission("second"), now, Some(&latest)),
            Judgement::Current
        );
        assert_eq!(
            judge(now, &Submission("first"), now, Some(&latest)),
            Judgement::Superseded
        );
    }

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
