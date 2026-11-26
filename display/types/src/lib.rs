//! The shared types of kosmo: the values that the framework, the display
//! server and the display protocol pass between them, defined once here.
//!
//! - Geometry in the 26.6 fixed-point format: [`Fx`], counted in 1/64
//!   pixel, with [`Point`], [`Size`] and [`Rect`], and [`round`], the one
//!   rounding function of kosmo's geometry.
//! - Logical time: [`MonoTime`], a monotonic clock reading in nanoseconds.
//! - The focus generation [`FocusGen`] and the [`Epoch`], which mark the key
//!   events of one focus.
//! - The two layers of keyboard event records: the device record
//!   [`InputRecord`] and the delivered key event [`KeyEvent`], with the
//!   types of their fields.
//! - Pixels: the four [`PixelFormat`]s and the buffer description
//!   [`BufferDesc`], with the buffer limits [`MAX_BUFFER_SIDE`] and
//!   [`MAX_BUFFER_BYTES`].
//! - Damage regions: the rectangles of a buffer whose pixels changed,
//!   [`DamageRect`], and the bounded region [`Damage`].
//!
//! Every type is fixed-size and `Copy` and never allocates. The crate is
//! pure core: `no_std` without `alloc`, without unsafe code, without
//! dependencies and without interior mutability. The conversion of pixels
//! between buffers is planned here with the renderer.
//!
//! ```
//! use kosmo_types::{Fx, Point, Rect, Size, round};
//!
//! let px = |pixels| Fx::from_pixels(pixels).unwrap();
//! let button = Rect::new(Point::new(px(10), px(10)), Size::new(px(80), px(24)).unwrap());
//! // A text library's position of 12.3 pixels, multiplied by 64 and rounded.
//! let x = round(12.3_f32 * 64.0).unwrap();
//! assert_eq!(x, Fx::from_units(787));
//! assert_eq!(button.contains(Point::new(x, px(20))), Some(true));
//! assert_eq!(round(x), 12);
//! ```

#![no_std]
#![forbid(unsafe_code)]

mod damage;
mod focus;
mod geometry;
mod input;
mod pixel;
mod time;

pub use damage::{Damage, DamageRect, MAX_DAMAGE_RECTS};
pub use focus::{Epoch, FocusGen};
pub use geometry::{Fx, Point, Rect, Round, Size, round};
pub use input::{
    HidUsage, InputRecord, KeyAction, KeyEvent, KeyMeaning, KeyState, ModSet, NamedKey, RecordBody,
};
pub use pixel::{BufferDesc, BufferError, MAX_BUFFER_BYTES, MAX_BUFFER_SIDE, PixelFormat};
pub use time::MonoTime;

#[cfg(test)]
mod tests {
    use super::*;

    /// Every shared type is `Copy` and owns no memory elsewhere.
    fn fixed_size<T: Copy + Sized + 'static>() {}

    #[test]
    fn every_shared_type_is_fixed_size_and_copy() {
        fixed_size::<Fx>();
        fixed_size::<Point>();
        fixed_size::<Size>();
        fixed_size::<Rect>();
        fixed_size::<MonoTime>();
        fixed_size::<FocusGen>();
        fixed_size::<Epoch>();
        fixed_size::<InputRecord>();
        fixed_size::<RecordBody>();
        fixed_size::<HidUsage>();
        fixed_size::<KeyState>();
        fixed_size::<KeyEvent>();
        fixed_size::<KeyAction>();
        fixed_size::<KeyMeaning>();
        fixed_size::<NamedKey>();
        fixed_size::<ModSet>();
        fixed_size::<PixelFormat>();
        fixed_size::<BufferDesc>();
        fixed_size::<BufferError>();
        fixed_size::<DamageRect>();
        fixed_size::<Damage>();
    }
}
