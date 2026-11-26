//! Pixel formats and buffer descriptions: the four formats in which
//! kosmo's buffers hold pixels, and the description of a buffer's size,
//! row stride and format, checked against the buffer limit when it is
//! built.
//!
//! Colors are sRGB-encoded values. A packed format is named by its
//! components from the high bits to the low bits of a little-endian word,
//! as DRM fourcc codes are; a format whose name ends in `8` is named in
//! memory byte order. The layout does not depend on the host's byte order.

/// The most pixels in a row or a column of a buffer: 8,192. The sizes of
/// the raster library are `u16`, and positions up to 2^18 pixels convert to
/// `f32` exactly, so every buffer lies within both.
pub const MAX_BUFFER_SIDE: u32 = 8_192;

/// The most bytes that a buffer holds, its stride times its height: 2^26,
/// 64 MiB. A 3840 by 2160 buffer of four bytes per pixel holds 33,177,600.
pub const MAX_BUFFER_BYTES: u32 = 1 << 26;

/// How the bytes of a pixel hold its color.
///
/// The set of formats is fixed: adding or removing one is a design change.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PixelFormat {
    /// Four bytes, R, G, B and A in memory order, with the color
    /// premultiplied by alpha. The raster library draws in this format.
    Rgba8Premul,
    /// A little-endian 32-bit word with X, R, G and B from the high bits to
    /// the low bits, so bytes B, G, R and X in memory. X is always written
    /// as `0xFF`.
    Xrgb8888,
    /// A little-endian 16-bit word with R in bits 15 to 11, G in bits 10 to
    /// 5 and B in bits 4 to 0.
    Rgb565,
    /// A little-endian 32-bit word with A, R, G and B from the high bits to
    /// the low bits, so bytes B, G, R and A in memory, with the color
    /// premultiplied by alpha. It is meant for ARGB hardware planes.
    Argb8888Premul,
}

impl PixelFormat {
    /// The bytes of one pixel: 2 for [`PixelFormat::Rgb565`], 4 for the
    /// others.
    pub const fn bytes_per_pixel(self) -> u32 {
        match self {
            PixelFormat::Rgb565 => 2,
            PixelFormat::Rgba8Premul | PixelFormat::Xrgb8888 | PixelFormat::Argb8888Premul => 4,
        }
    }
}

/// Why a buffer description was not built.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BufferError {
    /// The width or the height is 0.
    Empty,
    /// The width or the height is more than [`MAX_BUFFER_SIDE`].
    SideTooLong,
    /// The stride is less than the width times the bytes per pixel.
    StrideTooShort,
    /// The stride times the height is more than [`MAX_BUFFER_BYTES`].
    TooManyBytes,
}

/// The description of a buffer of pixels: its width and height in pixels,
/// its stride, the bytes from the start of one row to the start of the
/// next, and its format.
///
/// The first byte of the pixel (`x`, `y`) is at `y * stride + x * bpp`,
/// where `bpp` is the format's [bytes per
/// pixel](PixelFormat::bytes_per_pixel), and row 0 is at the top. Rotation
/// is not part of the description.
///
/// ```
/// use kosmo_types::{BufferDesc, BufferError, PixelFormat};
///
/// let desc = BufferDesc::new(800, 480, 1_600, PixelFormat::Rgb565).unwrap();
/// assert_eq!(desc.byte_len(), 768_000);
/// assert_eq!(
///     BufferDesc::new(800, 480, 1_599, PixelFormat::Rgb565),
///     Err(BufferError::StrideTooShort)
/// );
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BufferDesc {
    width: u32,
    height: u32,
    stride: u32,
    format: PixelFormat,
}

impl BufferDesc {
    /// The description of a buffer `width` by `height` pixels in `format`,
    /// whose rows start `stride` bytes apart.
    ///
    /// The width and the height are 1 to [`MAX_BUFFER_SIDE`], the stride
    /// is at least the width times the bytes per pixel, and the stride
    /// times the height is at most [`MAX_BUFFER_BYTES`], so it fits in a
    /// `u32`. The first of these that fails, in this order, is the error.
    pub const fn new(
        width: u32,
        height: u32,
        stride: u32,
        format: PixelFormat,
    ) -> Result<BufferDesc, BufferError> {
        if width == 0 || height == 0 {
            return Err(BufferError::Empty);
        }
        if width > MAX_BUFFER_SIDE || height > MAX_BUFFER_SIDE {
            return Err(BufferError::SideTooLong);
        }
        // At most 8,192 times 4, so the product fits.
        if stride < width * format.bytes_per_pixel() {
            return Err(BufferError::StrideTooShort);
        }
        // At most (2^32 - 1) times 2^13, so the product fits in a u64.
        if stride as u64 * height as u64 > MAX_BUFFER_BYTES as u64 {
            return Err(BufferError::TooManyBytes);
        }
        Ok(BufferDesc {
            width,
            height,
            stride,
            format,
        })
    }

    /// The width in pixels.
    pub const fn width(self) -> u32 {
        self.width
    }

    /// The height in pixels.
    pub const fn height(self) -> u32 {
        self.height
    }

    /// The bytes from the start of one row to the start of the next.
    pub const fn stride(self) -> u32 {
        self.stride
    }

    /// The format of the pixels.
    pub const fn format(self) -> PixelFormat {
        self.format
    }

    /// The bytes of the buffer: the stride times the height.
    pub const fn byte_len(self) -> u32 {
        // Checked against MAX_BUFFER_BYTES when the description was built.
        self.stride * self.height
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [PixelFormat; 4] = [
        PixelFormat::Rgba8Premul,
        PixelFormat::Xrgb8888,
        PixelFormat::Rgb565,
        PixelFormat::Argb8888Premul,
    ];

    #[test]
    fn rgb565_has_two_bytes_per_pixel_and_the_others_four() {
        let bytes = ALL.map(PixelFormat::bytes_per_pixel);
        assert_eq!(bytes, [4, 4, 2, 4]);
    }

    #[test]
    fn a_description_keeps_its_size_stride_and_format() {
        let desc = BufferDesc::new(3, 2, 16, PixelFormat::Xrgb8888).unwrap();
        assert_eq!(
            (desc.width(), desc.height(), desc.stride(), desc.format()),
            (3, 2, 16, PixelFormat::Xrgb8888)
        );
        assert_eq!(desc.byte_len(), 32);
        // A tightly packed stride is enough.
        for format in ALL {
            let stride = 7 * format.bytes_per_pixel();
            let desc = BufferDesc::new(7, 1, stride, format).unwrap();
            assert_eq!(desc.byte_len(), stride, "{format:?}");
        }
    }

    #[test]
    fn an_empty_buffer_is_rejected() {
        for (width, height) in [(0, 1), (1, 0), (0, 0)] {
            assert_eq!(
                BufferDesc::new(width, height, 4, PixelFormat::Rgba8Premul),
                Err(BufferError::Empty),
                "{width} by {height}"
            );
        }
    }

    #[test]
    fn a_side_longer_than_the_limit_is_rejected() {
        let side = MAX_BUFFER_SIDE;
        assert!(BufferDesc::new(side, 1, side * 4, PixelFormat::Rgba8Premul).is_ok());
        assert!(BufferDesc::new(1, side, 4, PixelFormat::Rgba8Premul).is_ok());
        assert_eq!(
            BufferDesc::new(side + 1, 1, (side + 1) * 4, PixelFormat::Rgba8Premul),
            Err(BufferError::SideTooLong)
        );
        assert_eq!(
            BufferDesc::new(1, side + 1, 4, PixelFormat::Rgba8Premul),
            Err(BufferError::SideTooLong)
        );
        assert_eq!(
            BufferDesc::new(u32::MAX, 1, u32::MAX, PixelFormat::Rgb565),
            Err(BufferError::SideTooLong)
        );
    }

    #[test]
    fn a_stride_shorter_than_a_row_is_rejected() {
        assert_eq!(
            BufferDesc::new(10, 1, 39, PixelFormat::Rgba8Premul),
            Err(BufferError::StrideTooShort)
        );
        assert_eq!(
            BufferDesc::new(10, 1, 19, PixelFormat::Rgb565),
            Err(BufferError::StrideTooShort)
        );
        assert!(BufferDesc::new(10, 1, 20, PixelFormat::Rgb565).is_ok());
    }

    #[test]
    fn the_bytes_are_limited_by_the_stride_times_the_height() {
        // Exactly 2^26 bytes is allowed.
        let desc = BufferDesc::new(4_096, 4_096, 16_384, PixelFormat::Rgba8Premul).unwrap();
        assert_eq!(desc.byte_len(), MAX_BUFFER_BYTES);
        // A wide stride counts, though the pixels would fit.
        assert_eq!(
            BufferDesc::new(1, 4_096, 16_385, PixelFormat::Rgb565),
            Err(BufferError::TooManyBytes)
        );
        // A stride that would overflow a u32 when multiplied.
        assert_eq!(
            BufferDesc::new(1, 2, u32::MAX, PixelFormat::Rgb565),
            Err(BufferError::TooManyBytes)
        );
    }
}
