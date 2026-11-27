//! Support of the integration tests: the font set of the two bundled fonts,
//! the strings whose glyphs the tests pin, the cases of line breaking and
//! the pixel goldens of the strings.
//! Each test file declares it `pub`, so the parts that it does not use are
//! not reported as unused.

pub mod golden;

use kosmo_render::{Color, DisplayList, FontId, ListBuilder};
use kosmo_text::{FontSet, MONO, Paragraph, SANS, TextStyle};
use kosmo_types::{Fx, Point};

/// The result of a helper; the error says what went wrong.
pub type Checked<T = ()> = Result<T, String>;

/// `error` as the error of a helper.
pub fn failed(error: impl std::fmt::Debug) -> String {
    format!("{error:?}")
}

/// The bytes of Fira Sans from the asset directory.
pub const SANS_BYTES: &[u8] = include_bytes!("../../assets/fonts/firasans/FiraSans-Regular.ttf");

/// The bytes of Fira Mono from the asset directory.
pub const MONO_BYTES: &[u8] = include_bytes!("../../assets/fonts/firamono/FiraMono-Regular.ttf");

/// The font set of the two bundled fonts.
pub fn font_set() -> Checked<FontSet> {
    FontSet::new(SANS_BYTES, MONO_BYTES).map_err(failed)
}

/// Opaque black, the color in which the tests draw text.
pub const BLACK: Color = Color::opaque(0, 0, 0);

/// A string whose glyphs a test pins: it is laid out in `font` at each of
/// `sizes`, in pixels per em, without a width, and drawn with its origin
/// moved right by each of `offsets`, in 1/64 pixel.
pub struct Shaped {
    /// The name of the case.
    pub name: &'static str,
    /// The string.
    pub text: &'static str,
    /// The font.
    pub font: FontId,
    /// The sizes in pixels per em.
    pub sizes: &'static [i32],
    /// The offsets of the origin to the right, in 1/64 pixel.
    pub offsets: &'static [i32],
}

/// The strings: ASCII letters and punctuation, digits and ASCII operators,
/// kerning and ligatures, the operators of the calculator, mathematical
/// values and symbols, Greek letters, combining characters in both fonts,
/// monospaced input and operators, the four quarter-pixel steps and three
/// sizes.
pub const STRINGS: [Shaped; 13] = [
    shaped(
        "text-ascii",
        "The quick brown fox jumps over the lazy dog.",
        SANS,
    ),
    shaped("text-digits", "0123456789 + - * / ^ ( ) = . , : ! ?", SANS),
    shaped("text-kerning-ligatures", "AVA Ta To Ty Wa fi ffi fl", SANS),
    shaped(
        "text-operators",
        "\u{2212}3 \u{d7} 4 \u{f7} 2 \u{b1} 0.5 = \u{2212}5.5 \u{2260} 7",
        SANS,
    ),
    shaped(
        "text-math-values",
        "\u{221a}2 \u{2248} 1.414, \u{3c0} \u{2248} 3.142, \u{221e}, 10\u{b3}, x\u{b2}, 45\u{b0}",
        SANS,
    ),
    shaped(
        "text-math-symbols",
        "a \u{2264} b \u{2265} c, \u{2211} \u{222b} \u{2202} \u{2206} \u{22c5} \u{2192} \u{2190} \u{2191} \u{2193} \u{2215}",
        SANS,
    ),
    shaped(
        "text-greek",
        "\u{3b1}\u{3b2}\u{3b3}\u{3b4}\u{3b5}\u{3b6}\u{3b7}\u{3b8}\u{3b9}\u{3ba}\u{3bb}\u{3bc}\
         \u{3bd}\u{3be}\u{3bf}\u{3c0}\u{3c1}\u{3c3}\u{3c4}\u{3c5}\u{3c6}\u{3c7}\u{3c8}\u{3c9} \
         \u{391}\u{392}\u{393}\u{394}\u{3a3}\u{3a6}\u{3a8}\u{3a9}",
        SANS,
    ),
    shaped(
        "text-combining",
        "e\u{301} \u{e9} n\u{303} a\u{308} x\u{304} y\u{307}",
        SANS,
    ),
    shaped("mono-input", "f(x) = 3*x^2 - 1/2", MONO),
    shaped(
        "mono-operators",
        "\u{2212}12.5 \u{d7} 4 = \u{2212}50 \u{2264} \u{3c0}\u{221a}2",
        MONO,
    ),
    shaped("mono-combining", "e\u{301} \u{e9} x\u{304} a\u{308}", MONO),
    Shaped {
        offsets: &[0, 16, 32, 48],
        ..shaped("text-steps", "Aa1", SANS)
    },
    Shaped {
        sizes: &[12, 24, 32],
        ..shaped("text-sizes", "Aa1", SANS)
    },
];

/// A string at 16 pixels per em with its origin not moved.
const fn shaped(name: &'static str, text: &'static str, font: FontId) -> Shaped {
    Shaped {
        name,
        text,
        font,
        sizes: &[16],
        offsets: &[0],
    }
}

/// A case of line breaking: a string laid out in `font` at 16 pixels per
/// em, at `width` pixels or without a width.
pub struct Break {
    /// The name of the case.
    pub name: &'static str,
    /// The string.
    pub text: &'static str,
    /// The font.
    pub font: FontId,
    /// The width in pixels, or `None` for none.
    pub width: Option<i32>,
}

/// The cases: a break at a space, a line feed, a word longer than the
/// width, a minus sign kept with its number, a no-break space, a combining
/// mark at the end of a line, spaces that hang at the end of a line, no
/// width, and the monospaced font with a fraction.
pub const BREAKS: [Break; 9] = [
    broken(
        "break-at-space",
        "The result is 1/3 and the error is 0",
        SANS,
        Some(160),
    ),
    broken("line-feed", "1 + 2\n= 3", SANS, None),
    broken(
        "long-word",
        "Supercalifragilistic expression",
        SANS,
        Some(100),
    ),
    broken("minus-sign", "x = \u{2212}3.14159 + 2", SANS, Some(66)),
    broken("no-break-space", "Mass 10\u{a0}kg total", SANS, Some(60)),
    broken("combining-mark", "cafe\u{301} au lait", SANS, Some(40)),
    broken("hanging-spaces", "1 + 2 = 3   4", SANS, Some(60)),
    broken(
        "no-width",
        "The result is 1/3 and the error is 0",
        SANS,
        None,
    ),
    broken("mono-fraction", "f(x) = 3*x^2 - 1/2", MONO, Some(100)),
];

const fn broken(name: &'static str, text: &'static str, font: FontId, width: Option<i32>) -> Break {
    Break {
        name,
        text,
        font,
        width,
    }
}

/// `pixels` whole pixels as an [`Fx`].
pub fn px(pixels: i32) -> Checked<Fx> {
    Fx::from_pixels(pixels).ok_or_else(|| format!("{pixels} pixels do not fit"))
}

/// `text` laid out in `font` at `size` pixels per em, at `width` pixels or
/// without a width.
pub fn lay_out(
    set: &mut FontSet,
    text: &str,
    font: FontId,
    size: i32,
    width: Option<i32>,
) -> Checked<Paragraph> {
    let style = TextStyle::new(font, px(size)?).map_err(failed)?;
    let width = width.map(px).transpose()?;
    set.layout(text, style, width).map_err(failed)
}

/// The display list of `paragraph` in black, with its origin at (`x`, `y`)
/// in 1/64 pixel: one glyph run.
pub fn list(paragraph: &Paragraph, x: i32, y: i32) -> Checked<DisplayList> {
    let origin = Point::new(Fx::from_units(x), Fx::from_units(y));
    let mut builder = ListBuilder::new();
    builder
        .push(paragraph.run(origin, BLACK).map_err(failed)?)
        .map_err(failed)?;
    builder.finish().map_err(failed)
}

/// The text of the display list of each size and offset of `shaped`, in
/// the order of its sizes and then of its offsets.
pub fn runs(set: &mut FontSet, shaped: &Shaped) -> Checked<Vec<String>> {
    let mut runs = Vec::new();
    for &size in shaped.sizes {
        let paragraph = lay_out(set, shaped.text, shaped.font, size, None)?;
        for &offset in shaped.offsets {
            runs.push(list(&paragraph, offset, 0)?.text());
        }
    }
    Ok(runs)
}

/// The text of each line of `case`, laid out at 16 pixels per em.
pub fn lines(set: &mut FontSet, case: &Break) -> Checked<Vec<String>> {
    let paragraph = lay_out(set, case.text, case.font, 16, case.width)?;
    paragraph
        .lines()
        .iter()
        .map(|line| {
            case.text
                .get(line.text())
                .map(String::from)
                .ok_or_else(|| format!("{}: a line is not text of the string", case.name))
        })
        .collect()
}
