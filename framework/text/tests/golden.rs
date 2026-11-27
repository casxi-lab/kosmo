//! Each pinned string, laid out by the font set and drawn by vello_cpu from
//! the coverage masks of its glyph cache, matches its pixel golden byte for
//! byte, without tolerance. The ignored tests write the goldens;
//! `cargo xtask golden` runs them.

pub mod support;

use std::fs;

use kosmo_render::VelloRasterizer;
use support::golden::{self, matches_golden};
use support::{Checked, STRINGS, failed, font_set, lay_out};

#[test]
fn ascii_letters_and_punctuation_match_their_golden() -> Checked {
    matches_golden("text-ascii")
}

#[test]
fn digits_and_ascii_operators_match_their_golden() -> Checked {
    matches_golden("text-digits")
}

#[test]
fn kerning_and_ligatures_match_their_golden() -> Checked {
    matches_golden("text-kerning-ligatures")
}

#[test]
fn operators_of_the_calculator_match_their_golden() -> Checked {
    matches_golden("text-operators")
}

#[test]
fn mathematical_values_match_their_golden() -> Checked {
    matches_golden("text-math-values")
}

#[test]
fn mathematical_symbols_match_their_golden() -> Checked {
    matches_golden("text-math-symbols")
}

#[test]
fn greek_letters_match_their_golden() -> Checked {
    matches_golden("text-greek")
}

#[test]
fn combining_characters_match_their_golden() -> Checked {
    matches_golden("text-combining")
}

#[test]
fn monospaced_input_matches_its_golden() -> Checked {
    matches_golden("mono-input")
}

#[test]
fn monospaced_operators_match_their_golden() -> Checked {
    matches_golden("mono-operators")
}

#[test]
fn monospaced_combining_characters_match_their_golden() -> Checked {
    matches_golden("mono-combining")
}

#[test]
fn the_four_quarter_pixel_steps_match_their_golden() -> Checked {
    matches_golden("text-steps")
}

#[test]
fn three_sizes_match_their_golden() -> Checked {
    matches_golden("text-sizes")
}

#[test]
#[ignore = "writes the goldens; cargo xtask golden runs it and then writes MANIFEST"]
fn write_goldens() -> Checked {
    golden::write(golden::GOLDENS)
}

#[test]
#[ignore = "draws the goldens again into the build directory; cargo xtask golden --verify runs it"]
fn redraw_goldens() -> Checked {
    golden::write(golden::REDRAWN)
}

/// The canvas of each golden, its width and height in pixels: the layout
/// box rounded outwards with a margin of 2 pixels on each side.
const CANVASES: [(&str, u32, u32); 13] = [
    ("text-ascii", 331, 24),
    ("text-digits", 219, 24),
    ("text-kerning-ligatures", 164, 24),
    ("text-operators", 174, 24),
    ("text-math-values", 246, 24),
    ("text-math-symbols", 205, 24),
    ("text-greek", 295, 24),
    ("text-combining", 77, 24),
    ("mono-input", 177, 24),
    ("mono-operators", 206, 24),
    ("mono-combining", 72, 24),
    ("text-steps", 29, 81),
    ("text-sizes", 52, 86),
];

#[test]
fn each_golden_is_its_layout_box_rounded_outwards_with_a_margin() -> Checked {
    let names: Vec<&str> = STRINGS.iter().map(|shaped| shaped.name).collect();
    let pinned: Vec<&str> = CANVASES.iter().map(|&(name, _, _)| name).collect();
    assert_eq!(names, pinned);
    let mut set = font_set()?;
    for (shaped, (name, width, height)) in STRINGS.iter().zip(CANVASES) {
        let path = format!("{}/{name}.px", golden::GOLDENS);
        let bytes = fs::read(&path).map_err(|error| format!("{path}: {error}"))?;
        // A tightly packed `Rgba8Premul` buffer, format code 1.
        assert_eq!(
            golden::description(&bytes),
            Some([width, height, width * 4, 1]),
            "{name}"
        );
        assert!(
            bytes[golden::HEADER..]
                .chunks(4)
                .any(|pixel| pixel == [0, 0, 0, 255]),
            "{name} has no opaque black pixel"
        );
        // One paragraph: its width and height rounded up to whole pixels,
        // and 2 pixels on each side.
        if let ([size], [0]) = (shaped.sizes, shaped.offsets) {
            let paragraph = lay_out(&mut set, shaped.text, shaped.font, *size, None)?;
            let up = |fx: kosmo_types::Fx| (fx.units() + 63).div_euclid(64) as u32;
            assert_eq!(
                (up(paragraph.width()) + 4, up(paragraph.height()) + 4),
                (width, height),
                "{name}"
            );
        }
    }
    Ok(())
}

#[test]
fn the_margin_holds_every_glyph_of_each_golden() -> Checked {
    // Drawn with a margin 2 pixels wider, a string has no ink outside the
    // canvas of its golden and the pixels of the golden inside it, so the
    // golden cuts no glyph off. The division slash at the end of
    // `text-math-symbols` reaches the last column of its golden.
    let mut set = font_set()?;
    let mut rasterizer = VelloRasterizer::new();
    for shaped in &STRINGS {
        let name = shaped.name;
        let wide = golden::draw(&mut set, &mut rasterizer, shaped, golden::MARGIN + 2)?;
        let path = format!("{}/{name}.px", golden::GOLDENS);
        let narrow = fs::read(&path).map_err(|error| format!("{path}: {error}"))?;
        let [width, height, ..] = golden::description(&narrow).ok_or("no description")?;
        let [wide_width, wide_height, ..] = golden::description(&wide).ok_or("no description")?;
        assert_eq!((wide_width, wide_height), (width + 4, height + 4), "{name}");
        let pixel = |bytes: &[u8], row: u32, x: u32, y: u32| {
            let start = golden::HEADER + ((y * row + x) * 4) as usize;
            bytes[start..start + 4].to_vec()
        };
        for y in 0..wide_height {
            for x in 0..wide_width {
                let inside = (2..width + 2).contains(&x) && (2..height + 2).contains(&y);
                let expected = if inside {
                    pixel(&narrow, width, x - 2, y - 2)
                } else {
                    vec![0; 4]
                };
                assert_eq!(
                    pixel(&wide, wide_width, x, y),
                    expected,
                    "{name} ({x}, {y})"
                );
            }
        }
    }
    Ok(())
}

#[test]
fn a_golden_file_one_byte_off_its_drawing_fails() -> Checked {
    let dir = concat!(env!("CARGO_TARGET_TMPDIR"), "/text-golden-one-byte-off");
    fs::create_dir_all(dir).map_err(failed)?;
    let golden = format!("{}/text-ascii.px", golden::GOLDENS);
    let mut bytes = fs::read(golden).map_err(failed)?;
    fs::write(format!("{dir}/text-ascii.px"), &bytes).map_err(failed)?;
    golden::matches_file_in(dir, "text-ascii")?;
    // The alpha of the first pixel of the third row, inside the margin.
    let [width, ..] = golden::description(&bytes).ok_or("no description")?;
    let at = golden::HEADER + (2 * width * 4) as usize + 3;
    bytes[at] ^= 1;
    fs::write(format!("{dir}/text-ascii.px"), &bytes).map_err(failed)?;
    assert_eq!(
        golden::matches_file_in(dir, "text-ascii"),
        Err(
            "text-ascii: pixel (0, 2) is [00, 00, 00, 00] drawn and [00, 00, 00, 01] in the golden"
                .to_string()
        )
    );
    Ok(())
}

#[test]
fn a_drawing_of_another_canvas_or_length_fails_with_both() -> Checked {
    let (name, golden) = golden::drawings()?.remove(0);
    assert_eq!(name, "text-ascii");
    let mut drawn = golden.clone();
    drawn[0] ^= 1;
    let error = golden::compare(name, &golden, &drawn).unwrap_err();
    assert!(error.starts_with("text-ascii: drawn Some(["), "{error}");
    assert_eq!(
        golden::compare(name, &golden[..golden.len() - 1], &golden),
        Err(format!(
            "text-ascii: {} bytes drawn, but the golden has {}",
            golden.len() - golden::HEADER,
            golden.len() - golden::HEADER - 1
        ))
    );
    assert_eq!(golden::compare(name, &golden, &golden), Ok(()));
    Ok(())
}

#[test]
fn the_helper_of_the_comparisons_fails_without_a_string() {
    // Not a string literal, so the goldens step does not count it as a
    // reference to a golden.
    let name = String::from("nowhere");
    assert_eq!(
        matches_golden(&name),
        Err("no string is named nowhere".to_string())
    );
}
