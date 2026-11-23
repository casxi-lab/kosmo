//! The SDK facade of kosmo: the framework libraries that the `kosmo`
//! program and applications build on, re-exported as modules under their
//! own names.
//!
//! The facade is unstable and is not published: its modules can change in
//! any version, and it is not on crates.io. It only re-exports, so each
//! module is documented in the crate it comes from:
//!
//! - [`kosmo_math_model`]: the editing structure of one-line formula input,
//!   the versions of results and the display model of exact results;
//! - [`kosmo_jobs`]: the job layer, which runs computations as versioned
//!   jobs, with its host stub and its controllable backend;
//! - [`kosmo_math_kalyx`]: formula conversion into the text that kalyx
//!   reads, the reading of kalyx's output lines and the kalyx runner of the
//!   job layer;
//! - [`kosmo_scratchpad`]: the Scratchpad, with its history of submissions
//!   and the reuse of their inputs and results.
//!
//! It is the only export crate of the framework so far, so crates outside
//! the `framework/` workspace reach the framework only through it. An SDK
//! interface of its own, with its contracts, is planned; until then the
//! facade adds nothing to what it re-exports.
//!
//! ```
//! use kosmo_sdk::kosmo_math_kalyx::convert;
//! use kosmo_sdk::kosmo_scratchpad::Scratchpad;
//!
//! let mut pad = Scratchpad::new();
//! pad.insert_text("1/3+1/6")?;
//! assert_eq!(convert(&pad.line().snapshot())?, "1/3+1/6");
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

#![forbid(unsafe_code)]

pub use kosmo_jobs;
pub use kosmo_math_kalyx;
pub use kosmo_math_model;
pub use kosmo_scratchpad;
