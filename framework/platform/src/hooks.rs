//! The value types of the test hooks.

use kosmo_types::{BufferDesc, DamageRect};

/// What a surface presented last: the description and bytes of its buffer
/// at presentation, and the update rectangles of the presentation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReadBack<'a> {
    /// The description of the buffer.
    pub desc: BufferDesc,
    /// The bytes of the buffer, `desc.byte_len()` of them.
    pub bytes: &'a [u8],
    /// The update rectangles.
    pub damage: &'a [DamageRect],
}

/// A port method at which
/// [`PlatformTestHooks::inject_fault`](crate::PlatformTestHooks::inject_fault)
/// makes the next call fail.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FaultPort {
    /// [`EventSource::wait`](crate::EventSource::wait).
    Wait,
    /// [`SurfacePort::create_surface`](crate::SurfacePort::create_surface).
    CreateSurface,
    /// [`SurfacePort::destroy_surface`](crate::SurfacePort::destroy_surface).
    DestroySurface,
    /// [`Presenter::buffer`](crate::Presenter::buffer).
    Buffer,
    /// [`Presenter::present`](crate::Presenter::present).
    Present,
    /// [`ResourceProvider::resource`](crate::ResourceProvider::resource).
    Resource,
    /// [`AccessibilitySink::update`](crate::AccessibilitySink::update).
    Update,
    /// [`Platform::shutdown`](crate::Platform::shutdown).
    Shutdown,
}
