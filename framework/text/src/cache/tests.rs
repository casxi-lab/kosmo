use alloc::string::ToString;
use alloc::vec;

use kosmo_render::{Color, GlyphId, ImageStore, ListBuilder, RenderTarget, Step, VelloRasterizer};
use kosmo_types::{BufferDesc, PixelFormat, Rect, Size};

use super::*;
use crate::{FontSet, MONO, SANS, TextStyle};

const SANS_BYTES: &[u8] = include_bytes!("../../assets/fonts/firasans/FiraSans-Regular.ttf");
const MONO_BYTES: &[u8] = include_bytes!("../../assets/fonts/firamono/FiraMono-Regular.ttf");

/// 16 pixels per em.
const SIZE: Fx = Fx::from_units(1024);

fn px(pixels: i32) -> Fx {
    Fx::from_pixels(pixels).unwrap()
}

/// The font set of the two fonts, with a cache of `max_entries` entries and
/// `max_bytes` bytes.
fn font_set(max_entries: usize, max_bytes: usize) -> FontSet {
    let mut set = FontSet::new(SANS_BYTES, MONO_BYTES).unwrap();
    set.glyphs = GlyphCache::new(set.glyphs.fonts.clone(), max_entries, max_bytes);
    set
}

/// The key of glyph `glyph` of Fira Sans at 16 pixels per em and `step`.
fn key(glyph: u32, step: Step) -> GlyphKey {
    GlyphKey {
        font: SANS,
        size: SIZE,
        glyph: GlyphId::new(glyph),
        step,
    }
}

/// A display list of the glyphs `glyphs` of `font` at `size`, 20 pixels
/// apart on whole pixels, so each is at the step [`Step::Whole`].
fn frame_of(font: FontId, size: Fx, glyphs: &[u32]) -> DisplayList {
    let glyphs = glyphs
        .iter()
        .zip(0..)
        .map(|(&glyph, index)| kosmo_render::Glyph {
            id: GlyphId::new(glyph),
            origin: Point::new(px(20 * index), px(20)),
        })
        .collect();
    let mut builder = ListBuilder::new();
    builder
        .push(Command::Glyphs {
            font,
            size,
            color: Color::opaque(0, 0, 0),
            glyphs,
        })
        .unwrap();
    builder.finish().unwrap()
}

fn frame(glyphs: &[u32]) -> DisplayList {
    frame_of(SANS, SIZE, glyphs)
}

/// A display list of glyphs 3 to `2 + count` of Fira Sans at the largest
/// size, each at every step of `steps`.
fn largest(count: u32, steps: &[Step]) -> DisplayList {
    let mut glyphs = Vec::new();
    for &step in steps {
        for glyph in 3..3 + count {
            glyphs.push(kosmo_render::Glyph {
                id: GlyphId::new(glyph),
                origin: Point::new(step.offset(), px(300)),
            });
        }
    }
    let mut builder = ListBuilder::new();
    builder
        .push(Command::Glyphs {
            font: SANS,
            size: MAX_SIZE,
            color: Color::opaque(0, 0, 0),
            glyphs,
        })
        .unwrap();
    builder.finish().unwrap()
}

/// The bytes of the masks of the entries of `cache`, added together.
fn mask_bytes(cache: &GlyphCache) -> usize {
    cache
        .entries
        .values()
        .map(|entry| entry.mask.coverage().len())
        .sum()
}

/// A rasterizer that counts the masks it fills.
struct Counting {
    inner: VelloRasterizer,
    fills: usize,
}

impl Counting {
    fn new() -> Counting {
        Counting {
            inner: VelloRasterizer::new(),
            fills: 0,
        }
    }
}

impl Rasterizer for Counting {
    fn render(
        &mut self,
        list: &DisplayList,
        damage: &kosmo_types::Damage,
        glyphs: &dyn GlyphCoverage,
        images: &ImageStore,
        target: &mut RenderTarget,
    ) -> Result<kosmo_render::WorkCount, RenderError> {
        self.inner.render(list, damage, glyphs, images, target)
    }

    fn fill_coverage(&mut self, outline: &Path, out: &mut CoverageMask) -> Result<(), RenderError> {
        self.fills += 1;
        self.inner.fill_coverage(outline, out)
    }
}

/// Lays `text` out in Fira Sans at 16 pixels per em, draws it into a
/// buffer of its box with 2 pixels around it through the glyph cache of
/// `set`, and returns what the preparation did and the pixels.
fn draw(set: &mut FontSet, text: &str) -> (Prepared, Vec<u8>) {
    let paragraph = set
        .layout(text, TextStyle::new(SANS, SIZE).unwrap(), None)
        .unwrap();
    let mut builder = ListBuilder::new();
    let run = paragraph
        .run(Point::new(px(2), px(2)), Color::opaque(0, 0, 0))
        .unwrap();
    builder.push(run).unwrap();
    let list = builder.finish().unwrap();
    let mut rasterizer = VelloRasterizer::new();
    let prepared = set.prepare(&list, &mut rasterizer).unwrap();
    let whole = |length: Fx| (length.units() + 63).div_euclid(64) as u32 + 4;
    let (width, height) = (whole(paragraph.width()), whole(paragraph.height()));
    let desc = BufferDesc::new(width, height, width * 4, PixelFormat::Rgba8Premul).unwrap();
    let mut target = RenderTarget::new(desc).unwrap();
    let damage = target.whole();
    rasterizer
        .render(
            &list,
            &damage,
            set.glyphs(),
            &ImageStore::EMPTY,
            &mut target,
        )
        .unwrap();
    (prepared, target.into_parts().1)
}

#[test]
fn the_capacity_is_4096_entries_and_4_mib() {
    assert_eq!(MAX_GLYPH_ENTRIES, 4_096);
    assert_eq!(MAX_GLYPH_BYTES, 4_194_304);
    let set = FontSet::new(SANS_BYTES, MONO_BYTES).unwrap();
    let cache = set.glyphs();
    assert_eq!(
        (cache.max_entries, cache.max_bytes),
        (MAX_GLYPH_ENTRIES, MAX_GLYPH_BYTES)
    );
    assert_eq!((cache.entries(), cache.bytes(), cache.frame()), (0, 0, 0));
}

#[test]
fn a_new_key_evicts_the_one_key_that_the_frame_does_not_use() {
    let mut set = font_set(8, MAX_GLYPH_BYTES);
    let mut rasterizer = VelloRasterizer::new();
    let first = [3, 4, 5, 6, 7, 8, 9, 10];
    let prepared = set.prepare(&frame(&first), &mut rasterizer).unwrap();
    assert_eq!(
        prepared,
        Prepared {
            keys: 8,
            filled: 8,
            evicted: 0
        }
    );
    // Seven keys of the first frame and a new one.
    let second = [3, 4, 6, 7, 8, 9, 10, 11];
    let prepared = set.prepare(&frame(&second), &mut rasterizer).unwrap();
    assert_eq!(
        prepared,
        Prepared {
            keys: 8,
            filled: 1,
            evicted: 1
        }
    );
    let cache = set.glyphs();
    assert_eq!(cache.entries(), 8);
    assert!(!cache.contains(key(5, Step::Whole)));
    assert!(cache.coverage(key(5, Step::Whole)).is_none());
    for glyph in second {
        assert!(cache.contains(key(glyph, Step::Whole)), "{glyph}");
    }
    assert_eq!(cache.bytes(), mask_bytes(cache));
    assert_eq!(cache.frame(), 2);
}

#[test]
fn entries_are_evicted_by_the_frame_that_last_used_them_and_then_by_key() {
    let mut set = font_set(4, MAX_GLYPH_BYTES);
    let mut rasterizer = VelloRasterizer::new();
    set.prepare(&frame(&[3, 4]), &mut rasterizer).unwrap();
    set.prepare(&frame(&[6, 5]), &mut rasterizer).unwrap();
    // Glyph 6 is used again; 3 and 4, of the first frame, make room.
    let prepared = set.prepare(&frame(&[6, 8, 7]), &mut rasterizer).unwrap();
    assert_eq!(prepared.evicted, 2);
    let held = |set: &FontSet| -> Vec<u32> {
        set.glyphs()
            .entries
            .keys()
            .map(|key| key.glyph.get())
            .collect()
    };
    assert_eq!(held(&set), [5, 6, 7, 8]);
    // Glyph 5, last used in the second frame, goes before 6, 7 and 8.
    set.prepare(&frame(&[9]), &mut rasterizer).unwrap();
    assert_eq!(held(&set), [6, 7, 8, 9]);
    // Among keys of one frame, the smaller goes first.
    set.prepare(&frame(&[10, 11]), &mut rasterizer).unwrap();
    assert_eq!(held(&set), [8, 9, 10, 11]);
}

#[test]
fn an_older_frame_goes_first_whatever_its_key() {
    let mut set = font_set(2, MAX_GLYPH_BYTES);
    let mut rasterizer = VelloRasterizer::new();
    set.prepare(&frame(&[9]), &mut rasterizer).unwrap();
    set.prepare(&frame(&[3]), &mut rasterizer).unwrap();
    // Glyph 9, last used in the first frame, goes before glyph 3, of the
    // second, although its key is greater.
    let prepared = set.prepare(&frame(&[5]), &mut rasterizer).unwrap();
    assert_eq!(prepared.evicted, 1);
    assert!(!set.glyphs().contains(key(9, Step::Whole)));
    assert!(set.glyphs().contains(key(3, Step::Whole)));
    assert!(set.glyphs().contains(key(5, Step::Whole)));
}

#[test]
fn the_order_matches_the_entries_after_clearing() {
    let mut set = font_set(2, MAX_GLYPH_BYTES);
    let mut rasterizer = VelloRasterizer::new();
    set.prepare(&frame(&[3, 4]), &mut rasterizer).unwrap();
    set.clear_glyphs();
    set.prepare(&frame(&[5]), &mut rasterizer).unwrap();
    let prepared = set.prepare(&frame(&[6, 7]), &mut rasterizer).unwrap();
    assert_eq!(prepared.evicted, 1);
    let cache = set.glyphs();
    let order: Vec<(u64, GlyphKey)> = cache.order.iter().copied().collect();
    let mut entries: Vec<(u64, GlyphKey)> = cache
        .entries
        .iter()
        .map(|(key, entry)| (entry.used, *key))
        .collect();
    entries.sort();
    assert_eq!(order, entries);
}

#[test]
fn bytes_beyond_the_capacity_evict_too() {
    let mut set = font_set(MAX_GLYPH_ENTRIES, MAX_GLYPH_BYTES);
    let mut rasterizer = VelloRasterizer::new();
    set.prepare(&frame(&[3, 4]), &mut rasterizer).unwrap();
    let both = set.glyphs().bytes();
    let mut set = font_set(MAX_GLYPH_ENTRIES, both);
    set.prepare(&frame(&[3, 4]), &mut rasterizer).unwrap();
    assert_eq!(set.glyphs().bytes(), both);
    let prepared = set.prepare(&frame(&[3]), &mut rasterizer).unwrap();
    assert_eq!(prepared.evicted, 0);
    let prepared = set.prepare(&frame(&[3, 5]), &mut rasterizer).unwrap();
    assert_eq!((prepared.filled, prepared.evicted), (1, 1));
    assert!(!set.glyphs().contains(key(4, Step::Whole)));
    assert!(set.glyphs().bytes() <= both);
    assert_eq!(set.glyphs().bytes(), mask_bytes(set.glyphs()));
}

#[test]
fn held_masks_count_towards_a_frame_beyond_the_bytes() {
    let mut set = font_set(MAX_GLYPH_ENTRIES, MAX_GLYPH_BYTES);
    let mut rasterizer = Counting::new();
    set.prepare(&frame(&[3, 4]), &mut rasterizer).unwrap();
    let both = set.glyphs().bytes();
    let mut set = font_set(MAX_GLYPH_ENTRIES, both);
    set.prepare(&frame(&[3, 4]), &mut rasterizer).unwrap();
    rasterizer.fills = 0;
    let error = set
        .prepare(&frame(&[3, 4, 5]), &mut rasterizer)
        .unwrap_err();
    let GlyphError::FrameTooLarge { entries, bytes } = error else {
        panic!("{error:?}");
    };
    assert_eq!(entries, 3);
    assert!(bytes > both);
    assert_eq!(rasterizer.fills, 0);
    assert_eq!((set.glyphs().bytes(), set.glyphs().frame()), (both, 1));
}

#[test]
fn a_frame_at_the_capacity_evicts_nothing() {
    let mut set = font_set(8, MAX_GLYPH_BYTES);
    let mut rasterizer = Counting::new();
    let prepared = set
        .prepare(&frame(&[3, 4, 5, 6, 7, 8, 9, 10]), &mut rasterizer)
        .unwrap();
    assert_eq!(prepared.evicted, 0);
    assert_eq!(rasterizer.fills, 8);
    assert_eq!(set.glyphs().entries(), 8);
    // The same frame again fills nothing.
    let prepared = set
        .prepare(&frame(&[3, 4, 5, 6, 7, 8, 9, 10]), &mut rasterizer)
        .unwrap();
    assert_eq!((prepared.filled, prepared.evicted), (0, 0));
    assert_eq!(rasterizer.fills, 8);
}

#[test]
fn a_frame_beyond_the_capacity_is_refused_before_filling() {
    let mut set = font_set(8, MAX_GLYPH_BYTES);
    let mut rasterizer = Counting::new();
    set.prepare(&frame(&[3, 4, 5, 6, 7, 8, 9, 10]), &mut rasterizer)
        .unwrap();
    let bytes = set.glyphs().bytes();
    rasterizer.fills = 0;
    let nine = [3, 4, 5, 6, 7, 8, 9, 10, 11];
    let error = set.prepare(&frame(&nine), &mut rasterizer).unwrap_err();
    let GlyphError::FrameTooLarge { entries, .. } = error else {
        panic!("{error:?}");
    };
    assert_eq!(entries, 9);
    assert_eq!(rasterizer.fills, 0);
    let cache = set.glyphs();
    assert_eq!(
        (cache.entries(), cache.bytes(), cache.frame()),
        (8, bytes, 1)
    );
    assert_eq!(cache.bytes(), mask_bytes(cache));
    // Beyond the bytes: the frame's masks need one byte more than the cache
    // holds.
    let mut set = font_set(8, bytes - 1);
    let error = set
        .prepare(&frame(&[3, 4, 5, 6, 7, 8, 9, 10]), &mut rasterizer)
        .unwrap_err();
    assert_eq!(error, GlyphError::FrameTooLarge { entries: 8, bytes });
    assert_eq!(rasterizer.fills, 0);
    assert_eq!((set.glyphs().entries(), set.glyphs().bytes()), (0, 0));
}

#[test]
fn a_frame_beyond_the_capacity_is_refused_before_its_masks_are_allocated() {
    let mut set = font_set(MAX_GLYPH_ENTRIES, MAX_GLYPH_BYTES);
    let mut rasterizer = Counting::new();
    set.prepare(&frame(&[3, 4]), &mut rasterizer).unwrap();
    let held = |set: &FontSet| {
        let cache = set.glyphs();
        (cache.entries(), cache.bytes(), cache.frame())
    };
    let before = held(&set);
    rasterizer.fills = 0;
    // 1,025 glyphs at each of the four steps are 4,100 keys, beyond the
    // entries, whose masks at 256 pixels per em would take some 24 times
    // the bytes of the cache. The step that lists the missing keys refuses
    // them before it reads any outline, so it counts no bytes.
    let many = largest(1_025, &Step::ALL);
    let many_keys = keys(&many).unwrap();
    let refused = GlyphError::FrameTooLarge {
        entries: 4_100,
        bytes: 0,
    };
    assert_eq!(set.glyphs.missing(&many_keys), Err(refused));
    assert_eq!(set.prepare(&many, &mut rasterizer), Err(refused));
    assert_eq!(
        refused.to_string(),
        "the frame needs 4100 glyph masks, more than the cache holds"
    );
    // Not even a glyph without an outline is read.
    let mut small = font_set(2, MAX_GLYPH_BYTES);
    assert_eq!(
        small.prepare(&frame(&[3, 4, 60_000]), &mut rasterizer),
        Err(GlyphError::FrameTooLarge {
            entries: 3,
            bytes: 0
        })
    );
    // 200 glyphs at one step fit in the entries. Their masks are measured
    // without being allocated, and their bytes, exactly those of the masks,
    // exceed the cache.
    let font = FontRef::new(SANS_BYTES).unwrap();
    let wide = largest(200, &[Step::Whole]);
    let wide_keys = keys(&wide).unwrap();
    let bytes: usize = wide_keys
        .iter()
        .map(|&key| outline(&font, key).unwrap().1.coverage().len())
        .sum();
    assert!(bytes > MAX_GLYPH_BYTES, "{bytes}");
    let refused = GlyphError::FrameTooLarge {
        entries: 200,
        bytes,
    };
    assert_eq!(set.glyphs.missing(&wide_keys), Err(refused));
    assert_eq!(set.prepare(&wide, &mut rasterizer), Err(refused));
    assert_eq!(
        refused.to_string(),
        alloc::format!("the frame needs 200 glyph masks of {bytes} bytes, beyond the cache")
    );
    assert_eq!(rasterizer.fills, 0);
    assert_eq!(held(&set), before);
    // A frame within both limits lists the keys that the cache lacks, and
    // their masks are allocated as they are filled.
    let three = keys(&frame(&[3, 4, 5])).unwrap();
    assert_eq!(set.glyphs.missing(&three), Ok(vec![key(5, Step::Whole)]));
    let prepared = set.prepare(&frame(&[3, 4, 5]), &mut rasterizer).unwrap();
    assert_eq!((prepared.filled, rasterizer.fills), (1, 1));
}

#[test]
fn masks_are_measured_from_the_bounds_of_their_points() {
    let font = FontRef::new(SANS_BYTES).unwrap();
    // The measure of a mask is the mask that the outline allocates.
    for size in [SIZE, MAX_SIZE] {
        for glyph in 2..40 {
            for step in Step::ALL {
                let key = GlyphKey {
                    font: SANS,
                    size,
                    glyph: GlyphId::new(glyph),
                    step,
                };
                let (_, mask) = outline(&font, key).unwrap();
                let area = measure(&font, key).unwrap();
                assert_eq!(
                    (area.left, area.top, area.width, area.height),
                    (mask.left(), mask.top(), mask.width(), mask.height()),
                    "{key:?}"
                );
                assert_eq!(area.bytes(), mask.coverage().len());
            }
        }
    }
    // Bounds in units take the whole pixels that cover them.
    let bounds = |extent| Bounds {
        step: Fx::ZERO,
        bounds: Some(extent),
        out_of_range: false,
    };
    let area = Area {
        left: -1,
        top: -11,
        width: 11,
        height: 12,
    };
    assert_eq!(bounds((-1, -700, 640, 1)).area(), Ok(area));
    assert_eq!(area.bytes(), 132);
    assert_eq!(Bounds::new(Fx::ZERO).area(), Ok(Area::default()));
    // A side of the longest buffer is measured, and a longer one refused,
    // as `CoverageMask::new` would refuse it.
    let side = i64::from(MAX_BUFFER_SIDE) * 64;
    assert_eq!(
        bounds((0, 0, side, 64)).area().map(Area::bytes),
        Ok(MAX_BUFFER_SIDE as usize)
    );
    assert_eq!(
        bounds((0, 0, side + 1, 64)).area(),
        Err(GlyphError::OutOfRange)
    );
    assert_eq!(
        bounds((0, 0, 64, side + 1)).area(),
        Err(GlyphError::OutOfRange)
    );
    assert!(CoverageMask::new(0, 0, MAX_BUFFER_SIDE + 1, 1).is_none());
    let mut beyond = Bounds::new(Fx::ZERO);
    beyond.out_of_range = true;
    assert_eq!(beyond.area(), Err(GlyphError::OutOfRange));
}

#[test]
fn a_drawing_rebuilt_after_evictions_has_the_same_pixels() {
    let (pangram, operators) = (
        "The quick brown fox jumps over the lazy dog.",
        "\u{2212}3 \u{d7} 4 \u{f7} 2 \u{b1} 0.5 = \u{2212}5.5 \u{2260} 7",
    );
    // With an ample cache.
    let mut ample = font_set(MAX_GLYPH_ENTRIES, MAX_GLYPH_BYTES);
    let (first, pixels) = draw(&mut ample, pangram);
    let (other, _) = draw(&mut ample, operators);
    // With room for the larger frame only, every frame evicts entries of
    // the frame before.
    let capacity = if first.keys > other.keys {
        first.keys
    } else {
        other.keys
    };
    let mut small = font_set(capacity, MAX_GLYPH_BYTES);
    let (prepared, drawn) = draw(&mut small, pangram);
    assert_eq!(prepared.evicted, 0);
    assert_eq!(drawn, pixels);
    for _ in 0..2 {
        let (prepared, _) = draw(&mut small, operators);
        assert!(prepared.evicted > 0, "{prepared:?}");
        let (prepared, again) = draw(&mut small, pangram);
        assert!(prepared.evicted > 0 && prepared.filled > 0, "{prepared:?}");
        assert_eq!(again, pixels);
        assert!(small.glyphs().entries() <= capacity);
    }
    small.clear_glyphs();
    assert_eq!((small.glyphs().entries(), small.glyphs().bytes()), (0, 0));
    let (prepared, again) = draw(&mut small, pangram);
    assert_eq!(prepared.filled, first.keys);
    assert_eq!(again, pixels);
    // The drawing is not blank.
    assert!(pixels.chunks(4).any(|pixel| pixel == [0, 0, 0, 255]));
}

#[test]
fn keys_take_the_steps_of_origins_with_the_offsets_of_their_groups() {
    let mut set = font_set(MAX_GLYPH_ENTRIES, MAX_GLYPH_BYTES);
    let mut builder = ListBuilder::new();
    let rect = Rect::new(Point::default(), Size::new(px(40), px(40)).unwrap());
    let offset = Point::new(Fx::from_units(16), px(-1));
    builder.push(Command::PushClip { rect, offset }).unwrap();
    for command in frame(&[3]).commands() {
        builder.push(command.clone()).unwrap();
    }
    builder.push(Command::PopClip).unwrap();
    builder.push(frame(&[3]).commands()[0].clone()).unwrap();
    let list = builder.finish().unwrap();
    let mut rasterizer = VelloRasterizer::new();
    let prepared = set.prepare(&list, &mut rasterizer).unwrap();
    assert_eq!(prepared.keys, 2);
    assert!(set.glyphs().contains(key(3, Step::Quarter)));
    assert!(set.glyphs().contains(key(3, Step::Whole)));
    let desc = BufferDesc::new(40, 40, 160, PixelFormat::Rgba8Premul).unwrap();
    let mut target = RenderTarget::new(desc).unwrap();
    let damage = target.whole();
    assert!(
        rasterizer
            .render(
                &list,
                &damage,
                set.glyphs(),
                &ImageStore::EMPTY,
                &mut target
            )
            .is_ok()
    );
}

#[test]
fn keys_take_the_offsets_of_nested_groups_until_each_pops() {
    let mut set = font_set(MAX_GLYPH_ENTRIES, MAX_GLYPH_BYTES);
    let mut builder = ListBuilder::new();
    let rect = Rect::new(Point::default(), Size::new(px(40), px(40)).unwrap());
    let glyph = frame(&[3]).commands()[0].clone();
    // A quarter pixel in the outer group and half a pixel in the inner one
    // add up to three quarters; popping the inner group leaves the outer
    // quarter, and popping that one the whole pixel.
    for offset in [16, 32] {
        let offset = Point::new(Fx::from_units(offset), Fx::ZERO);
        builder.push(Command::PushClip { rect, offset }).unwrap();
    }
    builder.push(glyph.clone()).unwrap();
    builder.push(Command::PopClip).unwrap();
    builder.push(glyph.clone()).unwrap();
    builder.push(Command::PopClip).unwrap();
    builder.push(glyph).unwrap();
    let list = builder.finish().unwrap();
    let mut rasterizer = VelloRasterizer::new();
    let prepared = set.prepare(&list, &mut rasterizer).unwrap();
    assert_eq!(prepared.keys, 3);
    for step in [Step::ThreeQuarters, Step::Quarter, Step::Whole] {
        assert!(set.glyphs().contains(key(3, step)), "{step:?}");
    }
    assert!(!set.glyphs().contains(key(3, Step::Half)));
    // The rasterizer places the glyphs alike and finds every mask.
    let desc = BufferDesc::new(40, 40, 160, PixelFormat::Rgba8Premul).unwrap();
    let mut target = RenderTarget::new(desc).unwrap();
    let damage = target.whole();
    assert!(
        rasterizer
            .render(
                &list,
                &damage,
                set.glyphs(),
                &ImageStore::EMPTY,
                &mut target
            )
            .is_ok()
    );
}

#[test]
fn masks_are_the_unhinted_outlines_at_each_step() {
    let mut set = font_set(MAX_GLYPH_ENTRIES, MAX_GLYPH_BYTES);
    let mut rasterizer = Counting::new();
    let mut builder = ListBuilder::new();
    for step in Step::ALL {
        let glyph = kosmo_render::Glyph {
            id: GlyphId::new(3),
            origin: Point::new(step.offset(), px(20)),
        };
        builder
            .push(Command::Glyphs {
                font: SANS,
                size: SIZE,
                color: Color::opaque(0, 0, 0),
                glyphs: vec![glyph],
            })
            .unwrap();
    }
    let list = builder.finish().unwrap();
    set.prepare(&list, &mut rasterizer).unwrap();
    assert_eq!(rasterizer.fills, 4);
    let masks: Vec<&CoverageMask> = Step::ALL
        .iter()
        .map(|&step| set.glyphs().coverage(key(3, step)).unwrap())
        .collect();
    // The capital A of 16 pixels per em: 12 pixels above the baseline and
    // fully covered inside its strokes.
    for mask in &masks {
        assert_eq!((mask.top(), mask.height()), (-12, 12));
        assert!(mask.coverage().contains(&255));
    }
    for pair in masks.windows(2) {
        assert_ne!(pair[0].coverage(), pair[1].coverage());
    }
    // Each mask is the outline drawn through skrifa, moved by its step.
    let font = FontRef::new(SANS_BYTES).unwrap();
    for (&step, mask) in Step::ALL.iter().zip(&masks) {
        let (outline, mut expected) = outline(&font, key(3, step)).unwrap();
        rasterizer.fill_coverage(&outline, &mut expected).unwrap();
        assert_eq!(&&expected, mask);
    }
    // A space has an empty mask, which still is an entry.
    set.prepare(&frame(&[2]), &mut rasterizer).unwrap();
    let space = set.glyphs().coverage(key(2, Step::Whole)).unwrap();
    assert_eq!((space.width(), space.height()), (0, 0));
}

#[test]
fn sizes_and_fonts_outside_the_set_are_refused() {
    let mut set = font_set(MAX_GLYPH_ENTRIES, MAX_GLYPH_BYTES);
    let mut rasterizer = Counting::new();
    for size in [63, 16_385, 0, -64] {
        let size = Fx::from_units(size);
        assert_eq!(
            set.prepare(&frame_of(SANS, size, &[3]), &mut rasterizer),
            Err(GlyphError::SizeOutOfRange(size))
        );
        assert_eq!(
            TextStyle::new(SANS, size),
            Err(crate::TextError::SizeOutOfRange(size))
        );
    }
    for size in [64, 16_384] {
        let size = Fx::from_units(size);
        assert!(
            set.prepare(&frame_of(MONO, size, &[3]), &mut rasterizer)
                .is_ok()
        );
        assert!(TextStyle::new(MONO, size).is_ok());
    }
    let other = FontId::new(2);
    assert_eq!(
        set.prepare(&frame_of(other, SIZE, &[3]), &mut rasterizer),
        Err(GlyphError::UnknownFont(other))
    );
    assert_eq!(
        TextStyle::new(other, SIZE),
        Err(crate::TextError::UnknownFont(other))
    );
    let missing = key(60_000, Step::Whole);
    assert_eq!(
        set.prepare(&frame(&[60_000]), &mut rasterizer),
        Err(GlyphError::NoOutline(missing))
    );
    assert_eq!(set.glyphs().frame(), 2);
    assert_eq!(rasterizer.fills, 2);
}
