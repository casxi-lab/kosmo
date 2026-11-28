//! The goldens of `kosmo-ui`: the layout tree goldens of the layout cases
//! and of the scene of the design tokens, the pixel golden of that scene,
//! the comparison of a layout or a drawing with its golden and the writer
//! of the goldens.
//!
//! The golden `<name>` of a case of [`CASES`] is the file
//! `tests/golden/<name>.tree`, which holds the case's layout tree as
//! `LayoutTree::text` writes it. The scene of the tokens has the layout
//! tree golden `layout-tokens.tree` and the pixel golden `tokens.px`, as
//! [`tokens`] describes. `cargo xtask golden` runs the writers and lists
//! the digests of the files in `tests/golden/MANIFEST`; the other tests
//! only read the goldens.

use std::fs;

use kosmo_ui::TOKENS;

use super::layout::{CASES, Checked, failed, font_set, lay_out_case};
use super::tokens::{self, HEADER, PIXELS, TREE, WIDTH};

/// The directory of the goldens.
pub const GOLDENS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden");

/// The directory, in the workspace's default build directory `target/`,
/// into which `cargo xtask golden --verify` writes the goldens again.
pub const REDRAWN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../target/golden/kosmo-ui");

/// The names of the goldens: the layout cases in their order, then the
/// layout tree and the drawing of the scene of the tokens.
pub fn names() -> Vec<&'static str> {
    let mut names: Vec<&str> = CASES.iter().map(|(name, _)| *name).collect();
    names.extend([TREE, PIXELS]);
    names
}

/// The file of the golden `name` in `dir`: `<name>.px` for the drawing of
/// the scene of the tokens, and `<name>.tree` for a layout tree.
pub fn file_in(dir: &str, name: &str) -> String {
    let extension = if name == PIXELS { "px" } else { "tree" };
    format!("{dir}/{name}.{extension}")
}

/// Compares the layout tree `laid`, as text, with `golden`, the golden
/// `name`, byte for byte. A difference fails with the first line that
/// differs, its number from 1 and the line in both.
pub fn compare(name: &str, golden: &[u8], laid: &str) -> Checked {
    if golden == laid.as_bytes() {
        return Ok(());
    }
    let golden = String::from_utf8_lossy(golden);
    let mut old = golden.split_inclusive('\n');
    let mut new = laid.split_inclusive('\n');
    for number in 1.. {
        match (new.next(), old.next()) {
            (Some(a), Some(b)) if a == b => {}
            (a, b) => {
                let shown = |line: Option<&str>| {
                    line.map_or("the end".to_string(), |line| format!("{line:?}"))
                };
                return Err(format!(
                    "{name}: line {number} is {} laid out and {} in the golden",
                    shown(a),
                    shown(b)
                ));
            }
        }
    }
    Ok(())
}

/// Compares the encoding `drawn` of the scene of the tokens with `golden`,
/// the golden `name`, byte for byte. A difference fails with the first
/// pixel that differs, its position and its bytes in both, or with the
/// lengths when one encoding is the start of the other.
pub fn compare_pixels(name: &str, golden: &[u8], drawn: &[u8]) -> Checked {
    if golden == drawn {
        return Ok(());
    }
    let Some(at) = golden.iter().zip(drawn).position(|(a, b)| a != b) else {
        let (drawn, golden) = (drawn.len(), golden.len());
        return Err(format!(
            "{name}: {drawn} bytes drawn, but the golden has {golden}"
        ));
    };
    let Some(offset) = at.checked_sub(HEADER) else {
        return Err(format!(
            "{name}: byte {at} of the description differs from the golden's"
        ));
    };
    let stride = WIDTH as usize * 4;
    let (x, y) = ((offset % stride) / 4, offset / stride);
    let pixel = |bytes: &[u8]| {
        let start = HEADER + y * stride + x * 4;
        match bytes.get(start..start + 4) {
            Some(pixel) => format!("{pixel:02x?}"),
            None => "cut off".to_string(),
        }
    };
    let (new, old) = (pixel(drawn), pixel(golden));
    Err(format!(
        "{name}: pixel ({x}, {y}) is {new} drawn and {old} in the golden"
    ))
}

/// The golden `name` matches its file in the golden directory byte for
/// byte: the layout tree of a case, or the layout tree or the drawing of
/// the scene of the tokens. The goldens step of `cargo xtask check` finds
/// the goldens that the tests refer to by the calls of this function with
/// a string literal.
pub fn matches_golden(name: &str) -> Checked {
    matches_file_in(GOLDENS, name)
}

/// The golden `name`, laid out alone through a new font set, and drawn for
/// the drawing of the scene of the tokens, matches its file in `dir` byte
/// for byte.
pub fn matches_file_in(dir: &str, name: &str) -> Checked {
    let mut fonts = font_set()?;
    let read = |path: String| fs::read(&path).map_err(|error| format!("{path}: {error}"));
    if name == PIXELS {
        let (_, drawn) = tokens::scene(TOKENS, &mut fonts)?;
        return compare_pixels(name, &read(file_in(dir, name))?, &drawn);
    }
    let laid = if name == TREE {
        tokens::scene(TOKENS, &mut fonts)?.0
    } else {
        lay_out_case(name, &mut fonts)?
    };
    compare(name, &read(file_in(dir, name))?, &laid.text())
}

/// Writes the golden of each case into `dir`, the cases laid out in their
/// order through one font set, and then the two goldens of the scene of the
/// tokens, laid out and drawn through a new font set.
pub fn write(dir: &str) -> Checked {
    fs::create_dir_all(dir).map_err(failed)?;
    let mut fonts = font_set()?;
    for (name, _) in CASES {
        let laid = lay_out_case(name, &mut fonts)?.text();
        fs::write(file_in(dir, name), laid).map_err(failed)?;
    }
    let (laid, drawn) = tokens::scene(TOKENS, &mut font_set()?)?;
    fs::write(file_in(dir, TREE), laid.text()).map_err(failed)?;
    fs::write(file_in(dir, PIXELS), drawn).map_err(failed)?;
    Ok(())
}
