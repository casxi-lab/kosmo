//! The renderer interface with two implementations: the per-pixel
//! reference rasterizer and vello_cpu draw the same bytes, with the same
//! work count, for scenes of opaque colors and whole-pixel edges, whose
//! pixels are decided by their centers.

pub mod support;

use kosmo_render::{
    Color, Command, ImageStore, ListBuilder, NoGlyphs, Rasterizer, RenderError, RenderTarget,
    VelloRasterizer,
};
use support::reference::Reference;
use support::scenes::{self, Case, canvas, rect};
use support::{Checked, failed};

/// The case named `name`.
fn case(name: &str) -> Checked<Case> {
    scenes::reference_cases()?
        .into_iter()
        .find(|case| case.name == name)
        .ok_or_else(|| failed(name))
}

/// Draws `case` with both rasterizers, checks that they give the same bytes
/// and work count and that the case draws more than its background, and
/// returns the canvas.
fn same_with_both(case: &Case) -> Checked<RenderTarget> {
    let (reference, reference_work) = case.draw(&mut Reference)?;
    let (vello, vello_work) = case.draw(&mut VelloRasterizer::new())?;
    assert_eq!(reference_work, vello_work, "{}", case.name);
    let first = reference
        .bytes()
        .iter()
        .zip(vello.bytes())
        .position(|(a, b)| a != b);
    assert_eq!(first, None, "{}: the first byte that differs", case.name);
    let pixels = reference.bytes().as_chunks::<4>().0;
    assert!(
        pixels.iter().any(|pixel| pixel != &pixels[0]),
        "{}",
        case.name
    );
    Ok(reference)
}

#[test]
fn overlapping_opaque_rectangles_are_drawn_in_order() -> Checked {
    same_with_both(&case("overlapping-rects")?)?;
    Ok(())
}

#[test]
fn rectangles_beyond_the_edges_are_clipped_and_empty_ones_draw_nothing() -> Checked {
    same_with_both(&case("rects-beyond-the-edges")?)?;
    Ok(())
}

#[test]
fn nested_clips_intersect() -> Checked {
    same_with_both(&case("nested-clips")?)?;
    Ok(())
}

#[test]
fn a_scrolled_group_moves_its_content_out_of_its_clip() -> Checked {
    same_with_both(&case("scrolled-group")?)?;
    Ok(())
}

#[test]
fn only_the_damage_is_drawn_over_the_previous_frame() -> Checked {
    let damaged = case("damage-only")?;
    let drawn = same_with_both(&damaged)?;
    // Outside the damage the previous frame stays; inside it the new list
    // is drawn as a whole redraw draws it.
    let whole = |list| Case {
        previous: None,
        list,
        damage: Vec::new(),
        ..case("damage-only").unwrap()
    };
    let (previous, _) = whole(damaged.previous.clone().unwrap()).draw(&mut Reference)?;
    let (redrawn, _) = whole(damaged.list.clone()).draw(&mut Reference)?;
    let inside = |x: u32, y: u32| {
        damaged.damage.iter().any(|&(left, top, width, height)| {
            left <= x && x < left + width && top <= y && y < top + height
        })
    };
    let (mut kept, mut changed) = (0, 0);
    for y in 0..48 {
        for x in 0..64 {
            let at = (y * 64 + x) as usize * 4;
            let pixel = |target: &RenderTarget| target.bytes()[at..at + 4].to_vec();
            let expected = if inside(x, y) { &redrawn } else { &previous };
            assert_eq!(pixel(&drawn), pixel(expected), "pixel {x}, {y}");
            if pixel(&previous) != pixel(&redrawn) {
                if inside(x, y) {
                    changed += 1;
                } else {
                    kept += 1;
                }
            }
        }
    }
    // The two frames differ both inside and outside the damage.
    assert!(kept > 0 && changed > 0);
    Ok(())
}

#[test]
fn glyphs_with_coverage_of_0_and_255_at_whole_pixels_are_drawn_alike() -> Checked {
    same_with_both(&case("binary-glyphs")?)?;
    Ok(())
}

#[test]
fn the_reference_reports_other_commands_as_unsupported() -> Checked {
    let mut target = RenderTarget::new(canvas(4, 4)?).ok_or_else(|| failed("the target"))?;
    let whole = target.whole();
    let half = Color::premultiplied(0, 0, 64, 128).unwrap();
    let rounded = Command::FillRoundedRect {
        rect: rect(0, 0, 2, 2),
        radius: Default::default(),
        color: half,
    };
    let translucent = Command::FillRect {
        rect: rect(0, 0, 2, 2),
        color: half,
    };
    for command in [rounded, translucent] {
        let mut builder = ListBuilder::new();
        builder.push(command).unwrap();
        let list = builder.finish().unwrap();
        let result = Reference.render(&list, &whole, &NoGlyphs, &ImageStore::EMPTY, &mut target);
        assert_eq!(result, Err(RenderError::Unsupported));
    }
    assert!(target.bytes().iter().all(|&byte| byte == 0));
    Ok(())
}
