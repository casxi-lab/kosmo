//! Shaping and line breaking: a paragraph of one of the two fonts, laid out
//! by parley and rounded once into the 26.6 fixed point of `kosmo-types`.

use alloc::vec::Vec;
use core::fmt;
use core::ops::Range;

use kosmo_render::{Color, Command, FontId, Glyph, GlyphId};
use kosmo_types::{Fx, Point, round};
use parley::{BaseDirection, FontFamily, Layout, PositionedLayoutItem, StyleProperty};

use crate::fonts::{FontSet, MONO, SANS};

/// The smallest size of a font: 1 pixel per em, 64 units.
pub const MIN_SIZE: Fx = Fx::from_units(64);

/// The largest size of a font: 256 pixels per em, 16,384 units, which
/// bounds the coverage mask of one glyph.
pub const MAX_SIZE: Fx = Fx::from_units(16_384);

/// The largest width of a paragraph: 2^18 pixels, 2^24 units, within which
/// a width converts to `f32` exactly.
pub const MAX_WIDTH: Fx = Fx::from_units(1 << 24);

/// The style of a paragraph: one of the two fonts, [`SANS`] or [`MONO`], at
/// a size from [`MIN_SIZE`] to [`MAX_SIZE`].
///
/// ```
/// use kosmo_text::{MAX_SIZE, SANS, TextError, TextStyle};
/// use kosmo_types::Fx;
///
/// let style = TextStyle::new(SANS, Fx::from_units(1024)).unwrap();
/// assert_eq!((style.font(), style.size().units()), (SANS, 1024));
/// let large = Fx::from_units(MAX_SIZE.units() + 1);
/// assert_eq!(TextStyle::new(SANS, large), Err(TextError::SizeOutOfRange(large)));
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TextStyle {
    font: FontId,
    size: Fx,
}

impl TextStyle {
    /// The style of `font` at `size`, its pixels per em, or an error for a
    /// font other than [`SANS`] and [`MONO`] or a size outside
    /// [`MIN_SIZE`] to [`MAX_SIZE`].
    pub fn new(font: FontId, size: Fx) -> Result<TextStyle, TextError> {
        if font != SANS && font != MONO {
            return Err(TextError::UnknownFont(font));
        }
        if size < MIN_SIZE || size > MAX_SIZE {
            return Err(TextError::SizeOutOfRange(size));
        }
        Ok(TextStyle { font, size })
    }

    /// The font.
    pub fn font(self) -> FontId {
        self.font
    }

    /// The size: the pixels per em.
    pub fn size(self) -> Fx {
        self.size
    }
}

/// Why text cannot be laid out or drawn.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextError {
    /// The font is neither [`SANS`] nor [`MONO`].
    UnknownFont(FontId),
    /// The size lies outside [`MIN_SIZE`] to [`MAX_SIZE`].
    SizeOutOfRange(Fx),
    /// The width is negative or larger than [`MAX_WIDTH`].
    WidthOutOfRange(Fx),
    /// A position or length of the layout, or a glyph origin moved to where
    /// it is drawn, does not fit in an [`Fx`].
    OutOfRange,
}

impl fmt::Display for TextError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TextError::UnknownFont(font) => write!(f, "font {} is not in the font set", font.get()),
            TextError::SizeOutOfRange(size) => write!(
                f,
                "size {} is outside {} to {} units",
                size.units(),
                MIN_SIZE.units(),
                MAX_SIZE.units()
            ),
            TextError::WidthOutOfRange(width) => write!(
                f,
                "width {} is outside 0 to {} units",
                width.units(),
                MAX_WIDTH.units()
            ),
            TextError::OutOfRange => f.write_str("a position does not fit in the fixed point"),
        }
    }
}

impl core::error::Error for TextError {}

/// A laid-out paragraph: its lines, each with its glyphs, in the
/// coordinates of the paragraph, whose top-left corner is (0, 0), with `x`
/// growing to the right and `y` growing down.
///
/// Every position and length is an [`Fx`]: parley's `f32` value multiplied
/// by 64, which is exact, and rounded once by [`kosmo_types::round`].
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Paragraph {
    style: TextStyle,
    width: Fx,
    height: Fx,
    lines: Vec<Line>,
}

impl Paragraph {
    /// The style that laid the paragraph out.
    pub fn style(&self) -> TextStyle {
        self.style
    }

    /// The width: the advance of the longest line, without the spaces that
    /// hang at the ends of lines.
    pub fn width(&self) -> Fx {
        self.width
    }

    /// The height: the line heights of the font added together.
    pub fn height(&self) -> Fx {
        self.height
    }

    /// The lines, from the top.
    pub fn lines(&self) -> &[Line] {
        &self.lines
    }

    /// The glyphs of every line, as one [`Command::Glyphs`] run of the
    /// paragraph's font and size in `color`, with each origin moved by
    /// `origin`, or [`TextError::OutOfRange`] when a moved origin does not
    /// fit.
    pub fn run(&self, origin: Point, color: Color) -> Result<Command, TextError> {
        let glyphs = self
            .lines
            .iter()
            .flat_map(|line| &line.glyphs)
            .map(|glyph| {
                let moved = glyph.origin.translated(origin.x, origin.y);
                moved
                    .map(|origin| Glyph {
                        id: glyph.id,
                        origin,
                    })
                    .ok_or(TextError::OutOfRange)
            })
            .collect::<Result<Vec<Glyph>, TextError>>()?;
        Ok(Command::Glyphs {
            font: self.style.font,
            size: self.style.size,
            color,
            glyphs,
        })
    }
}

/// A line of a [`Paragraph`].
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Line {
    text: Range<usize>,
    baseline: Fx,
    glyphs: Vec<Glyph>,
}

impl Line {
    /// The bytes of the paragraph's text that the line holds, the spaces
    /// and the line break at its end included.
    pub fn text(&self) -> Range<usize> {
        self.text.clone()
    }

    /// The baseline, from the top of the paragraph.
    pub fn baseline(&self) -> Fx {
        self.baseline
    }

    /// The glyphs, in visual order, each with its origin on the baseline.
    pub fn glyphs(&self) -> &[Glyph] {
        &self.glyphs
    }
}

impl FontSet {
    /// Lays out `text` in `style`, from left to right, breaking lines at
    /// `width`, or only at line breaks of the text without a width.
    ///
    /// parley shapes the text with harfrust and breaks lines by the Unicode
    /// line breaking algorithm, with the data of icu_segmenter compiled
    /// into the program. The font is the style's font alone: there is no
    /// fallback, so a character that it lacks becomes its glyph `.notdef`.
    /// The base direction of the paragraph is left to right whatever its
    /// first character; neither font has right-to-left characters, and
    /// parley still orders a run of them, as `.notdef` glyphs, by the
    /// Unicode bidirectional algorithm. The size and the width reach parley
    /// as exact `f32` values, its alignment to pixels is off, and each
    /// position it gives is rounded once into an [`Fx`]. The width is
    /// between 0 and [`MAX_WIDTH`]; a word longer than it stays on a line of
    /// its own.
    pub fn layout(
        &mut self,
        text: &str,
        style: TextStyle,
        width: Option<Fx>,
    ) -> Result<Paragraph, TextError> {
        let max_advance = match width {
            None => None,
            Some(width) if width >= Fx::ZERO && width <= MAX_WIDTH => Some(exact(width)),
            Some(width) => return Err(TextError::WidthOutOfRange(width)),
        };
        let family = self
            .names
            .get(style.font.get() as usize)
            .ok_or(TextError::UnknownFont(style.font))?;
        let mut builder = self
            .layout_context
            .ranged_builder(&mut self.context, text, 1.0, false);
        builder.set_base_direction(BaseDirection::Ltr);
        builder.push_default(StyleProperty::FontFamily(FontFamily::named(family)));
        builder.push_default(StyleProperty::FontSize(exact(style.size)));
        let mut layout: Layout<()> = builder.build(text);
        layout.break_all_lines(max_advance);
        let mut lines = Vec::with_capacity(layout.len());
        for line in layout.lines() {
            let mut glyphs = Vec::new();
            for item in line.items() {
                let PositionedLayoutItem::GlyphRun(run) = item else {
                    continue;
                };
                for glyph in run.positioned_glyphs() {
                    glyphs.push(Glyph {
                        id: GlyphId::new(glyph.id),
                        origin: Point::new(units(glyph.x)?, units(glyph.y)?),
                    });
                }
            }
            lines.push(Line {
                text: line.text_range(),
                baseline: units(line.metrics().baseline)?,
                glyphs,
            });
        }
        Ok(Paragraph {
            style,
            width: units(layout.width())?,
            height: units(layout.height())?,
            lines,
        })
    }
}

/// `value` in pixels as an `f32`: exact, since `value` is within 2^24 units
/// and a division by 64 only changes the exponent.
fn exact(value: Fx) -> f32 {
    value.units() as f32 / 64.0
}

/// A value of parley in pixels as an [`Fx`]: multiplied by 64, which is
/// exact, and rounded once.
fn units(pixels: f32) -> Result<Fx, TextError> {
    round(pixels * 64.0).ok_or(TextError::OutOfRange)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_and_widths_reach_parley_exactly() {
        for units in [64, 1000, 1024, 16_384, 1 << 24] {
            let value = exact(Fx::from_units(units));
            assert_eq!(value * 64.0, units as f32);
            assert_eq!(super::units(value), Ok(Fx::from_units(units)));
        }
        assert_eq!(super::units(0.5 / 64.0), Ok(Fx::from_units(1)));
        assert_eq!(super::units(-0.5 / 64.0), Ok(Fx::ZERO));
        assert_eq!(super::units(f32::INFINITY), Err(TextError::OutOfRange));
        assert_eq!(super::units(f32::NAN), Err(TextError::OutOfRange));
    }
}
