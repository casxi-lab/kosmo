//! The real clock of the program.

use std::time::Instant;

use kosmo_sdk::kosmo_jobs::JobClock;

/// The monotonic clock of the host: nanoseconds since the clock was built.
/// A fork reads the same clock from the same origin.
pub(crate) struct HostClock {
    origin: Instant,
}

impl HostClock {
    /// A clock that reads 0 now.
    pub(crate) fn new() -> HostClock {
        HostClock {
            origin: Instant::now(),
        }
    }
}

impl JobClock for HostClock {
    fn now(&mut self) -> u64 {
        // Saturates after about 584 years.
        u64::try_from(self.origin.elapsed().as_nanos()).unwrap_or(u64::MAX)
    }

    fn fork(&self) -> Box<dyn JobClock> {
        Box::new(HostClock {
            origin: self.origin,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readings_never_decrease_and_a_fork_reads_the_same_clock() {
        let mut clock = HostClock::new();
        let first = clock.now();
        let mut fork = clock.fork();
        let forked = fork.now();
        let last = clock.now();
        assert!(first <= forked && forked <= last, "{first} {forked} {last}");
    }
}
