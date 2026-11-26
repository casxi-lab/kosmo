//! A glyph coverage for tests: three glyphs of one font and size, whose
//! masks come either from their outlines through `fill_coverage` or, with
//! coverage of only 0 and 255, from their shapes in whole pixels.

use std::collections::BTreeMap;

use kosmo_render::{
    CoverageMask, FontId, GlyphCoverage, GlyphId, GlyphKey, Path, Rasterizer, Step,
};
use kosmo_types::{Fx, Point};

use super::{Checked, failed};

/// The font of the test glyphs.
pub const FONT: FontId = FontId::new(1);

/// Their size, 12 pixels per em.
pub const SIZE: Fx = Fx::from_units(12 * 64);

/// A box with a square hole, an arch and a ring.
pub const GLYPHS: [GlyphId; 3] = [GlyphId::new(1), GlyphId::new(2), GlyphId::new(3)];

/// Where every mask lies relative to the whole pixel of its glyph's
/// origin, and its size: the glyphs lie from 0 to 8 pixels to the right of
/// the origin and up to 10 pixels above the baseline.
const LEFT: i32 = -1;
const TOP: i32 = -11;
const WIDTH: u32 = 10;
const HEIGHT: u32 = 12;

/// The key of `glyph` at `step`.
pub fn key(glyph: GlyphId, step: Step) -> GlyphKey {
    GlyphKey {
        font: FONT,
        size: SIZE,
        glyph,
        step,
    }
}

/// The point `x`, `y` in 1/64 pixel, moved right by `step`.
fn at(x: i32, y: i32, step: Step) -> Point {
    Point::new(Fx::from_units(x + step.offset().units()), Fx::from_units(y))
}

/// The outline of `glyph` at `step`, relative to the whole pixel of its
/// origin, in 1/64 pixel.
pub fn outline(glyph: GlyphId, step: Step) -> Path {
    let p = |x, y| at(x, y, step);
    match glyph.get() {
        1 => {
            // The box clockwise and its hole counterclockwise.
            let mut path = Path::new(p(0, -640));
            path.line_to(p(448, -640))
                .line_to(p(448, 0))
                .line_to(p(0, 0))
                .close();
            path.move_to(p(128, -512))
                .line_to(p(128, -128))
                .line_to(p(320, -128))
                .line_to(p(320, -512))
                .close();
            path
        }
        2 => {
            // An arch: a triangle with a curved cut from its base.
            let mut path = Path::new(p(0, 0));
            path.line_to(p(224, -640))
                .line_to(p(448, 0))
                .line_to(p(352, 0))
                .quad_to(p(224, -416), p(96, 0))
                .close();
            path
        }
        _ => {
            // A ring: two circles of four cubic segments each, around
            // (3.5, -5) pixels with radii 3.5 and 2 pixels; 0.5523 times
            // the radius places the control points.
            let mut path = Path::new(p(448, -320));
            path.cubic_to(p(448, -196), p(348, -96), p(224, -96))
                .cubic_to(p(100, -96), p(0, -196), p(0, -320))
                .cubic_to(p(0, -444), p(100, -544), p(224, -544))
                .cubic_to(p(348, -544), p(448, -444), p(448, -320))
                .close();
            path.move_to(p(352, -320))
                .cubic_to(p(352, -391), p(295, -448), p(224, -448))
                .cubic_to(p(153, -448), p(96, -391), p(96, -320))
                .cubic_to(p(96, -249), p(153, -192), p(224, -192))
                .cubic_to(p(295, -192), p(352, -249), p(352, -320))
                .close();
            path
        }
    }
}

/// Whether the whole-pixel shape of `glyph` covers the pixel at `x`, `y`
/// pixels from the origin.
fn covers(glyph: GlyphId, x: i32, y: i32) -> bool {
    let inside = |left, top, right, bottom| left <= x && x < right && top <= y && y < bottom;
    match glyph.get() {
        1 => inside(0, -10, 7, 0) && !inside(2, -8, 5, -2),
        // Rows that widen from one pixel at the top, with a gap at the base.
        2 => inside(0, -10, 7, 0) && (x - 3).abs() <= (y + 11) / 3 && !inside(2, -3, 5, 0),
        _ => inside(1, -9, 6, -1) && !inside(2, -7, 5, -3),
    }
}

/// A mask of the size and position of every test glyph, with no coverage.
fn empty() -> Checked<CoverageMask> {
    CoverageMask::new(LEFT, TOP, WIDTH, HEIGHT).ok_or_else(|| failed("the mask is too large"))
}

/// Coverage masks by key.
#[derive(Clone, Debug, Default)]
pub struct TestGlyphs {
    masks: BTreeMap<GlyphKey, CoverageMask>,
}

impl TestGlyphs {
    /// No masks.
    pub fn none() -> TestGlyphs {
        TestGlyphs::default()
    }

    /// The masks of `keys`, each filled from its outline by `rasterizer`.
    pub fn filled(rasterizer: &mut dyn Rasterizer, keys: &[GlyphKey]) -> Checked<TestGlyphs> {
        let mut masks = BTreeMap::new();
        for &key in keys {
            let mut mask = empty()?;
            rasterizer
                .fill_coverage(&outline(key.glyph, key.step), &mut mask)
                .map_err(failed)?;
            masks.insert(key, mask);
        }
        Ok(TestGlyphs { masks })
    }

    /// The masks of the three glyphs at the whole-pixel step, with coverage
    /// 255 on the pixels of their whole-pixel shapes and 0 elsewhere.
    pub fn binary() -> Checked<TestGlyphs> {
        let mut masks = BTreeMap::new();
        for glyph in GLYPHS {
            let mut mask = empty()?;
            for (index, value) in mask.coverage_mut().iter_mut().enumerate() {
                let (column, row) = (index as i32 % WIDTH as i32, index as i32 / WIDTH as i32);
                if covers(glyph, LEFT + column, TOP + row) {
                    *value = 255;
                }
            }
            masks.insert(key(glyph, Step::Whole), mask);
        }
        Ok(TestGlyphs { masks })
    }

    /// The keys that have masks.
    pub fn keys(&self) -> Vec<GlyphKey> {
        self.masks.keys().copied().collect()
    }
}

impl GlyphCoverage for TestGlyphs {
    fn coverage(&self, key: GlyphKey) -> Option<&CoverageMask> {
        self.masks.get(&key)
    }
}
