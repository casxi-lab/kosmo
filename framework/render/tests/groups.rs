//! Drawing inside a group: a glyph lies where its origin, with the offset
//! of the group added, places it.

pub mod support;

use kosmo_render::{
    Color, Command, Glyph, GlyphKey, ListBuilder, RenderTarget, Step, VelloRasterizer, place_glyph,
};
use kosmo_types::{Fx, Point};
use support::glyphs::{self, FONT, GLYPHS, SIZE, TestGlyphs};
use support::scenes::{Scene, canvas, fx, rect};
use support::{Checked, failed};

const BACKGROUND: Color = Color::opaque(240, 240, 236);
const INK: Color = Color::opaque(20, 20, 60);

/// The offset of the group: 6 1/4 pixels to the left and 10 pixels up.
const OFFSET: Point = Point::new(Fx::from_units(-400), Fx::from_units(-640));

/// `commands` over the background of a canvas of 48 by 32 pixels, drawn
/// whole by `rasterizer` with the masks of `coverage`.
fn drawn(
    rasterizer: &mut VelloRasterizer,
    coverage: &TestGlyphs,
    commands: Vec<Command>,
) -> Checked<RenderTarget> {
    let mut list = ListBuilder::new();
    list.push(Command::FillRect {
        rect: rect(0, 0, 48, 32),
        color: BACKGROUND,
    })
    .map_err(failed)?;
    for command in commands {
        list.push(command).map_err(failed)?;
    }
    let scene = Scene {
        name: "groups",
        desc: canvas(48, 32)?,
        list: list.finish().map_err(failed)?,
        glyphs: coverage.clone(),
    };
    scene.draw(rasterizer)
}

/// A run of the three test glyphs at `origins`.
fn run(origins: [Point; 3]) -> Command {
    Command::Glyphs {
        font: FONT,
        size: SIZE,
        color: INK,
        glyphs: GLYPHS
            .iter()
            .zip(origins)
            .map(|(&id, origin)| Glyph { id, origin })
            .collect(),
    }
}

/// A glyph run in a group moved by (-6.25, -10) pixels, whose clip covers
/// the whole canvas, draws the bytes of the same run outside any group with
/// each origin moved by the offset, and not those of the run at its own
/// origins. The quarter pixel of the offset moves each glyph to another
/// step.
#[test]
fn a_glyph_in_a_moved_group_lies_at_its_origin_with_the_offset() -> Checked {
    let mut rasterizer = VelloRasterizer::new();
    let keys: Vec<GlyphKey> = GLYPHS
        .iter()
        .flat_map(|&glyph| Step::ALL.map(|step| glyphs::key(glyph, step)))
        .collect();
    let coverage = TestGlyphs::filled(&mut rasterizer, &keys)?;
    let baseline = fx(30, 20);
    let origins = [
        Point::new(fx(10, 13), baseline),
        Point::new(fx(22, 30), baseline),
        Point::new(fx(34, 45), baseline),
    ];
    let mut moved = origins;
    for (origin, own) in moved.iter_mut().zip(origins) {
        *origin = own
            .translated(OFFSET.x, OFFSET.y)
            .ok_or_else(|| failed("the moved origin"))?;
        let step = |at: Point| place_glyph(at.x, at.y).step;
        assert_ne!(step(*origin), step(own), "{own:?}");
    }
    let grouped = drawn(
        &mut rasterizer,
        &coverage,
        vec![
            Command::PushClip {
                rect: rect(0, 0, 48, 32),
                offset: OFFSET,
            },
            run(origins),
            Command::PopClip,
        ],
    )?;
    let direct = drawn(&mut rasterizer, &coverage, vec![run(moved)])?;
    let unmoved = drawn(&mut rasterizer, &coverage, vec![run(origins)])?;
    assert!(
        grouped == direct,
        "the glyphs in the group differ from the glyphs at the moved origins"
    );
    assert!(
        grouped != unmoved,
        "the glyphs in the group lie at their own origins"
    );
    Ok(())
}
