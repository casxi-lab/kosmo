//! Test support of kosmo's framework. So far it holds the
//! [`conformance`] suite of the platform interface, which every backend
//! passes with its test hooks on, and the call [`probes`] that a crate
//! places at its entries for tests of call relations. Such a test checks
//! the record of the whole process, so it owns its test binary: it is the
//! only test in its file under `tests/`, and the calls of other tests do
//! not mix in.
//!
//! The crate depends on `kosmo-platform` with the feature `test-hooks`,
//! so only tests and test drivers depend on it. Its records use ordered
//! containers only.

#![forbid(unsafe_code)]

pub mod conformance;
pub mod probes;
