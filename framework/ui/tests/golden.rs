//! The layout tree of each layout case matches its golden byte for byte.
//! The ignored tests write the goldens; `cargo xtask golden` runs them.

mod support;

use std::fs;

use support::golden::{self, matches_golden};
use support::layout::{CASES, Checked, failed, font_set, lay_out_case};

#[test]
fn a_row_with_spacing_matches_its_golden() -> Checked {
    matches_golden("layout-row-spacing")
}

#[test]
fn rows_aligned_at_the_start_centre_and_end_match_their_golden() -> Checked {
    matches_golden("layout-row-cross")
}

#[test]
fn a_row_with_padding_matches_its_golden() -> Checked {
    matches_golden("layout-row-padding")
}

#[test]
fn a_row_wider_than_its_largest_width_matches_its_golden() -> Checked {
    matches_golden("layout-row-overflow")
}

#[test]
fn a_row_under_a_tight_constraint_matches_its_golden() -> Checked {
    matches_golden("layout-row-tight")
}

#[test]
fn a_column_with_spacing_matches_its_golden() -> Checked {
    matches_golden("layout-column-spacing")
}

#[test]
fn columns_aligned_at_the_start_centre_and_end_match_their_golden() -> Checked {
    matches_golden("layout-column-cross")
}

#[test]
fn text_broken_at_the_width_of_its_column_matches_its_golden() -> Checked {
    matches_golden("layout-column-wrap")
}

#[test]
fn a_column_taller_than_its_largest_height_matches_its_golden() -> Checked {
    matches_golden("layout-column-overflow")
}

#[test]
fn a_vertical_scroll_container_at_offset_zero_matches_its_golden() -> Checked {
    matches_golden("layout-scroll-y")
}

#[test]
fn a_vertical_scroll_container_at_an_offset_in_range_matches_its_golden() -> Checked {
    matches_golden("layout-scroll-y-offset")
}

#[test]
fn a_horizontal_scroll_container_matches_its_golden() -> Checked {
    matches_golden("layout-scroll-x")
}

#[test]
fn content_wider_than_a_vertical_viewport_matches_its_golden() -> Checked {
    matches_golden("layout-scroll-cross")
}

#[test]
fn text_of_two_sizes_on_one_baseline_matches_its_golden() -> Checked {
    matches_golden("layout-baseline-sizes")
}

#[test]
fn text_of_two_fonts_on_one_baseline_matches_its_golden() -> Checked {
    matches_golden("layout-baseline-fonts")
}

#[test]
fn a_box_without_a_baseline_beside_text_matches_its_golden() -> Checked {
    matches_golden("layout-baseline-box")
}

#[test]
fn the_baseline_of_a_nested_column_matches_its_golden() -> Checked {
    matches_golden("layout-baseline-nested")
}

#[test]
fn text_of_several_lines_on_one_baseline_matches_its_golden() -> Checked {
    matches_golden("layout-baseline-lines")
}

#[test]
#[ignore = "writes the goldens; cargo xtask golden runs it and then writes MANIFEST"]
fn write_goldens() -> Checked {
    golden::write(golden::GOLDENS)
}

#[test]
#[ignore = "writes the goldens again into the build directory; cargo xtask golden --verify runs it"]
fn redraw_goldens() -> Checked {
    golden::write(golden::REDRAWN)
}

#[test]
fn every_case_laid_out_twice_writes_the_bytes_of_its_golden() -> Checked {
    let mut fonts = font_set()?;
    for (name, _) in CASES {
        let path = golden::file_in(golden::GOLDENS, name);
        let bytes = fs::read(&path).map_err(|error| format!("{path}: {error}"))?;
        let first = lay_out_case(name, &mut fonts)?.text();
        let second = lay_out_case(name, &mut fonts)?.text();
        golden::compare(name, &bytes, &first)?;
        golden::compare(name, &bytes, &second)?;
    }
    Ok(())
}

#[test]
fn a_second_font_set_of_the_same_bytes_writes_the_same_goldens() -> Checked {
    // Each case through a font set of its own, and all of them through one
    // other font set built from the same bytes, give the bytes of the
    // golden.
    let mut other = font_set()?;
    for (name, _) in CASES {
        let path = golden::file_in(golden::GOLDENS, name);
        let bytes = fs::read(&path).map_err(|error| format!("{path}: {error}"))?;
        let own = lay_out_case(name, &mut font_set()?)?.text();
        let shared = lay_out_case(name, &mut other)?.text();
        golden::compare(name, &bytes, &own)?;
        golden::compare(name, &bytes, &shared)?;
    }
    Ok(())
}

#[test]
fn every_case_has_a_golden_of_its_name() -> Checked {
    let mut names: Vec<String> = fs::read_dir(golden::GOLDENS)
        .map_err(failed)?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let file = entry.file_name().to_string_lossy().into_owned();
            file.strip_suffix(".tree").map(str::to_string)
        })
        .collect();
    names.sort();
    let mut cases: Vec<&str> = CASES.iter().map(|(name, _)| *name).collect();
    cases.sort_unstable();
    assert_eq!(names, cases);
    Ok(())
}

#[test]
fn a_golden_file_one_line_off_its_layout_fails() -> Checked {
    let dir = concat!(env!("CARGO_TARGET_TMPDIR"), "/ui-golden-one-line-off");
    fs::create_dir_all(dir).map_err(failed)?;
    let name = "layout-row-spacing";
    let text = fs::read_to_string(golden::file_in(golden::GOLDENS, name)).map_err(failed)?;
    fs::write(golden::file_in(dir, name), &text).map_err(failed)?;
    golden::matches_file_in(dir, name)?;
    // The second child one unit further right.
    let moved = text.replacen(" at 1792 0 ", " at 1793 0 ", 1);
    assert_ne!(moved, text);
    fs::write(golden::file_in(dir, name), &moved).map_err(failed)?;
    let error = golden::matches_file_in(dir, name).unwrap_err();
    assert!(
        error.starts_with("layout-row-spacing: line 4 is \"2 parent 0 kind \\\"fixed\\\" key 1"),
        "{error}"
    );
    // The line as laid out comes first, and the line of the golden second.
    let (laid, in_golden) = error.split_once(" laid out and ").unwrap_or_default();
    assert!(
        laid.contains(" at 1792 0 ") && in_golden.contains(" at 1793 0 "),
        "{error}"
    );
    // A golden that ends early, or holds one line more.
    let short: String = text.split_inclusive('\n').take(2).collect();
    assert_eq!(
        golden::compare(name, short.as_bytes(), &text).unwrap_err(),
        format!(
            "{name}: line 3 is {:?} laid out and the end in the golden",
            text.split_inclusive('\n').nth(2).unwrap_or_default()
        )
    );
    let long = format!("{text}extra\n");
    assert_eq!(
        golden::compare(name, long.as_bytes(), &text).unwrap_err(),
        format!("{name}: line 6 is the end laid out and \"extra\\n\" in the golden")
    );
    Ok(())
}

#[test]
fn a_golden_file_that_differs_only_in_its_line_ends_fails() -> Checked {
    let name = "layout-row-spacing";
    let text = fs::read_to_string(golden::file_in(golden::GOLDENS, name)).map_err(failed)?;
    // Line ends of CR LF, as a checkout that converted them would give,
    // and a file without its last line end.
    let crlf = text.replace('\n', "\r\n");
    assert_eq!(
        golden::compare(name, crlf.as_bytes(), &text),
        Err(format!(
            "{name}: line 1 is \"layout-tree 1\\n\" laid out and \"layout-tree 1\\r\\n\" in the golden"
        ))
    );
    let cut = text.strip_suffix('\n').unwrap_or_default();
    assert!(golden::compare(name, cut.as_bytes(), &text).is_err());
    Ok(())
}

#[test]
fn the_helper_of_the_comparisons_fails_without_a_case() {
    // Not a string literal, so the goldens step does not count it as a
    // reference to a golden.
    let name = String::from("nowhere");
    assert_eq!(
        matches_golden(&name),
        Err("no case is named nowhere".to_string())
    );
}
