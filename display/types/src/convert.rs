//! The conversion of pixels between buffers: the one function with which
//! the framework's rendering and the display server write pixels of one
//! format into a buffer of another.

use crate::damage::DamageRect;
use crate::pixel::{BufferDesc, PixelFormat};

/// Why [`convert`] wrote nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ConvertError {
    /// The source bytes are not as many as its description's
    /// [`byte_len`](BufferDesc::byte_len).
    SourceLength,
    /// The target bytes are not as many as its description's
    /// [`byte_len`](BufferDesc::byte_len).
    TargetLength,
    /// The rectangle does not lie inside the source.
    OutsideSource,
    /// The rectangle, moved to its place in the target, does not lie
    /// inside the target.
    OutsideTarget,
}

/// Writes the pixels of `rect` in the source, described by `source` and
/// held in `source_bytes`, into the target, described by `target` and held
/// in `target_bytes`, with the top-left pixel of `rect` at (`x`, `y`) in
/// the target, converted to the target's format. Every other byte of the
/// target stays as it was.
///
/// The function never allocates, never dithers and uses integer arithmetic
/// only:
///
/// - within one format it copies the bytes of each row of the rectangle;
/// - between the two premultiplied formats it only reorders bytes;
/// - from a premultiplied to an opaque format it takes the premultiplied
///   components as they are, and from an opaque to a premultiplied format it
///   sets alpha to 255;
/// - an 8-bit component `c` becomes 5 bits as `(c * 31 + 127) / 255` and 6
///   bits as `(c * 63 + 127) / 255`, and a 5-bit or 6-bit component `v`
///   becomes 8 bits as `(v * 255 + m / 2) / m`, where `m` is 31 or 63;
/// - converting from another format into [`PixelFormat::Xrgb8888`] writes X
///   as `0xFF`; within `Xrgb8888` the bytes, X included, are copied as they
///   are.
///
/// It returns an error and writes nothing when a byte slice is not as long
/// as its description says, or when the rectangle does not fit in the
/// source or, at (`x`, `y`), in the target.
///
/// ```
/// use kosmo_types::{BufferDesc, DamageRect, PixelFormat, convert};
///
/// // One opaque orange pixel, R 0xF0, G 0x80, B 0x10.
/// let source = BufferDesc::new(1, 1, 4, PixelFormat::Rgba8Premul).unwrap();
/// let target = BufferDesc::new(2, 1, 4, PixelFormat::Rgb565).unwrap();
/// let mut pixels = [0; 4];
/// let rect = DamageRect::new(0, 0, 1, 1).unwrap();
/// convert(source, &[0xF0, 0x80, 0x10, 0xFF], rect, target, &mut pixels, 1, 0).unwrap();
/// // R 29, G 32, B 2 in a little-endian word, in the second pixel.
/// assert_eq!(pixels, [0, 0, 0x02, 0xEC]);
/// ```
pub fn convert(
    source: BufferDesc,
    source_bytes: &[u8],
    rect: DamageRect,
    target: BufferDesc,
    target_bytes: &mut [u8],
    x: u32,
    y: u32,
) -> Result<(), ConvertError> {
    if source_bytes.len() != len(source) {
        return Err(ConvertError::SourceLength);
    }
    if target_bytes.len() != len(target) {
        return Err(ConvertError::TargetLength);
    }
    if !fits(source, rect.x(), rect.y(), rect) {
        return Err(ConvertError::OutsideSource);
    }
    if !fits(target, x, y, rect) {
        return Err(ConvertError::OutsideTarget);
    }
    let (from, to) = (source.format(), target.format());
    for row in 0..rect.height() {
        // Inside both buffers, so the rows can be taken.
        let source_row = row_of(source, rect.x(), rect.y() + row, rect.width());
        let target_row = row_of(target, x, y + row, rect.width());
        let (Some(source_row), Some(target_row)) = (
            source_bytes.get(source_row),
            target_bytes.get_mut(target_row),
        ) else {
            return Err(ConvertError::OutsideSource);
        };
        convert_row(from, source_row, to, target_row);
    }
    Ok(())
}

/// The bytes of a buffer, as a `usize`.
fn len(desc: BufferDesc) -> usize {
    // At most 2^26 bytes.
    desc.byte_len() as usize
}

/// Whether the pixels of a rectangle of the size of `rect`, with its
/// top-left pixel at (`x`, `y`), are all in the buffer `desc`.
fn fits(desc: BufferDesc, x: u32, y: u32, rect: DamageRect) -> bool {
    u64::from(x) + u64::from(rect.width()) <= u64::from(desc.width())
        && u64::from(y) + u64::from(rect.height()) <= u64::from(desc.height())
}

/// The byte range of the `width` pixels from (`x`, `y`) on, for pixels
/// inside the buffer `desc`.
fn row_of(desc: BufferDesc, x: u32, y: u32, width: u32) -> core::ops::Range<usize> {
    let bpp = desc.format().bytes_per_pixel() as usize;
    let start = y as usize * desc.stride() as usize + x as usize * bpp;
    start..start + width as usize * bpp
}

/// The order of the bytes of a four-byte format.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Order {
    /// R, G, B and A, premultiplied.
    Rgba,
    /// B, G, R and X, opaque.
    Bgrx,
    /// B, G, R and A, premultiplied.
    Bgra,
}

impl Order {
    /// The order of a four-byte format, or `None` for
    /// [`PixelFormat::Rgb565`].
    const fn of(format: PixelFormat) -> Option<Order> {
        match format {
            PixelFormat::Rgba8Premul => Some(Order::Rgba),
            PixelFormat::Xrgb8888 => Some(Order::Bgrx),
            PixelFormat::Argb8888Premul => Some(Order::Bgra),
            PixelFormat::Rgb565 => None,
        }
    }

    /// The R, G, B and A of a pixel in this order; alpha is 255 when the
    /// format is opaque.
    const fn read(self, [first, second, third, fourth]: [u8; 4]) -> [u8; 4] {
        match self {
            Order::Rgba => [first, second, third, fourth],
            Order::Bgrx => [third, second, first, 0xFF],
            Order::Bgra => [third, second, first, fourth],
        }
    }

    /// The bytes of a pixel of R, G, B and A in this order; alpha is left
    /// out when the format is opaque.
    const fn write(self, [r, g, b, a]: [u8; 4]) -> [u8; 4] {
        match self {
            Order::Rgba => [r, g, b, a],
            Order::Bgrx => [b, g, r, 0xFF],
            Order::Bgra => [b, g, r, a],
        }
    }
}

/// Converts one row of pixels of the format `from` into the row `target`
/// of the format `to`, of the same number of pixels.
fn convert_row(from: PixelFormat, source: &[u8], to: PixelFormat, target: &mut [u8]) {
    match (Order::of(from), Order::of(to)) {
        // The same format: both rows have the same length.
        (Some(from), Some(to)) if from == to => target.copy_from_slice(source),
        (None, None) => target.copy_from_slice(source),
        (Some(from), Some(to)) => pixels(source, target, |pixel| to.write(from.read(pixel))),
        (Some(from), None) => pixels(source, target, |pixel| pack_565(from.read(pixel))),
        (None, Some(to)) => pixels(source, target, |pixel| to.write(unpack_565(pixel))),
    }
}

/// Writes `each` of each pixel of `source`, of `S` bytes, into the
/// pixels of `target`, of `T` bytes, one by one.
fn pixels<const S: usize, const T: usize>(
    source: &[u8],
    target: &mut [u8],
    each: impl Fn([u8; S]) -> [u8; T],
) {
    let (source, _) = source.as_chunks::<S>();
    let (target, _) = target.as_chunks_mut::<T>();
    for (from, to) in source.iter().zip(target) {
        *to = each(*from);
    }
}

/// The bytes of a [`PixelFormat::Rgb565`] pixel of R, G and B; alpha is
/// left out.
const fn pack_565([r, g, b, _]: [u8; 4]) -> [u8; 2] {
    let word = (to_bits(r, 31) << 11) | (to_bits(g, 63) << 5) | to_bits(b, 31);
    word.to_le_bytes()
}

/// The R, G, B and A of a [`PixelFormat::Rgb565`] pixel, with alpha 255.
const fn unpack_565(bytes: [u8; 2]) -> [u8; 4] {
    let word = u16::from_le_bytes(bytes);
    [
        from_bits(word >> 11, 31),
        from_bits((word >> 5) & 63, 63),
        from_bits(word & 31, 31),
        0xFF,
    ]
}

/// The 8-bit component `c` in 5 bits, for `max` 31, or 6 bits, for `max`
/// 63: `(c * max + 127) / 255`.
const fn to_bits(c: u8, max: u16) -> u16 {
    // At most 255 * 63 + 127, so it fits.
    (c as u16 * max + 127) / 255
}

/// The 5-bit component `v`, for `max` 31, or 6-bit, for `max` 63, in 8
/// bits: `(v * 255 + max / 2) / max`.
const fn from_bits(v: u16, max: u16) -> u8 {
    // At most 63 * 255 + 31, and the quotient at most 255.
    ((v * 255 + max / 2) / max) as u8
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pixel::PixelFormat::{Argb8888Premul, Rgb565, Rgba8Premul, Xrgb8888};

    /// An opaque orange: R 0xF0, G 0x80, B 0x10.
    const ORANGE: [u8; 4] = [0xF0, 0x80, 0x10, 0xFF];
    /// A translucent blue, premultiplied: R 0x10, G 0x20, B 0x40, A 0x80.
    const BLUE: [u8; 4] = [0x10, 0x20, 0x40, 0x80];
    /// The orange in Rgb565: R 29 of 31, G 32 of 63 and B 2 of 31.
    const ORANGE_565: u16 = 0xEC02;
    /// That Rgb565 orange in eight bits: R 239, G 130 and B 16.
    const ORANGE_FROM_565: [u8; 4] = [239, 130, 16, 0xFF];

    fn desc(width: u32, height: u32, format: PixelFormat) -> BufferDesc {
        let stride = width * format.bytes_per_pixel();
        BufferDesc::new(width, height, stride, format).unwrap()
    }

    /// The pixels of `bytes` in `from`, converted whole into `to`.
    fn converted<const N: usize>(from: PixelFormat, bytes: &[u8], to: PixelFormat) -> [u8; N] {
        let width = bytes.len() as u32 / from.bytes_per_pixel();
        let mut out = [0x55; N];
        let rect = DamageRect::new(0, 0, width, 1).unwrap();
        convert(
            desc(width, 1, from),
            bytes,
            rect,
            desc(width, 1, to),
            &mut out,
            0,
            0,
        )
        .unwrap();
        out
    }

    fn flat<const N: usize, const M: usize>(pixels: [[u8; N]; 2]) -> [u8; M] {
        let mut out = [0; M];
        out[..N].copy_from_slice(&pixels[0]);
        out[N..].copy_from_slice(&pixels[1]);
        out
    }

    // The byte order and bit layout of each format, with a known color.

    #[test]
    fn rgba8_premul_holds_r_g_b_and_a_in_memory_order() {
        // The first byte is read as red, the fourth as alpha.
        assert_eq!(
            converted::<4>(Rgba8Premul, &BLUE, Argb8888Premul),
            [0x40, 0x20, 0x10, 0x80]
        );
        assert_eq!(
            converted::<4>(Rgba8Premul, &ORANGE, Xrgb8888),
            [0x10, 0x80, 0xF0, 0xFF]
        );
    }

    #[test]
    fn xrgb8888_holds_b_g_r_and_x_in_memory_order() {
        let word = u32::from_le_bytes(converted::<4>(Rgba8Premul, &ORANGE, Xrgb8888));
        assert_eq!(word, 0xFF_F0_80_10);
        assert_eq!(
            converted::<4>(Xrgb8888, &[0x10, 0x80, 0xF0, 0x00], Rgba8Premul),
            ORANGE
        );
    }

    #[test]
    fn rgb565_holds_r_g_and_b_from_the_high_bits_of_a_little_endian_word() {
        let word = u16::from_le_bytes(converted::<2>(Rgba8Premul, &ORANGE, Rgb565));
        assert_eq!(word, (29 << 11) | (32 << 5) | 2);
        assert_eq!(word, ORANGE_565);
        assert_eq!(
            converted::<4>(Rgb565, &0xF800_u16.to_le_bytes(), Rgba8Premul),
            [0xFF, 0, 0, 0xFF]
        );
        assert_eq!(
            converted::<4>(Rgb565, &0x07E0_u16.to_le_bytes(), Rgba8Premul),
            [0, 0xFF, 0, 0xFF]
        );
        assert_eq!(
            converted::<4>(Rgb565, &0x001F_u16.to_le_bytes(), Rgba8Premul),
            [0, 0, 0xFF, 0xFF]
        );
    }

    #[test]
    fn argb8888_premul_holds_b_g_r_and_a_in_memory_order() {
        let word = u32::from_le_bytes(converted::<4>(Rgba8Premul, &BLUE, Argb8888Premul));
        assert_eq!(word, 0x80_10_20_40);
    }

    // Round trips through Rgba8Premul.

    #[test]
    fn opaque_xrgb8888_comes_back_unchanged_through_rgba8_premul() {
        for pixel in [
            [0x10, 0x80, 0xF0, 0xFF],
            [0, 0, 0, 0xFF],
            [0xFF; 4],
            [1, 2, 3, 0xFF],
        ] {
            let rgba = converted::<4>(Xrgb8888, &pixel, Rgba8Premul);
            assert_eq!(converted::<4>(Rgba8Premul, &rgba, Xrgb8888), pixel);
        }
    }

    #[test]
    fn every_rgb565_pixel_comes_back_unchanged_through_rgba8_premul() {
        for word in 0..=u16::MAX {
            let rgba = converted::<4>(Rgb565, &word.to_le_bytes(), Rgba8Premul);
            assert_eq!(
                converted::<2>(Rgba8Premul, &rgba, Rgb565),
                word.to_le_bytes(),
                "{word:#06x}"
            );
        }
    }

    #[test]
    fn argb8888_premul_comes_back_unchanged_through_rgba8_premul() {
        for pixel in [
            [0x40, 0x20, 0x10, 0x80],
            [0, 0, 0, 0],
            [1, 2, 3, 4],
            [0xFF; 4],
        ] {
            let rgba = converted::<4>(Argb8888Premul, &pixel, Rgba8Premul);
            assert_eq!(converted::<4>(Rgba8Premul, &rgba, Argb8888Premul), pixel);
        }
    }

    #[test]
    fn every_5_bit_and_6_bit_value_widened_and_narrowed_is_unchanged() {
        for v in 0..=31 {
            assert_eq!(to_bits(from_bits(v, 31), 31), v, "{v} of 31");
        }
        for v in 0..=63 {
            assert_eq!(to_bits(from_bits(v, 63), 63), v, "{v} of 63");
        }
        assert_eq!(
            (from_bits(0, 31), from_bits(31, 31), from_bits(16, 31)),
            (0, 255, 132)
        );
        assert_eq!(
            (from_bits(0, 63), from_bits(63, 63), from_bits(32, 63)),
            (0, 255, 130)
        );
        assert_eq!(
            (to_bits(0x80, 31), to_bits(0x80, 63), to_bits(0xFF, 63)),
            (16, 32, 63)
        );
    }

    // One case for each ordered pair of formats, with two pixels.

    #[test]
    fn rgba8_premul_to_xrgb8888() {
        let out: [u8; 8] = converted(Rgba8Premul, &flat::<4, 8>([ORANGE, BLUE]), Xrgb8888);
        assert_eq!(
            out,
            flat([[0x10, 0x80, 0xF0, 0xFF], [0x40, 0x20, 0x10, 0xFF]])
        );
    }

    #[test]
    fn rgba8_premul_to_rgb565() {
        let out: [u8; 4] = converted(Rgba8Premul, &flat::<4, 8>([ORANGE, BLUE]), Rgb565);
        // Blue keeps its premultiplied components: 2, 8 and 8.
        let blue = (2_u16 << 11) | (8 << 5) | 8;
        assert_eq!(out, flat([ORANGE_565.to_le_bytes(), blue.to_le_bytes()]));
    }

    #[test]
    fn rgba8_premul_to_argb8888_premul() {
        let out: [u8; 8] = converted(Rgba8Premul, &flat::<4, 8>([ORANGE, BLUE]), Argb8888Premul);
        assert_eq!(
            out,
            flat([[0x10, 0x80, 0xF0, 0xFF], [0x40, 0x20, 0x10, 0x80]])
        );
    }

    #[test]
    fn xrgb8888_to_rgba8_premul() {
        let source = flat::<4, 8>([[0x10, 0x80, 0xF0, 0x00], [0x40, 0x20, 0x10, 0x7F]]);
        let out: [u8; 8] = converted(Xrgb8888, &source, Rgba8Premul);
        assert_eq!(out, flat([ORANGE, [0x10, 0x20, 0x40, 0xFF]]));
    }

    #[test]
    fn xrgb8888_to_rgb565() {
        let source = flat::<4, 8>([[0x10, 0x80, 0xF0, 0x00], [0xFF, 0xFF, 0xFF, 0xFF]]);
        let out: [u8; 4] = converted(Xrgb8888, &source, Rgb565);
        assert_eq!(
            out,
            flat([ORANGE_565.to_le_bytes(), 0xFFFF_u16.to_le_bytes()])
        );
    }

    #[test]
    fn xrgb8888_to_argb8888_premul() {
        let source = flat::<4, 8>([[0x10, 0x80, 0xF0, 0x00], [0x40, 0x20, 0x10, 0x7F]]);
        let out: [u8; 8] = converted(Xrgb8888, &source, Argb8888Premul);
        assert_eq!(
            out,
            flat([[0x10, 0x80, 0xF0, 0xFF], [0x40, 0x20, 0x10, 0xFF]])
        );
    }

    #[test]
    fn rgb565_to_rgba8_premul() {
        let source = flat::<2, 4>([ORANGE_565.to_le_bytes(), 0x0000_u16.to_le_bytes()]);
        let out: [u8; 8] = converted(Rgb565, &source, Rgba8Premul);
        assert_eq!(out, flat([ORANGE_FROM_565, [0, 0, 0, 0xFF]]));
    }

    #[test]
    fn rgb565_to_xrgb8888() {
        let source = flat::<2, 4>([ORANGE_565.to_le_bytes(), 0xFFFF_u16.to_le_bytes()]);
        let out: [u8; 8] = converted(Rgb565, &source, Xrgb8888);
        assert_eq!(out, flat([[16, 130, 239, 0xFF], [0xFF; 4]]));
    }

    #[test]
    fn rgb565_to_argb8888_premul() {
        let source = flat::<2, 4>([ORANGE_565.to_le_bytes(), 0x001F_u16.to_le_bytes()]);
        let out: [u8; 8] = converted(Rgb565, &source, Argb8888Premul);
        assert_eq!(out, flat([[16, 130, 239, 0xFF], [0xFF, 0, 0, 0xFF]]));
    }

    #[test]
    fn argb8888_premul_to_rgba8_premul() {
        let source = flat::<4, 8>([[0x10, 0x80, 0xF0, 0xFF], [0x40, 0x20, 0x10, 0x80]]);
        let out: [u8; 8] = converted(Argb8888Premul, &source, Rgba8Premul);
        assert_eq!(out, flat([ORANGE, BLUE]));
    }

    #[test]
    fn argb8888_premul_to_xrgb8888() {
        let source = flat::<4, 8>([[0x10, 0x80, 0xF0, 0xFF], [0x40, 0x20, 0x10, 0x80]]);
        let out: [u8; 8] = converted(Argb8888Premul, &source, Xrgb8888);
        assert_eq!(
            out,
            flat([[0x10, 0x80, 0xF0, 0xFF], [0x40, 0x20, 0x10, 0xFF]])
        );
    }

    #[test]
    fn argb8888_premul_to_rgb565() {
        let source = flat::<4, 8>([[0x10, 0x80, 0xF0, 0xFF], [0x40, 0x20, 0x10, 0x80]]);
        let out: [u8; 4] = converted(Argb8888Premul, &source, Rgb565);
        let blue = (2_u16 << 11) | (8 << 5) | 8;
        assert_eq!(out, flat([ORANGE_565.to_le_bytes(), blue.to_le_bytes()]));
    }

    // Rectangles, strides and errors.

    #[test]
    fn the_same_format_copies_the_bytes_of_each_row() {
        // X of an Xrgb8888 source is copied as it is within the format.
        for (format, pixel) in [
            (Rgba8Premul, BLUE),
            (Xrgb8888, [1, 2, 3, 4]),
            (Argb8888Premul, BLUE),
        ] {
            assert_eq!(converted::<4>(format, &pixel, format), pixel, "{format:?}");
        }
        assert_eq!(converted::<2>(Rgb565, &[0x34, 0x12], Rgb565), [0x34, 0x12]);
    }

    #[test]
    fn only_the_rectangle_is_written_at_its_place_in_the_target() {
        // A 3 by 3 source with a stride of 16 bytes, pixel (x, y) holding
        // R = 16 * y + x.
        let source = BufferDesc::new(3, 3, 16, Rgba8Premul).unwrap();
        let mut source_bytes = [0xEE; 48];
        for y in 0..3 {
            for x in 0..3 {
                let at = y * 16 + x * 4;
                source_bytes[at..at + 4].copy_from_slice(&[(16 * y + x) as u8, 0, 0, 0xFF]);
            }
        }
        // A 4 by 4 target with a stride of 10 bytes.
        let target = BufferDesc::new(4, 4, 10, Rgb565).unwrap();
        let mut target_bytes = [0xAA; 40];
        let rect = DamageRect::new(1, 1, 2, 2).unwrap();
        convert(source, &source_bytes, rect, target, &mut target_bytes, 2, 1).unwrap();
        let mut expected = [0xAA; 40];
        for (y, x, r) in [(1, 2, 17), (1, 3, 18), (2, 2, 33), (2, 3, 34)] {
            let word = to_bits(r, 31) << 11;
            let at = y * 10 + x * 2;
            expected[at..at + 2].copy_from_slice(&word.to_le_bytes());
        }
        assert_eq!(target_bytes, expected);
    }

    #[test]
    fn slices_of_another_length_are_rejected() {
        let one = desc(1, 1, Rgba8Premul);
        let rect = DamageRect::new(0, 0, 1, 1).unwrap();
        let mut target = [0; 4];
        assert_eq!(
            convert(one, &[0; 3], rect, one, &mut target, 0, 0),
            Err(ConvertError::SourceLength)
        );
        assert_eq!(
            convert(one, &[0; 5], rect, one, &mut target, 0, 0),
            Err(ConvertError::SourceLength)
        );
        assert_eq!(
            convert(one, &[1; 4], rect, one, &mut [0; 8], 0, 0),
            Err(ConvertError::TargetLength)
        );
        assert_eq!(target, [0; 4]);
    }

    #[test]
    fn a_rectangle_outside_either_buffer_is_rejected_and_writes_nothing() {
        let source = desc(4, 2, Rgba8Premul);
        let target = desc(2, 2, Xrgb8888);
        let source_bytes = [0x11; 32];
        let mut target_bytes = [0; 16];
        let wide = DamageRect::new(1, 0, 4, 1).unwrap();
        assert_eq!(
            convert(source, &source_bytes, wide, target, &mut target_bytes, 0, 0),
            Err(ConvertError::OutsideSource)
        );
        let far = DamageRect::new(u32::MAX, u32::MAX, u32::MAX, u32::MAX).unwrap();
        assert_eq!(
            convert(source, &source_bytes, far, target, &mut target_bytes, 0, 0),
            Err(ConvertError::OutsideSource)
        );
        let rect = DamageRect::new(0, 0, 2, 2).unwrap();
        assert_eq!(
            convert(source, &source_bytes, rect, target, &mut target_bytes, 1, 0),
            Err(ConvertError::OutsideTarget)
        );
        assert_eq!(
            convert(
                source,
                &source_bytes,
                rect,
                target,
                &mut target_bytes,
                0,
                u32::MAX
            ),
            Err(ConvertError::OutsideTarget)
        );
        assert_eq!(target_bytes, [0; 16]);
        assert_eq!(
            convert(source, &source_bytes, rect, target, &mut target_bytes, 0, 0),
            Ok(())
        );
        let pixel = [0x11, 0x11, 0x11, 0xFF];
        assert_eq!(
            target_bytes,
            flat::<8, 16>([flat([pixel, pixel]), flat([pixel, pixel])])
        );
    }
}
