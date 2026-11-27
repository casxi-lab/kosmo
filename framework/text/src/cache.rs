//! The glyph cache: the coverage masks of the glyphs of the two fonts,
//! filled by the rasterizer from outlines that skrifa reads, bounded in
//! entries and bytes and prepared once for each frame.

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::vec::Vec;
use core::cmp::{max, min};
use core::fmt;

use kosmo_render::{
    Command, CoverageMask, DisplayList, FontId, GlyphCoverage, GlyphKey, Path, Rasterizer,
    RenderError, place_glyph,
};
use kosmo_types::{Fx, MAX_BUFFER_SIDE, Point, round};
use parley::fontique::Blob;
use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::{FontRef, GlyphId, MetadataProvider};

use crate::layout::{MAX_SIZE, MIN_SIZE};

/// The most entries of the glyph cache: 4,096. A glyph at each of the four
/// quarter-pixel steps is an entry of its own.
pub const MAX_GLYPH_ENTRIES: usize = 4_096;

/// The most coverage values of the glyph cache: 4 MiB, 4,194,304 values of
/// one byte, the widths times the heights of its masks added together.
pub const MAX_GLYPH_BYTES: usize = 4 * 1024 * 1024;

/// The coverage masks of glyphs, which a [`FontSet`](crate::FontSet) keeps
/// for its two fonts and drops with them.
///
/// [`FontSet::prepare`](crate::FontSet::prepare) fills the masks that a
/// display list needs before it is drawn, and the cache is then handed to
/// the rasterizer, read only, as the list's [`GlyphCoverage`]. A key names
/// a font by its [`FontId`], which stands for the bytes of that font in
/// this font set alone; a font set built from other bytes has a cache of
/// its own, which starts empty.
///
/// The cache counts its own entries and the bytes of their masks, at most
/// [`MAX_GLYPH_ENTRIES`] and [`MAX_GLYPH_BYTES`]. Each preparation is a
/// frame with a number one greater than the last. It first marks every key
/// of the frame as used in it; when a new entry would bring the cache
/// beyond either limit, it evicts entries not used in the frame, in
/// ascending order of the frame that last used them and then of their key,
/// until the new entry fits.
///
/// A frame that alone exceeds a limit is refused before any of its masks
/// is allocated. The cache counts the keys of the frame first and refuses
/// a frame with more keys than it holds entries before it reads any
/// outline. It then measures the mask of each key that it does not hold
/// from the bounds of the points of its outline, without building a path,
/// adds the bytes of the masks that it holds, and refuses a frame beyond
/// its bytes. Only then are the outlines of the new keys built and their
/// masks allocated and filled, one outline at a time: a preparation
/// allocates masks of at most the bytes of the cache, and a refused frame
/// none. Dropping masks and filling them again gives the same bytes, so
/// what is drawn does not depend on what the cache held before.
pub struct GlyphCache {
    fonts: [Blob<u8>; 2],
    max_entries: usize,
    max_bytes: usize,
    entries: BTreeMap<GlyphKey, Entry>,
    /// The keys of `entries` by the frame that last used them.
    order: BTreeSet<(u64, GlyphKey)>,
    bytes: usize,
    frame: u64,
}

/// A mask and the frame that last used it.
struct Entry {
    mask: CoverageMask,
    used: u64,
}

/// What [`FontSet::prepare`](crate::FontSet::prepare) did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Prepared {
    /// The keys of the display list.
    pub keys: usize,
    /// The masks filled through the rasterizer.
    pub filled: usize,
    /// The entries evicted to make room.
    pub evicted: usize,
}

/// Why a frame could not be prepared. The cache is then as it was.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GlyphError {
    /// The frame alone needs more entries or bytes than the cache holds:
    /// its keys and the bytes of their masks.
    FrameTooLarge {
        /// The keys of the frame.
        entries: usize,
        /// The bytes of their masks, counted only when the keys fit in the
        /// entries of the cache, and 0 otherwise: a frame with more keys is
        /// refused before any outline is read.
        bytes: usize,
    },
    /// The font is neither [`SANS`](crate::SANS) nor
    /// [`MONO`](crate::MONO).
    UnknownFont(FontId),
    /// The size lies outside [`MIN_SIZE`](crate::MIN_SIZE) to
    /// [`MAX_SIZE`](crate::MAX_SIZE).
    SizeOutOfRange(Fx),
    /// The font has no outline for the glyph.
    NoOutline(GlyphKey),
    /// An origin with the offsets of its groups added, or a point of an
    /// outline, does not fit in an [`Fx`], or a mask would be larger than a
    /// buffer.
    OutOfRange,
    /// The rasterizer could not fill a mask.
    Render(RenderError),
}

impl fmt::Display for GlyphError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GlyphError::FrameTooLarge { entries, bytes: 0 } => write!(
                f,
                "the frame needs {entries} glyph masks, more than the cache holds"
            ),
            GlyphError::FrameTooLarge { entries, bytes } => write!(
                f,
                "the frame needs {entries} glyph masks of {bytes} bytes, beyond the cache"
            ),
            GlyphError::UnknownFont(font) => {
                write!(f, "font {} is not in the font set", font.get())
            }
            GlyphError::SizeOutOfRange(size) => write!(
                f,
                "size {} is outside {} to {} units",
                size.units(),
                MIN_SIZE.units(),
                MAX_SIZE.units()
            ),
            GlyphError::NoOutline(key) => write!(
                f,
                "font {} has no outline for glyph {}",
                key.font.get(),
                key.glyph.get()
            ),
            GlyphError::OutOfRange => f.write_str("a glyph position does not fit"),
            GlyphError::Render(error) => write!(f, "the rasterizer failed: {error:?}"),
        }
    }
}

impl core::error::Error for GlyphError {}

impl GlyphCache {
    /// An empty cache for the fonts `fonts`, [`SANS`](crate::SANS) and
    /// [`MONO`](crate::MONO), that holds at most `max_entries` entries and
    /// `max_bytes` bytes.
    pub(crate) fn new(fonts: [Blob<u8>; 2], max_entries: usize, max_bytes: usize) -> GlyphCache {
        GlyphCache {
            fonts,
            max_entries,
            max_bytes,
            entries: BTreeMap::new(),
            order: BTreeSet::new(),
            bytes: 0,
            frame: 0,
        }
    }

    /// The entries.
    pub fn entries(&self) -> usize {
        self.entries.len()
    }

    /// The bytes of the masks of the entries.
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    /// The number of the last frame prepared, 0 before the first.
    pub fn frame(&self) -> u64 {
        self.frame
    }

    /// Whether the cache holds the mask of `key`.
    pub fn contains(&self, key: GlyphKey) -> bool {
        self.entries.contains_key(&key)
    }

    /// Drops every entry; frames keep their numbers.
    pub(crate) fn clear(&mut self) {
        self.entries.clear();
        self.order.clear();
        self.bytes = 0;
    }

    /// Prepares the masks of `list` as the next frame, as the
    /// [cache](GlyphCache) states.
    pub(crate) fn prepare(
        &mut self,
        list: &DisplayList,
        rasterizer: &mut dyn Rasterizer,
    ) -> Result<Prepared, GlyphError> {
        let keys = keys(list)?;
        let missing = self.missing(&keys)?;
        // The frame is within both limits: only now are the outlines of its
        // new keys built and their masks allocated, as each is filled.
        let mut fonts = Fonts::new(&self.fonts);
        let mut masks = Vec::with_capacity(missing.len());
        for key in missing {
            let (outline, mut mask) = outline(fonts.get(key)?, key)?;
            rasterizer
                .fill_coverage(&outline, &mut mask)
                .map_err(GlyphError::Render)?;
            masks.push((key, mask));
        }
        self.frame += 1;
        let frame = self.frame;
        for key in &keys {
            if let Some(entry) = self.entries.get_mut(key) {
                self.order.remove(&(entry.used, *key));
                entry.used = frame;
                self.order.insert((frame, *key));
            }
        }
        let mut evicted = 0;
        let filled = masks.len();
        for (key, mask) in masks {
            let size = mask.coverage().len();
            while self.entries.len() + 1 > self.max_entries || self.bytes + size > self.max_bytes {
                // The frame fits, so the oldest entry is of an earlier frame.
                let Some(&(used, oldest)) = self.order.first() else {
                    break;
                };
                if used == frame {
                    break;
                }
                self.order.remove(&(used, oldest));
                if let Some(entry) = self.entries.remove(&oldest) {
                    self.bytes -= entry.mask.coverage().len();
                }
                evicted += 1;
            }
            self.bytes += size;
            self.order.insert((frame, key));
            self.entries.insert(key, Entry { mask, used: frame });
        }
        Ok(Prepared {
            keys: keys.len(),
            filled,
            evicted,
        })
    }

    /// The keys of `keys` whose masks the cache does not hold, in their
    /// order, once the frame is known to be within both limits, as the
    /// [cache](GlyphCache) states. Neither a path nor a mask is made here.
    fn missing(&self, keys: &BTreeSet<GlyphKey>) -> Result<Vec<GlyphKey>, GlyphError> {
        if keys.len() > self.max_entries {
            return Err(GlyphError::FrameTooLarge {
                entries: keys.len(),
                bytes: 0,
            });
        }
        let mut bytes = 0;
        let mut missing = Vec::new();
        let mut fonts = Fonts::new(&self.fonts);
        for &key in keys {
            if let Some(entry) = self.entries.get(&key) {
                bytes += entry.mask.coverage().len();
                continue;
            }
            bytes += measure(fonts.get(key)?, key)?.bytes();
            missing.push(key);
        }
        if bytes > self.max_bytes {
            return Err(GlyphError::FrameTooLarge {
                entries: keys.len(),
                bytes,
            });
        }
        Ok(missing)
    }
}

/// The two fonts of a cache, each read by skrifa when a key first needs
/// it.
struct Fonts<'a> {
    data: &'a [Blob<u8>; 2],
    fonts: [Option<FontRef<'a>>; 2],
}

impl<'a> Fonts<'a> {
    fn new(data: &'a [Blob<u8>; 2]) -> Fonts<'a> {
        Fonts {
            data,
            fonts: [None, None],
        }
    }

    /// The font of `key`, when it is one of the two and the size of `key`
    /// lies within [`MIN_SIZE`] to [`MAX_SIZE`].
    fn get(&mut self, key: GlyphKey) -> Result<&FontRef<'a>, GlyphError> {
        let index = key.font.get() as usize;
        let (Some(data), Some(slot)) = (self.data.get(index), self.fonts.get_mut(index)) else {
            return Err(GlyphError::UnknownFont(key.font));
        };
        if key.size < MIN_SIZE || key.size > MAX_SIZE {
            return Err(GlyphError::SizeOutOfRange(key.size));
        }
        match slot {
            Some(font) => Ok(font),
            None => {
                let font = FontRef::new(data.data()).map_err(|_| GlyphError::NoOutline(key))?;
                Ok(slot.insert(font))
            }
        }
    }
}

impl GlyphCoverage for GlyphCache {
    fn coverage(&self, key: GlyphKey) -> Option<&CoverageMask> {
        self.entries.get(&key).map(|entry| &entry.mask)
    }
}

impl fmt::Debug for GlyphCache {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GlyphCache")
            .field("entries", &self.entries.len())
            .field("bytes", &self.bytes)
            .field("frame", &self.frame)
            .finish()
    }
}

/// The keys of the glyphs of `list`: each glyph's font, size and glyph, and
/// the quarter-pixel step of its origin with the offsets of its enclosing
/// groups added, as the rasterizer places it.
fn keys(list: &DisplayList) -> Result<BTreeSet<GlyphKey>, GlyphError> {
    let mut keys = BTreeSet::new();
    let mut groups: Vec<Point> = Vec::new();
    let mut at = Point::default();
    for command in list.commands() {
        match command {
            Command::PushClip { offset, .. } => {
                groups.push(at);
                at = at
                    .translated(offset.x, offset.y)
                    .ok_or(GlyphError::OutOfRange)?;
            }
            Command::PopClip => at = groups.pop().unwrap_or_default(),
            Command::Glyphs {
                font, size, glyphs, ..
            } => {
                for glyph in glyphs {
                    let origin = glyph
                        .origin
                        .translated(at.x, at.y)
                        .ok_or(GlyphError::OutOfRange)?;
                    keys.insert(GlyphKey {
                        font: *font,
                        size: *size,
                        glyph: glyph.id,
                        step: place_glyph(origin.x, origin.y).step,
                    });
                }
            }
            _ => {}
        }
    }
    Ok(keys)
}

/// The outline of `key`, unhinted, at its size and moved right by its step,
/// in pixels from the glyph's whole pixel on the baseline with `y` growing
/// down, and an empty mask that covers it.
fn outline(font: &FontRef<'_>, key: GlyphKey) -> Result<(Path, CoverageMask), GlyphError> {
    let mut pen = Pen {
        bounds: Bounds::new(key.step.offset()),
        path: None,
    };
    draw(font, key, &mut pen)?;
    let area = pen.bounds.area()?;
    let path = pen.path.unwrap_or_else(|| Path::new(Point::default()));
    Ok((path, area.mask()?))
}

/// The area of the mask of `key`, which [`outline`] would allocate,
/// measured from the bounds of the points of the outline without building
/// a path or allocating the mask.
fn measure(font: &FontRef<'_>, key: GlyphKey) -> Result<Area, GlyphError> {
    let mut bounds = Bounds::new(key.step.offset());
    draw(font, key, &mut bounds)?;
    bounds.area()
}

/// Draws the outline of `key`, unhinted at its size, into `pen`.
fn draw(font: &FontRef<'_>, key: GlyphKey, pen: &mut impl OutlinePen) -> Result<(), GlyphError> {
    let glyph = font
        .outline_glyphs()
        .get(GlyphId::new(key.glyph.get()))
        .ok_or(GlyphError::NoOutline(key))?;
    // The size is within 2^14 units, so it converts exactly.
    let ppem = key.size.units() as f32 / 64.0;
    glyph
        .draw(
            DrawSettings::unhinted(Size::new(ppem), LocationRef::default()),
            pen,
        )
        .map_err(|_| GlyphError::NoOutline(key))?;
    Ok(())
}

/// The whole pixels of a mask: its top-left value at (`left`, `top`) and
/// `width` by `height` values.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Area {
    left: i32,
    top: i32,
    width: u32,
    height: u32,
}

impl Area {
    /// The bytes of the mask, at most 2^26, as each side is within
    /// [`MAX_BUFFER_SIDE`].
    fn bytes(self) -> usize {
        self.width as usize * self.height as usize
    }

    /// An empty mask of the area.
    fn mask(self) -> Result<CoverageMask, GlyphError> {
        CoverageMask::new(self.left, self.top, self.width, self.height)
            .ok_or(GlyphError::OutOfRange)
    }
}

/// The bounds in units of the points of an outline of skrifa, whose `y`
/// grows up, moved right by `step` with `y` growing down.
struct Bounds {
    step: Fx,
    /// The least `x` and `y` and the greatest `x` and `y`.
    bounds: Option<(i64, i64, i64, i64)>,
    out_of_range: bool,
}

impl Bounds {
    fn new(step: Fx) -> Bounds {
        Bounds {
            step,
            bounds: None,
            out_of_range: false,
        }
    }

    /// The point of skrifa's (`x`, `y`): TrueType outlines are scaled in
    /// units of 1/64 pixel, so the products are whole numbers and the
    /// rounding changes nothing.
    fn point(&mut self, x: f32, y: f32) -> Point {
        let moved = round(x * 64.0)
            .and_then(|x| x.checked_add(self.step))
            .zip(round(-y * 64.0));
        let Some((x, y)) = moved else {
            self.out_of_range = true;
            return Point::default();
        };
        let (x, y) = (i64::from(x.units()), i64::from(y.units()));
        self.bounds = Some(match self.bounds {
            None => (x, y, x, y),
            Some((left, top, right, bottom)) => {
                (min(left, x), min(top, y), max(right, x), max(bottom, y))
            }
        });
        Point::new(Fx::from_units(x as i32), Fx::from_units(y as i32))
    }

    /// The whole pixels that cover the points, none without points. A side
    /// longer than [`MAX_BUFFER_SIDE`] is refused here, as
    /// [`CoverageMask::new`] would refuse it.
    fn area(&self) -> Result<Area, GlyphError> {
        if self.out_of_range {
            return Err(GlyphError::OutOfRange);
        }
        let Some((left, top, right, bottom)) = self.bounds else {
            return Ok(Area::default());
        };
        let (left, top) = (left.div_euclid(64), top.div_euclid(64));
        let (right, bottom) = (-(-right).div_euclid(64), -(-bottom).div_euclid(64));
        let width = u32::try_from(right - left).map_err(|_| GlyphError::OutOfRange)?;
        let height = u32::try_from(bottom - top).map_err(|_| GlyphError::OutOfRange)?;
        if width > MAX_BUFFER_SIDE || height > MAX_BUFFER_SIDE {
            return Err(GlyphError::OutOfRange);
        }
        Ok(Area {
            left: left as i32,
            top: top as i32,
            width,
            height,
        })
    }
}

impl OutlinePen for Bounds {
    fn move_to(&mut self, x: f32, y: f32) {
        self.point(x, y);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.point(x, y);
    }

    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        self.point(cx0, cy0);
        self.point(x, y);
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.point(cx0, cy0);
        self.point(cx1, cy1);
        self.point(x, y);
    }

    fn close(&mut self) {}
}

/// The pen that writes an outline of skrifa as a [`Path`] whose `y` grows
/// down, moved right by its step, and keeps the bounds of its points.
struct Pen {
    bounds: Bounds,
    path: Option<Path>,
}

impl Pen {
    fn path(&mut self, start: Point) -> &mut Path {
        self.path.get_or_insert_with(|| Path::new(start))
    }
}

impl OutlinePen for Pen {
    fn move_to(&mut self, x: f32, y: f32) {
        let to = self.bounds.point(x, y);
        match &mut self.path {
            Some(path) => {
                path.move_to(to);
            }
            None => self.path = Some(Path::new(to)),
        }
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let to = self.bounds.point(x, y);
        self.path(Point::default()).line_to(to);
    }

    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        let control = self.bounds.point(cx0, cy0);
        let to = self.bounds.point(x, y);
        self.path(Point::default()).quad_to(control, to);
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        let first = self.bounds.point(cx0, cy0);
        let second = self.bounds.point(cx1, cy1);
        let to = self.bounds.point(x, y);
        self.path(Point::default()).cubic_to(first, second, to);
    }

    fn close(&mut self) {
        self.path(Point::default()).close();
    }
}

#[cfg(test)]
mod tests;
