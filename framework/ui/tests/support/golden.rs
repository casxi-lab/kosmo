//! Layout tree goldens of the layout cases: the comparison of a case's
//! layout tree with its golden and the writer of the goldens.
//!
//! The golden `<name>` of a case of [`CASES`] is the file
//! `tests/golden/<name>.tree`, which holds the case's layout tree as
//! `LayoutTree::text` writes it. `cargo xtask golden` runs the writers and
//! lists the digests of the files in `tests/golden/MANIFEST`; the other
//! tests only read the goldens.

use std::fs;

use super::layout::{CASES, Checked, failed, font_set, lay_out_case};

/// The directory of the goldens.
pub const GOLDENS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden");

/// The directory, in the workspace's default build directory `target/`,
/// into which `cargo xtask golden --verify` writes the goldens again.
pub const REDRAWN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../target/golden/kosmo-ui");

/// The file of the golden `name` in `dir`.
pub fn file_in(dir: &str, name: &str) -> String {
    format!("{dir}/{name}.tree")
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

/// The layout tree of the case `name` matches the golden `name` byte for
/// byte. The goldens step of `cargo xtask check` finds the goldens that
/// the tests refer to by the calls of this function with a string literal.
pub fn matches_golden(name: &str) -> Checked {
    matches_file_in(GOLDENS, name)
}

/// The case `name`, laid out alone through a new font set, matches the
/// file `<dir>/<name>.tree` byte for byte.
pub fn matches_file_in(dir: &str, name: &str) -> Checked {
    let laid = lay_out_case(name, &mut font_set()?)?.text();
    let path = file_in(dir, name);
    let golden = fs::read(&path).map_err(|error| format!("{path}: {error}"))?;
    compare(name, &golden, &laid)
}

/// Writes the golden of each case into `dir`, the cases laid out in their
/// order through one font set.
pub fn write(dir: &str) -> Checked {
    fs::create_dir_all(dir).map_err(failed)?;
    let mut fonts = font_set()?;
    for (name, _) in CASES {
        let laid = lay_out_case(name, &mut fonts)?.text();
        fs::write(file_in(dir, name), laid).map_err(failed)?;
    }
    Ok(())
}
