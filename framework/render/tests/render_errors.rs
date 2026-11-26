//! The errors of the renderer interface: each is distinct, and rendering
//! that fails writes nothing into the target.

pub mod support;

use kosmo_render::{
    Color, Command, CoverageMask, Glyph, ImageStore, ListBuilder, MAX_ITEMS, NoGlyphs, Path,
    Rasterizer, RenderError, RenderTarget, Step, VelloRasterizer,
};
use kosmo_types::{Damage, Fx, Point};
use support::Checked;
use support::glyphs::{self, FONT, GLYPHS, SIZE, TestGlyphs};
use support::scenes::{canvas, px, rect};

/// A target of 16 by 8 pixels whose bytes are all 7.
fn sevens() -> Checked<RenderTarget> {
    let desc = canvas(16, 8)?;
    RenderTarget::from_bytes(desc, vec![7; desc.byte_len() as usize])
        .ok_or_else(|| support::failed("the target"))
}

#[test]
fn a_damage_region_of_a_buffer_of_another_size_is_refused() -> Checked {
    let mut target = sevens()?;
    let mut builder = ListBuilder::new();
    let fill = Command::FillRect {
        rect: rect(0, 0, 4, 4),
        color: Color::opaque(1, 2, 3),
    };
    builder.push(fill).unwrap();
    let list = builder.finish().unwrap();
    let mut damage = Damage::new(canvas(16, 9)?);
    damage.add(0, 0, 4, 4);
    let result =
        VelloRasterizer::new().render(&list, &damage, &NoGlyphs, &ImageStore::EMPTY, &mut target);
    assert_eq!(result, Err(RenderError::DamageMismatch));
    assert!(target.bytes().iter().all(|&byte| byte == 7));
    Ok(())
}

#[test]
fn a_missing_glyph_is_refused_even_outside_the_damage() -> Checked {
    let mut target = sevens()?;
    let coverage = TestGlyphs::binary()?;
    let mut builder = ListBuilder::new();
    let glyphs = [
        (GLYPHS[0], px(1, 12)),
        (GLYPHS[1], Point::new(Fx::from_units(64 + 20), px(0, 12).y)),
    ]
    .map(|(id, origin)| Glyph { id, origin })
    .to_vec();
    builder
        .push(Command::Glyphs {
            font: FONT,
            size: SIZE,
            color: Color::opaque(0, 0, 0),
            glyphs,
        })
        .unwrap();
    let list = builder.finish().unwrap();
    // The second glyph lies at the quarter step, which has no mask.
    let mut damage = Damage::new(target.desc());
    damage.add(15, 0, 1, 1);
    let result =
        VelloRasterizer::new().render(&list, &damage, &coverage, &ImageStore::EMPTY, &mut target);
    assert_eq!(
        result,
        Err(RenderError::MissingGlyph(glyphs::key(
            GLYPHS[1],
            Step::Quarter
        )))
    );
    assert!(target.bytes().iter().all(|&byte| byte == 7));
    Ok(())
}

#[test]
fn an_origin_that_does_not_fit_with_its_offsets_is_refused() -> Checked {
    let mut target = sevens()?;
    let mut builder = ListBuilder::new();
    let far = Point::new(Fx::MAX, Fx::ZERO);
    builder
        .push(Command::PushClip {
            rect: rect(0, 0, 16, 8),
            offset: far,
        })
        .unwrap();
    let glyph = Glyph {
        id: GLYPHS[0],
        origin: px(1, 0),
    };
    builder
        .push(Command::Glyphs {
            font: FONT,
            size: SIZE,
            color: Color::opaque(0, 0, 0),
            glyphs: vec![glyph],
        })
        .unwrap();
    builder.push(Command::PopClip).unwrap();
    let list = builder.finish().unwrap();
    let whole = target.whole();
    let result =
        VelloRasterizer::new().render(&list, &whole, &NoGlyphs, &ImageStore::EMPTY, &mut target);
    assert_eq!(result, Err(RenderError::OutOfRange));
    assert!(target.bytes().iter().all(|&byte| byte == 7));
    Ok(())
}

#[test]
fn offsets_of_nested_groups_that_do_not_fit_are_refused() -> Checked {
    let mut target = sevens()?;
    let mut builder = ListBuilder::new();
    let far = Point::new(Fx::MAX, Fx::ZERO);
    for _ in 0..2 {
        builder
            .push(Command::PushClip {
                rect: rect(0, 0, 16, 8),
                offset: far,
            })
            .unwrap();
    }
    builder
        .push(Command::FillRect {
            rect: rect(0, 0, 4, 4),
            color: Color::opaque(1, 2, 3),
        })
        .unwrap();
    for _ in 0..2 {
        builder.push(Command::PopClip).unwrap();
    }
    let list = builder.finish().unwrap();
    let whole = target.whole();
    let result =
        VelloRasterizer::new().render(&list, &whole, &NoGlyphs, &ImageStore::EMPTY, &mut target);
    assert_eq!(result, Err(RenderError::OutOfRange));
    assert!(target.bytes().iter().all(|&byte| byte == 7));
    Ok(())
}

#[test]
fn an_outline_beyond_the_items_is_over_the_limit() {
    let mut outline = Path::new(Point::default());
    for _ in 0..MAX_ITEMS {
        outline.line_to(px(1, 1));
    }
    let mut mask = CoverageMask::new(0, 0, 2, 2).unwrap();
    mask.coverage_mut()[0] = 9;
    let mut rasterizer = VelloRasterizer::new();
    assert_eq!(
        rasterizer.fill_coverage(&outline, &mut mask),
        Err(RenderError::OverLimit)
    );
    assert_eq!(mask.coverage(), [9, 0, 0, 0]);
}
