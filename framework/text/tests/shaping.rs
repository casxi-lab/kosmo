//! The glyphs of each pinned string, as the text of its display list: the
//! glyph numbers and their origins in 1/64 pixel, measured once and written
//! here. The strings are those of `support::STRINGS`, laid out without a
//! width with the origin of the paragraph at (0, 0).

pub mod support;

use kosmo_render::{Command, Step, place_glyph};
use support::{Checked, STRINGS, font_set, lay_out, list, runs};

/// Checks that the runs of the string named `name` are `expected`.
fn pinned(name: &str, expected: &[&str]) -> Checked {
    let shaped = STRINGS
        .iter()
        .find(|shaped| shaped.name == name)
        .ok_or_else(|| format!("no string is named {name}"))?;
    let mut set = font_set()?;
    let runs = runs(&mut set, shaped)?;
    assert_eq!(runs, expected, "{name}");
    Ok(())
}

/// ASCII letters and punctuation: one glyph for each character, with the
/// advances and the kerning of the font.
const TEXT_ASCII: [&str; 1] = [concat!(
    "glyphs #000000ff font 0 size 1024",
    " 254@0,957 480@529,957 417@1129,957 2@1688,957 617@1959,957 672@2571,957",
    " 491@3167,957 387@3456,957 519@3945,957 2@4470,957 377@4741,957 621@5349,957",
    " 566@5714,957 705@6312,957 549@7046,957 2@7646,957 457@7918,957 566@8250,957",
    " 713@8839,957 2@9336,957 511@9607,957 672@9894,957 543@10490,957 610@11367,957",
    " 637@11976,957 2@12454,957 566@12725,957 701@13319,957 417@13819,957 621@14377,957",
    " 2@14772,957 651@15044,957 480@15413,957 417@16013,957 2@16571,957 525@16843,957",
    " 340@17138,957 726@17695,957 714@18142,957 2@18646,957 398@18917,957 566@19530,957",
    " 460@20128,957 2163@20660,957",
    "\n"
)];

/// Digits and the ASCII operators: one glyph for each character.
const TEXT_DIGITS: [&str; 1] = [concat!(
    "glyphs #000000ff font 0 size 1024",
    " 2047@0,957 2048@546,957 2049@989,957 2050@1491,957 2051@1997,957 2052@2531,957",
    " 2053@3044,957 2054@3565,957 2055@3999,957 2056@4546,957 2@5083,957 2317@5354,957",
    " 2@5865,957 2203@6137,957 2@6550,957 2149@6821,957 2@7270,957 2169@7542,957",
    " 2@8074,957 2439@8346,957 2@8899,957 2191@9170,957 2@9502,957 2192@9773,957",
    " 2@10105,957 2321@10376,957 2@10888,957 2163@11160,957 2@11405,957 2156@11677,957",
    " 2@11922,957 2155@12194,957 2@12440,957 2159@12711,957 2@12958,957 2164@13229,957",
    "\n"
)];

/// Kerning and ligatures: `AV`, `Ta`, `To`, `Ty` and `Wa` are kerned, `fi`
/// and `fl` become one ligature each, and `ffi` becomes `f` and the `fi`
/// ligature.
const TEXT_KERNING_LIGATURES: [&str; 1] = [concat!(
    "glyphs #000000ff font 0 size 1024",
    " 3@0,957 298@572,957 3@1127,957 2@1714,957 254@1986,957 340@2464,957",
    " 2@3021,957 254@3292,957 566@3765,957 2@4363,957 254@4635,957 714@5123,957",
    " 2@5627,957 301@5898,957 340@6713,957 2@7270,957 746@7542,957 2@8131,957",
    " 457@8402,957 746@8745,957 2@9334,957 747@9605,957",
    "\n"
)];

/// The operators of the calculator, the minus sign U+2212 among them: one
/// glyph for each character.
const TEXT_OPERATORS: [&str; 1] = [concat!(
    "glyphs #000000ff font 0 size 1024",
    " 2318@0,957 2050@451,957 2@962,957 2319@1233,957 2@1745,957 2051@2016,957",
    " 2@2561,957 2320@2832,957 2@3344,957 2049@3616,957 2@4123,957 2327@4394,957",
    " 2@4906,957 2047@5177,957 2163@5749,957 2052@6005,957 2@6518,957 2321@6789,957",
    " 2@7301,957 2318@7572,957 2052@8074,957 2163@8587,957 2052@8843,957 2@9356,957",
    " 2322@9628,957 2@10140,957 2054@10411,957",
    "\n"
)];

/// The radical sign, approximately equal, pi, infinity, superscripts and
/// the degree sign: one glyph for each character.
const TEXT_MATH_VALUES: [&str; 1] = [concat!(
    "glyphs #000000ff font 0 size 1024",
    " 2338@0,957 2049@573,957 2@1080,957 2328@1352,957 2@1851,957 2048@2123,957",
    " 2163@2566,957 2051@2778,957 2048@3263,957 2051@3707,957 2156@4252,957 2@4497,957",
    " 1690@4769,957 2@5378,957 2328@5649,957 2@6149,957 2050@6420,957 2163@6931,957",
    " 2048@7075,957 2051@7518,957 2049@8042,957 2156@8549,957 2@8795,957 2332@9066,957",
    " 2156@9983,957 2@10229,957 2048@10500,957 2047@10943,957 2124@11515,957 2156@11924,957",
    " 2@12170,957 713@12442,957 2123@12942,957 2156@13352,957 2@13598,957 2051@13869,957",
    " 2052@14404,957 2431@14906,957",
    "\n"
)];

/// Comparisons, sums, integrals, partial derivatives, the increment and the
/// dot operator, arrows and the division slash: one glyph for each
/// character.
const TEXT_MATH_SYMBOLS: [&str; 1] = [concat!(
    "glyphs #000000ff font 0 size 1024",
    " 340@0,957 2@557,957 2326@828,957 2@1340,957 377@1612,957 2@2220,957",
    " 2325@2491,957 2@3003,957 387@3275,957 2156@3774,957 2@4020,957 2337@4292,957",
    " 2@4807,957 2333@5078,957 2@5461,957 2340@5732,957 2@6255,957 2335@6526,957",
    " 2@7123,957 2309@7394,957 2@7640,957 2412@7911,957 2@8833,957 2414@9104,957",
    " 2@10026,957 2411@10297,957 2@11219,957 2413@11490,957 2@12412,957 2305@12683,957",
    "\n"
)];

/// Greek letters: one glyph for each character.
const TEXT_GREEK: [&str; 1] = [concat!(
    "glyphs #000000ff font 0 size 1024",
    " 1675@0,957 1676@607,957 1677@1215,957 1678@1705,957 1679@2298,957 1680@2762,957",
    " 1681@3159,957 1682@3765,957 1683@4353,957 1684@4643,957 1685@5180,957 1686@5682,957",
    " 1687@6284,957 1688@6774,957 1689@7191,957 1690@7773,957 1691@8375,957 1693@8973,957",
    " 1694@9574,957 1695@10044,957 1696@10629,957 1697@11363,957 1698@11825,957 1699@12597,957",
    " 2@13394,957 1527@13665,957 1528@14252,957 1529@14875,957 1530@15292,957 1544@15889,957",
    " 1547@16372,957 1549@17201,957 1550@17912,957",
    "\n"
)];

/// Combining characters: `e` with U+0301 becomes the glyph of U+00E9, `n`
/// with U+0303 and `a` with U+0308 become their precomposed glyphs, and
/// `x` with U+0304 and `y` with U+0307, which have none, a base glyph and
/// a mark placed over it.
const TEXT_COMBINING: [&str; 1] = [concat!(
    "glyphs #000000ff font 0 size 1024",
    " 418@0,957 2@558,957 418@829,957 2@1388,957 565@1659,957 2@2259,957",
    " 357@2530,957 2@3087,957 713@3359,957 2486@3780,957 2@3855,957 714@4127,957",
    " 2477@4475,957",
    "\n"
)];

/// Monospaced input: one glyph for each character, each 9.6 pixels to the
/// right of the one before, rounded once.
const MONO_INPUT: [&str; 1] = [concat!(
    "glyphs #000000ff font 1 size 1024",
    " 169@0,957 1006@614,957 258@1229,957 1007@1843,957 3@2458,957 1081@3072,957",
    " 3@3686,957 901@4301,957 977@4915,957 258@5530,957 1388@6144,957 900@6758,957",
    " 3@7373,957 1024@7987,957 3@8602,957 899@9216,957 993@9830,957 900@10445,957",
    "\n"
)];

/// Monospaced mathematical symbols: one glyph for each character, each 9.6
/// pixels to the right of the one before.
const MONO_OPERATORS: [&str; 1] = [concat!(
    "glyphs #000000ff font 1 size 1024",
    " 1096@0,957 899@614,957 900@1229,957 987@1843,957 903@2458,957 3@3072,957",
    " 1099@3686,957 3@4301,957 902@4915,957 3@5530,957 1081@6144,957 3@6758,957",
    " 1096@7373,957 903@7987,957 898@8602,957 3@9216,957 1093@9830,957 3@10445,957",
    " 730@11059,957 1109@11674,957 900@12288,957",
    "\n"
)];

/// Monospaced combining characters: `e` with U+0301 becomes the glyph of
/// U+00E9, `a` with U+0308 its precomposed glyph, and `x` with U+0304 a
/// base glyph and a mark at the next position.
const MONO_COMBINING: [&str; 1] = [concat!(
    "glyphs #000000ff font 1 size 1024",
    " 160@0,957 3@614,957 160@1229,957 3@1843,957 258@2458,957 1419@3072,957",
    " 3@3072,957 140@3686,957",
    "\n"
)];

/// The four quarter-pixel steps: the string drawn with its origin moved
/// right by 0, 16, 32 and 48 units, so that its first glyph lies at each
/// step.
const TEXT_STEPS: [&str; 4] = [
    concat!(
        "glyphs #000000ff font 0 size 1024",
        " 3@0,957 340@587,957 2048@1093,957",
        "\n"
    ),
    concat!(
        "glyphs #000000ff font 0 size 1024",
        " 3@16,957 340@603,957 2048@1109,957",
        "\n"
    ),
    concat!(
        "glyphs #000000ff font 0 size 1024",
        " 3@32,957 340@619,957 2048@1125,957",
        "\n"
    ),
    concat!(
        "glyphs #000000ff font 0 size 1024",
        " 3@48,957 340@635,957 2048@1141,957",
        "\n"
    ),
];

/// Three sizes: the string at 12, 24 and 32 pixels per em.
const TEXT_SIZES: [&str; 3] = [
    concat!(
        "glyphs #000000ff font 0 size 768",
        " 3@0,718 340@440,718 2048@819,718",
        "\n"
    ),
    concat!(
        "glyphs #000000ff font 0 size 1536",
        " 3@0,1436 340@880,1436 2048@1639,1436",
        "\n"
    ),
    concat!(
        "glyphs #000000ff font 0 size 2048",
        " 3@0,1915 340@1174,1915 2048@2185,1915",
        "\n"
    ),
];

#[test]
fn ascii_letters_and_punctuation() -> Checked {
    pinned("text-ascii", &TEXT_ASCII)
}

#[test]
fn digits_and_ascii_operators() -> Checked {
    pinned("text-digits", &TEXT_DIGITS)
}

#[test]
fn kerning_and_ligatures() -> Checked {
    pinned("text-kerning-ligatures", &TEXT_KERNING_LIGATURES)
}

#[test]
fn operators_of_the_calculator() -> Checked {
    pinned("text-operators", &TEXT_OPERATORS)
}

#[test]
fn mathematical_values() -> Checked {
    pinned("text-math-values", &TEXT_MATH_VALUES)
}

#[test]
fn mathematical_symbols() -> Checked {
    pinned("text-math-symbols", &TEXT_MATH_SYMBOLS)
}

#[test]
fn greek_letters() -> Checked {
    pinned("text-greek", &TEXT_GREEK)
}

#[test]
fn combining_characters() -> Checked {
    pinned("text-combining", &TEXT_COMBINING)
}

#[test]
fn monospaced_input() -> Checked {
    pinned("mono-input", &MONO_INPUT)
}

#[test]
fn monospaced_operators() -> Checked {
    pinned("mono-operators", &MONO_OPERATORS)
}

#[test]
fn monospaced_combining_characters() -> Checked {
    pinned("mono-combining", &MONO_COMBINING)
}

#[test]
fn the_four_quarter_pixel_steps() -> Checked {
    pinned("text-steps", &TEXT_STEPS)
}

#[test]
fn three_sizes() -> Checked {
    pinned("text-sizes", &TEXT_SIZES)
}

#[test]
fn every_string_is_pinned() {
    let names: Vec<&str> = STRINGS.iter().map(|shaped| shaped.name).collect();
    assert_eq!(
        names,
        [
            "text-ascii",
            "text-digits",
            "text-kerning-ligatures",
            "text-operators",
            "text-math-values",
            "text-math-symbols",
            "text-greek",
            "text-combining",
            "mono-input",
            "mono-operators",
            "mono-combining",
            "text-steps",
            "text-sizes",
        ]
    );
}

#[test]
fn the_first_glyph_lies_at_each_step() -> Checked {
    let mut set = font_set()?;
    let paragraph = lay_out(&mut set, "Aa1", kosmo_text::SANS, 16, None)?;
    let mut steps = Vec::new();
    for offset in [0, 16, 32, 48] {
        let list = list(&paragraph, offset, 0)?;
        let Some(Command::Glyphs { glyphs, .. }) = list.commands().first() else {
            return Err("no glyph run".into());
        };
        let first = glyphs.first().ok_or("no glyph")?;
        steps.push(place_glyph(first.origin.x, first.origin.y).step);
    }
    assert_eq!(steps, Step::ALL);
    Ok(())
}

#[test]
fn a_combining_acute_accent_shapes_as_the_precomposed_letter() -> Checked {
    let mut set = font_set()?;
    for font in [kosmo_text::SANS, kosmo_text::MONO] {
        let paragraph = lay_out(&mut set, "e\u{301} \u{e9}", font, 16, None)?;
        let glyphs = paragraph.lines()[0].glyphs();
        assert_eq!(glyphs.len(), 3, "{font:?}");
        assert_eq!(glyphs[0].id, glyphs[2].id, "{font:?}");
    }
    Ok(())
}

#[test]
fn monospaced_glyphs_lie_9_6_pixels_apart() -> Checked {
    let mut set = font_set()?;
    for name in ["mono-input", "mono-operators"] {
        let shaped = STRINGS
            .iter()
            .find(|shaped| shaped.name == name)
            .ok_or(name)?;
        let paragraph = lay_out(&mut set, shaped.text, shaped.font, 16, None)?;
        let glyphs = paragraph.lines()[0].glyphs();
        assert_eq!(glyphs.len(), shaped.text.chars().count(), "{name}");
        for (index, glyph) in glyphs.iter().enumerate() {
            // 9.6 pixels are 614.4 units, rounded once from the position.
            let exact = index as i64 * 3072;
            let rounded = (exact + 2) / 5;
            assert_eq!(i64::from(glyph.origin.x.units()), rounded, "{name} {index}");
        }
    }
    Ok(())
}
