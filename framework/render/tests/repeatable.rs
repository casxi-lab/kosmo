//! Rasterizing the same display list twice gives the same bytes: each
//! scene of one kind of command and the composite scene, drawn by a new
//! rasterizer and by one that has drawn every scene before, with glyph
//! masks that each of them fills.

pub mod support;

use kosmo_render::{Step, VelloRasterizer, place_glyph};
use support::Checked;
use support::scenes::{self, PRIMITIVES, Scene};

/// Every scene, with masks filled by `rasterizer`.
fn all(rasterizer: &mut VelloRasterizer) -> Checked<Vec<Scene>> {
    let mut scenes = scenes::primitives(rasterizer)?;
    scenes.push(scenes::composite(rasterizer)?);
    Ok(scenes)
}

#[test]
fn a_new_and_a_reused_rasterizer_draw_every_scene_alike() -> Checked {
    let mut reused = VelloRasterizer::new();
    for scene in all(&mut reused)? {
        scene.draw(&mut reused)?;
    }
    let names: Vec<&str> = all(&mut reused)?.iter().map(|scene| scene.name).collect();
    assert_eq!(names[..6], PRIMITIVES);
    assert_eq!(names[6], "composite");
    for (index, name) in names.iter().enumerate() {
        let mut fresh = VelloRasterizer::new();
        let first = all(&mut fresh)?.remove(index).draw(&mut fresh)?;
        let second = all(&mut reused)?.remove(index).draw(&mut reused)?;
        assert!(
            first == second,
            "{name}: a new and a reused rasterizer differ"
        );
        let again = all(&mut reused)?.remove(index).draw(&mut reused)?;
        assert!(again == second, "{name}: the second drawing differs");
    }
    Ok(())
}

#[test]
fn the_glyph_scene_places_one_glyph_at_each_quarter_step() {
    let steps = scenes::glyph_origins().map(|(_, origin, step)| {
        assert_eq!(place_glyph(origin.x, origin.y).step, step);
        step
    });
    assert_eq!(steps, Step::ALL);
}
