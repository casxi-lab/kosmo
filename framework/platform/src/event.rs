//! Surface identities, focus stamps and the events that a wait returns.

use kosmo_types::{Epoch, FocusGen, KeyEvent};

use crate::surface::PixelSize;

/// The identity of a surface. A backend gives each surface that it creates
/// an identity that no other surface of the backend has had.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SurfaceId(u32);

impl SurfaceId {
    /// The surface `value`.
    pub const fn new(value: u32) -> SurfaceId {
        SurfaceId(value)
    }

    /// The surface as a number.
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// The focus stamp that travels beside a key event and a focus change: the
/// epoch of the display server and the focus generation of the surface.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Stamp {
    /// The epoch.
    pub epoch: Epoch,
    /// The focus generation.
    pub generation: FocusGen,
}

impl Stamp {
    /// The stamp of `epoch` and `generation`.
    pub const fn new(epoch: Epoch, generation: FocusGen) -> Stamp {
        Stamp { epoch, generation }
    }
}

/// An event that [`EventSource::wait`](crate::EventSource::wait) returns.
///
/// Frame events and presentation feedback will be new kinds. Lifecycle,
/// clipboard, memory pressure and pointer events are not returned, and
/// events carry no trace number.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlatformEvent {
    /// A delivered key event for a surface, with its focus stamp.
    Key {
        /// The surface.
        surface: SurfaceId,
        /// The focus stamp.
        stamp: Stamp,
        /// The key event.
        event: KeyEvent,
    },
    /// The surface gained focus with this stamp.
    FocusIn {
        /// The surface.
        surface: SurfaceId,
        /// The focus stamp.
        stamp: Stamp,
    },
    /// The surface lost the focus that it gained with this stamp.
    FocusOut {
        /// The surface.
        surface: SurfaceId,
        /// The focus stamp.
        stamp: Stamp,
    },
    /// The surface has this size in pixels.
    Configure {
        /// The surface.
        surface: SurfaceId,
        /// Its size.
        size: PixelSize,
    },
    /// The buffer that the surface presented last is given back, so
    /// [`Presenter::buffer`](crate::Presenter::buffer) hands it out again.
    Released {
        /// The surface.
        surface: SurfaceId,
    },
}

/// How a wait ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WaitEnd {
    /// It returned at least one event.
    Events,
    /// It returned no event, and the deadline is not later than the clock.
    Deadline,
    /// It returned no event and has nothing to wait for: the deadline is
    /// later than the clock, or there is none. Only a backend that never
    /// blocks returns it.
    Idle,
}
