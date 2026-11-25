//! The timer table: one-shot timers with absolute deadlines.

use std::collections::BTreeMap;
use std::fmt;

use kosmo_types::MonoTime;

use crate::tree::WidgetId;

/// The most timers that the timer table holds at once.
pub const TIMER_CAPACITY: usize = 64;

/// The label of a timer, which its requester chooses and receives back
/// when the timer is due.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TimerLabel(u64);

impl TimerLabel {
    /// The label `value`.
    pub const fn new(value: u64) -> TimerLabel {
        TimerLabel(value)
    }

    /// The label as a number.
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Who requested a timer: the application or a control.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Requester {
    /// The application, which receives the timer as a message.
    App,
    /// A control, which receives the timer if it still exists.
    Control(WidgetId),
}

impl fmt::Display for Requester {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Requester::App => write!(f, "app"),
            Requester::Control(id) => write!(f, "control {id}"),
        }
    }
}

/// A timer request as `update` or a control outputs it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TimerRequest {
    pub(crate) deadline: MonoTime,
    pub(crate) label: TimerLabel,
    pub(crate) requester: Requester,
}

/// The table is full: a registration beyond [`TIMER_CAPACITY`] timers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TableFull;

/// The registered timers, ordered by deadline and then by the order of
/// registration.
#[derive(Debug, Default)]
pub(crate) struct TimerTable {
    timers: BTreeMap<(MonoTime, u64), (Requester, TimerLabel)>,
    registered: u64,
}

impl TimerTable {
    /// Registers `request`, or refuses it when the table is full and
    /// changes nothing.
    pub(crate) fn register(&mut self, request: TimerRequest) -> Result<(), TableFull> {
        if self.timers.len() >= TIMER_CAPACITY {
            return Err(TableFull);
        }
        let order = self.registered;
        self.registered = order.checked_add(1).ok_or(TableFull)?;
        self.timers.insert(
            (request.deadline, order),
            (request.requester, request.label),
        );
        Ok(())
    }

    /// Removes and returns the timers whose deadline is not later than
    /// `now`, by deadline and then by registration order.
    pub(crate) fn take_due(&mut self, now: MonoTime) -> Vec<TimerRequest> {
        let mut due = Vec::new();
        while let Some(entry) = self.timers.first_entry() {
            let deadline = entry.key().0;
            if deadline > now {
                break;
            }
            let (requester, label) = entry.remove();
            due.push(TimerRequest {
                deadline,
                label,
                requester,
            });
        }
        due
    }

    /// The earliest deadline, if a timer is registered.
    pub(crate) fn next_deadline(&self) -> Option<MonoTime> {
        self.timers.first_key_value().map(|(key, _)| key.0)
    }

    /// The number of registered timers.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.timers.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(deadline: u64, label: u64) -> TimerRequest {
        TimerRequest {
            deadline: MonoTime::from_nanos(deadline),
            label: TimerLabel::new(label),
            requester: Requester::App,
        }
    }

    #[test]
    fn due_timers_come_by_deadline_then_by_registration_order() {
        let mut table = TimerTable::default();
        for (deadline, label) in [(30, 1), (10, 2), (30, 3), (10, 4), (50, 5)] {
            table.register(request(deadline, label)).unwrap();
        }
        let due: Vec<u64> = table
            .take_due(MonoTime::from_nanos(30))
            .iter()
            .map(|timer| timer.label.get())
            .collect();
        assert_eq!(due, [2, 4, 1, 3]);
        assert_eq!(table.len(), 1);
        assert_eq!(table.next_deadline(), Some(MonoTime::from_nanos(50)));
        assert!(table.take_due(MonoTime::from_nanos(49)).is_empty());
    }

    #[test]
    fn a_full_table_refuses_a_timer_and_keeps_the_others() {
        let mut table = TimerTable::default();
        for label in 0..TIMER_CAPACITY as u64 {
            table.register(request(100 + label, label)).unwrap();
        }
        assert_eq!(table.register(request(1, 999)), Err(TableFull));
        assert_eq!(table.len(), TIMER_CAPACITY);
        assert_eq!(table.next_deadline(), Some(MonoTime::from_nanos(100)));
        let due = table.take_due(MonoTime::from_nanos(u64::MAX));
        assert_eq!(due.len(), TIMER_CAPACITY);
        assert!(due.iter().all(|timer| timer.label.get() != 999));
    }
}
