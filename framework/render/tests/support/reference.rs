//! The per-pixel reference rasterizer: it decides each pixel of the damage
//! region by itself, from the pixel's center, for the commands whose
//! pixels that decides exactly: opaque rectangles and clips with
//! whole-pixel edges and offsets, and glyphs at the whole-pixel step with
//! coverage of only 0 and 255. Every other command, and filling coverage,
//! is unsupported.

use kosmo_render::{
    Color, Command, CoverageMask, DisplayList, GlyphCoverage, GlyphKey, ImageStore, Path,
    Rasterizer, RenderError, RenderTarget, Step, WorkCount, place_glyph,
};
use kosmo_types::{BufferDesc, Damage, DamageRect, Fx, PixelFormat, Point, Rect, convert};

/// The reference rasterizer.
pub struct Reference;

/// A rectangle in pixels: left, top, right and bottom, the right and bottom
/// edges excluded.
#[derive(Clone, Copy)]
struct Pixels {
    left: i64,
    top: i64,
    right: i64,
    bottom: i64,
}

impl Pixels {
    /// `rect` moved by `offset`, or unsupported when an edge is not a whole
    /// pixel.
    fn of(rect: Rect, offset: Point) -> Result<Pixels, RenderError> {
        let left = whole(rect.origin().x)? + whole(offset.x)?;
        let top = whole(rect.origin().y)? + whole(offset.y)?;
        Ok(Pixels {
            left,
            top,
            right: left + whole(rect.size().width())?,
            bottom: top + whole(rect.size().height())?,
        })
    }

    fn holds(self, x: i64, y: i64) -> bool {
        self.left <= x && x < self.right && self.top <= y && y < self.bottom
    }
}

/// `value` in whole pixels, or unsupported when it has a fraction.
fn whole(value: Fx) -> Result<i64, RenderError> {
    let units = i64::from(value.units());
    if units % 64 == 0 {
        Ok(units / 64)
    } else {
        Err(RenderError::Unsupported)
    }
}

/// A command as the reference draws it.
enum Op {
    Fill(Pixels, Color),
    Glyphs(Vec<(i64, i64, CoverageMask)>, Color),
    Push(Pixels),
    Pop,
}

/// The steps of `list`, or the error of its first command that the
/// reference does not draw or whose glyph has no mask.
fn plan(list: &DisplayList, coverage: &dyn GlyphCoverage) -> Result<Vec<Op>, RenderError> {
    let mut steps = Vec::new();
    let mut offsets = vec![Point::default()];
    for command in list.commands() {
        let at = *offsets.last().ok_or(RenderError::Unsupported)?;
        let step = match command {
            Command::FillRect { rect, color } if color.is_opaque() => {
                Op::Fill(Pixels::of(*rect, at)?, *color)
            }
            Command::Glyphs {
                font,
                size,
                color,
                glyphs,
            } if color.is_opaque() => {
                let mut placed = Vec::new();
                for glyph in glyphs {
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
                    if placement.step != Step::Whole {
                        return Err(RenderError::Unsupported);
                    }
                    let mask = coverage
                        .coverage(key)
                        .ok_or(RenderError::MissingGlyph(key))?;
                    if mask
                        .coverage()
                        .iter()
                        .any(|&value| value != 0 && value != 255)
                    {
                        return Err(RenderError::Unsupported);
                    }
                    placed.push((
                        i64::from(placement.x) + i64::from(mask.left()),
                        i64::from(placement.baseline) + i64::from(mask.top()),
                        mask.clone(),
                    ));
                }
                Op::Glyphs(placed, *color)
            }
            Command::PushClip { rect, offset } => {
                let clip = Pixels::of(*rect, at)?;
                whole(offset.x)?;
                whole(offset.y)?;
                offsets.push(
                    at.translated(offset.x, offset.y)
                        .ok_or(RenderError::OutOfRange)?,
                );
                Op::Push(clip)
            }
            Command::PopClip => {
                offsets.pop();
                Op::Pop
            }
            _ => return Err(RenderError::Unsupported),
        };
        steps.push(step);
    }
    Ok(steps)
}

/// The color of pixel (`x`, `y`) after `steps`, from transparent black.
fn pixel(steps: &[Op], x: i64, y: i64) -> Color {
    let mut color = Color::TRANSPARENT;
    // Whether the pixel is inside the clip of each open group.
    let mut inside: Vec<bool> = Vec::new();
    for step in steps {
        let visible = inside.iter().all(|&clip| clip);
        match step {
            Op::Fill(rect, fill) => {
                if visible && rect.holds(x, y) {
                    color = *fill;
                }
            }
            Op::Glyphs(placed, fill) => {
                for (left, top, mask) in placed {
                    let (column, row) = (x - left, y - top);
                    let in_mask = (0..i64::from(mask.width())).contains(&column)
                        && (0..i64::from(mask.height())).contains(&row);
                    if visible && in_mask {
                        let index = row * i64::from(mask.width()) + column;
                        if mask.coverage()[index as usize] == 255 {
                            color = *fill;
                        }
                    }
                }
            }
            Op::Push(clip) => inside.push(clip.holds(x, y)),
            Op::Pop => {
                inside.pop();
            }
        }
    }
    color
}

impl Rasterizer for Reference {
    fn render(
        &mut self,
        list: &DisplayList,
        damage: &Damage,
        glyphs: &dyn GlyphCoverage,
        _images: &ImageStore,
        target: &mut RenderTarget,
    ) -> Result<WorkCount, RenderError> {
        if !target.accepts(damage) {
            return Err(RenderError::DamageMismatch);
        }
        let steps = plan(list, glyphs)?;
        let desc = target.desc();
        for rect in damage.rects() {
            let mut bytes = Vec::new();
            for y in rect.y()..rect.y() + rect.height() {
                for x in rect.x()..rect.x() + rect.width() {
                    bytes.extend(pixel(&steps, x.into(), y.into()).to_bytes());
                }
            }
            let source = BufferDesc::new(
                rect.width(),
                rect.height(),
                rect.width() * 4,
                PixelFormat::Rgba8Premul,
            )
            .map_err(|_| RenderError::OverLimit)?;
            let all =
                DamageRect::new(0, 0, rect.width(), rect.height()).ok_or(RenderError::OverLimit)?;
            convert(
                source,
                &bytes,
                all,
                desc,
                target.bytes_mut(),
                rect.x(),
                rect.y(),
            )
            .map_err(|_| RenderError::OverLimit)?;
        }
        Ok(WorkCount::of(list, damage))
    }

    fn fill_coverage(
        &mut self,
        _outline: &Path,
        _out: &mut CoverageMask,
    ) -> Result<(), RenderError> {
        Err(RenderError::Unsupported)
    }
}
