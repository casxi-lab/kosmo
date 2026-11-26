//! Surfaces, their buffers, the capabilities of a backend and resource
//! identities.

use kosmo_types::{BufferDesc, MAX_BUFFER_SIDE, PixelFormat};

/// A size in whole pixels, each side 1 to `MAX_BUFFER_SIDE` of
/// `kosmo-types`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PixelSize {
    width: u32,
    height: u32,
}

impl PixelSize {
    /// The size `width` by `height`, or `None` when a side is 0 or more
    /// than `MAX_BUFFER_SIDE`.
    pub const fn new(width: u32, height: u32) -> Option<PixelSize> {
        if width == 0 || height == 0 || width > MAX_BUFFER_SIDE || height > MAX_BUFFER_SIDE {
            None
        } else {
            Some(PixelSize { width, height })
        }
    }

    /// The width in pixels.
    pub const fn width(self) -> u32 {
        self.width
    }

    /// The height in pixels.
    pub const fn height(self) -> u32 {
        self.height
    }
}

/// What a surface is for. The first release has only the main surface;
/// pop-ups and dialogs will be new roles.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SurfaceRole {
    /// The main surface of an application.
    Main,
}

/// A request for a surface: its role, its size and the format of its
/// buffer, which is one of the accepted formats of
/// [`Capabilities::caps`](crate::Capabilities::caps).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SurfaceRequest {
    /// The role.
    pub role: SurfaceRole,
    /// The size in pixels.
    pub size: PixelSize,
    /// The pixel format of the buffer.
    pub format: PixelFormat,
}

/// The buffer of a surface, borrowed from the backend until the caller
/// presents it, so nothing writes to it after presentation.
#[derive(Debug)]
pub struct PixelBufferMut<'a> {
    /// The description of the buffer: the surface's size and format.
    pub desc: BufferDesc,
    /// The bytes of the buffer, `desc.byte_len()` of them.
    pub bytes: &'a mut [u8],
}

/// What a backend supports: in the first release the accepted pixel
/// formats, in the backend's order of preference, and the output size.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PlatformCaps {
    formats: Vec<PixelFormat>,
    output: PixelSize,
}

impl PlatformCaps {
    /// The capabilities of accepting `formats`, of which a repeated format
    /// counts once, on an output of `output`.
    pub fn new(formats: Vec<PixelFormat>, output: PixelSize) -> PlatformCaps {
        let mut unique: Vec<PixelFormat> = Vec::with_capacity(formats.len());
        for format in formats {
            if !unique.contains(&format) {
                unique.push(format);
            }
        }
        PlatformCaps {
            formats: unique,
            output,
        }
    }

    /// The accepted pixel formats, the preferred first.
    pub fn formats(&self) -> &[PixelFormat] {
        &self.formats
    }

    /// Whether `format` is accepted.
    pub fn accepts(&self, format: PixelFormat) -> bool {
        self.formats.contains(&format)
    }

    /// The size of the output in pixels.
    pub fn output(&self) -> PixelSize {
        self.output
    }
}

/// The identity of a resource that
/// [`ResourceProvider::resource`](crate::ResourceProvider::resource)
/// returns: its file name in the asset manifest of the crate that owns it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ResourceId(&'static str);

impl ResourceId {
    /// The resource named `name`.
    pub const fn new(name: &'static str) -> ResourceId {
        ResourceId(name)
    }

    /// The name of the resource.
    pub const fn name(self) -> &'static str {
        self.0
    }
}
