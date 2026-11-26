//! The buffer limit: a buffer description of more bytes than the limit is
//! rejected with an error of its own. The unit tests of the description
//! cover the limit on the width and the height.

use kosmo_types::{BufferDesc, BufferError, MAX_BUFFER_BYTES, PixelFormat};

#[test]
fn a_4097_by_4097_rgba8_premul_buffer_is_rejected_as_too_many_bytes() {
    let side = 4_097;
    let stride = side * PixelFormat::Rgba8Premul.bytes_per_pixel();
    let bytes = u64::from(stride) * u64::from(side);
    assert_eq!(bytes, 67_141_636);
    assert!(bytes > u64::from(MAX_BUFFER_BYTES));
    assert_eq!(
        BufferDesc::new(side, side, stride, PixelFormat::Rgba8Premul),
        Err(BufferError::TooManyBytes)
    );
    // The same buffer one pixel smaller each way is within the limit.
    let desc = BufferDesc::new(4_096, 4_096, 4_096 * 4, PixelFormat::Rgba8Premul);
    assert_eq!(desc.map(BufferDesc::byte_len), Ok(MAX_BUFFER_BYTES));
}
