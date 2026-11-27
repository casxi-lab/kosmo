//! Call probes: a test of call relations checks that what it runs reaches
//! a fixed list of public entries, without coverage tools.
//!
//! A crate with probes has the feature `call-probes`, off by default, which
//! turns on an optional dependency on this crate and nothing else:
//!
//! ```toml
//! [features]
//! call-probes = ["dep:kosmo-testkit"]
//!
//! [dependencies]
//! kosmo-testkit = { workspace = true, optional = true }
//! ```
//!
//! Each probe is a statement behind that feature, at the start of the
//! entry, and names the crate and the entry:
//!
//! ```ignore
//! #[cfg(feature = "call-probes")]
//! kosmo_testkit::probe!("kosmo-ui::run_round");
//! ```
//!
//! Without the feature the compiler removes the statement before it
//! resolves the macro's path, and the crate does not depend on this one, so
//! ordinary builds and tests compile no probe code. The feature is turned
//! on only from the command line, as `--features <crate>/call-probes`: the
//! layout step of `cargo xtask check` rejects a manifest whose default
//! features or dependency declarations turn it on, and a dependency on this
//! crate other than a dev-dependency or the optional one above. The clippy
//! and test steps of the check cover the `framework/` workspace once more
//! with the call probes of every crate that has them and the test hooks of
//! `kosmo-backend-headless`, as `PROBE_PASSES` in `xtask/src/check.rs`
//! lists, and so lint the probe code and run the tests of call relations.
//!
//! This crate's own probe, at the entry of the script runner, is behind its
//! feature `call-probes`, which turns on nothing, and is written as
//! `crate::probe!`. A crate without probes whose tests check call relations
//! may have a feature `call-probes` that turns on the `call-probes` of its
//! dev-dependencies, as `kosmo-backend-headless` does for
//! `tests/call_relations.rs`. Only features named `call-probes` may turn on
//! call probes, so all of them come from the command line.
//!
//! The record holds each entry once, in the order of their names, for the
//! whole process, so the calls of every test in a test binary mix in it. A
//! test of call relations therefore owns its test binary: it is the only
//! test in its file under `tests/`. It runs what it covers and then calls
//! [`check`] with its entries:
//!
//! ```
//! kosmo_testkit::probe!("kosmo-example::open");
//! assert_eq!(kosmo_testkit::probes::check(&["kosmo-example::open"]), Ok(()));
//! ```

use std::collections::BTreeSet;
use std::fmt;
use std::sync::{Mutex, PoisonError};

/// The entries recorded in this process.
static RECORDED: Mutex<BTreeSet<&'static str>> = Mutex::new(BTreeSet::new());

/// Records a call of the entry named by the literal, `"<crate>::<entry>"`.
#[macro_export]
macro_rules! probe {
    ($entry:literal) => {
        $crate::probes::record($entry)
    };
}

/// Records a call of `entry`, as [`probe!`](crate::probe) does.
pub fn record(entry: &'static str) {
    RECORDED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(entry);
}

/// The entries recorded so far in this process, in order.
pub fn recorded() -> Vec<&'static str> {
    RECORDED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .iter()
        .copied()
        .collect()
}

/// Checks that every entry of `entries` was recorded in this process.
///
/// # Errors
///
/// [`Missing`] lists the entries of `entries` that were not recorded, in
/// the order of `entries`.
pub fn check(entries: &[&str]) -> Result<(), Missing> {
    let recorded = RECORDED.lock().unwrap_or_else(PoisonError::into_inner);
    let missing: Vec<String> = entries
        .iter()
        .filter(|entry| !recorded.contains(**entry))
        .map(|entry| (*entry).to_string())
        .collect();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(Missing { entries: missing })
    }
}

/// The entries of a test of call relations that no probe recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Missing {
    /// The entries, in the order in which the test listed them.
    pub entries: Vec<String>,
}

impl fmt::Display for Missing {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "no probe recorded {}", self.entries.join(", "))
    }
}

impl std::error::Error for Missing {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_recorded_entry_passes() {
        crate::probe!("kosmo-testkit::recorded");
        assert_eq!(check(&["kosmo-testkit::recorded"]), Ok(()));
        assert!(recorded().contains(&"kosmo-testkit::recorded"));
    }

    #[test]
    fn a_missing_entry_is_listed() {
        record("kosmo-testkit::reached");
        let missing = check(&[
            "kosmo-testkit::never",
            "kosmo-testkit::reached",
            "kosmo-testkit::also_never",
        ])
        .unwrap_err();
        assert_eq!(
            missing.entries,
            ["kosmo-testkit::never", "kosmo-testkit::also_never"]
        );
        assert_eq!(
            missing.to_string(),
            "no probe recorded kosmo-testkit::never, kosmo-testkit::also_never"
        );
    }
}
