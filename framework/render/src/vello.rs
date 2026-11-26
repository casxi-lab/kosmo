//! The rasterizer on vello_cpu 0.4.0: its `u8` pipeline on one thread,
//! with an explicit SIMD level and with the transcendental functions of
//! `libm`.
//!
//! A call records the display list once in a render context of the
//! target's size, in the target's coordinates. Each damage rectangle is
//! then rasterized as a cut-out of that scene, with vello_cpu's offset of
//! the destination, into a tightly packed `Rgba8Premul` buffer of its own,
//! which starts as transparent black, and [`kosmo_types::convert`] writes
//! the buffer into the target. vello_cpu decides how it composites each
//! pixel from the whole scene, so a cut-out gives each pixel the bytes of a
//! whole redraw; a render context of the rectangle's size would not, since
//! the `u8` pipeline rounds a non-opaque source differently in the interior
//! of a shape and near its edges, and which pixels count as near the edges
//! depends on where the context starts. Glyph masks are drawn as images
//! sampled at whole pixels and tinted with the color through their alpha,
//! as vello_cpu draws the glyphs of its own cache.

use alloc::collections::BTreeMap;
use alloc::collections::btree_map::Entry;
use alloc::sync::Arc;
use alloc::vec::Vec;

use kosmo_types::{BufferDesc, DamageRect, Fx, PixelFormat, Point, Rect, convert};
use vello_cpu::color::{AlphaColor, Srgb};
use vello_cpu::kurbo::{self, Affine, BezPath, RoundedRect, Shape};
use vello_cpu::peniko::{Extend, Fill, ImageQuality, ImageSampler};
use vello_cpu::{
    Image, ImageSource, Level, Pixmap, PixmapMut, RasterizerSettings, RenderContext, RenderMode,
    RenderSettings, Resources, TargetInit, Tint, TintMode,
};

use crate::list::{Cap, Color, Command, DisplayList, Join, MAX_ITEMS, Path, PathElement};
use crate::raster::{
    CoverageMask, GlyphCoverage, GlyphKey, ImageStore, Rasterizer, RenderError, RenderTarget,
    WorkCount, place_glyph,
};

/// The SIMD level of every render context: the baseline of the
/// compilation target, never a level detected at run time. On AArch64,
/// the architecture of every target platform of kosmo, it is Neon. On
/// another architecture it is that target's baseline, SSE2 on x86-64, so a
/// build there also never depends on the processor it runs on; goldens are
/// made and compared on the target platforms only.
const LEVEL: Level = Level::baseline();

/// The settings of every render context: [`LEVEL`], on the calling thread.
const SETTINGS: RenderSettings = RenderSettings {
    level: LEVEL,
    num_threads: 0,
};

/// How far, in pixels, the curves of a rounded rectangle may lie from its
/// circular corners.
const TOLERANCE: f64 = 0.1;

/// The miter limit of strokes, in half widths.
const MITER_LIMIT: f64 = 4.0;

/// The rows of a drawn rectangle that one conversion writes into the
/// target, so the source of each conversion lies within the buffer limits.
const CONVERT_ROWS: u32 = 64;

/// The rasterizer on vello_cpu. It keeps one render context and one
/// drawing buffer and resets them for each call, so a reused rasterizer
/// gives the same bytes as a new one.
pub struct VelloRasterizer {
    context: RenderContext,
    resources: Resources,
    scratch: Vec<u8>,
    path: BezPath,
}

impl VelloRasterizer {
    /// A new rasterizer.
    pub fn new() -> VelloRasterizer {
        VelloRasterizer {
            context: RenderContext::new_with(1, 1, SETTINGS),
            resources: Resources::new(),
            scratch: Vec::new(),
            path: BezPath::new(),
        }
    }

    /// Records `list` in the render context, sized as the target, in the
    /// target's coordinates.
    fn record(&mut self, list: &DisplayList, plan: &Plan, (width, height): (u16, u16)) {
        let context = &mut self.context;
        context.reset_and_resize(width, height);
        context.set_fill_rule(Fill::NonZero);
        let moved = |at: Point| Affine::translate((pixels(at.x), pixels(at.y)));
        let mut glyphs = plan.glyphs.iter();
        for (command, &at) in list.commands().iter().zip(&plan.offsets) {
            match command {
                Command::FillRect { rect, color } => {
                    context.set_transform(moved(at));
                    context.set_paint(straight(*color));
                    context.fill_rect(&kurbo_rect(*rect));
                }
                Command::FillRoundedRect {
                    rect,
                    radius,
                    color,
                } => {
                    context.set_transform(moved(at));
                    context.set_paint(straight(*color));
                    let rounded = RoundedRect::from_rect(kurbo_rect(*rect), pixels(*radius));
                    context.fill_path(&rounded.to_path(TOLERANCE));
                }
                Command::FillPath { path, color } => {
                    bezier(&mut self.path, path);
                    context.set_transform(moved(at));
                    context.set_paint(straight(*color));
                    context.fill_path(&self.path);
                }
                Command::StrokePath {
                    path,
                    width,
                    join,
                    cap,
                    color,
                } => {
                    if *width == Fx::ZERO {
                        continue;
                    }
                    bezier(&mut self.path, path);
                    context.set_transform(moved(at));
                    context.set_paint(straight(*color));
                    context.set_stroke(
                        kurbo::Stroke::new(pixels(*width))
                            .with_join(match join {
                                Join::Miter => kurbo::Join::Miter,
                                Join::Round => kurbo::Join::Round,
                                Join::Bevel => kurbo::Join::Bevel,
                            })
                            .with_caps(match cap {
                                Cap::Butt => kurbo::Cap::Butt,
                                Cap::Round => kurbo::Cap::Round,
                                Cap::Square => kurbo::Cap::Square,
                            })
                            .with_miter_limit(MITER_LIMIT),
                    );
                    context.stroke_path(&self.path);
                }
                Command::Glyphs {
                    color, glyphs: run, ..
                } => {
                    let tint = Tint {
                        color: straight(*color),
                        mode: TintMode::AlphaMask,
                    };
                    for _ in run {
                        let Some(placed) = glyphs.next() else {
                            break;
                        };
                        let Some(Mask {
                            left,
                            top,
                            pixmap: Some(pixmap),
                        }) = plan.masks.get(&placed.key)
                        else {
                            continue;
                        };
                        context.set_transform(Affine::translate((
                            (placed.x + i64::from(*left)) as f64,
                            (placed.y + i64::from(*top)) as f64,
                        )));
                        context.set_paint(Image {
                            image: ImageSource::Pixmap(Arc::clone(pixmap)),
                            sampler: ImageSampler {
                                x_extend: Extend::Pad,
                                y_extend: Extend::Pad,
                                quality: ImageQuality::Low,
                                alpha: 1.0,
                            },
                        });
                        context.reset_paint_transform();
                        context.set_tint(Some(tint));
                        context.fill_rect(&kurbo::Rect::new(
                            0.0,
                            0.0,
                            f64::from(pixmap.width()),
                            f64::from(pixmap.height()),
                        ));
                        context.set_tint(None);
                    }
                }
                Command::PushClip { rect, .. } => {
                    context.set_transform(moved(at));
                    context.push_clip_rect(&kurbo_rect(*rect));
                }
                Command::PopClip => context.pop_clip(),
            }
        }
        context.flush();
    }

    /// Rasterizes the recorded scene inside the rectangle from `left`,
    /// `top` of `width` by `height` pixels into the scratch buffer, which
    /// starts as transparent black. The scene is cut out at the rectangle,
    /// so each pixel gets the bytes that rasterizing the whole scene gives
    /// it.
    fn rasterize(&mut self, (left, top): (i32, i32), (width, height): (u16, u16)) {
        self.scratch.clear();
        self.scratch
            .resize(usize::from(width) * usize::from(height) * 4, 0);
        let Some(pixmap) = PixmapMut::new(width, height, &mut self.scratch) else {
            return;
        };
        let settings = RasterizerSettings {
            render_mode: RenderMode::OptimizeSpeed,
            target_init: TargetInit::Clear(AlphaColor::TRANSPARENT),
            offset: (-left, -top),
            ..RasterizerSettings::default()
        };
        self.context
            .render_with(pixmap, &mut self.resources, settings);
    }

    /// Writes the scratch buffer, `width` by `height` pixels, into
    /// `target` at `left`, `top`, `CONVERT_ROWS` rows at a time.
    fn write(
        &self,
        target: &mut RenderTarget,
        (left, top, width, height): (u32, u32, u32, u32),
    ) -> Result<(), RenderError> {
        let desc = target.desc();
        let row_bytes = width as usize * 4;
        let mut row = 0;
        while row < height {
            let rows = CONVERT_ROWS.min(height - row);
            let source = BufferDesc::new(width, rows, width * 4, PixelFormat::Rgba8Premul)
                .map_err(|_| RenderError::OverLimit)?;
            let start = row as usize * row_bytes;
            let bytes = self
                .scratch
                .get(start..start + rows as usize * row_bytes)
                .ok_or(RenderError::OverLimit)?;
            let rect = DamageRect::new(0, 0, width, rows).ok_or(RenderError::OverLimit)?;
            convert(
                source,
                bytes,
                rect,
                desc,
                target.bytes_mut(),
                left,
                top + row,
            )
            .map_err(|_| RenderError::OverLimit)?;
            row += rows;
        }
        Ok(())
    }
}

impl Default for VelloRasterizer {
    fn default() -> VelloRasterizer {
        VelloRasterizer::new()
    }
}

impl Rasterizer for VelloRasterizer {
    fn render(
        &mut self,
        list: &DisplayList,
        damage: &kosmo_types::Damage,
        glyphs: &dyn GlyphCoverage,
        _images: &ImageStore,
        target: &mut RenderTarget,
    ) -> Result<WorkCount, RenderError> {
        if !target.accepts(damage) {
            return Err(RenderError::DamageMismatch);
        }
        let plan = Plan::of(list, glyphs)?;
        let desc = target.desc();
        // Within the buffer limits, so at most 8,192 pixels each.
        let side = |value: u32| u16::try_from(value).map_err(|_| RenderError::OverLimit);
        let corner = |value: u32| i32::try_from(value).map_err(|_| RenderError::OverLimit);
        self.record(list, &plan, (side(desc.width())?, side(desc.height())?));
        for rect in damage.rects() {
            let size = (side(rect.width())?, side(rect.height())?);
            self.rasterize((corner(rect.x())?, corner(rect.y())?), size);
            self.write(target, (rect.x(), rect.y(), rect.width(), rect.height()))?;
        }
        Ok(WorkCount::of(list, damage))
    }

    fn fill_coverage(&mut self, outline: &Path, out: &mut CoverageMask) -> Result<(), RenderError> {
        if outline.elements().len() > MAX_ITEMS {
            return Err(RenderError::OverLimit);
        }
        // At most 8,192 pixels by construction.
        let (Ok(width), Ok(height)) = (u16::try_from(out.width()), u16::try_from(out.height()))
        else {
            return Err(RenderError::OverLimit);
        };
        if width == 0 || height == 0 {
            return Ok(());
        }
        bezier(&mut self.path, outline);
        let context = &mut self.context;
        context.reset_and_resize(width, height);
        context.set_fill_rule(Fill::NonZero);
        context.set_transform(Affine::translate((
            -f64::from(out.left()),
            -f64::from(out.top()),
        )));
        context.set_paint(straight(Color::opaque(255, 255, 255)));
        context.fill_path(&self.path);
        context.flush();
        self.rasterize((0, 0), (width, height));
        for (value, pixel) in out
            .coverage_mut()
            .iter_mut()
            .zip(self.scratch.as_chunks::<4>().0)
        {
            // Opaque white times the coverage: the alpha is the coverage.
            *value = pixel[3];
        }
        Ok(())
    }
}

/// What [`VelloRasterizer::render`] checks before it draws: the offset of
/// each command's coordinates, the placement and mask of each glyph, and
/// the masks as images.
struct Plan {
    /// For each command, the offsets of its enclosing groups added
    /// together; for a [`Command::PushClip`], without its own.
    offsets: Vec<Point>,
    /// Each glyph of the list in order, placed.
    glyphs: Vec<Placed>,
    /// The mask of each key that the list uses.
    masks: BTreeMap<GlyphKey, Mask>,
}

/// A glyph with its whole pixel, its baseline and its key.
struct Placed {
    x: i64,
    y: i64,
    key: GlyphKey,
}

/// A coverage mask as an image of opaque white with the coverage as its
/// alpha, and its position; no image when it covers nothing.
struct Mask {
    left: i32,
    top: i32,
    pixmap: Option<Arc<Pixmap>>,
}

impl Plan {
    fn of(list: &DisplayList, coverage: &dyn GlyphCoverage) -> Result<Plan, RenderError> {
        let mut offsets = Vec::with_capacity(list.commands().len());
        let mut glyphs = Vec::new();
        let mut masks = BTreeMap::new();
        let mut groups: Vec<Point> = Vec::new();
        let mut at = Point::default();
        for command in list.commands() {
            offsets.push(at);
            match command {
                Command::PushClip { offset, .. } => {
                    groups.push(at);
                    at = at
                        .translated(offset.x, offset.y)
                        .ok_or(RenderError::OutOfRange)?;
                }
                Command::PopClip => at = groups.pop().unwrap_or_default(),
                Command::Glyphs {
                    font,
                    size,
                    glyphs: run,
                    ..
                } => {
                    for glyph in run {
                        let origin = glyph
                            .origin
                            .translated(at.x, at.y)
                            .ok_or(RenderError::OutOfRange)?;
                        let placement = place_glyph(origin.x, origin.y);
                        let key = GlyphKey {
                            font: *font,
                            size: *size,
                            glyph: glyph.id,
                            step: placement.step,
                        };
                        if let Entry::Vacant(entry) = masks.entry(key) {
                            let mask = coverage
                                .coverage(key)
                                .ok_or(RenderError::MissingGlyph(key))?;
                            entry.insert(Mask::of(mask));
                        }
                        glyphs.push(Placed {
                            x: placement.x.into(),
                            y: placement.baseline.into(),
                            key,
                        });
                    }
                }
                _ => {}
            }
        }
        Ok(Plan {
            offsets,
            glyphs,
            masks,
        })
    }
}

impl Mask {
    fn of(mask: &CoverageMask) -> Mask {
        let (Ok(width), Ok(height)) = (u16::try_from(mask.width()), u16::try_from(mask.height()))
        else {
            return Mask {
                left: mask.left(),
                top: mask.top(),
                pixmap: None,
            };
        };
        let pixmap = (width > 0 && height > 0).then(|| {
            let mut pixmap = Pixmap::new(width, height);
            for (pixel, &value) in pixmap
                .data_as_u8_slice_mut()
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(mask.coverage())
            {
                pixel.copy_from_slice(&[value; 4]);
            }
            Arc::new(pixmap)
        });
        Mask {
            left: mask.left(),
            top: mask.top(),
            pixmap,
        }
    }
}

/// A value in pixels: exact, because an `f64` holds every `i32` divided by
/// 64.
fn pixels(value: Fx) -> f64 {
    f64::from(value.units()) / 64.0
}

fn kurbo_point(point: Point) -> kurbo::Point {
    kurbo::Point::new(pixels(point.x), pixels(point.y))
}

fn kurbo_rect(rect: Rect) -> kurbo::Rect {
    let origin = rect.origin();
    let size = rect.size();
    let (x, y) = (pixels(origin.x), pixels(origin.y));
    kurbo::Rect::new(x, y, x + pixels(size.width()), y + pixels(size.height()))
}

/// Writes `path` into `out` as a Bézier path in pixels.
fn bezier(out: &mut BezPath, path: &Path) {
    out.truncate(0);
    for element in path.elements() {
        match *element {
            PathElement::MoveTo(to) => out.move_to(kurbo_point(to)),
            PathElement::LineTo(to) => out.line_to(kurbo_point(to)),
            PathElement::QuadTo(control, to) => out.quad_to(kurbo_point(control), kurbo_point(to)),
            PathElement::CubicTo(first, second, to) => {
                out.curve_to(kurbo_point(first), kurbo_point(second), kurbo_point(to));
            }
            PathElement::Close => out.close_path(),
        }
    }
}

/// The straight-alpha color of vello_cpu whose premultiplied 8-bit
/// components are those of `color`: the components divided by alpha, which
/// vello_cpu multiplies by alpha again and rounds back to the same bytes.
fn straight(color: Color) -> AlphaColor<Srgb> {
    let alpha = color.alpha();
    if alpha == 0 {
        return AlphaColor::TRANSPARENT;
    }
    let divide = |component: u8| f32::from(component) / f32::from(alpha);
    AlphaColor::new([
        divide(color.red()),
        divide(color.green()),
        divide(color.blue()),
        f32::from(alpha) / 255.0,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_arch = "aarch64")]
    #[test]
    fn the_simd_level_is_neon_on_aarch64() {
        assert!(matches!(LEVEL, Level::Neon(_)));
        let rasterizer = VelloRasterizer::new();
        assert!(
            rasterizer
                .context
                .render_settings()
                .level
                .as_neon()
                .is_some()
        );
        assert_eq!(rasterizer.context.render_settings().num_threads, 0);
    }

    #[cfg(not(target_arch = "aarch64"))]
    #[test]
    fn the_simd_level_is_the_baseline_of_the_target() {
        assert!(!LEVEL.is_fallback());
        assert_eq!(
            VelloRasterizer::new().context.render_settings().num_threads,
            0
        );
    }

    #[test]
    fn every_premultiplied_color_reaches_vello_cpu_unchanged() {
        for alpha in 0..=255_u8 {
            for component in 0..=alpha {
                let color = Color::premultiplied(component, component, component, alpha).unwrap();
                // A solid paint, as vello_cpu stores it.
                let solid = straight(color).premultiply().to_rgba8();
                assert_eq!([solid.r, solid.g, solid.b, solid.a], color.to_bytes());
                // A tint, as vello_cpu's u8 pipeline converts it.
                let tint = straight(color).premultiply().components;
                let tint = tint.map(|value| (value * 255.0 + 0.5) as u8);
                assert_eq!(tint, color.to_bytes());
            }
        }
    }
}
