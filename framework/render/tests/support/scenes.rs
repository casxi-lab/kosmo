//! The scenes that the tests draw: one for each kind of command on a
//! canvas of 64 by 48 pixels, a composite of all of them on 128 by 96, and
//! the cases that the reference rasterizer draws too.

use kosmo_render::{
    Cap, Color, Command, DisplayList, Glyph, GlyphId, GlyphKey, ImageStore, Join, ListBuilder,
    ListError, Path, Rasterizer, RenderTarget, Step, WorkCount,
};
use kosmo_types::{BufferDesc, Damage, Fx, PixelFormat, Point, Rect, Size};

use super::glyphs::{self, FONT, GLYPHS, SIZE, TestGlyphs};
use super::{Checked, failed};

/// The names of the scenes of one kind of command each, in the order that
/// [`primitives`] gives them.
pub const PRIMITIVES: [&str; 6] = [
    "fill-rect",
    "fill-rounded-rect",
    "fill-path",
    "stroke-path",
    "glyphs",
    "clip-group",
];

/// A scene: a display list for a whole redraw of an `Rgba8Premul` canvas,
/// with the glyph coverage it reads.
pub struct Scene {
    pub name: &'static str,
    pub desc: BufferDesc,
    pub list: DisplayList,
    pub glyphs: TestGlyphs,
}

impl Scene {
    /// The scene drawn whole by `rasterizer` into a new canvas, checking
    /// the work count.
    pub fn draw(&self, rasterizer: &mut dyn Rasterizer) -> Checked<RenderTarget> {
        let mut target = RenderTarget::new(self.desc).ok_or_else(|| failed("the target"))?;
        let whole = target.whole();
        let work = rasterizer
            .render(
                &self.list,
                &whole,
                &self.glyphs,
                &ImageStore::EMPTY,
                &mut target,
            )
            .map_err(failed)?;
        if work != WorkCount::of(&self.list, &whole) {
            return Err(format!("{}: work count {work:?}", self.name));
        }
        Ok(target)
    }
}

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

/// `red`, `green`, `blue` and `alpha`, premultiplied.
fn premultiplied(red: u8, green: u8, blue: u8, alpha: u8) -> Checked<Color> {
    Color::premultiplied(red, green, blue, alpha).ok_or_else(|| failed("not premultiplied"))
}

/// The circle around `x`, `y` pixels of radius `radius` pixels, of four
/// cubic segments whose control points lie 0.5523 times the radius from
/// the ends, rounded down to 1/64 pixel.
fn circle(x: i32, y: i32, radius: i32) -> Path {
    let (cx, cy, r) = (x * 64, y * 64, radius * 64);
    let k = r * 5523 / 10000;
    let p = |dx: i32, dy: i32| Point::new(Fx::from_units(cx + dx), Fx::from_units(cy + dy));
    let mut path = Path::new(p(r, 0));
    path.cubic_to(p(r, k), p(k, r), p(0, r))
        .cubic_to(p(-k, r), p(-r, k), p(-r, 0))
        .cubic_to(p(-r, -k), p(-k, -r), p(0, -r))
        .cubic_to(p(k, -r), p(r, -k), p(r, 0))
        .close();
    path
}

/// A five-pointed star drawn as one self-intersecting subpath around
/// (44, 35) pixels with a radius of 11 pixels; its vertices, at every
/// second fifth of a turn from the top, are written in 1/64 pixel.
fn star() -> Path {
    let p = |x, y| Point::new(Fx::from_units(x), Fx::from_units(y));
    let mut path = Path::new(p(2816, 1536));
    path.line_to(p(3230, 2810))
        .line_to(p(2146, 2022))
        .line_to(p(3486, 2022))
        .line_to(p(2402, 2810))
        .close();
    path
}

/// A triangle with one corner at a half and a quarter pixel.
fn triangle() -> Path {
    let mut path = Path::new(px(4, 40));
    path.line_to(Point::new(fx(16, 32), fx(8, 16)))
        .line_to(px(28, 40))
        .close();
    path
}

/// The zigzag of the stroke scene, `top` pixels from the canvas's top.
fn zigzag(top: i32) -> Path {
    let mut path = Path::new(px(6, top + 10));
    path.line_to(px(18, top))
        .line_to(px(30, top + 10))
        .line_to(px(42, top))
        .line_to(px(56, top + 8));
    path
}

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

/// A scene of 64 by 48 pixels without glyphs.
fn small(name: &'static str, list: Builder) -> Checked<Scene> {
    Ok(Scene {
        name,
        desc: canvas(64, 48)?,
        list: list.finish()?,
        glyphs: TestGlyphs::none(),
    })
}

/// Rectangles with whole-pixel edges and with quarter-pixel edges, and a
/// half-transparent one over both.
fn fill_rect() -> Checked<Scene> {
    let mut list = Builder::new();
    let quarters = units_rect(
        fx(28, 16).units(),
        fx(4, 48).units(),
        fx(58, 32).units(),
        fx(20, 16).units(),
    );
    list.fill(rect(0, 0, 64, 48), BACKGROUND)
        .fill(rect(4, 4, 20, 16), RED)
        .fill(quarters, BLUE)
        .fill(rect(12, 16, 40, 24), premultiplied(0, 64, 16, 128)?);
    small("fill-rect", list)
}

/// Rounded rectangles with radius 0, 4 pixels and more than half their
/// shorter side.
fn fill_rounded_rect() -> Checked<Scene> {
    let rounded = |rect, radius, color| Command::FillRoundedRect {
        rect,
        radius,
        color,
    };
    let mut list = Builder::new();
    list.fill(rect(0, 0, 64, 48), BACKGROUND)
        .push(rounded(rect(4, 4, 24, 16), Fx::ZERO, RED))
        .push(rounded(rect(34, 4, 26, 16), fx(4, 0), BLUE))
        .push(rounded(rect(4, 26, 56, 18), fx(20, 0), GREEN));
    small("fill-rounded-rect", list)
}

/// A triangle, a circle of four cubic segments and a self-intersecting
/// star, which the nonzero rule fills in the middle too.
fn fill_path() -> Checked<Scene> {
    let mut list = Builder::new();
    list.fill(rect(0, 0, 64, 48), BACKGROUND)
        .push(Command::FillPath {
            path: triangle(),
            color: RED,
        })
        .push(Command::FillPath {
            path: circle(42, 13, 10),
            color: BLUE,
        })
        .push(Command::FillPath {
            path: star(),
            color: GREEN,
        });
    small("fill-path", list)
}

/// The same zigzag stroked three times, 3 pixels wide: with a miter join
/// and butt caps, round joins and caps, and a bevel join and square caps.
fn stroke_path() -> Checked<Scene> {
    let mut list = Builder::new();
    list.fill(rect(0, 0, 64, 48), BACKGROUND);
    for (top, join, cap, color) in [
        (3, Join::Miter, Cap::Butt, RED),
        (18, Join::Round, Cap::Round, BLUE),
        (33, Join::Bevel, Cap::Square, GREEN),
    ] {
        let path = zigzag(top);
        list.push(Command::StrokePath {
            path,
            width: fx(3, 0),
            join,
            cap,
            color,
        });
    }
    small("stroke-path", list)
}

/// The glyphs of the glyph scene with their origins and the steps that
/// the origins give: the three glyphs and the first again, at the four
/// quarter-pixel steps in order.
pub fn glyph_origins() -> [(GlyphId, Point, Step); 4] {
    let baseline = fx(30, 20);
    [
        (GLYPHS[0], Point::new(fx(6, 3), baseline), Step::Whole),
        (GLYPHS[1], Point::new(fx(20, 13), baseline), Step::Quarter),
        (GLYPHS[2], Point::new(fx(34, 30), baseline), Step::Half),
        (
            GLYPHS[0],
            Point::new(fx(48, 45), baseline),
            Step::ThreeQuarters,
        ),
    ]
}

/// Four glyphs, one at each quarter-pixel step, whose masks `rasterizer`
/// fills from their outlines.
fn glyph_run(rasterizer: &mut dyn Rasterizer) -> Checked<Scene> {
    let origins = glyph_origins();
    let keys: Vec<GlyphKey> = origins
        .iter()
        .map(|&(glyph, _, step)| glyphs::key(glyph, step))
        .collect();
    let mut list = Builder::new();
    list.fill(rect(0, 0, 64, 48), BACKGROUND)
        .push(run(INK, &origins.map(|(glyph, origin, _)| (glyph, origin))));
    Ok(Scene {
        glyphs: TestGlyphs::filled(rasterizer, &keys)?,
        ..small("glyphs", list)?
    })
}

/// Two nested groups: the outer one moved by whole pixels, the inner one
/// by a negative offset of whole pixels and a quarter pixel, with shapes
/// across the edges of both clips.
fn clip_group() -> Checked<Scene> {
    let mut list = Builder::new();
    list.fill(rect(0, 0, 64, 48), BACKGROUND)
        .clip(rect(6, 4, 52, 40), px(2, 3))
        .fill(rect(0, 0, 60, 50), Color::opaque(200, 220, 240))
        .clip(rect(8, 6, 32, 24), Point::new(fx(-6, -16), fx(-10, 0)))
        .fill(rect(10, 12, 20, 28), RED)
        .push(Command::FillPath {
            path: circle(32, 22, 9),
            color: BLUE,
        })
        .pop()
        .fill(rect(40, 30, 30, 20), GREEN)
        .pop();
    small("clip-group", list)
}

/// The scenes of one kind of command each, named as [`PRIMITIVES`] lists
/// them; `rasterizer` fills the masks of the glyph scene.
pub fn primitives(rasterizer: &mut dyn Rasterizer) -> Checked<Vec<Scene>> {
    Ok(vec![
        fill_rect()?,
        fill_rounded_rect()?,
        fill_path()?,
        stroke_path()?,
        glyph_run(rasterizer)?,
        clip_group()?,
    ])
}

/// A scene of 128 by 96 pixels with every kind of command: a button with
/// a label, a star, a triangle, a stroked line, a scrolled group with rows
/// of text and a half-transparent circle, and a half-transparent band;
/// `rasterizer` fills the masks of the three glyphs at every step.
pub fn composite(rasterizer: &mut dyn Rasterizer) -> Checked<Scene> {
    let keys: Vec<GlyphKey> = GLYPHS
        .iter()
        .flat_map(|&glyph| Step::ALL.map(|step| glyphs::key(glyph, step)))
        .collect();
    let white = Color::opaque(255, 255, 255);
    let mut list = Builder::new();
    list.fill(rect(0, 0, 128, 96), BACKGROUND)
        .push(Command::FillRoundedRect {
            rect: rect(8, 8, 48, 24),
            radius: fx(12, 0),
            color: BLUE,
        })
        .push(run(
            white,
            &[
                (GLYPHS[0], Point::new(fx(14, 0), fx(25, 0))),
                (GLYPHS[1], Point::new(fx(23, 20), fx(25, 0))),
                (GLYPHS[2], Point::new(fx(32, 40), fx(25, 0))),
                (GLYPHS[1], Point::new(fx(41, 58), fx(25, 0))),
            ],
        ))
        .push(Command::FillPath {
            path: star(),
            color: RED,
        })
        .push(Command::FillPath {
            path: triangle(),
            color: GREEN,
        });
    let mut line = Path::new(px(6, 88));
    line.line_to(px(30, 50))
        .quad_to(px(44, 30), Point::new(fx(60, 32), fx(56, 16)))
        .cubic_to(px(70, 70), px(90, 20), px(122, 36));
    let (join, cap) = (Join::Round, Cap::Round);
    list.push(Command::StrokePath {
        path: line,
        width: fx(2, 32),
        join,
        cap,
        color: INK,
    })
    .clip(rect(64, 40, 60, 52), Point::new(Fx::ZERO, fx(-31, 0)));
    for row in 0..6_u8 {
        let top = 40 + 16 * i32::from(row);
        let shade = 255 - 20 * row;
        let first = GLYPHS[usize::from(row) % 3];
        let second = GLYPHS[(usize::from(row) + 1) % 3];
        list.fill(rect(64, top, 60, 14), Color::opaque(shade, shade, 255))
            .push(run(
                INK,
                &[
                    (
                        first,
                        Point::new(fx(68, 16 * i32::from(row)), fx(top + 12, 0)),
                    ),
                    (second, Point::new(fx(78, 0), fx(top + 12, 40))),
                ],
            ));
    }
    let glaze = premultiplied(0, 64, 16, 128)?;
    let band = units_rect(0, fx(70, 16).units(), fx(128, 0).units(), fx(96, 0).units());
    list.push(Command::FillPath {
        path: circle(100, 82, 12),
        color: glaze,
    })
    .pop()
    .fill(band, premultiplied(0, 0, 32, 64)?);
    Ok(Scene {
        name: "composite",
        desc: canvas(128, 96)?,
        list: list.finish()?,
        glyphs: TestGlyphs::filled(rasterizer, &keys)?,
    })
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
        let mut target = RenderTarget::new(self.desc).ok_or_else(|| failed("the target"))?;
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
