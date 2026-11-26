//! Geometry in the 26.6 fixed-point format: the number type [`Fx`], the
//! points, sizes and rectangles built on it, and [`round`], the one
//! rounding function of kosmo's geometry.
//!
//! Arithmetic checks overflow: an operation whose result does not fit
//! returns `None` and never panics. Comparisons are of integers, so they
//! give the same answer on every machine.

use core::cmp::{max, min};

/// A coordinate or length in units of 1/64 pixel: an `i32` in the 26.6
/// fixed-point format, 26 bits of whole pixels and 6 bits of fraction.
///
/// Layout coordinates, text positions and sizes and the geometry of display
/// lists use it. Addition and subtraction are exact. Multiplication and
/// division go through `i64` intermediate values and round their exact
/// result through [`round`]. Every operation returns `None` instead of a
/// result outside the range of `i32`.
///
/// ```
/// use kosmo_types::Fx;
///
/// let half = Fx::from_units(32);
/// let three = Fx::from_pixels(3).unwrap();
/// assert_eq!(three.checked_mul(half), Some(Fx::from_units(96)));
/// assert_eq!(Fx::MAX.checked_add(Fx::from_units(1)), None);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Fx(i32);

impl Fx {
    /// The units in one pixel.
    pub const UNITS_PER_PIXEL: i32 = 64;
    /// Zero.
    pub const ZERO: Fx = Fx(0);
    /// The smallest value, `i32::MIN` units.
    pub const MIN: Fx = Fx(i32::MIN);
    /// The largest value, `i32::MAX` units.
    pub const MAX: Fx = Fx(i32::MAX);

    /// The value of `units` 1/64-pixel units.
    pub const fn from_units(units: i32) -> Fx {
        Fx(units)
    }

    /// The number of 1/64-pixel units.
    pub const fn units(self) -> i32 {
        self.0
    }

    /// `pixels` whole pixels, or `None` when they do not fit.
    pub const fn from_pixels(pixels: i32) -> Option<Fx> {
        match pixels.checked_mul(Fx::UNITS_PER_PIXEL) {
            Some(units) => Some(Fx(units)),
            None => None,
        }
    }

    /// `self + other`, or `None` when the sum does not fit.
    pub const fn checked_add(self, other: Fx) -> Option<Fx> {
        match self.0.checked_add(other.0) {
            Some(units) => Some(Fx(units)),
            None => None,
        }
    }

    /// `self - other`, or `None` when the difference does not fit.
    pub const fn checked_sub(self, other: Fx) -> Option<Fx> {
        match self.0.checked_sub(other.0) {
            Some(units) => Some(Fx(units)),
            None => None,
        }
    }

    /// `self * other`, rounded to a unit through [`round`], or `None` when
    /// the product does not fit.
    pub fn checked_mul(self, other: Fx) -> Option<Fx> {
        // At most 2^62 in magnitude, so the product fits in an i64.
        let product = i64::from(self.0) * i64::from(other.0);
        round(Ratio {
            numerator: product,
            denominator: i64::from(Fx::UNITS_PER_PIXEL),
        })
    }

    /// `self / other`, rounded to a unit through [`round`], or `None` when
    /// `other` is zero or the quotient does not fit.
    pub fn checked_div(self, other: Fx) -> Option<Fx> {
        // At most 2^37 in magnitude, so both fit in an i64 with their signs
        // changed.
        let numerator = i64::from(self.0) * i64::from(Fx::UNITS_PER_PIXEL);
        let denominator = i64::from(other.0);
        match denominator {
            0 => None,
            _ if denominator < 0 => round(Ratio {
                numerator: -numerator,
                denominator: -denominator,
            }),
            _ => round(Ratio {
                numerator,
                denominator,
            }),
        }
    }
}

/// A point: a position in 1/64-pixel units, with `x` growing to the right
/// and `y` growing down. Hit testing takes one.
///
/// ```
/// use kosmo_types::{Fx, Point};
///
/// let point = Point::new(Fx::from_units(10), Fx::from_units(20));
/// let moved = point.translated(Fx::from_units(-10), Fx::from_units(5));
/// assert_eq!(moved, Some(Point::new(Fx::ZERO, Fx::from_units(25))));
/// assert_eq!(point.translated(Fx::MAX, Fx::ZERO), None);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Point {
    /// The horizontal position.
    pub x: Fx,
    /// The vertical position.
    pub y: Fx,
}

impl Point {
    /// The point at `x` and `y`.
    pub const fn new(x: Fx, y: Fx) -> Point {
        Point { x, y }
    }

    /// The point moved by `dx` and `dy`, or `None` when it does not fit.
    pub const fn translated(self, dx: Fx, dy: Fx) -> Option<Point> {
        match (self.x.checked_add(dx), self.y.checked_add(dy)) {
            (Some(x), Some(y)) => Some(Point { x, y }),
            _ => None,
        }
    }
}

/// A size: a width and a height, neither of them negative.
///
/// ```
/// use kosmo_types::{Fx, Size};
///
/// let size = Size::new(Fx::from_units(64), Fx::ZERO).unwrap();
/// assert_eq!((size.width(), size.height()), (Fx::from_units(64), Fx::ZERO));
/// assert_eq!(Size::new(Fx::from_units(-1), Fx::ZERO), None);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Size {
    width: Fx,
    height: Fx,
}

impl Size {
    /// No width and no height.
    pub const ZERO: Size = Size {
        width: Fx::ZERO,
        height: Fx::ZERO,
    };

    /// The size `width` by `height`, or `None` when either is negative.
    pub const fn new(width: Fx, height: Fx) -> Option<Size> {
        if width.0 < 0 || height.0 < 0 {
            None
        } else {
            Some(Size { width, height })
        }
    }

    /// The width.
    pub const fn width(self) -> Fx {
        self.width
    }

    /// The height.
    pub const fn height(self) -> Fx {
        self.height
    }
}

/// A rectangle: its top-left corner and its size. It includes its left and
/// top edges but not its right and bottom edges, so a rectangle without
/// width or height contains no point, and two rectangles that share an edge
/// share no point. Hit testing uses rectangles as boxes, clipped by the
/// boxes around them.
///
/// The right and bottom edges are computed when they are needed;
/// [`Rect::contains`] and [`Rect::intersection`] return `None` when one does
/// not fit.
///
/// ```
/// use kosmo_types::{Fx, Point, Rect, Size};
///
/// let px = |pixels| Fx::from_pixels(pixels).unwrap();
/// let rect = Rect::new(Point::new(px(0), px(0)), Size::new(px(4), px(2)).unwrap());
/// assert_eq!(rect.contains(Point::new(px(0), px(1))), Some(true));
/// assert_eq!(rect.contains(Point::new(px(4), px(1))), Some(false));
///
/// let other = Rect::new(Point::new(px(3), px(1)), Size::new(px(4), px(4)).unwrap());
/// let clipped = rect.intersection(other).unwrap();
/// assert_eq!(clipped, Rect::new(Point::new(px(3), px(1)), Size::new(px(1), px(1)).unwrap()));
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rect {
    origin: Point,
    size: Size,
}

impl Rect {
    /// The rectangle whose top-left corner is `origin`, of size `size`.
    pub const fn new(origin: Point, size: Size) -> Rect {
        Rect { origin, size }
    }

    /// The top-left corner.
    pub const fn origin(self) -> Point {
        self.origin
    }

    /// The size.
    pub const fn size(self) -> Size {
        self.size
    }

    /// The right and bottom edges, or `None` when either does not fit.
    const fn far_edges(self) -> Option<(Fx, Fx)> {
        match (
            self.origin.x.checked_add(self.size.width),
            self.origin.y.checked_add(self.size.height),
        ) {
            (Some(right), Some(bottom)) => Some((right, bottom)),
            _ => None,
        }
    }

    /// Whether `point` lies in the rectangle, or `None` when its right or
    /// bottom edge does not fit.
    pub fn contains(self, point: Point) -> Option<bool> {
        let (right, bottom) = self.far_edges()?;
        Some(
            self.origin.x <= point.x
                && point.x < right
                && self.origin.y <= point.y
                && point.y < bottom,
        )
    }

    /// The points that lie in both rectangles, as a rectangle, or `None`
    /// when a right or bottom edge does not fit. When the rectangles do
    /// not overlap, the result has no width or no height and contains no
    /// point.
    pub fn intersection(self, other: Rect) -> Option<Rect> {
        let (right, bottom) = self.far_edges()?;
        let (other_right, other_bottom) = other.far_edges()?;
        let left = max(self.origin.x, other.origin.x);
        let top = max(self.origin.y, other.origin.y);
        let size = Size {
            width: extent(left, min(right, other_right))?,
            height: extent(top, min(bottom, other_bottom))?,
        };
        Some(Rect {
            origin: Point::new(left, top),
            size,
        })
    }
}

/// The length from `start` to `end`, or zero when `end` is not after
/// `start`. Between the edges of an intersection it is never more than the
/// width or height of either rectangle, so it fits.
fn extent(start: Fx, end: Fx) -> Option<Fx> {
    if end > start {
        end.checked_sub(start)
    } else {
        Some(Fx::ZERO)
    }
}

/// Rounds `value` to the nearest whole value, with ties toward positive
/// infinity: an [`Fx`] to whole pixels, and an `f32` count of 1/64-pixel
/// units, such as a position from a text library multiplied by 64, to an
/// [`Fx`]. Every rounding of kosmo's geometry goes through this function.
///
/// For an [`Fx`] `x` the result is `(x + 32) >> 6`, computed without
/// overflow. For an `f32` the function takes the floor with `as`
/// conversions and comparisons and compares the exact fraction with one
/// half, so the result does not depend on the platform's floating-point
/// library; it is `None` for NaN, an infinity, and a value outside the
/// range of `i32`.
///
/// ```
/// use kosmo_types::{Fx, round};
///
/// assert_eq!(round(Fx::from_units(32)), 1);
/// assert_eq!(round(Fx::from_units(-32)), 0);
/// assert_eq!(round(2.5_f32), Some(Fx::from_units(3)));
/// assert_eq!(round(-2.5_f32), Some(Fx::from_units(-2)));
/// assert_eq!(round(f32::NAN), None);
/// ```
pub fn round<V: Round>(value: V) -> V::Rounded {
    value.nearest()
}

/// What [`round`] takes: an [`Fx`], or an `f32` count of 1/64-pixel units.
/// Only this crate implements it; inside it, the exact results of
/// [`Fx::checked_mul`] and [`Fx::checked_div`] are rounded through it too.
pub trait Round: Copy + sealed::Rounding {}

impl Round for Fx {}

impl Round for f32 {}

impl Round for Ratio {}

/// The exact value `numerator / denominator` in 1/64-pixel units, for a
/// positive `denominator`: the result of a multiplication or division of
/// [`Fx`] values before it is rounded.
#[derive(Clone, Copy)]
struct Ratio {
    numerator: i64,
    denominator: i64,
}

mod sealed {
    /// The rounding of one kind of value, which [`super::round`] calls.
    pub trait Rounding {
        /// The rounded value.
        type Rounded;

        /// The value rounded by the rule of [`super::round`].
        fn nearest(self) -> Self::Rounded;
    }
}

impl sealed::Rounding for Fx {
    type Rounded = i32;

    fn nearest(self) -> i32 {
        // The floor in whole pixels, plus one when the fraction, in 64ths,
        // is at least one half: `(x + 32) >> 6` without the overflow of
        // `x + 32`.
        (self.0 >> 6) + i32::from(self.0 & 63 >= 32)
    }
}

impl sealed::Rounding for Ratio {
    type Rounded = Option<Fx>;

    fn nearest(self) -> Option<Fx> {
        let floor = self.numerator.div_euclid(self.denominator);
        // The exact fraction is `rest / denominator`, in [0, 1).
        let rest = self.numerator.rem_euclid(self.denominator);
        let units = floor + i64::from(2 * rest >= self.denominator);
        i32::try_from(units).ok().map(Fx)
    }
}

impl sealed::Rounding for f32 {
    type Rounded = Option<Fx>;

    fn nearest(self) -> Option<Fx> {
        // -2^31 and 2^31 are exact f32 values; NaN fails both comparisons.
        const LOW: f32 = -2_147_483_648.0;
        const HIGH: f32 = 2_147_483_648.0;
        // 2^23: every f32 at least this large in magnitude is whole.
        const WHOLE: f32 = 8_388_608.0;
        if !(LOW..HIGH).contains(&self) {
            return None;
        }
        // In this range `as` truncates toward zero.
        let truncated = self as i32;
        let may_have_fraction = self > -WHOLE && self < WHOLE;
        if !may_have_fraction {
            return Some(Fx(truncated));
        }
        // Here the truncated value, the floor and the floor plus one half
        // are exact f32 values, so the comparisons are exact.
        let floor = if truncated as f32 > self {
            truncated - 1
        } else {
            truncated
        };
        let units = if self >= floor as f32 + 0.5 {
            floor + 1
        } else {
            floor
        };
        Some(Fx(units))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fx(units: i32) -> Fx {
        Fx::from_units(units)
    }

    fn px(pixels: i32) -> Fx {
        Fx::from_pixels(pixels).unwrap()
    }

    fn rect(x: i32, y: i32, width: i32, height: i32) -> Rect {
        Rect::new(
            Point::new(fx(x), fx(y)),
            Size::new(fx(width), fx(height)).unwrap(),
        )
    }

    #[test]
    fn rounds_to_whole_pixels_with_ties_toward_positive_infinity() {
        for (units, pixels) in [
            (32, 1),
            (31, 0),
            (-32, 0),
            (-33, -1),
            (0, 0),
            (64, 1),
            (95, 1),
            (96, 2),
            (-96, -1),
            (-97, -2),
        ] {
            assert_eq!(round(fx(units)), pixels, "{units}");
        }
        // As (x + 32) >> 6 in a wider type, without overflow at the ends.
        for units in [i32::MIN, i32::MIN + 31, -1, 1, i32::MAX - 31, i32::MAX] {
            let wide = (i64::from(units) + 32) >> 6;
            assert_eq!(i64::from(round(fx(units))), wide, "{units}");
        }
    }

    #[test]
    fn rounds_an_f32_by_its_exact_fraction() {
        for (value, units) in [
            (0.5_f32, 1),
            (-0.5, 0),
            (2.5, 3),
            (-2.5, -2),
            (0.0, 0),
            (-0.0, 0),
            (0.499_999_97, 0),
            (-0.499_999_97, 0),
            (-0.500_000_06, -1),
            (1.25, 1),
            (-1.75, -2),
            (8_388_607.5, 8_388_608),
            (-8_388_607.5, -8_388_607),
            (16_777_216.0, 16_777_216),
            (-2_147_483_648.0, i32::MIN),
            (2_147_483_520.0, 2_147_483_520),
        ] {
            assert_eq!(round(value), Some(fx(units)), "{value}");
        }
        for value in [
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
            2_147_483_648.0,
            -2_147_483_904.0,
        ] {
            assert_eq!(round(value), None, "{value}");
        }
    }

    #[test]
    fn arithmetic_returns_none_on_overflow() {
        assert_eq!(Fx::MAX.checked_add(fx(1)), None);
        assert_eq!(Fx::MIN.checked_sub(fx(1)), None);
        assert_eq!(Fx::MAX.checked_mul(px(2)), None);
        assert_eq!(Fx::MIN.checked_div(fx(-64)), None);
        assert_eq!(Fx::from_pixels(1 << 25), None);
        assert_eq!(fx(64).checked_div(Fx::ZERO), None);
        assert_eq!(Fx::MAX.checked_add(fx(0)), Some(Fx::MAX));
        assert_eq!(Fx::MAX.checked_mul(px(1)), Some(Fx::MAX));
    }

    #[test]
    fn multiplication_and_division_round_their_exact_result() {
        // 1/64 times 1/2 is 1/128 pixel: half a unit, rounded up.
        assert_eq!(fx(1).checked_mul(fx(32)), Some(fx(1)));
        assert_eq!(fx(-1).checked_mul(fx(32)), Some(fx(0)));
        assert_eq!(fx(1).checked_mul(fx(31)), Some(fx(0)));
        assert_eq!(px(3).checked_mul(px(-2)), Some(px(-6)));
        // One pixel divided by three is 21.33 units; two by three 42.67.
        assert_eq!(px(1).checked_div(px(3)), Some(fx(21)));
        assert_eq!(px(2).checked_div(px(3)), Some(fx(43)));
        assert_eq!(px(-2).checked_div(px(3)), Some(fx(-43)));
        assert_eq!(px(2).checked_div(px(-3)), Some(fx(-43)));
        // 1/128 pixel: ties toward positive infinity on either side of 0.
        assert_eq!(fx(1).checked_div(px(2)), Some(fx(1)));
        assert_eq!(fx(-1).checked_div(px(2)), Some(fx(0)));
        assert_eq!(fx(1).checked_div(px(-2)), Some(fx(0)));
    }

    #[test]
    fn points_translate_with_checked_overflow() {
        let point = Point::new(fx(1), fx(-1));
        assert_eq!(
            point.translated(fx(2), fx(3)),
            Some(Point::new(fx(3), fx(2)))
        );
        assert_eq!(point.translated(Fx::MAX, fx(0)), None);
        assert_eq!(point.translated(fx(0), Fx::MIN), None);
    }

    #[test]
    fn sizes_are_never_negative() {
        assert_eq!(Size::new(fx(-1), fx(0)), None);
        assert_eq!(Size::new(fx(0), fx(-1)), None);
        let size = Size::new(fx(0), Fx::MAX).unwrap();
        assert_eq!((size.width(), size.height()), (fx(0), Fx::MAX));
        assert_eq!(Size::new(fx(0), fx(0)), Some(Size::ZERO));
    }

    #[test]
    fn rectangles_include_their_left_and_top_edges_only() {
        let r = rect(0, 0, 64, 64);
        assert_eq!(r.contains(Point::new(fx(0), fx(0))), Some(true));
        assert_eq!(r.contains(Point::new(fx(63), fx(63))), Some(true));
        assert_eq!(r.contains(Point::new(fx(64), fx(0))), Some(false));
        assert_eq!(r.contains(Point::new(fx(0), fx(64))), Some(false));
        assert_eq!(r.contains(Point::new(fx(-1), fx(0))), Some(false));
        let empty = rect(5, 5, 0, 10);
        assert_eq!(empty.contains(Point::new(fx(5), fx(5))), Some(false));
        let far = rect(i32::MAX, 0, 1, 1);
        assert_eq!(far.contains(Point::new(fx(0), fx(0))), None);
    }

    #[test]
    fn intersections_clip_and_check_overflow() {
        let a = rect(0, 0, 100, 100);
        assert_eq!(
            a.intersection(rect(50, -10, 100, 30)),
            Some(rect(50, 0, 50, 20))
        );
        assert_eq!(a.intersection(a), Some(a));
        // Apart, and sharing an edge: no point in common.
        let apart = a.intersection(rect(200, 300, 10, 10)).unwrap();
        assert_eq!(apart.size(), Size::ZERO);
        let touching = a.intersection(rect(100, 0, 10, 10)).unwrap();
        assert_eq!(touching.contains(Point::new(fx(100), fx(0))), Some(false));
        assert_eq!(touching.size().width(), fx(0));
        // An edge that does not fit.
        assert_eq!(a.intersection(rect(0, i32::MAX, 0, 1)), None);
        assert_eq!(rect(i32::MAX, 0, 1, 0).intersection(a), None);
        // Two rectangles at the ends of the range.
        let low = rect(i32::MIN, i32::MIN, i32::MAX, i32::MAX);
        let high = rect(0, 0, i32::MAX, i32::MAX);
        assert_eq!(low.intersection(high).unwrap().size(), Size::ZERO);
        let left_end = rect(i32::MIN, 0, 0, 1);
        let right_end = rect(i32::MAX - 1, 0, 1, 1);
        assert_eq!(
            left_end.intersection(right_end),
            Some(rect(i32::MAX - 1, 0, 0, 1))
        );
    }
}
