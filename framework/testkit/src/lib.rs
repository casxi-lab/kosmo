//! Test support of kosmo's framework. So far it holds the
//! [`conformance`] suite of the platform interface, which every backend
//! passes with its test hooks on.
//!
//! The crate depends on `kosmo-platform` with the feature `test-hooks`,
//! so only tests and test drivers depend on it. Its records use ordered
//! containers only.

#![forbid(unsafe_code)]

pub mod conformance;
