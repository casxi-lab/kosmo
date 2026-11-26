//! Redrawing only the damage over the previous frame gives the bytes of a
//! whole redraw: the composite scene under random damage regions of 1 to
//! 16 rectangles, from a fixed seed.

pub mod support;

use kosmo_render::{ImageStore, Rasterizer, RenderTarget, VelloRasterizer};
use kosmo_types::Damage;
use support::{Checked, scenes};

/// The groups of random damage; lower it in this one place if the check
/// needs less time.
const GROUPS: u64 = 1_000;

/// The seed of group `n` is `SEED + n`.
const SEED: u64 = 0x6B6F_736D_6F00_0000;

/// The most rectangles of one group.
const MAX_RECTS: u64 = 16;

/// SplitMix64, written out here so that the sequence depends on no crate.
struct SplitMix64(u64);

impl SplitMix64 {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A value from `low` to `high`, both included.
    fn between(&mut self, low: u32, high: u32) -> u32 {
        low + (self.next() % u64::from(high - low + 1)) as u32
    }
}

#[test]
fn split_mix_64_gives_its_published_sequence() {
    let mut random = SplitMix64(0);
    assert_eq!(random.next(), 0xE220_A839_7B1D_CDAF);
    assert_eq!(random.next(), 0x6E78_9E6A_A1B9_65F4);
}

#[test]
fn redrawing_random_damage_over_the_previous_frame_equals_a_whole_redraw() -> Checked {
    let mut rasterizer = VelloRasterizer::new();
    let scene = scenes::composite(&mut rasterizer)?;
    let whole = scene.draw(&mut rasterizer)?;
    let (width, height) = (scene.desc.width(), scene.desc.height());
    // A previous frame that differs from the new one at every pixel.
    let previous: Vec<u8> = whole.bytes().iter().map(|byte| byte ^ 0x5A).collect();
    for group in 0..GROUPS {
        let seed = SEED + group;
        let mut random = SplitMix64(seed);
        let count = random.between(1, MAX_RECTS as u32);
        let mut damage = Damage::new(scene.desc);
        let mut expected = previous.clone();
        for _ in 0..count {
            let x = random.between(0, width - 1);
            let y = random.between(0, height - 1);
            let w = random.between(1, width - x);
            let h = random.between(1, height - y);
            damage.add(x, y, w, h);
            for row in y..y + h {
                let start = (row * width + x) as usize * 4;
                let end = start + w as usize * 4;
                expected[start..end].copy_from_slice(&whole.bytes()[start..end]);
            }
        }
        let mut target = RenderTarget::from_bytes(scene.desc, previous.clone()).unwrap();
        rasterizer
            .render(
                &scene.list,
                &damage,
                &scene.glyphs,
                &ImageStore::EMPTY,
                &mut target,
            )
            .unwrap();
        let first = target
            .bytes()
            .iter()
            .zip(&expected)
            .position(|(a, b)| a != b);
        assert!(
            first.is_none(),
            "seed {seed:#x}, group {group}: byte {first:?} differs from a whole redraw"
        );
    }
    Ok(())
}
