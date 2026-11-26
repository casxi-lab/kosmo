//! The result slot of a region.

use kosmo_math_model::version::{Judgement, Versions, judge};

/// A result with the three versions and the submission it was computed
/// for, as a job report carries it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tagged<T, S> {
    /// The result.
    pub value: T,
    /// The versions that the result was computed from.
    pub versions: Versions,
    /// The submission that the result belongs to.
    pub submission: S,
}

/// What a delivery did with a result: its judgement, and the result itself,
/// for the caller to write into the record of its submission or to count
/// toward the region's discard count.
#[must_use = "a delivered result goes into the record of its submission or the discard count"]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Delivered<T, S> {
    /// How the result relates to the region. Only a current result was
    /// written into the slot.
    pub judgement: Judgement,
    /// The result, unchanged.
    pub result: Tagged<T, S>,
}

/// The display value of a slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shown<'a, T> {
    /// The slot holds no result.
    Empty,
    /// The slot's result is current for the region.
    Current(&'a T),
    /// The slot's result was current once, but the region's versions or its
    /// latest submission have changed since.
    Stale(&'a T),
}

/// The current result slot of a region: at most one result, with its
/// versions and submission.
///
/// The slot is part of the application model, beside the holder of the
/// region, which keeps the region's current versions and its latest
/// accepted submission and passes them in. It takes no reference to an
/// editing structure or a cursor, so writing a result never changes what
/// is being edited. A result is judged with
/// `kosmo_math_model::version::judge`, the same function as the job
/// layer's, and only a current result is written:
///
/// ```
/// use kosmo_math_model::EditBuffer;
/// use kosmo_math_model::version::{Judgement, Versions};
/// use kosmo_ui::{ResultSlot, Shown, Tagged};
///
/// let mut line = EditBuffer::new();
/// line.insert_text("x")?;
/// let versions = Versions { source: line.version(), ..Versions::default() };
/// let mut slot: ResultSlot<&str, u64> = ResultSlot::new();
///
/// // Submission 1 is the latest, so its result is current and written.
/// let result = Tagged { value: "1", versions, submission: 1 };
/// let delivered = slot.deliver(result, versions, Some(&1));
/// assert_eq!(delivered.judgement, Judgement::Current);
/// assert_eq!(slot.shown(versions, Some(&1)), Shown::Current(&"1"));
///
/// // After an edit the result is shown as stale until a current one comes.
/// line.insert_char('+')?;
/// let edited = Versions { source: line.version(), ..versions };
/// assert_eq!(slot.shown(edited, Some(&1)), Shown::Stale(&"1"));
/// # Ok::<(), kosmo_math_model::Rejected>(())
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResultSlot<T, S> {
    held: Option<Tagged<T, S>>,
}

impl<T, S> Default for ResultSlot<T, S> {
    fn default() -> Self {
        ResultSlot { held: None }
    }
}

impl<T: Clone, S: Clone + PartialEq> ResultSlot<T, S> {
    /// An empty slot.
    pub const fn new() -> ResultSlot<T, S> {
        ResultSlot { held: None }
    }

    /// Delivers `result`, judged against the region's `current` versions
    /// and its `latest` accepted submission. A current result is written
    /// into the slot, replacing the one it held; any other result leaves the
    /// slot as it was. The result comes back either way.
    #[must_use = "a delivered result goes into the record of its submission or the discard count"]
    pub fn deliver(
        &mut self,
        result: Tagged<T, S>,
        current: Versions,
        latest: Option<&S>,
    ) -> Delivered<T, S> {
        let judgement = judge(result.versions, &result.submission, current, latest);
        if judgement == Judgement::Current {
            self.held = Some(result.clone());
        }
        Delivered { judgement, result }
    }

    /// The display value against the region's `current` versions and its
    /// `latest` accepted submission: empty, the held result as current, or
    /// the held result as stale.
    pub fn shown(&self, current: Versions, latest: Option<&S>) -> Shown<'_, T> {
        match &self.held {
            None => Shown::Empty,
            Some(held) => match judge(held.versions, &held.submission, current, latest) {
                Judgement::Current => Shown::Current(&held.value),
                _ => Shown::Stale(&held.value),
            },
        }
    }
}
