//! The same text gives the same glyphs and lines: laid out twice by one
//! font set, by two font sets and in two processes. The hash containers
//! inside parley and its dependencies take seeds that change from process
//! to process, so the second process is a run of this test binary of its
//! own, which writes what it laid out to its standard output.

pub mod support;

use std::fmt::Write;
use std::process::Command;

use kosmo_text::FontSet;
use support::{BREAKS, Checked, STRINGS, failed, font_set, lay_out, runs};

/// The line that starts what [`this_process_writes_its_layout`] writes.
const BEGIN: &str = "--- the layout of this process ---\n";

/// The line that ends it.
const END: &str = "--- end of the layout ---\n";

/// The runs of every string and the lines of every case of line breaking,
/// laid out by `set`, as text: the display lists of the runs, and for each
/// line its bytes, its baseline and its glyphs.
fn layout(set: &mut FontSet) -> Checked<String> {
    let mut text = String::new();
    for shaped in &STRINGS {
        for run in runs(set, shaped)? {
            write!(text, "{} {run}", shaped.name).map_err(failed)?;
        }
    }
    for case in &BREAKS {
        let paragraph = lay_out(set, case.text, case.font, 16, case.width)?;
        writeln!(
            text,
            "{} {}x{}",
            case.name,
            paragraph.width().units(),
            paragraph.height().units()
        )
        .map_err(failed)?;
        for line in paragraph.lines() {
            write!(
                text,
                "  {:?} baseline {}:",
                line.text(),
                line.baseline().units()
            )
            .map_err(failed)?;
            for glyph in line.glyphs() {
                write!(
                    text,
                    " {}@{},{}",
                    glyph.id.get(),
                    glyph.origin.x.units(),
                    glyph.origin.y.units()
                )
                .map_err(failed)?;
            }
            text.push('\n');
        }
    }
    Ok(text)
}

#[test]
fn one_font_set_lays_the_same_text_out_alike_twice() -> Checked {
    let mut set = font_set()?;
    let first = layout(&mut set)?;
    assert_eq!(layout(&mut set)?, first);
    assert_eq!(layout(&mut font_set()?)?, first);
    for case in &BREAKS {
        let once = lay_out(&mut set, case.text, case.font, 16, case.width)?;
        let again = lay_out(&mut font_set()?, case.text, case.font, 16, case.width)?;
        assert_eq!(once, again, "{}", case.name);
    }
    Ok(())
}

/// The test that the second process runs: it writes the layout between
/// [`BEGIN`] and [`END`]. Run in this process, its output is captured.
#[test]
fn this_process_writes_its_layout() -> Checked {
    let layout = layout(&mut font_set()?)?;
    print!("{BEGIN}{layout}{END}");
    Ok(())
}

#[test]
fn another_process_lays_the_same_text_out_alike() -> Checked {
    let ours = layout(&mut font_set()?)?;
    let binary = std::env::current_exe().map_err(failed)?;
    let output = Command::new(binary)
        .args([
            "this_process_writes_its_layout",
            "--exact",
            "--nocapture",
            "--test-threads=1",
        ])
        .output()
        .map_err(failed)?;
    let stdout = String::from_utf8(output.stdout).map_err(failed)?;
    assert!(output.status.success(), "{stdout}");
    let start = stdout
        .find(BEGIN)
        .ok_or("no layout from the other process")?
        + BEGIN.len();
    let end = stdout[start..].find(END).ok_or("no end of the layout")? + start;
    assert_eq!(&stdout[start..end], ours);
    Ok(())
}
