//! The renderer interface: the [`Rasterizer`] trait, which states the
//! pixel semantics of display lists, the kept previous frame
//! [`RenderTarget`], the glyph coverage that rendering reads, and the work
//! count and errors of rendering.

use alloc::vec;
use alloc::vec::Vec;
use core::fmt;

use kosmo_types::{
    BufferDesc, ConvertError, Damage, DamageRect, Fx, MAX_BUFFER_SIDE, PixelFormat, convert,
};

use crate::list::{DisplayList, FontId, GlyphId, Path};

/// A rasterizer: it draws display lists into a kept previous frame and
/// fills outlines into coverage masks.
///
/// # Pixel semantics
///
/// Coordinates are [`Fx`] values, in 1/64 pixel, with `x` growing to the
/// right and `y` growing down. Pixel (x, y) of the target is the square
/// from (x, y) to (x + 1, y + 1) in pixels; row 0 is at the top. The
/// coverage of a pixel by a shape is the share of the pixel's area inside
/// the shape, as an 8-bit value from 0 to 255 that the rasterizer computes.
///
/// A command is composited with source over: its source is its color with
/// each component multiplied by the coverage, divided by 255, and each
/// premultiplied component of the pixel becomes the source plus the pixel
/// times 255 minus the source's alpha, divided by 255. The rasterizer
/// rounds these products; an opaque source with full coverage replaces the
/// pixel exactly, and zero coverage leaves it exactly as it was.
///
/// - [`FillRect`](crate::Command::FillRect): the rectangle, its left and
///   top edges included and its right and bottom edges excluded. A
///   rectangle without width or height covers nothing.
/// - [`FillRoundedRect`](crate::Command::FillRoundedRect): the rectangle
///   with each corner replaced by a quarter circle of the radius, or of
///   half the shorter side of the rectangle when the radius is greater.
/// - [`FillPath`](crate::Command::FillPath): the points of the path's
///   subpaths, each closed by a straight segment to its start, whose
///   winding number is not zero.
/// - [`StrokePath`](crate::Command::StrokePath): the area that a line of
///   the width, centered on the path and perpendicular to it, sweeps along
///   each segment, with the [`Join`](crate::Join) at every joint, also
///   where a closed subpath ends, and the [`Cap`](crate::Cap) at both ends
///   of each open subpath. A miter join extends the outer edges until they
///   meet, or is a bevel join where they would meet farther than 4 half
///   widths from the joint; a round join adds a circle of the half width
///   around the joint; a bevel join adds the straight edge between the
///   outer corners. A butt cap ends at the end point, a round cap adds a
///   half circle of the half width, and a square cap goes on by the half
///   width. A stroke without width covers nothing. The rasterizer computes
///   the outline of the stroke.
/// - [`Glyphs`](crate::Command::Glyphs): each glyph's origin (x, y), with
///   the offsets of the enclosing groups added, is placed as
///   [`place_glyph`] states: x to the nearest quarter pixel and y to the
///   nearest whole pixel. The [`CoverageMask`] that the [`GlyphCoverage`]
///   gives for the font, the size, the glyph and the quarter-pixel step
///   lies with its top-left pixel at the whole pixel of the origin moved by
///   the mask's [`left`](CoverageMask::left) and [`top`](CoverageMask::top),
///   and its values are the coverage of the pixels under it.
/// - [`PushClip`](crate::Command::PushClip) starts a group, which ends at
///   the matching [`PopClip`](crate::Command::PopClip). Its `rect` is in
///   the coordinates outside the group: it is moved by the offsets of the
///   enclosing groups and not by the group's own `offset`. The commands
///   inside the group are moved by its `offset` and the offsets of the
///   enclosing groups, added together, and their coverage is multiplied by
///   the coverage of its `rect` and of the clips of the enclosing groups,
///   so nested clips intersect. A clip with whole-pixel edges has coverage
///   255 inside and 0 outside.
///
/// # Rendering
///
/// [`Rasterizer::render`] draws `list` into `target`, which holds the
/// previous frame, and writes only inside `damage`, a region of a buffer of
/// the target's size. For each rectangle of the region, in order, every
/// pixel inside it starts as transparent black, (0, 0, 0, 0) in
/// `Rgba8Premul`, the commands are drawn in order, and the rectangle is
/// written into the target, a tightly packed `Rgba8Premul` buffer.
/// Drawing is clipped to the rectangle, and pixels outside every rectangle
/// of the region keep the bytes they had. A pixel inside the region does
/// not depend on the previous frame or on the other rectangles, so drawing
/// only the damage over the previous frame gives, byte for byte, the frame
/// that a whole redraw gives.
///
/// Before it writes anything, `render` checks the input: it returns
/// [`RenderError::DamageMismatch`] when `damage` is not a region of a
/// buffer of the target's size, [`RenderError::MissingGlyph`] when the
/// glyph coverage has no mask for a glyph of the list, wherever the glyph
/// lies, [`RenderError::Unsupported`] for a command that the rasterizer does
/// not draw and [`RenderError::OutOfRange`] when an origin, or the offsets
/// of nested groups, added together do not fit in an [`Fx`]. After an
/// error the target is as it was. On success it returns the [`WorkCount`]
/// of the list and the region.
///
/// `glyphs` and `images` are read only. Rendering starts no threads and
/// reads no clock and no file, so the same input gives the same bytes on
/// every run.
///
/// # Coverage
///
/// [`Rasterizer::fill_coverage`] fills `outline` with the nonzero winding
/// rule into `out`, as [`CoverageMask`] states, with the same rasterization
/// as [`FillPath`](crate::Command::FillPath).
pub trait Rasterizer {
    /// Draws `list` into the previous frame `target` inside `damage`, as
    /// the [trait](Rasterizer) states.
    fn render(
        &mut self,
        list: &DisplayList,
        damage: &Damage,
        glyphs: &dyn GlyphCoverage,
        images: &ImageStore,
        target: &mut RenderTarget,
    ) -> Result<WorkCount, RenderError>;

    /// Fills `outline` into `out`, replacing its values, as the
    /// [trait](Rasterizer) states. An outline of more than
    /// [`MAX_ITEMS`](crate::MAX_ITEMS) elements gives
    /// [`RenderError::OverLimit`] and leaves `out` as it was.
    fn fill_coverage(&mut self, outline: &Path, out: &mut CoverageMask) -> Result<(), RenderError>;
}

/// Why a rasterizer drew nothing.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RenderError {
    /// The input is beyond a limit of the rasterizer.
    OverLimit,
    /// The rasterizer does not draw a command of the list, or does not fill
    /// coverage.
    Unsupported,
    /// The glyph coverage has no mask for this glyph.
    MissingGlyph(GlyphKey),
    /// The damage region is not a region of a buffer of the target's size.
    DamageMismatch,
    /// A position with the offsets of its groups does not fit in an
    /// [`Fx`].
    OutOfRange,
}

/// The work of one call of [`Rasterizer::render`]: a count that depends
/// only on the display list and the damage region, so it is the same on
/// every host and for every rasterizer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct WorkCount {
    /// The rectangles of the damage region that were drawn.
    pub rects: u32,
    /// The commands drawn: the commands of the list once for each
    /// rectangle.
    pub commands: u64,
    /// The pixels written: the areas of the rectangles added together.
    pub pixels: u64,
}

impl WorkCount {
    /// The work count of drawing `list` inside `damage`.
    pub fn of(list: &DisplayList, damage: &Damage) -> WorkCount {
        let rects = damage.rects();
        // At most 16 rectangles and 16,384 commands.
        let count = rects.len() as u64;
        WorkCount {
            rects: count as u32,
            commands: count * list.commands().len() as u64,
            pixels: rects
                .iter()
                .map(|rect| u64::from(rect.width()) * u64::from(rect.height()))
                .sum(),
        }
    }
}

/// The previous frame that a rasterizer draws into and that rendering
/// keeps: a tightly packed `Rgba8Premul` buffer of the [`BufferDesc`] it
/// was made with, which lies within [`MAX_BUFFER_SIDE`] and
/// [`MAX_BUFFER_BYTES`](kosmo_types::MAX_BUFFER_BYTES), with its bytes.
/// Before presentation, [`convert_into`](RenderTarget::convert_into)
/// converts each damage rectangle into the buffer to be presented, of any
/// pixel format; repainting by buffer age, for double buffering, converts
/// any rectangle again.
#[derive(Clone, PartialEq, Eq)]
pub struct RenderTarget {
    desc: BufferDesc,
    bytes: Vec<u8>,
}

impl RenderTarget {
    /// A target of `desc` whose bytes are all 0, or `None` when `desc` is
    /// not a tightly packed `Rgba8Premul` buffer.
    pub fn new(desc: BufferDesc) -> Option<RenderTarget> {
        tight(desc).then(|| RenderTarget {
            desc,
            bytes: vec![0; desc.byte_len() as usize],
        })
    }

    /// A target of `desc` that holds `bytes`, or `None` when `desc` is not
    /// a tightly packed `Rgba8Premul` buffer or the bytes are not as many
    /// as the description's [`byte_len`](BufferDesc::byte_len).
    pub fn from_bytes(desc: BufferDesc, bytes: Vec<u8>) -> Option<RenderTarget> {
        (tight(desc) && bytes.len() == desc.byte_len() as usize)
            .then_some(RenderTarget { desc, bytes })
    }

    /// The description of the buffer.
    pub fn desc(&self) -> BufferDesc {
        self.desc
    }

    /// The bytes of the buffer.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// The bytes of the buffer, as a rasterizer writes them.
    pub fn bytes_mut(&mut self) -> &mut [u8] {
        &mut self.bytes
    }

    /// The bytes and the description.
    pub fn into_parts(self) -> (BufferDesc, Vec<u8>) {
        (self.desc, self.bytes)
    }

    /// The region of the whole buffer.
    pub fn whole(&self) -> Damage {
        let mut damage = Damage::new(self.desc);
        damage.add(0, 0, self.desc.width(), self.desc.height());
        damage
    }

    /// Whether `damage` is a region of a buffer of the target's width and
    /// height: adding its rectangles in order to an empty region of the
    /// target gives the same region.
    pub fn accepts(&self, damage: &Damage) -> bool {
        let mut own = Damage::new(self.desc);
        for rect in damage.rects() {
            own.add(rect.x(), rect.y(), rect.width(), rect.height());
        }
        own == *damage
    }

    /// Writes each rectangle of `damage` into the buffer to be presented,
    /// described by `target` and held in `target_bytes`, at the same place,
    /// converted to the target's format with [`kosmo_types::convert`];
    /// every other byte of the target stays as it was. It writes nothing
    /// and returns an error when the target bytes are not as many as the
    /// description says, or when a rectangle does not lie inside this
    /// frame or the target.
    pub fn convert_into(
        &self,
        damage: &Damage,
        target: BufferDesc,
        target_bytes: &mut [u8],
    ) -> Result<(), ConvertError> {
        if target_bytes.len() as u64 != u64::from(target.byte_len()) {
            return Err(ConvertError::TargetLength);
        }
        let inside = |rect: &DamageRect, desc: BufferDesc| {
            u64::from(rect.x()) + u64::from(rect.width()) <= u64::from(desc.width())
                && u64::from(rect.y()) + u64::from(rect.height()) <= u64::from(desc.height())
        };
        for rect in damage.rects() {
            if !inside(rect, self.desc) {
                return Err(ConvertError::OutsideSource);
            }
            if !inside(rect, target) {
                return Err(ConvertError::OutsideTarget);
            }
        }
        for rect in damage.rects() {
            convert(
                self.desc,
                &self.bytes,
                *rect,
                target,
                target_bytes,
                rect.x(),
                rect.y(),
            )?;
        }
        Ok(())
    }
}

/// Whether `desc` is a tightly packed `Rgba8Premul` buffer.
fn tight(desc: BufferDesc) -> bool {
    desc.format() == PixelFormat::Rgba8Premul
        && u64::from(desc.stride()) == u64::from(desc.width()) * 4
}

impl fmt::Debug for RenderTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RenderTarget")
            .field("desc", &self.desc)
            .field("bytes", &self.bytes.len())
            .finish()
    }
}

/// The horizontal position of a glyph within a pixel, in quarter pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Step {
    /// At the whole pixel.
    Whole,
    /// A quarter pixel to the right of it.
    Quarter,
    /// Half a pixel to the right of it.
    Half,
    /// Three quarters of a pixel to the right of it.
    ThreeQuarters,
}

impl Step {
    /// The four steps, from the whole pixel to the right.
    pub const ALL: [Step; 4] = [Step::Whole, Step::Quarter, Step::Half, Step::ThreeQuarters];

    /// How far the step lies to the right of the whole pixel.
    pub const fn offset(self) -> Fx {
        Fx::from_units(match self {
            Step::Whole => 0,
            Step::Quarter => 16,
            Step::Half => 32,
            Step::ThreeQuarters => 48,
        })
    }
}

/// Where a glyph lies in the target: the whole pixel of its origin and the
/// quarter-pixel step to the right of it, and the row of its baseline.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Placement {
    /// The column of the whole pixel.
    pub x: i32,
    /// The step within the pixel.
    pub step: Step,
    /// The row of the baseline.
    pub baseline: i32,
}

/// The placement of a glyph whose origin, with the offsets of its groups
/// added, is (`x`, `y`): `x` rounded to the nearest quarter pixel as
/// `q = (x + 8) >> 4`, an arithmetic shift, whose whole pixel is `q >> 2`
/// and step `q & 3`, and `y` rounded to the nearest whole pixel by
/// [`kosmo_types::round`]; ties go toward positive infinity.
///
/// ```
/// use kosmo_render::{Step, place_glyph};
/// use kosmo_types::Fx;
///
/// // 10 pixels and 24/64: the nearest quarter is 10 + 2/4.
/// let placement = place_glyph(Fx::from_units(664), Fx::from_units(-33));
/// assert_eq!((placement.x, placement.step, placement.baseline), (10, Step::Half, -1));
/// ```
pub fn place_glyph(x: Fx, y: Fx) -> Placement {
    let quarters = (i64::from(x.units()) + 8) >> 4;
    let step = match quarters & 3 {
        0 => Step::Whole,
        1 => Step::Quarter,
        2 => Step::Half,
        _ => Step::ThreeQuarters,
    };
    Placement {
        // Within the range of i32, as x / 64 is.
        x: (quarters >> 2) as i32,
        step,
        baseline: kosmo_types::round(y),
    }
}

/// What a [`GlyphCoverage`] gives a mask for: a glyph of a font at a size
/// and a quarter-pixel step.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GlyphKey {
    /// The font.
    pub font: FontId,
    /// The size of the font.
    pub size: Fx,
    /// The glyph.
    pub glyph: GlyphId,
    /// The step of the glyph's origin within its pixel.
    pub step: Step,
}

/// The coverage masks of glyphs, prepared before drawing, which a
/// rasterizer reads while it draws [`Command::Glyphs`](crate::Command::Glyphs).
pub trait GlyphCoverage {
    /// The mask of `key`, or `None` when there is none.
    fn coverage(&self, key: GlyphKey) -> Option<&CoverageMask>;
}

/// A glyph coverage without masks, for display lists without glyphs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct NoGlyphs;

impl GlyphCoverage for NoGlyphs {
    fn coverage(&self, _key: GlyphKey) -> Option<&CoverageMask> {
        None
    }
}

/// An 8-bit coverage mask: `width` by `height` values, row by row from the
/// top, each the coverage of one pixel from 0 to 255.
///
/// The mask lies at a position relative to an origin: its value (i, j)
/// covers the pixel from (`left` + i, `top` + j) to (`left` + i + 1,
/// `top` + j + 1), in pixels from the origin. For a glyph, the origin is
/// the whole pixel of the glyph's placement, and the glyph's outline at the
/// placement's step is filled relative to it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CoverageMask {
    left: i32,
    top: i32,
    width: u32,
    height: u32,
    coverage: Vec<u8>,
}

impl CoverageMask {
    /// A mask of `width` by `height` values, all 0, with its top-left
    /// value at (`left`, `top`), or `None` when a side is longer than
    /// [`MAX_BUFFER_SIDE`], so that the values are at most
    /// [`MAX_BUFFER_BYTES`](kosmo_types::MAX_BUFFER_BYTES). A mask may have
    /// no width or no height; it then covers nothing.
    pub fn new(left: i32, top: i32, width: u32, height: u32) -> Option<CoverageMask> {
        if width > MAX_BUFFER_SIDE || height > MAX_BUFFER_SIDE {
            return None;
        }
        Some(CoverageMask {
            left,
            top,
            width,
            height,
            // At most 2^26 values.
            coverage: vec![0; width as usize * height as usize],
        })
    }

    /// The column of the left values, relative to the origin.
    pub fn left(&self) -> i32 {
        self.left
    }

    /// The row of the top values, relative to the origin.
    pub fn top(&self) -> i32 {
        self.top
    }

    /// The width in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// The height in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// The values, row by row from the top.
    pub fn coverage(&self) -> &[u8] {
        &self.coverage
    }

    /// The values, to write them.
    pub fn coverage_mut(&mut self) -> &mut [u8] {
        &mut self.coverage
    }
}

/// The images that a rasterizer reads while it draws. Display lists have
/// no image commands yet, so the store holds no images.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ImageStore {
    _images: (),
}

impl ImageStore {
    /// The store without images.
    pub const EMPTY: ImageStore = ImageStore { _images: () };
}

#[cfg(test)]
mod tests {
    use kosmo_types::MAX_BUFFER_BYTES;

    use super::*;

    fn target(width: u32, height: u32) -> RenderTarget {
        RenderTarget::new(
            BufferDesc::new(width, height, width * 4, PixelFormat::Rgba8Premul).unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn a_target_accepts_only_regions_of_buffers_of_its_size() {
        let small = target(8, 8);
        let mut damage = Damage::new(small.desc());
        damage.add(2, 2, 4, 4);
        assert!(small.accepts(&damage));
        assert!(small.accepts(&small.whole()));
        assert!(!target(9, 8).accepts(&damage));
        let mut larger = Damage::new(target(16, 16).desc());
        larger.add(2, 2, 4, 4);
        assert!(!small.accepts(&larger));
    }

    #[test]
    fn a_target_from_bytes_needs_the_length_of_its_description() {
        let desc = BufferDesc::new(2, 2, 8, PixelFormat::Rgba8Premul).unwrap();
        assert_eq!(RenderTarget::from_bytes(desc, vec![0; 15]), None);
        assert_eq!(
            RenderTarget::from_bytes(desc, vec![7; 16]).map(|t| t.bytes()[15]),
            Some(7)
        );
    }

    #[test]
    fn a_target_is_a_tightly_packed_rgba8_premul_buffer() {
        let desc = |stride, format| BufferDesc::new(2, 2, stride, format).unwrap();
        assert!(RenderTarget::new(desc(8, PixelFormat::Rgba8Premul)).is_some());
        assert_eq!(RenderTarget::new(desc(12, PixelFormat::Rgba8Premul)), None);
        assert_eq!(RenderTarget::new(desc(8, PixelFormat::Xrgb8888)), None);
        assert_eq!(RenderTarget::new(desc(4, PixelFormat::Rgb565)), None);
        let bytes = |len| vec![0; len];
        assert_eq!(
            RenderTarget::from_bytes(desc(12, PixelFormat::Rgba8Premul), bytes(24)),
            None
        );
        assert_eq!(
            RenderTarget::from_bytes(desc(8, PixelFormat::Argb8888Premul), bytes(16)),
            None
        );
    }

    #[test]
    fn the_damage_is_converted_into_the_buffer_to_present() {
        // A 3 by 2 frame, pixel (x, y) of red x * 10 + y, green 1, blue 2.
        let mut frame = target(3, 2);
        for (i, pixel) in frame.bytes_mut().chunks_mut(4).enumerate() {
            let (x, y) = (i % 3, i / 3);
            pixel.copy_from_slice(&[(x * 10 + y) as u8, 1, 2, 255]);
        }
        let mut damage = Damage::new(frame.desc());
        damage.add(1, 0, 1, 1);
        damage.add(2, 1, 1, 1);
        let shown = BufferDesc::new(3, 2, 16, PixelFormat::Xrgb8888).unwrap();
        let mut bytes = vec![0x11; 32];
        frame.convert_into(&damage, shown, &mut bytes).unwrap();
        let mut expected = vec![0x11; 32];
        // B, G, R, X at (1, 0) and at (2, 1), in rows of 16 bytes.
        expected[4..8].copy_from_slice(&[2, 1, 10, 0xFF]);
        expected[24..28].copy_from_slice(&[2, 1, 21, 0xFF]);
        assert_eq!(bytes, expected);
    }

    #[test]
    fn a_conversion_that_cannot_be_done_writes_nothing() {
        let frame = target(4, 4);
        let mut damage = Damage::new(frame.desc());
        damage.add(0, 0, 1, 1);
        damage.add(3, 3, 1, 1);
        let small = BufferDesc::new(3, 3, 12, PixelFormat::Rgba8Premul).unwrap();
        let mut bytes = vec![9; 36];
        assert_eq!(
            frame.convert_into(&damage, small, &mut bytes),
            Err(ConvertError::OutsideTarget)
        );
        assert!(bytes.iter().all(|&byte| byte == 9));
        assert_eq!(
            frame.convert_into(&damage, small, &mut [9; 35]),
            Err(ConvertError::TargetLength)
        );
        let mut larger = Damage::new(target(8, 8).desc());
        larger.add(6, 6, 1, 1);
        let shown = BufferDesc::new(8, 8, 32, PixelFormat::Rgba8Premul).unwrap();
        let mut bytes = vec![9; 256];
        assert_eq!(
            frame.convert_into(&larger, shown, &mut bytes),
            Err(ConvertError::OutsideSource)
        );
        assert!(bytes.iter().all(|&byte| byte == 9));
    }

    #[test]
    fn glyph_origins_go_to_the_nearest_quarter_and_whole_pixel() {
        let place = |x, y| place_glyph(Fx::from_units(x), Fx::from_units(y));
        assert_eq!(
            place(0, 0),
            Placement {
                x: 0,
                step: Step::Whole,
                baseline: 0
            }
        );
        assert_eq!(place(7, 31).step, Step::Whole);
        assert_eq!(
            place(8, 32),
            Placement {
                x: 0,
                step: Step::Quarter,
                baseline: 1
            }
        );
        assert_eq!(
            place(56, 0),
            Placement {
                x: 1,
                step: Step::Whole,
                baseline: 0
            }
        );
        assert_eq!(
            place(-9, -33),
            Placement {
                x: -1,
                step: Step::ThreeQuarters,
                baseline: -1
            }
        );
        assert_eq!(place(-8, -32).step, Step::Whole);
    }

    #[test]
    fn a_mask_is_bounded_like_a_buffer() {
        assert!(CoverageMask::new(0, 0, MAX_BUFFER_SIDE, 0).is_some());
        assert!(CoverageMask::new(0, 0, MAX_BUFFER_SIDE + 1, 1).is_none());
        assert!(CoverageMask::new(0, 0, 1, MAX_BUFFER_SIDE + 1).is_none());
        assert_eq!(
            u64::from(MAX_BUFFER_SIDE) * u64::from(MAX_BUFFER_SIDE),
            u64::from(MAX_BUFFER_BYTES)
        );
        assert_eq!(
            CoverageMask::new(-1, -2, 3, 2).map(|mask| mask.coverage().len()),
            Some(6)
        );
    }

    #[test]
    fn the_work_count_depends_on_the_list_and_the_region_only() {
        let whole = target(10, 5).whole();
        let count = WorkCount::of(&DisplayList::default(), &whole);
        assert_eq!(
            count,
            WorkCount {
                rects: 1,
                commands: 0,
                pixels: 50
            }
        );
    }
}
