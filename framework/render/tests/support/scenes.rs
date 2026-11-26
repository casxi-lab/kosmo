//! The cases that the reference rasterizer and vello_cpu both draw.

use kosmo_render::{
    Color, Command, DisplayList, Glyph, GlyphId, ImageStore, ListBuilder, ListError, Rasterizer,
    RenderTarget, WorkCount,
};
use kosmo_types::{BufferDesc, Damage, Fx, PixelFormat, Point, Rect, Size};

use super::glyphs::{FONT, GLYPHS, SIZE, TestGlyphs};
use super::{Checked, failed};

/// A value of `pixels` and `units` 64ths of a pixel.
pub fn fx(pixels: i32, units: i32) -> Fx {
    Fx::from_units(pixels * 64 + units)
}

/// The point at `x`, `y` pixels.
pub fn px(x: i32, y: i32) -> Point {
    Point::new(fx(x, 0), fx(y, 0))
}

/// The rectangle from `left`, `top` to `right`, `bottom`, in 1/64 pixel;
/// empty when `right` or `bottom` lies before the start.
fn units_rect(left: i32, top: i32, right: i32, bottom: i32) -> Rect {
    let origin = Point::new(Fx::from_units(left), Fx::from_units(top));
    let size = Size::new(Fx::from_units(right - left), Fx::from_units(bottom - top));
    Rect::new(origin, size.unwrap_or_default())
}

/// The rectangle from `x`, `y` of `width` by `height` pixels; empty for a
/// negative size.
pub fn rect(x: i32, y: i32, width: i32, height: i32) -> Rect {
    units_rect(x * 64, y * 64, (x + width) * 64, (y + height) * 64)
}

/// An `Rgba8Premul` canvas of `width` by `height` pixels.
pub fn canvas(width: u32, height: u32) -> Checked<BufferDesc> {
    BufferDesc::new(width, height, width * 4, PixelFormat::Rgba8Premul).map_err(failed)
}

/// A display list builder that keeps the first error, which `finish`
/// returns.
struct Builder {
    list: ListBuilder,
    error: Option<ListError>,
}

impl Builder {
    fn new() -> Builder {
        Builder {
            list: ListBuilder::new(),
            error: None,
        }
    }

    fn push(&mut self, command: Command) -> &mut Builder {
        if self.error.is_none() {
            self.error = self.list.push(command).err();
        }
        self
    }

    fn fill(&mut self, rect: Rect, color: Color) -> &mut Builder {
        self.push(Command::FillRect { rect, color })
    }

    fn clip(&mut self, rect: Rect, offset: Point) -> &mut Builder {
        self.push(Command::PushClip { rect, offset })
    }

    fn pop(&mut self) -> &mut Builder {
        self.push(Command::PopClip)
    }

    fn finish(self) -> Checked<DisplayList> {
        match self.error {
            Some(error) => Err(failed(error)),
            None => self.list.finish().map_err(failed),
        }
    }
}

const BACKGROUND: Color = Color::opaque(240, 240, 236);
const RED: Color = Color::opaque(200, 40, 30);
const BLUE: Color = Color::opaque(30, 90, 200);
const GREEN: Color = Color::opaque(40, 150, 60);
const INK: Color = Color::opaque(20, 20, 60);
/// A run of `glyphs` of the test font with their origins.
fn run(color: Color, glyphs: &[(GlyphId, Point)]) -> Command {
    Command::Glyphs {
        font: FONT,
        size: SIZE,
        color,
        glyphs: glyphs
            .iter()
            .map(|&(id, origin)| Glyph { id, origin })
            .collect(),
    }
}

/// A case that both rasterizers draw: opaque colors and whole-pixel edges,
/// so every pixel is decided by its center. When `previous` is set, it is
/// drawn whole first, and the case's list is drawn over it inside the
/// rectangles of `damage` only; otherwise the list is drawn whole.
pub struct Case {
    pub name: &'static str,
    pub desc: BufferDesc,
    pub previous: Option<DisplayList>,
    pub list: DisplayList,
    pub damage: Vec<(u32, u32, u32, u32)>,
    pub glyphs: TestGlyphs,
}

impl Case {
    /// The canvas that `rasterizer` draws for the case, and the work count
    /// of drawing its list.
    pub fn draw(&self, rasterizer: &mut dyn Rasterizer) -> Checked<(RenderTarget, WorkCount)> {
        let mut target = RenderTarget::new(self.desc);
        let whole = target.whole();
        let damage = match &self.previous {
            Some(previous) => {
                rasterizer
                    .render(
                        previous,
                        &whole,
                        &self.glyphs,
                        &ImageStore::EMPTY,
                        &mut target,
                    )
                    .map_err(failed)?;
                let mut damage = Damage::new(self.desc);
                for &(x, y, width, height) in &self.damage {
                    damage.add(x, y, width, height);
                }
                damage
            }
            None => whole,
        };
        let work = rasterizer
            .render(
                &self.list,
                &damage,
                &self.glyphs,
                &ImageStore::EMPTY,
                &mut target,
            )
            .map_err(failed)?;
        Ok((target, work))
    }
}

/// A background and three opaque rectangles that overlap in drawing
/// order, the outer two moved by `shift` pixels.
fn overlapping(colors: [Color; 3], shift: i32) -> Checked<DisplayList> {
    let mut list = Builder::new();
    list.fill(rect(0, 0, 64, 48), BACKGROUND)
        .fill(rect(4 + shift, 4, 30, 20), colors[0])
        .fill(rect(16, 12 + shift, 30, 22), colors[1])
        .fill(rect(28 - shift, 20, 32, 24), colors[2]);
    list.finish()
}

/// Rectangles partly beyond every edge of the canvas, and rectangles
/// without width or height.
fn beyond_the_edges() -> Checked<DisplayList> {
    let mut list = Builder::new();
    list.fill(rect(0, 0, 64, 48), BACKGROUND)
        .fill(rect(-10, -5, 30, 20), RED)
        .fill(rect(50, 30, 30, 30), BLUE)
        .fill(rect(-3, 40, 73, 12), GREEN)
        .fill(rect(30, 10, 0, 20), INK)
        .fill(rect(10, 25, 30, 0), INK);
    list.finish()
}

/// Two nested groups whose clips intersect, with rectangles across their
/// edges and one after the inner group.
fn nested_clips() -> Checked<DisplayList> {
    let mut list = Builder::new();
    list.fill(rect(0, 0, 64, 48), BACKGROUND)
        .clip(rect(8, 6, 40, 34), Point::default())
        .fill(rect(0, 0, 64, 48), RED)
        .clip(rect(20, 0, 40, 30), Point::default())
        .fill(rect(4, 4, 56, 40), BLUE)
        .pop()
        .fill(rect(40, 30, 24, 18), GREEN)
        .pop();
    list.finish()
}

/// A group scrolled up by one row of 31 pixels, with rows of rectangles
/// partly moved out of its clip.
fn scrolled() -> Checked<DisplayList> {
    let mut list = Builder::new();
    list.fill(rect(0, 0, 64, 48), BACKGROUND)
        .clip(rect(4, 4, 56, 40), px(0, -31));
    for (row, color) in [RED, BLUE, GREEN, INK, RED].into_iter().enumerate() {
        let row = row as i32;
        list.fill(rect(6 + row, row * 16, 50, 12), color);
    }
    list.pop();
    list.finish()
}

/// Glyphs whose origins lie at whole pixels, the third with a baseline
/// that rounds to a whole pixel.
fn whole_glyphs() -> Checked<DisplayList> {
    let mut list = Builder::new();
    list.fill(rect(0, 0, 64, 48), BACKGROUND).push(run(
        INK,
        &[
            (GLYPHS[0], px(6, 30)),
            (GLYPHS[1], px(20, 30)),
            (GLYPHS[2], Point::new(fx(34, 0), fx(30, 20))),
            (GLYPHS[0], px(48, 44)),
        ],
    ));
    list.finish()
}

/// The cases that the reference rasterizer draws too.
pub fn reference_cases() -> Checked<Vec<Case>> {
    let desc = canvas(64, 48)?;
    let case = |name, list| Case {
        name,
        desc,
        previous: None,
        list,
        damage: Vec::new(),
        glyphs: TestGlyphs::none(),
    };
    Ok(vec![
        case("overlapping-rects", overlapping([RED, BLUE, GREEN], 0)?),
        case("rects-beyond-the-edges", beyond_the_edges()?),
        case("nested-clips", nested_clips()?),
        case("scrolled-group", scrolled()?),
        Case {
            previous: Some(overlapping([RED, BLUE, GREEN], 0)?),
            damage: vec![(2, 2, 10, 10), (20, 15, 30, 8), (40, 30, 30, 30)],
            ..case("damage-only", overlapping([GREEN, RED, INK], 3)?)
        },
        Case {
            glyphs: TestGlyphs::binary()?,
            ..case("binary-glyphs", whole_glyphs()?)
        },
    ])
}
