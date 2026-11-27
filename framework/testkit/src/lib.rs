//! Test support of kosmo's framework. So far it holds the
//! [`conformance`] suite of the platform interface, which every backend
//! passes with its test hooks on, the [`script`] runner, which replays
//! event scripts through a driver of the framework and writes records at
//! their checkpoints, the digests of presented buffers in [`pixels`], and
//! the call [`probes`] that a crate places at its entries for tests of
//! call relations. Such a test checks the record of the whole
//! process, so it owns its test binary: it is the only test in its file
//! under `tests/`, and the calls of other tests do not mix in. The rules of
//! call probes, where crates place them, how the check keeps them out of
//! ordinary builds and what their pass costs in CI are in the section
//! [Call probes](https://github.com/casxi-lab/kosmo/blob/main/CONTRIBUTING.md#call-probes)
//! of `CONTRIBUTING.md` at the repository root.
//!
//! The crate depends on `kosmo-platform` with the feature `test-hooks`,
//! so only tests and test drivers depend on it. Its records use ordered
//! containers only. It is a boundary crate for files: it reads script
//! files with `std::fs` and converts the errors where it reads them. It
//! depends neither on the framework core nor on `kosmo-render`, so the
//! crates with call probes can depend on it; the records and display lists
//! of a run come from the driver.

#![forbid(unsafe_code)]

pub mod conformance;
pub mod pixels;
pub mod probes;
pub mod script;
