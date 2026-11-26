//! Strokes: a miter join is cut off as a bevel join where its point would
//! lie farther than 4 half widths from the joint, and a stroke without
//! width covers nothing.

pub mod support;

use kosmo_render::{Cap, Color, Command, Join, ListBuilder, Path, RenderTarget, VelloRasterizer};
use kosmo_types::{Fx, Point};
use support::glyphs::TestGlyphs;
use support::scenes::{Scene, canvas, fx, px, rect};
use support::{Checked, failed};

const BACKGROUND: Color = Color::opaque(240, 240, 236);
const INK: Color = Color::opaque(20, 20, 60);

/// `commands` over the background of a canvas of 64 by 48 pixels, drawn
/// whole by a new rasterizer.
fn drawn(commands: Vec<Command>) -> Checked<RenderTarget> {
    let mut list = ListBuilder::new();
    list.push(Command::FillRect {
        rect: rect(0, 0, 64, 48),
        color: BACKGROUND,
    })
    .map_err(failed)?;
    for command in commands {
        list.push(command).map_err(failed)?;
    }
    let scene = Scene {
        name: "strokes",
        desc: canvas(64, 48)?,
        list: list.finish().map_err(failed)?,
        glyphs: TestGlyphs::none(),
    };
    scene.draw(&mut VelloRasterizer::new())
}

/// A V with its apex at (32, `top`) pixels: two segments from the point
/// `dx` to the left of the apex and `dy` below it, to the apex, to the
/// point `dx` to the right of it and `dy` below it, in 1/64 pixel.
fn vee(top: i32, dx: i32, dy: i32) -> Path {
    let apex = px(32, top);
    let end = |dx: i32| {
        Point::new(
            Fx::from_units(apex.x.units() + dx),
            Fx::from_units(apex.y.units() + dy),
        )
    };
    let mut path = Path::new(end(-dx));
    path.line_to(apex).line_to(end(dx));
    path
}

fn stroke(path: &Path, width: Fx, join: Join, cap: Cap) -> Command {
    Command::StrokePath {
        path: path.clone(),
        width,
        join,
        cap,
        color: INK,
    }
}

/// At a joint of 20 degrees the point of a miter join lies about 5.8 half
/// widths from the joint, beyond the limit of 4, so the miter join draws
/// the bytes of a bevel join. At a joint of 90 degrees it lies about 1.4
/// half widths from the joint, and the two joins differ.
#[test]
fn a_miter_join_beyond_4_half_widths_is_a_bevel_join() -> Checked {
    // Arms of 28 pixels at 10 degrees from the vertical: 4.86 pixels to
    // the side and 27.58 down.
    let sharp = vee(16, 311, 1765);
    // Arms at 45 degrees from the vertical.
    let right = vee(12, 20 * 64, 20 * 64);
    let joined = |path: &Path, join| drawn(vec![stroke(path, fx(4, 0), join, Cap::Butt)]);
    assert!(
        joined(&sharp, Join::Miter)? == joined(&sharp, Join::Bevel)?,
        "at 20 degrees the miter join is not cut off"
    );
    assert!(
        joined(&right, Join::Miter)? != joined(&right, Join::Bevel)?,
        "at 90 degrees the miter join is cut off"
    );
    Ok(())
}

/// A stroke without width covers nothing, with every join and cap, along
/// lines and along a curve.
#[test]
fn a_stroke_without_width_covers_nothing() -> Checked {
    let mut path = vee(12, 20 * 64, 20 * 64);
    path.move_to(px(8, 40))
        .cubic_to(px(20, 30), px(40, 47), px(56, 38));
    let strokes = [
        (Join::Miter, Cap::Butt),
        (Join::Round, Cap::Round),
        (Join::Bevel, Cap::Square),
    ]
    .map(|(join, cap)| stroke(&path, Fx::ZERO, join, cap));
    assert!(
        drawn(strokes.to_vec())? == drawn(Vec::new())?,
        "a stroke without width changed the canvas"
    );
    Ok(())
}
