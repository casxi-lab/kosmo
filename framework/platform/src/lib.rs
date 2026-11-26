//! The platform interface of kosmo's framework: the nine traits through
//! which the framework reaches surfaces, input, time, resources and the
//! accessibility tree, with their value types and bounds. A backend
//! implements them; the framework holds a backend as one value.
//!
//! - [`Platform`], the root, has the other ports as supertraits and adds
//!   only [`Platform::shutdown`].
//! - [`EventSource`] waits with a deadline and returns [`PlatformEvent`]s
//!   by value; [`Clock`] reads the framework clock as a `MonoTime`.
//! - [`SurfacePort`] creates and destroys surfaces, [`Presenter`] hands out
//!   a surface's buffer and presents it with its update regions, and
//!   [`Capabilities`] reports the accepted pixel formats and the output
//!   size.
//! - [`ResourceProvider`] returns the bytes of a resource from the table
//!   that the backend was built with, and [`AccessibilitySink`] takes the
//!   AccessKit tree updates of a surface.
//! - [`PlatformTestHooks`], only with the feature `test-hooks`, which is
//!   off by default, injects events and failures, advances the clock and
//!   reads back what a backend received.
//!
//! The contract that every backend keeps:
//!
//! - Nothing blocks the caller's thread longer than [`EventSource::wait`]
//!   with its deadline, no method needs a thread, and no method calls back
//!   into the caller. A backend that never blocks returns
//!   [`WaitEnd::Idle`] instead of waiting without a deadline.
//! - Events come back in the order they arose, each once. Injected events
//!   keep any focus stamp they carry.
//! - Each operation either does all it says or returns a
//!   [`PlatformError`] and changes nothing.
//! - The numbers of events per wait, of rectangles per presentation, of
//!   surfaces, of injected events waiting to be returned and of
//!   accessibility nodes per surface are bounded by [`MAX_EVENTS_PER_WAIT`],
//!   [`MAX_RECTS_PER_PRESENT`], [`MAX_SURFACES`], [`MAX_INJECTED`] and
//!   [`MAX_A11Y_NODES`]. Events beyond the first bound wait for the next
//!   wait; any other operation beyond a bound returns
//!   [`PlatformError::Exhausted`].
//! - A handle of a destroyed surface is stale: every operation on it
//!   returns [`PlatformError::StaleHandle`].
//! - Events, requests and capabilities are plain values that can be
//!   compared and recorded; they hold no host types and no callbacks.
//!
//! The crate depends only on `kosmo-types` and on AccessKit, which it
//! re-exports as [`accesskit`] so that every crate uses the same version.
//! AccessKit's own handler traits are not part of the platform interface,
//! and backends do not implement them: the first release has no action
//! requests.
//!
//! ```
//! use kosmo_platform::{PixelSize, PlatformCaps, SurfaceRequest, SurfaceRole};
//! use kosmo_types::PixelFormat;
//!
//! let caps = PlatformCaps::new(vec![PixelFormat::Xrgb8888], PixelSize::new(800, 480).unwrap());
//! let request = SurfaceRequest {
//!     role: SurfaceRole::Main,
//!     size: caps.output(),
//!     format: caps.formats()[0],
//! };
//! assert!(caps.accepts(request.format));
//! assert!(!caps.accepts(PixelFormat::Rgb565));
//! ```

#![forbid(unsafe_code)]

mod a11y;
mod error;
mod event;
#[cfg(feature = "test-hooks")]
mod hooks;
mod ports;
mod surface;

pub use accesskit;

pub use a11y::AccessibilityTree;
pub use error::PlatformError;
pub use event::{PlatformEvent, Stamp, SurfaceId, WaitEnd};
#[cfg(feature = "test-hooks")]
pub use hooks::{FaultPort, ReadBack};
#[cfg(feature = "test-hooks")]
pub use ports::PlatformTestHooks;
pub use ports::{
    AccessibilitySink, Capabilities, Clock, EventSource, Platform, Presenter, ResourceProvider,
    SurfacePort,
};
pub use surface::{
    PixelBufferMut, PixelSize, PlatformCaps, ResourceId, SurfaceRequest, SurfaceRole,
};

/// The most events that one [`EventSource::wait`] returns: 64. Later
/// events stay queued, in order, for the next wait.
pub const MAX_EVENTS_PER_WAIT: usize = 64;

/// The most update rectangles of one [`Presenter::present`]: 16, the bound
/// of a damage region of `kosmo-types`.
pub const MAX_RECTS_PER_PRESENT: usize = kosmo_types::MAX_DAMAGE_RECTS;

/// The most surfaces that exist at once: 4.
pub const MAX_SURFACES: usize = 4;

/// The most injected events that wait to be returned: 1,024.
pub const MAX_INJECTED: usize = 1_024;

/// The most accessibility nodes in the tree of one surface: 4,096, one for
/// each control of the framework's control tree, whose bound is the same.
pub const MAX_A11Y_NODES: usize = 4_096;
