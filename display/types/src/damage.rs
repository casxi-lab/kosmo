//! Damage regions: the rectangles of a buffer whose pixels changed. The
//! framework redraws and converts only them, and a client's commit to the
//! display server carries them.

use core::cmp::{max, min};
use core::fmt;

use crate::pixel::BufferDesc;

/// The most rectangles in a [`Damage`]: 16. One more merges them all into
/// their bounding rectangle.
pub const MAX_DAMAGE_RECTS: usize = 16;

/// A rectangle of pixels in buffer coordinates: its top-left pixel (`x`,
/// `y`) and its width and height, both at least 1. Row 0 is at the top.
///
/// ```
/// use kosmo_types::DamageRect;
///
/// let rect = DamageRect::new(10, 20, 30, 40).unwrap();
/// assert_eq!((rect.x(), rect.y(), rect.width(), rect.height()), (10, 20, 30, 40));
/// assert_eq!(DamageRect::new(10, 20, 0, 40), None);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DamageRect {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

impl DamageRect {
    /// The rectangle of `width` by `height` pixels whose top-left pixel is
    /// (`x`, `y`), or `None` when the width or the height is 0.
    pub const fn new(x: u32, y: u32, width: u32, height: u32) -> Option<DamageRect> {
        if width == 0 || height == 0 {
            None
        } else {
            Some(DamageRect {
                x,
                y,
                width,
                height,
            })
        }
    }

    /// The column of the leftmost pixels.
    pub const fn x(self) -> u32 {
        self.x
    }

    /// The row of the top pixels.
    pub const fn y(self) -> u32 {
        self.y
    }

    /// The width in pixels.
    pub const fn width(self) -> u32 {
        self.width
    }

    /// The height in pixels.
    pub const fn height(self) -> u32 {
        self.height
    }

    /// The column after the rightmost pixels, which may not fit in a `u32`.
    const fn right(self) -> u64 {
        self.x as u64 + self.width as u64
    }

    /// The row after the bottom pixels, which may not fit in a `u32`.
    const fn bottom(self) -> u64 {
        self.y as u64 + self.height as u64
    }

    /// The part of the rectangle inside a buffer of `width` by `height`
    /// pixels, or `None` when no pixel of it is inside.
    fn clipped(self, width: u32, height: u32) -> Option<DamageRect> {
        let right = min(self.right(), u64::from(width));
        let bottom = min(self.bottom(), u64::from(height));
        if u64::from(self.x) >= right || u64::from(self.y) >= bottom {
            return None;
        }
        // Inside the buffer, so the sizes fit in a u32.
        let width = u32::try_from(right - u64::from(self.x)).ok()?;
        let height = u32::try_from(bottom - u64::from(self.y)).ok()?;
        DamageRect::new(self.x, self.y, width, height)
    }

    /// The smallest rectangle that holds both, for two rectangles inside
    /// one buffer.
    fn union(self, other: DamageRect) -> DamageRect {
        let x = min(self.x, other.x);
        let y = min(self.y, other.y);
        // Inside the buffer, so the edges fit in a u32.
        let right = max(self.x + self.width, other.x + other.width);
        let bottom = max(self.y + self.height, other.y + other.height);
        DamageRect {
            x,
            y,
            width: right - x,
            height: bottom - y,
        }
    }
}

/// The damage region of one buffer: at most [`MAX_DAMAGE_RECTS`]
/// rectangles inside it, in the order they were added. It never
/// allocates.
///
/// Adding a rectangle drops it when its width or its height is 0 or when
/// it lies outside the buffer, and clips it to the buffer otherwise. When
/// the region already holds [`MAX_DAMAGE_RECTS`] rectangles, the next one
/// that is kept replaces all of them, together with itself, by their
/// bounding rectangle. The same rectangles added in the same order give the
/// same region; rectangles that overlap are not merged otherwise.
///
/// ```
/// use kosmo_types::{BufferDesc, Damage, DamageRect, PixelFormat};
///
/// let buffer = BufferDesc::new(100, 50, 400, PixelFormat::Rgba8Premul).unwrap();
/// let mut damage = Damage::new(buffer);
/// damage.add(90, 40, 20, 20);
/// damage.add(0, 0, 0, 10);
/// damage.add(200, 0, 10, 10);
/// assert_eq!(damage.rects(), [DamageRect::new(90, 40, 10, 10).unwrap()]);
/// ```
#[derive(Clone, Copy)]
pub struct Damage {
    width: u32,
    height: u32,
    rects: [DamageRect; MAX_DAMAGE_RECTS],
    len: usize,
}

/// What fills the places of a [`Damage`] that hold no rectangle.
const UNUSED: DamageRect = DamageRect {
    x: 0,
    y: 0,
    width: 1,
    height: 1,
};

impl Damage {
    /// The empty damage region of the buffer that `buffer` describes.
    pub const fn new(buffer: BufferDesc) -> Damage {
        Damage {
            width: buffer.width(),
            height: buffer.height(),
            rects: [UNUSED; MAX_DAMAGE_RECTS],
            len: 0,
        }
    }

    /// Adds the rectangle of `width` by `height` pixels whose top-left
    /// pixel is (`x`, `y`), dropped, clipped or merged as the
    /// [type](Damage) describes.
    pub fn add(&mut self, x: u32, y: u32, width: u32, height: u32) {
        let Some(rect) = DamageRect::new(x, y, width, height) else {
            return;
        };
        let Some(rect) = rect.clipped(self.width, self.height) else {
            return;
        };
        if self.len < MAX_DAMAGE_RECTS {
            if let Some(place) = self.rects.get_mut(self.len) {
                *place = rect;
                self.len += 1;
            }
        } else {
            let bounds = self.rects.iter().fold(rect, |all, next| all.union(*next));
            self.rects = [UNUSED; MAX_DAMAGE_RECTS];
            self.rects[0] = bounds;
            self.len = 1;
        }
    }

    /// The rectangles, in the order they were added.
    pub fn rects(&self) -> &[DamageRect] {
        self.rects.get(..self.len).unwrap_or(&[])
    }

    /// Whether the region holds no rectangle.
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl PartialEq for Damage {
    fn eq(&self, other: &Damage) -> bool {
        (self.width, self.height) == (other.width, other.height) && self.rects() == other.rects()
    }
}

impl Eq for Damage {}

impl fmt::Debug for Damage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Damage")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("rects", &self.rects())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pixel::PixelFormat;

    fn rect(x: u32, y: u32, width: u32, height: u32) -> DamageRect {
        DamageRect::new(x, y, width, height).unwrap()
    }

    /// The empty region of a buffer of 64 by 48 pixels.
    fn damage() -> Damage {
        Damage::new(BufferDesc::new(64, 48, 256, PixelFormat::Rgba8Premul).unwrap())
    }

    #[test]
    fn a_rectangle_without_width_or_height_is_dropped() {
        assert_eq!(DamageRect::new(1, 1, 0, 1), None);
        assert_eq!(DamageRect::new(1, 1, 1, 0), None);
        let mut region = damage();
        region.add(1, 1, 0, 5);
        region.add(1, 1, 5, 0);
        assert!(region.is_empty());
        assert_eq!(region, damage());
    }

    #[test]
    fn a_rectangle_is_clipped_to_the_buffer() {
        let mut region = damage();
        region.add(60, 40, 10, 10);
        region.add(0, 0, 64, 48);
        region.add(0, 47, u32::MAX, u32::MAX);
        assert_eq!(
            region.rects(),
            [rect(60, 40, 4, 8), rect(0, 0, 64, 48), rect(0, 47, 64, 1)]
        );
    }

    #[test]
    fn a_rectangle_outside_the_buffer_is_dropped() {
        let mut region = damage();
        region.add(64, 0, 1, 1);
        region.add(0, 48, 1, 1);
        region.add(u32::MAX, u32::MAX, u32::MAX, u32::MAX);
        assert!(region.is_empty());
    }

    #[test]
    fn rectangles_keep_the_order_they_were_added_in() {
        let mut region = damage();
        let added = [rect(30, 30, 2, 2), rect(0, 0, 4, 4), rect(1, 1, 2, 2)];
        for r in added {
            region.add(r.x(), r.y(), r.width(), r.height());
        }
        // Overlapping and contained rectangles stay as they are.
        assert_eq!(region.rects(), added);
    }

    #[test]
    fn the_rectangle_after_the_limit_merges_all_into_their_bounds() {
        let mut region = damage();
        for i in 0..16 {
            region.add(10 + i, 20, 1, i + 1);
        }
        assert_eq!(region.rects().len(), MAX_DAMAGE_RECTS);
        assert_eq!(region.rects()[15], rect(25, 20, 1, 16));
        // A dropped rectangle does not count.
        region.add(5, 5, 0, 0);
        assert_eq!(region.rects().len(), MAX_DAMAGE_RECTS);
        region.add(2, 30, 3, 1);
        assert_eq!(region.rects(), [rect(2, 20, 24, 16)]);
        // Adding goes on after the merge.
        region.add(0, 0, 1, 1);
        assert_eq!(region.rects(), [rect(2, 20, 24, 16), rect(0, 0, 1, 1)]);
    }

    #[test]
    fn regions_of_buffers_of_another_size_are_not_equal() {
        let other = Damage::new(BufferDesc::new(32, 48, 128, PixelFormat::Rgba8Premul).unwrap());
        assert_ne!(damage(), other);
        let (mut first, mut second) = (damage(), other);
        first.add(1, 1, 2, 2);
        second.add(1, 1, 2, 2);
        // The same rectangles, in buffers of another width.
        assert_eq!(first.rects(), second.rects());
        assert_ne!(first, second);
    }

    #[test]
    fn the_same_rectangles_in_the_same_order_give_the_same_region() {
        let fill = |region: &mut Damage| {
            for i in 0..40 {
                region.add((i * 7) % 64, (i * 5) % 48, i % 9, 3);
            }
        };
        let (mut first, mut second) = (damage(), damage());
        fill(&mut first);
        fill(&mut second);
        assert_eq!(first, second);
        assert_eq!(first.rects(), second.rects());
    }
}
