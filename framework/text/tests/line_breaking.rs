//! Line breaking: the text of each line of each case of `support::BREAKS`,
//! measured once and written here, and the number of lines.

pub mod support;

use kosmo_text::{MAX_WIDTH, SANS, TextError, TextStyle};
use kosmo_types::Fx;
use support::{BREAKS, Checked, failed, font_set, lay_out, lines};

/// Checks that the case named `name` breaks into the lines `expected`.
fn breaks(name: &str, expected: &[&str]) -> Checked {
    let case = BREAKS
        .iter()
        .find(|case| case.name == name)
        .ok_or_else(|| format!("no case is named {name}"))?;
    let mut set = font_set()?;
    let lines = lines(&mut set, case)?;
    assert_eq!(lines.len(), expected.len(), "{name}");
    assert_eq!(lines, expected, "{name}");
    Ok(())
}

#[test]
fn a_line_breaks_at_a_space() -> Checked {
    breaks(
        "break-at-space",
        &["The result is 1/3 and ", "the error is 0"],
    )
}

#[test]
fn a_line_feed_breaks_the_line() -> Checked {
    breaks("line-feed", &["1 + 2\n", "= 3"])
}

#[test]
fn a_word_longer_than_the_width_stays_whole() -> Checked {
    breaks("long-word", &["Supercalifragilistic ", "expression"])
}

#[test]
fn a_minus_sign_stays_with_its_number() -> Checked {
    breaks("minus-sign", &["x = ", "\u{2212}3.14159 ", "+ 2"])
}

#[test]
fn a_no_break_space_does_not_break() -> Checked {
    breaks("no-break-space", &["Mass ", "10\u{a0}kg ", "total"])
}

#[test]
fn a_combining_mark_stays_with_its_letter() -> Checked {
    breaks("combining-mark", &["cafe\u{301} ", "au ", "lait"])
}

#[test]
fn spaces_at_the_end_of_a_line_hang() -> Checked {
    breaks("hanging-spaces", &["1 + 2 = 3   ", "4"])
}

#[test]
fn without_a_width_the_text_is_one_line() -> Checked {
    breaks("no-width", &["The result is 1/3 and the error is 0"])
}

#[test]
fn a_fraction_does_not_break_after_its_slash() -> Checked {
    breaks("mono-fraction", &["f(x) = ", "3*x^2 - ", "1/2"])
}

#[test]
fn every_case_is_pinned() {
    let names: Vec<&str> = BREAKS.iter().map(|case| case.name).collect();
    assert_eq!(
        names,
        [
            "break-at-space",
            "line-feed",
            "long-word",
            "minus-sign",
            "no-break-space",
            "combining-mark",
            "hanging-spaces",
            "no-width",
            "mono-fraction"
        ]
    );
}

#[test]
fn a_word_longer_than_the_width_overflows_it() -> Checked {
    // The first line of the long word is wider than its 100 pixels; the
    // widths of the other cases stay within theirs.
    let mut set = font_set()?;
    for case in &BREAKS {
        let paragraph = lay_out(&mut set, case.text, case.font, 16, case.width)?;
        let Some(width) = case.width else {
            continue;
        };
        let overflows = paragraph.width().units() > width * 64;
        assert_eq!(overflows, case.name == "long-word", "{}", case.name);
    }
    Ok(())
}

#[test]
fn lines_lie_one_line_height_apart() -> Checked {
    let mut set = font_set()?;
    // The minus sign case breaks into three lines.
    let paragraph = lay_out(&mut set, "x = \u{2212}3.14159 + 2", SANS, 16, Some(66))?;
    assert_eq!(paragraph.lines().len(), 3);
    let baselines: Vec<i32> = paragraph
        .lines()
        .iter()
        .map(|line| line.baseline().units())
        .collect();
    // A line of Fira Sans at 16 pixels per em is 19.2 pixels high, and each
    // value is rounded once: the height of three lines is 57.6 pixels.
    assert_eq!(baselines, [957, 2186, 3415]);
    assert_eq!(paragraph.height().units(), 3686);
    Ok(())
}

#[test]
fn widths_outside_0_to_the_largest_are_refused() -> Checked {
    let mut set = font_set()?;
    let style = TextStyle::new(SANS, Fx::from_units(1024)).map_err(failed)?;
    for width in [Fx::from_units(-1), Fx::from_units(MAX_WIDTH.units() + 1)] {
        assert_eq!(
            set.layout("1 + 2", style, Some(width)),
            Err(TextError::WidthOutOfRange(width))
        );
    }
    for width in [Fx::ZERO, MAX_WIDTH] {
        assert!(set.layout("1 + 2", style, Some(width)).is_ok());
    }
    Ok(())
}
