//! Each scene of one kind of command, drawn by vello_cpu, matches its pixel
//! golden byte for byte, without tolerance. The ignored tests write the
//! goldens; `cargo xtask golden` runs them.

pub mod support;

use std::fs;

use kosmo_types::{BufferDesc, PixelFormat};
use support::golden::{self, matches_golden};
use support::{Checked, failed};

#[test]
fn rectangles_match_their_golden() -> Checked {
    matches_golden("fill-rect")
}

#[test]
fn rounded_rectangles_match_their_golden() -> Checked {
    matches_golden("fill-rounded-rect")
}

#[test]
fn filled_paths_match_their_golden() -> Checked {
    matches_golden("fill-path")
}

#[test]
fn stroked_paths_match_their_golden() -> Checked {
    matches_golden("stroke-path")
}

#[test]
fn glyph_runs_match_their_golden() -> Checked {
    matches_golden("glyphs")
}

#[test]
fn clip_groups_match_their_golden() -> Checked {
    matches_golden("clip-group")
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

#[test]
fn the_encoding_is_the_description_and_the_bytes() -> Checked {
    let codes = [
        (PixelFormat::Rgba8Premul, 1),
        (PixelFormat::Xrgb8888, 2),
        (PixelFormat::Rgb565, 3),
        (PixelFormat::Argb8888Premul, 4),
    ];
    for (format, code) in codes {
        let desc = BufferDesc::new(3, 2, 7 * format.bytes_per_pixel(), format).map_err(failed)?;
        let bytes: Vec<u8> = (0..desc.byte_len()).map(|byte| byte as u8).collect();
        let stride = 7 * format.bytes_per_pixel();
        let mut expected = Vec::new();
        for field in [3, 2, stride, code] {
            expected.extend_from_slice(&field.to_le_bytes());
        }
        expected.extend_from_slice(&bytes);
        assert_eq!(golden::encode(desc, &bytes)?, expected, "{format:?}");
    }
    Ok(())
}

#[test]
fn a_drawing_one_byte_off_its_golden_fails_at_the_first_differing_pixel() -> Checked {
    let (name, golden) = golden::drawings()?.remove(0);
    assert_eq!(name, "fill-rect");
    // The green component of pixel (30, 20) of the 64-pixel rows, one
    // larger, and the blue one of pixel (40, 30) changed too.
    let mut drawn = golden.clone();
    let at = |x: usize, y: usize, byte: usize| 16 + y * 64 * 4 + x * 4 + byte;
    drawn[at(30, 20, 1)] = drawn[at(30, 20, 1)].wrapping_add(1);
    drawn[at(40, 30, 2)] ^= 0x80;
    let pixel = |bytes: &[u8]| format!("{:02x?}", &bytes[at(30, 20, 0)..at(31, 20, 0)]);
    let error = golden::compare(name, &golden, &drawn).unwrap_err();
    assert_eq!(
        error,
        format!(
            "fill-rect: pixel (30, 20) is {} drawn and {} in the golden",
            pixel(&drawn),
            pixel(&golden)
        )
    );
    assert_eq!(golden::compare(name, &golden, &golden), Ok(()));
    Ok(())
}

#[test]
fn a_golden_file_one_byte_off_its_drawing_fails() -> Checked {
    let dir = concat!(env!("CARGO_TARGET_TMPDIR"), "/golden-one-byte-off");
    fs::create_dir_all(dir).map_err(failed)?;
    let mut bytes = fs::read(format!("{}/fill-rect.px", golden::GOLDENS)).map_err(failed)?;
    fs::write(format!("{dir}/fill-rect.px"), &bytes).map_err(failed)?;
    golden::matches_file_in(dir, "fill-rect")?;
    // The green component of pixel (30, 20) of the 64-pixel rows.
    bytes[16 + 20 * 64 * 4 + 30 * 4 + 1] ^= 1;
    fs::write(format!("{dir}/fill-rect.px"), &bytes).map_err(failed)?;
    let error = golden::matches_file_in(dir, "fill-rect").unwrap_err();
    assert!(
        error.starts_with("fill-rect: pixel (30, 20) is "),
        "{error}"
    );
    Ok(())
}

#[test]
fn a_drawing_of_another_buffer_or_length_fails_with_both() -> Checked {
    let (name, golden) = golden::drawings()?.remove(0);
    let mut drawn = golden.clone();
    drawn[8] = 0;
    drawn[9] = 2;
    assert_eq!(
        golden::compare(name, &golden, &drawn),
        Err("fill-rect: drawn 64x48, stride 512, format 1, but the golden is 64x48, stride 256, format 1".to_string())
    );
    assert_eq!(
        golden::compare(name, &golden[..golden.len() - 1], &golden),
        Err("fill-rect: 12304 bytes drawn, but the golden has 12303".to_string())
    );
    assert_eq!(
        golden::compare(name, &golden[..10], &golden),
        Err("fill-rect: drawn 64x48, stride 256, format 1, but the golden is 10 bytes, shorter than a description".to_string())
    );
    Ok(())
}

#[test]
fn a_drawing_with_an_unknown_format_code_fails_with_the_code() {
    let mut golden = Vec::new();
    for field in [1_u32, 1, 4, 5] {
        golden.extend_from_slice(&field.to_le_bytes());
    }
    golden.extend_from_slice(&[0; 4]);
    let mut drawn = golden.clone();
    drawn[16] = 1;
    assert_eq!(
        golden::compare("codes", &golden, &drawn),
        Err("codes: format code 5 is unknown".to_string())
    );
}

#[test]
fn a_difference_after_the_pixels_of_a_row_names_the_byte() -> Checked {
    // Two pixels of `Rgb565` in rows of 6 bytes.
    let mut golden = Vec::new();
    for field in [2_u32, 2, 6, 3] {
        golden.extend_from_slice(&field.to_le_bytes());
    }
    golden.extend_from_slice(&[0; 12]);
    let mut drawn = golden.clone();
    drawn[16 + 6 + 4] = 0xab;
    assert_eq!(
        golden::compare("rows", &golden, &drawn),
        Err(
            "rows: byte 4 of row 1, after its pixels, is 0xab drawn and 0x00 in the golden"
                .to_string()
        )
    );
    drawn[16 + 6 + 4] = 0;
    drawn[16 + 6 + 3] = 1;
    assert_eq!(
        golden::compare("rows", &golden, &drawn),
        Err("rows: pixel (1, 1) is [00, 01] drawn and [00, 00] in the golden".to_string())
    );
    Ok(())
}
