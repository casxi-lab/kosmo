//! The nine traits of the platform interface.

use accesskit::TreeUpdate;
use kosmo_types::{DamageRect, MonoTime};

#[cfg(feature = "test-hooks")]
use crate::a11y::AccessibilityTree;
use crate::error::PlatformError;
use crate::event::{PlatformEvent, SurfaceId, WaitEnd};
#[cfg(feature = "test-hooks")]
use crate::hooks::{FaultPort, ReadBack};
use crate::surface::{PixelBufferMut, PlatformCaps, ResourceId, SurfaceRequest};

/// The root of the platform interface: a backend is one value with all the
/// ports, and the framework's `run` is generic over it.
pub trait Platform:
    EventSource + Clock + SurfacePort + Presenter + Capabilities + ResourceProvider + AccessibilitySink
{
    /// Ends the connection to the platform and releases its surfaces.
    /// Called once after the last round, and after a wait returned
    /// [`PlatformError::Disconnected`]; it does not panic.
    fn shutdown(&mut self) -> Result<(), PlatformError>;
}

/// The source of platform events.
pub trait EventSource {
    /// Appends the events that are due to `out`, at most
    /// [`MAX_EVENTS_PER_WAIT`](crate::MAX_EVENTS_PER_WAIT) of them in the
    /// order they arose, and says how the wait ended. Without events it
    /// returns [`WaitEnd::Deadline`] when `deadline` is not later than the
    /// clock and otherwise waits until an event arrives or the deadline
    /// comes; a backend that never blocks returns [`WaitEnd::Idle`]
    /// instead. The wait does not advance the clock.
    fn wait(
        &mut self,
        deadline: Option<MonoTime>,
        out: &mut Vec<PlatformEvent>,
    ) -> Result<WaitEnd, PlatformError>;
}

/// The framework clock: read-only and monotonic.
pub trait Clock {
    /// The current reading. Readings never decrease, and no operation of
    /// the platform interface other than advancing an injected clock
    /// changes it.
    fn now(&self) -> MonoTime;
}

/// The creation and destruction of surfaces.
pub trait SurfacePort {
    /// Creates a surface with one buffer of the requested size and format.
    /// Returns [`PlatformError::Unsupported`] for a format outside the
    /// capabilities and [`PlatformError::Exhausted`] when
    /// [`MAX_SURFACES`](crate::MAX_SURFACES) surfaces exist.
    fn create_surface(&mut self, req: SurfaceRequest) -> Result<SurfaceId, PlatformError>;

    /// Destroys a surface; its identity becomes stale.
    fn destroy_surface(&mut self, s: SurfaceId) -> Result<(), PlatformError>;
}

/// The buffers of surfaces and their presentation.
pub trait Presenter {
    /// Hands out the buffer of the surface, described with the surface's
    /// size and format. A presented buffer is not handed out again until a
    /// wait returned [`PlatformEvent::Released`] for the surface; until
    /// then this returns [`PlatformError::Busy`].
    fn buffer(&mut self, s: SurfaceId) -> Result<PixelBufferMut<'_>, PlatformError>;

    /// Presents the buffer of the surface with the rectangles whose pixels
    /// changed, at most [`MAX_RECTS_PER_PRESENT`](crate::MAX_RECTS_PER_PRESENT)
    /// of them and each inside the buffer. A later wait returns
    /// [`PlatformEvent::Released`] when the buffer is given back.
    fn present(&mut self, s: SurfaceId, damage: &[DamageRect]) -> Result<(), PlatformError>;
}

/// What the backend supports.
pub trait Capabilities {
    /// The accepted pixel formats and the output size; every reading is
    /// the same.
    fn caps(&self) -> PlatformCaps;
}

/// Read-only resources such as fonts.
pub trait ResourceProvider {
    /// The bytes of the resource from the table that the backend was built
    /// with, or [`PlatformError::NotFound`].
    fn resource(&self, id: ResourceId) -> Result<&[u8], PlatformError>;
}

/// The accessibility trees of the surfaces.
pub trait AccessibilitySink {
    /// Applies an AccessKit tree update to the tree of the surface, as
    /// [`AccessibilityTree::apply`](crate::AccessibilityTree::apply)
    /// describes. The first update of a surface holds the whole tree.
    fn update(&mut self, surface: SurfaceId, update: TreeUpdate) -> Result<(), PlatformError>;
}

/// The test hooks: what a test drives and reads back. Only with the feature
/// `test-hooks`, which the framework core, controls and programs never turn
/// on, so their code cannot reach these methods.
#[cfg(feature = "test-hooks")]
pub trait PlatformTestHooks: Platform {
    /// Queues an event, with any stamp, after the queued ones, so a later
    /// wait returns it. Returns [`PlatformError::Exhausted`] when
    /// [`MAX_INJECTED`](crate::MAX_INJECTED) injected events wait to be
    /// returned.
    fn inject(&mut self, e: PlatformEvent) -> Result<(), PlatformError>;

    /// Advances the injected clock to `time`. Returns
    /// [`PlatformError::InvalidArgument`] for a time earlier than the
    /// clock.
    fn advance_to(&mut self, time: MonoTime) -> Result<(), PlatformError>;

    /// The buffer that the surface presented last, with its description
    /// and update rectangles, or `None` before its first presentation.
    fn read_back(&self, s: SurfaceId) -> Result<Option<ReadBack<'_>>, PlatformError>;

    /// Makes the next call of `port` return `error` once, without doing
    /// anything else; later calls work again. Returns
    /// [`PlatformError::Busy`] when a fault is already waiting at `port`.
    fn inject_fault(&mut self, port: FaultPort, error: PlatformError) -> Result<(), PlatformError>;

    /// The accessibility tree that the backend keeps for the surface, or
    /// `None` before its first update.
    fn a11y_tree(&self, s: SurfaceId) -> Result<Option<&AccessibilityTree>, PlatformError>;
}
