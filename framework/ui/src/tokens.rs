//! The design tokens: the named spacings, font sizes and colours that
//! controls take their values from.

use kosmo_render::Color;
use kosmo_types::Fx;

/// A spacing token: the gap between the children of a row or column, or
/// the padding around them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Space {
    /// 2 pixels.
    Xs,
    /// 4 pixels.
    S,
    /// 8 pixels.
    M,
    /// 16 pixels.
    L,
}

/// A font size token: the pixels per em of a text leaf.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FontSize {
    /// 12 pixels.
    Small,
    /// 16 pixels.
    Body,
    /// 24 pixels.
    Large,
}

/// A colour token.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ColorToken {
    /// The background, `#FFFFFF`.
    Background,
    /// Text, `#1A1A1A`, the colour of a text leaf unless it names another.
    Text,
    /// Secondary text, `#595959`.
    TextSecondary,
    /// The accent, `#1F5FBF`.
    Accent,
    /// Errors, `#B3261E`.
    Error,
}

/// A table of design tokens: the value of each spacing, font size and
/// colour token.
///
/// [`TOKENS`] is kosmo's one table of design tokens: the crate defines no
/// other and no way to switch tables while a program runs. The fields are
/// public, so a test can build a changed copy, as in
/// `Tokens { space_m: Fx::from_units(640), ..TOKENS }`. Spacings and font
/// sizes are `Fx` values in 1/64 pixel, and colours opaque 8-bit sRGB
/// constants.
///
/// ```
/// use kosmo_ui::{ColorToken, FontSize, Space, TOKENS};
/// use kosmo_render::Color;
///
/// assert_eq!(TOKENS.space(Space::M).units(), 8 * 64);
/// assert_eq!(TOKENS.font_size(FontSize::Body).units(), 16 * 64);
/// assert_eq!(TOKENS.color(ColorToken::Accent), Color::opaque(0x1F, 0x5F, 0xBF));
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Tokens {
    /// The value of [`Space::Xs`].
    pub space_xs: Fx,
    /// The value of [`Space::S`].
    pub space_s: Fx,
    /// The value of [`Space::M`].
    pub space_m: Fx,
    /// The value of [`Space::L`].
    pub space_l: Fx,
    /// The value of [`FontSize::Small`].
    pub font_small: Fx,
    /// The value of [`FontSize::Body`].
    pub font_body: Fx,
    /// The value of [`FontSize::Large`].
    pub font_large: Fx,
    /// The value of [`ColorToken::Background`].
    pub background: Color,
    /// The value of [`ColorToken::Text`].
    pub text: Color,
    /// The value of [`ColorToken::TextSecondary`].
    pub text_secondary: Color,
    /// The value of [`ColorToken::Accent`].
    pub accent: Color,
    /// The value of [`ColorToken::Error`].
    pub error: Color,
}

/// The design tokens of kosmo's framework: spacings of 2, 4, 8 and 16
/// pixels, font sizes of 12, 16 and 24 pixels, and five opaque colours.
pub const TOKENS: Tokens = Tokens {
    space_xs: Fx::from_units(2 * 64),
    space_s: Fx::from_units(4 * 64),
    space_m: Fx::from_units(8 * 64),
    space_l: Fx::from_units(16 * 64),
    font_small: Fx::from_units(12 * 64),
    font_body: Fx::from_units(16 * 64),
    font_large: Fx::from_units(24 * 64),
    background: Color::opaque(0xFF, 0xFF, 0xFF),
    text: Color::opaque(0x1A, 0x1A, 0x1A),
    text_secondary: Color::opaque(0x59, 0x59, 0x59),
    accent: Color::opaque(0x1F, 0x5F, 0xBF),
    error: Color::opaque(0xB3, 0x26, 0x1E),
};

impl Tokens {
    /// The value of the spacing token `space`.
    pub const fn space(&self, space: Space) -> Fx {
        match space {
            Space::Xs => self.space_xs,
            Space::S => self.space_s,
            Space::M => self.space_m,
            Space::L => self.space_l,
        }
    }

    /// The value of the font size token `size`.
    pub const fn font_size(&self, size: FontSize) -> Fx {
        match size {
            FontSize::Small => self.font_small,
            FontSize::Body => self.font_body,
            FontSize::Large => self.font_large,
        }
    }

    /// The value of the colour token `color`.
    pub const fn color(&self, color: ColorToken) -> Color {
        match color {
            ColorToken::Background => self.background,
            ColorToken::Text => self.text,
            ColorToken::TextSecondary => self.text_secondary,
            ColorToken::Accent => self.accent,
            ColorToken::Error => self.error,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_holds_the_values_of_the_tokens() {
        let spaces = [Space::Xs, Space::S, Space::M, Space::L].map(|space| TOKENS.space(space));
        assert_eq!(spaces.map(Fx::units), [128, 256, 512, 1024]);
        let sizes =
            [FontSize::Small, FontSize::Body, FontSize::Large].map(|size| TOKENS.font_size(size));
        assert_eq!(sizes.map(Fx::units), [768, 1024, 1536]);
        let colors = [
            ColorToken::Background,
            ColorToken::Text,
            ColorToken::TextSecondary,
            ColorToken::Accent,
            ColorToken::Error,
        ]
        .map(|color| TOKENS.color(color).to_bytes());
        assert_eq!(
            colors,
            [
                [255, 255, 255, 255],
                [26, 26, 26, 255],
                [89, 89, 89, 255],
                [31, 95, 191, 255],
                [179, 38, 30, 255],
            ]
        );
    }

    #[test]
    fn the_font_sizes_are_sizes_that_kosmo_text_lays_out() {
        for size in [FontSize::Small, FontSize::Body, FontSize::Large] {
            let value = TOKENS.font_size(size);
            assert!(kosmo_text::MIN_SIZE <= value && value <= kosmo_text::MAX_SIZE);
        }
    }
}
