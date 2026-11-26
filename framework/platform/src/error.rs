//! The errors of the platform interface.

use std::error::Error;
use std::fmt;

/// Why an operation of the platform interface did nothing.
///
/// An operation that returns an error leaves the backend as it was.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PlatformError {
    /// The backend does not support what was asked, such as a pixel format
    /// outside its capabilities.
    Unsupported,
    /// The surface was destroyed or never existed.
    StaleHandle,
    /// An argument breaks the operation's rules, such as a clock reading
    /// earlier than the clock or a first tree update without the whole
    /// tree.
    InvalidArgument,
    /// The surface's buffer is presented and not given back yet, or, for
    /// the test hooks, a fault already waits at the port.
    Busy,
    /// The operation would pass one of the bounds of the interface.
    Exhausted,
    /// The connection to the platform is gone.
    Disconnected,
    /// The resource does not exist.
    NotFound,
    /// The backend failed for a reason of its own.
    Backend,
}

impl fmt::Display for PlatformError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            PlatformError::Unsupported => "not supported by the platform",
            PlatformError::StaleHandle => "the surface does not exist",
            PlatformError::InvalidArgument => "invalid argument",
            PlatformError::Busy => "the buffer is not given back yet",
            PlatformError::Exhausted => "a bound of the platform interface is reached",
            PlatformError::Disconnected => "the platform is disconnected",
            PlatformError::NotFound => "the resource does not exist",
            PlatformError::Backend => "the backend failed",
        })
    }
}

impl Error for PlatformError {}
