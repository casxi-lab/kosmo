//! Display lists: the drawing commands of one frame, in the 26.6
//! fixed-point geometry of `kosmo-types`, built by appending commands and
//! checked when construction ends.
//!
//! The renderer interface, planned in this crate, states what each command
//! draws.

use alloc::vec;
use alloc::vec::Vec;

use kosmo_types::{Fx, Point, Rect};

/// The most commands in a display list: 16,384. The command after them is
/// refused.
pub const MAX_COMMANDS: usize = 16_384;

/// The most items in a display list, path elements and glyphs together:
/// 65,536. A command that would bring the list beyond them is refused.
pub const MAX_ITEMS: usize = 65_536;

/// A color: red, green, blue and alpha as 8-bit sRGB-encoded values, with
/// red, green and blue premultiplied by alpha, as a pixel of
/// [`PixelFormat::Rgba8Premul`](kosmo_types::PixelFormat::Rgba8Premul)
/// holds them. No component is greater than alpha.
///
/// ```
/// use kosmo_render::Color;
///
/// // Half-transparent white.
/// let color = Color::premultiplied(128, 128, 128, 128).unwrap();
/// assert_eq!(color.to_bytes(), [128, 128, 128, 128]);
/// assert_eq!(Color::premultiplied(129, 0, 0, 128), None);
/// assert!(Color::opaque(255, 128, 0).is_opaque());
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Color {
    red: u8,
    green: u8,
    blue: u8,
    alpha: u8,
}

impl Color {
    /// Transparent black, all four components 0.
    pub const TRANSPARENT: Color = Color {
        red: 0,
        green: 0,
        blue: 0,
        alpha: 0,
    };

    /// The opaque color of `red`, `green` and `blue`, with alpha 255.
    pub const fn opaque(red: u8, green: u8, blue: u8) -> Color {
        Color {
            red,
            green,
            blue,
            alpha: 255,
        }
    }

    /// The color of the premultiplied components, or `None` when one of
    /// `red`, `green` and `blue` is greater than `alpha`.
    pub const fn premultiplied(red: u8, green: u8, blue: u8, alpha: u8) -> Option<Color> {
        if red > alpha || green > alpha || blue > alpha {
            None
        } else {
            Some(Color {
                red,
                green,
                blue,
                alpha,
            })
        }
    }

    /// Red, premultiplied.
    pub const fn red(self) -> u8 {
        self.red
    }

    /// Green, premultiplied.
    pub const fn green(self) -> u8 {
        self.green
    }

    /// Blue, premultiplied.
    pub const fn blue(self) -> u8 {
        self.blue
    }

    /// Alpha.
    pub const fn alpha(self) -> u8 {
        self.alpha
    }

    /// Whether alpha is 255.
    pub const fn is_opaque(self) -> bool {
        self.alpha == 255
    }

    /// The bytes R, G, B and A, in the memory order of a pixel of
    /// [`PixelFormat::Rgba8Premul`](kosmo_types::PixelFormat::Rgba8Premul).
    pub const fn to_bytes(self) -> [u8; 4] {
        [self.red, self.green, self.blue, self.alpha]
    }
}

/// One element of a [`Path`]. Points are in the coordinates of the command
/// that holds the path.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PathElement {
    /// Starts a subpath at the point.
    MoveTo(Point),
    /// A straight segment to the point.
    LineTo(Point),
    /// A quadratic Bézier segment with the control point and the end point.
    QuadTo(Point, Point),
    /// A cubic Bézier segment with two control points and the end point.
    CubicTo(Point, Point, Point),
    /// A straight segment back to the start of the subpath, which closes it.
    Close,
}

/// A path: subpaths of straight and Bézier segments, which always starts
/// with a [`PathElement::MoveTo`]. Each element counts as one item of a
/// display list.
///
/// ```
/// use kosmo_render::{Path, PathElement};
/// use kosmo_types::{Fx, Point};
///
/// let px = |x, y| Point::new(Fx::from_units(x * 64), Fx::from_units(y * 64));
/// let mut triangle = Path::new(px(0, 0));
/// triangle.line_to(px(10, 0)).line_to(px(5, 8)).close();
/// assert_eq!(triangle.elements().len(), 4);
/// assert_eq!(triangle.elements()[3], PathElement::Close);
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Path {
    elements: Vec<PathElement>,
}

impl Path {
    /// A path whose first subpath starts at `start`.
    pub fn new(start: Point) -> Path {
        Path {
            elements: vec![PathElement::MoveTo(start)],
        }
    }

    /// Starts a new subpath at `to`.
    pub fn move_to(&mut self, to: Point) -> &mut Path {
        self.elements.push(PathElement::MoveTo(to));
        self
    }

    /// Adds a straight segment to `to`.
    pub fn line_to(&mut self, to: Point) -> &mut Path {
        self.elements.push(PathElement::LineTo(to));
        self
    }

    /// Adds a quadratic Bézier segment with `control` to `to`.
    pub fn quad_to(&mut self, control: Point, to: Point) -> &mut Path {
        self.elements.push(PathElement::QuadTo(control, to));
        self
    }

    /// Adds a cubic Bézier segment with `first` and `second` to `to`.
    pub fn cubic_to(&mut self, first: Point, second: Point, to: Point) -> &mut Path {
        self.elements.push(PathElement::CubicTo(first, second, to));
        self
    }

    /// Closes the current subpath.
    pub fn close(&mut self) -> &mut Path {
        self.elements.push(PathElement::Close);
        self
    }

    /// The elements, the first of them a [`PathElement::MoveTo`].
    pub fn elements(&self) -> &[PathElement] {
        &self.elements
    }
}

/// How a stroke joins two segments.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Join {
    /// The outer edges meet in a point, or are cut off as by
    /// [`Join::Bevel`] where the point would lie farther than 4 times the
    /// half width from the joint.
    Miter,
    /// A circular arc of the half width around the joint.
    Round,
    /// A straight edge between the outer corners.
    Bevel,
}

/// How a stroke ends an open subpath.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Cap {
    /// The stroke ends at the end point.
    Butt,
    /// A half circle of the half width around the end point.
    Round,
    /// A half square: the stroke goes on by the half width.
    Square,
}

/// A font, as the glyph coverage that draws a display list names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FontId(u32);

impl FontId {
    /// The font numbered `number`.
    pub const fn new(number: u32) -> FontId {
        FontId(number)
    }

    /// The number of the font.
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// A glyph of a font.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GlyphId(u32);

impl GlyphId {
    /// The glyph numbered `number` in its font.
    pub const fn new(number: u32) -> GlyphId {
        GlyphId(number)
    }

    /// The number of the glyph.
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// A glyph of a [`Command::Glyphs`] run: the glyph and its origin, a point
/// on the baseline in the coordinates of the command.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Glyph {
    /// The glyph.
    pub id: GlyphId,
    /// The glyph's origin.
    pub origin: Point,
}

/// A command of a display list. Geometry is in [`Fx`], colors are
/// [`Color`]s, and no field is a floating-point value or a bitmap.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Command {
    /// Fills a rectangle.
    FillRect {
        /// The rectangle.
        rect: Rect,
        /// Its color.
        color: Color,
    },
    /// Fills a rectangle with rounded corners.
    FillRoundedRect {
        /// The rectangle.
        rect: Rect,
        /// The radius of all four corners, not negative.
        radius: Fx,
        /// Its color.
        color: Color,
    },
    /// Fills a path with the nonzero winding rule.
    FillPath {
        /// The path.
        path: Path,
        /// Its color.
        color: Color,
    },
    /// Strokes a path.
    StrokePath {
        /// The path.
        path: Path,
        /// The width of the stroke, not negative.
        width: Fx,
        /// How segments are joined.
        join: Join,
        /// How open subpaths end.
        cap: Cap,
        /// Its color.
        color: Color,
    },
    /// Draws a run of glyphs of one font and size.
    Glyphs {
        /// The font.
        font: FontId,
        /// The size of the font: its pixels per em.
        size: Fx,
        /// Their color.
        color: Color,
        /// The glyphs with their origins.
        glyphs: Vec<Glyph>,
    },
    /// Starts a group: until the matching [`Command::PopClip`], drawing is
    /// clipped to `rect` and moved by `offset`.
    PushClip {
        /// The clip, in the coordinates outside the group.
        rect: Rect,
        /// How far the commands of the group are moved.
        offset: Point,
    },
    /// Ends the innermost group.
    PopClip,
}

impl Command {
    /// The items of the command: its path elements or its glyphs.
    pub fn items(&self) -> usize {
        match self {
            Command::FillPath { path, .. } | Command::StrokePath { path, .. } => {
                path.elements().len()
            }
            Command::Glyphs { glyphs, .. } => glyphs.len(),
            Command::FillRect { .. }
            | Command::FillRoundedRect { .. }
            | Command::PushClip { .. }
            | Command::PopClip => 0,
        }
    }
}

/// Why a display list refused a command or could not be finished.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ListError {
    /// The list already holds [`MAX_COMMANDS`] commands.
    TooManyCommands,
    /// The command would bring the list beyond [`MAX_ITEMS`] items.
    TooManyItems,
    /// The radius of a rounded rectangle is negative.
    NegativeRadius,
    /// The width of a stroke is negative.
    NegativeWidth,
    /// A [`Command::PopClip`] has no group to end.
    UnpairedPopClip,
    /// Construction ended while a group was still open.
    UnclosedClip,
}

/// The construction of a display list: commands are appended one by one,
/// each checked when it is appended, and [`ListBuilder::finish`] checks
/// that every group is ended. A refused command leaves the builder as it
/// was.
///
/// ```
/// use kosmo_render::{Color, Command, ListBuilder, ListError};
/// use kosmo_types::{Fx, Point, Rect, Size};
///
/// let px = |pixels| Fx::from_pixels(pixels).unwrap();
/// let rect = Rect::new(Point::new(px(2), px(2)), Size::new(px(8), px(4)).unwrap());
/// let mut builder = ListBuilder::new();
/// builder.push(Command::PushClip { rect, offset: Point::new(px(0), px(-3)) }).unwrap();
/// builder.push(Command::FillRect { rect, color: Color::opaque(0, 0, 255) }).unwrap();
/// assert_eq!(builder.clone().finish(), Err(ListError::UnclosedClip));
/// builder.push(Command::PopClip).unwrap();
/// let list = builder.finish().unwrap();
/// assert_eq!(list.commands().len(), 3);
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ListBuilder {
    commands: Vec<Command>,
    items: usize,
    depth: usize,
}

impl ListBuilder {
    /// An empty builder.
    pub fn new() -> ListBuilder {
        ListBuilder::default()
    }

    /// Appends `command`, or refuses it and stays as it was: after
    /// [`MAX_COMMANDS`] commands, when its items would bring the list
    /// beyond [`MAX_ITEMS`], when a radius or a width is negative, and for
    /// a [`Command::PopClip`] without an open group.
    pub fn push(&mut self, command: Command) -> Result<(), ListError> {
        if self.commands.len() >= MAX_COMMANDS {
            return Err(ListError::TooManyCommands);
        }
        let items = self.items + command.items();
        if items > MAX_ITEMS {
            return Err(ListError::TooManyItems);
        }
        let depth = match &command {
            Command::FillRoundedRect { radius, .. } if *radius < Fx::ZERO => {
                return Err(ListError::NegativeRadius);
            }
            Command::StrokePath { width, .. } if *width < Fx::ZERO => {
                return Err(ListError::NegativeWidth);
            }
            Command::PushClip { .. } => self.depth + 1,
            Command::PopClip => self
                .depth
                .checked_sub(1)
                .ok_or(ListError::UnpairedPopClip)?,
            _ => self.depth,
        };
        self.commands.push(command);
        self.items = items;
        self.depth = depth;
        Ok(())
    }

    /// The commands appended so far.
    pub fn commands(&self) -> &[Command] {
        &self.commands
    }

    /// The items appended so far.
    pub fn items(&self) -> usize {
        self.items
    }

    /// The groups that are open.
    pub fn depth(&self) -> usize {
        self.depth
    }

    /// The display list, or [`ListError::UnclosedClip`] when a group is
    /// still open.
    pub fn finish(self) -> Result<DisplayList, ListError> {
        if self.depth != 0 {
            return Err(ListError::UnclosedClip);
        }
        Ok(DisplayList {
            commands: self.commands,
            items: self.items,
        })
    }
}

/// A display list: at most [`MAX_COMMANDS`] commands with at most
/// [`MAX_ITEMS`] items, in which every [`Command::PushClip`] has its
/// [`Command::PopClip`]. A [`ListBuilder`] builds it, and it does not
/// change afterwards.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DisplayList {
    commands: Vec<Command>,
    items: usize,
}

impl DisplayList {
    /// The commands, in drawing order.
    pub fn commands(&self) -> &[Command] {
        &self.commands
    }

    /// The items: path elements and glyphs.
    pub fn items(&self) -> usize {
        self.items
    }
}

#[cfg(test)]
mod tests {
    use kosmo_types::Size;

    use super::*;

    fn px(pixels: i32) -> Fx {
        Fx::from_pixels(pixels).unwrap()
    }

    fn rect() -> Rect {
        Rect::new(Point::new(px(1), px(1)), Size::new(px(4), px(4)).unwrap())
    }

    fn clip() -> Command {
        Command::PushClip {
            rect: rect(),
            offset: Point::default(),
        }
    }

    #[test]
    fn a_pop_without_a_group_is_refused() {
        let mut builder = ListBuilder::new();
        assert_eq!(
            builder.push(Command::PopClip),
            Err(ListError::UnpairedPopClip)
        );
        assert_eq!(builder, ListBuilder::new());
    }

    #[test]
    fn a_list_with_an_open_group_cannot_be_finished() {
        let mut builder = ListBuilder::new();
        builder.push(clip()).unwrap();
        builder.push(clip()).unwrap();
        builder.push(Command::PopClip).unwrap();
        assert_eq!(builder.depth(), 1);
        assert_eq!(builder.clone().finish(), Err(ListError::UnclosedClip));
        builder.push(Command::PopClip).unwrap();
        assert_eq!(builder.finish().unwrap().commands().len(), 4);
    }

    #[test]
    fn a_negative_radius_or_width_is_refused() {
        let mut builder = ListBuilder::new();
        let radius = Command::FillRoundedRect {
            rect: rect(),
            radius: Fx::from_units(-1),
            color: Color::TRANSPARENT,
        };
        assert_eq!(builder.push(radius), Err(ListError::NegativeRadius));
        let width = Command::StrokePath {
            path: Path::new(Point::default()),
            width: Fx::from_units(-1),
            join: Join::Round,
            cap: Cap::Round,
            color: Color::TRANSPARENT,
        };
        assert_eq!(builder.push(width), Err(ListError::NegativeWidth));
        assert_eq!(builder, ListBuilder::new());
    }

    #[test]
    fn paths_and_glyphs_count_as_items() {
        let mut path = Path::new(Point::default());
        path.line_to(Point::default()).close();
        let glyph = Glyph {
            id: GlyphId::new(1),
            origin: Point::default(),
        };
        let mut builder = ListBuilder::new();
        builder
            .push(Command::FillPath {
                path,
                color: Color::TRANSPARENT,
            })
            .unwrap();
        builder
            .push(Command::Glyphs {
                font: FontId::new(0),
                size: px(12),
                color: Color::TRANSPARENT,
                glyphs: vec![glyph; 2],
            })
            .unwrap();
        builder.push(clip()).unwrap();
        builder.push(Command::PopClip).unwrap();
        assert_eq!(builder.finish().unwrap().items(), 5);
    }

    #[test]
    fn a_color_component_beyond_alpha_is_refused() {
        assert_eq!(Color::premultiplied(0, 0, 11, 10), None);
        assert_eq!(
            Color::premultiplied(10, 0, 10, 10).map(Color::to_bytes),
            Some([10, 0, 10, 10])
        );
        assert_eq!(Color::opaque(1, 2, 3).alpha(), 255);
    }
}
