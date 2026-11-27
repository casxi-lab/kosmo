//! The text form of display lists: one command per line, with geometry as
//! the integers of the 26.6 fixed point and colors in hexadecimal, so that
//! records and tests compare display lists as text, byte for byte.

use alloc::string::{String, ToString};
use core::fmt;

use kosmo_types::{Point, Rect};

use crate::list::{Cap, Color, Command, DisplayList, Join, Path, PathElement};

impl DisplayList {
    /// The list as text: one line for each command, in drawing order, each
    /// followed by a line feed, so an empty list has no text. Every
    /// coordinate, size, radius and width is written as the integer of its
    /// [`Fx`](kosmo_types::Fx), in 1/64 pixel, and every color as `#` and
    /// the 8 lowercase hexadecimal digits of its premultiplied red, green,
    /// blue and alpha; nothing is written as a floating-point number. A
    /// point is `<x>,<y>`, a size `<width>x<height>`, and a rectangle its
    /// top-left corner and its size, `<x>,<y> <width>x<height>`. The lines
    /// are:
    ///
    /// - `fill-rect <color> <rect>`;
    /// - `fill-rounded-rect <color> <rect> radius <radius>`;
    /// - `fill-path <color>` and the path's elements;
    /// - `stroke-path <color> width <width> join <join> cap <cap>` and the
    ///   path's elements, with the join `miter`, `round` or `bevel` and the
    ///   cap `butt`, `round` or `square`;
    /// - `glyphs <color> font <font> size <size>` and, for each glyph,
    ///   ` <glyph>@<origin>`, the glyph's number and its origin;
    /// - `push-clip <rect> offset <point>`;
    /// - `pop-clip`.
    ///
    /// The elements of a path are ` M <point>`, ` L <point>`,
    /// ` Q <control> <point>`, ` C <first> <second> <point>` and ` Z` for
    /// a move, a line, a quadratic and a cubic segment and a close.
    ///
    /// The text depends only on the list. A line holds at most 92 bytes,
    /// its line feed included, and at most 74 more for each path element or
    /// glyph, so the text of a list, with at most
    /// [`MAX_COMMANDS`](crate::MAX_COMMANDS) commands and
    /// [`MAX_ITEMS`](crate::MAX_ITEMS) items, has at most
    /// 92 × 16,384 + 74 × 65,536 bytes, about 6.1 MiB.
    ///
    /// ```
    /// use kosmo_render::{Color, Command, ListBuilder};
    /// use kosmo_types::{Fx, Point, Rect, Size};
    ///
    /// let px = |pixels| Fx::from_pixels(pixels).unwrap();
    /// let rect = Rect::new(Point::new(px(1), px(2)), Size::new(px(3), px(4)).unwrap());
    /// let mut builder = ListBuilder::new();
    /// builder.push(Command::PushClip { rect, offset: Point::new(px(1), Fx::from_units(-32)) }).unwrap();
    /// builder.push(Command::FillRect { rect, color: Color::opaque(255, 0, 0) }).unwrap();
    /// builder.push(Command::PopClip).unwrap();
    /// assert_eq!(
    ///     builder.finish().unwrap().text(),
    ///     "push-clip 64,128 192x256 offset 64,-32\n\
    ///      fill-rect #ff0000ff 64,128 192x256\n\
    ///      pop-clip\n"
    /// );
    /// ```
    pub fn text(&self) -> String {
        Text(self).to_string()
    }
}

/// The text of a display list, written as [`DisplayList::text`] states.
struct Text<'a>(&'a DisplayList);

impl fmt::Display for Text<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for command in self.0.commands() {
            match command {
                Command::FillRect { rect, color } => {
                    write!(f, "fill-rect {} {}", Hex(*color), Area(*rect))?;
                }
                Command::FillRoundedRect {
                    rect,
                    radius,
                    color,
                } => write!(
                    f,
                    "fill-rounded-rect {} {} radius {}",
                    Hex(*color),
                    Area(*rect),
                    radius.units()
                )?,
                Command::FillPath { path, color } => {
                    write!(f, "fill-path {}{}", Hex(*color), Elements(path))?;
                }
                Command::StrokePath {
                    path,
                    width,
                    join,
                    cap,
                    color,
                } => write!(
                    f,
                    "stroke-path {} width {} join {} cap {}{}",
                    Hex(*color),
                    width.units(),
                    join_name(*join),
                    cap_name(*cap),
                    Elements(path)
                )?,
                Command::Glyphs {
                    font,
                    size,
                    color,
                    glyphs,
                } => {
                    write!(
                        f,
                        "glyphs {} font {} size {}",
                        Hex(*color),
                        font.get(),
                        size.units()
                    )?;
                    for glyph in glyphs {
                        write!(f, " {}@{}", glyph.id.get(), At(glyph.origin))?;
                    }
                }
                Command::PushClip { rect, offset } => {
                    write!(f, "push-clip {} offset {}", Area(*rect), At(*offset))?;
                }
                Command::PopClip => f.write_str("pop-clip")?,
            }
            f.write_str("\n")?;
        }
        Ok(())
    }
}

/// A color as `#` and its four components in hexadecimal.
struct Hex(Color);

impl fmt::Display for Hex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let [red, green, blue, alpha] = self.0.to_bytes();
        write!(f, "#{red:02x}{green:02x}{blue:02x}{alpha:02x}")
    }
}

/// A point as `<x>,<y>`.
struct At(Point);

impl fmt::Display for At {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{},{}", self.0.x.units(), self.0.y.units())
    }
}

/// A rectangle as `<x>,<y> <width>x<height>`.
struct Area(Rect);

impl fmt::Display for Area {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let size = self.0.size();
        write!(
            f,
            "{} {}x{}",
            At(self.0.origin()),
            size.width().units(),
            size.height().units()
        )
    }
}

/// The elements of a path, each after a space.
struct Elements<'a>(&'a Path);

impl fmt::Display for Elements<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for element in self.0.elements() {
            match *element {
                PathElement::MoveTo(to) => write!(f, " M {}", At(to))?,
                PathElement::LineTo(to) => write!(f, " L {}", At(to))?,
                PathElement::QuadTo(control, to) => write!(f, " Q {} {}", At(control), At(to))?,
                PathElement::CubicTo(first, second, to) => {
                    write!(f, " C {} {} {}", At(first), At(second), At(to))?;
                }
                PathElement::Close => f.write_str(" Z")?,
            }
        }
        Ok(())
    }
}

fn join_name(join: Join) -> &'static str {
    match join {
        Join::Miter => "miter",
        Join::Round => "round",
        Join::Bevel => "bevel",
    }
}

fn cap_name(cap: Cap) -> &'static str {
    match cap {
        Cap::Butt => "butt",
        Cap::Round => "round",
        Cap::Square => "square",
    }
}

#[cfg(test)]
mod tests {
    use alloc::vec;

    use kosmo_types::{Fx, Size};

    use super::*;
    use crate::list::{FontId, Glyph, GlyphId, ListBuilder};

    fn at(x: i32, y: i32) -> Point {
        Point::new(Fx::from_units(x), Fx::from_units(y))
    }

    fn rect(x: i32, y: i32, width: i32, height: i32) -> Rect {
        Rect::new(
            at(x, y),
            Size::new(Fx::from_units(width), Fx::from_units(height)).unwrap(),
        )
    }

    fn list(commands: impl IntoIterator<Item = Command>) -> DisplayList {
        let mut builder = ListBuilder::new();
        for command in commands {
            builder.push(command).unwrap();
        }
        builder.finish().unwrap()
    }

    #[test]
    fn every_command_kind_has_its_line() {
        let mut path = Path::new(at(0, -64));
        path.line_to(at(640, 0))
            .quad_to(at(1, 2), at(3, 4))
            .cubic_to(at(5, 6), at(7, 8), at(9, 10))
            .move_to(at(-1, -2))
            .close();
        let half_red = Color::premultiplied(128, 0, 0, 128).unwrap();
        let commands = [
            Command::PushClip {
                rect: rect(64, 64, 640, 320),
                offset: at(-32, 16),
            },
            Command::FillRect {
                rect: rect(0, 0, 128, 64),
                color: Color::opaque(255, 0, 0),
            },
            Command::FillRoundedRect {
                rect: rect(-64, 0, 0, 64),
                radius: Fx::from_units(16),
                color: half_red,
            },
            Command::FillPath {
                path: path.clone(),
                color: Color::opaque(0, 171, 205),
            },
            Command::StrokePath {
                path: Path::new(at(0, 0)),
                width: Fx::from_units(96),
                join: Join::Miter,
                cap: Cap::Butt,
                color: Color::TRANSPARENT,
            },
            Command::StrokePath {
                path: path.clone(),
                width: Fx::ZERO,
                join: Join::Round,
                cap: Cap::Round,
                color: Color::opaque(1, 2, 3),
            },
            Command::StrokePath {
                path: Path::new(at(1, 1)),
                width: Fx::from_units(64),
                join: Join::Bevel,
                cap: Cap::Square,
                color: Color::opaque(1, 2, 3),
            },
            Command::Glyphs {
                font: FontId::new(1),
                size: Fx::from_units(768),
                color: Color::opaque(0, 0, 0),
                glyphs: vec![
                    Glyph {
                        id: GlyphId::new(36),
                        origin: at(0, 640),
                    },
                    Glyph {
                        id: GlyphId::new(4_294_967_295),
                        origin: at(456, 640),
                    },
                ],
            },
            Command::Glyphs {
                font: FontId::new(0),
                size: Fx::from_units(1),
                color: Color::opaque(0, 0, 0),
                glyphs: vec![],
            },
            Command::PopClip,
        ];
        assert_eq!(
            list(commands).text(),
            "push-clip 64,64 640x320 offset -32,16\n\
             fill-rect #ff0000ff 0,0 128x64\n\
             fill-rounded-rect #80000080 -64,0 0x64 radius 16\n\
             fill-path #00abcdff M 0,-64 L 640,0 Q 1,2 3,4 C 5,6 7,8 9,10 M -1,-2 Z\n\
             stroke-path #00000000 width 96 join miter cap butt M 0,0\n\
             stroke-path #010203ff width 0 join round cap round M 0,-64 L 640,0 Q 1,2 3,4 C 5,6 7,8 9,10 M -1,-2 Z\n\
             stroke-path #010203ff width 64 join bevel cap square M 1,1\n\
             glyphs #000000ff font 1 size 768 36@0,640 4294967295@456,640\n\
             glyphs #000000ff font 0 size 1\n\
             pop-clip\n"
        );
    }

    #[test]
    fn an_empty_list_has_no_text() {
        assert_eq!(DisplayList::default().text(), "");
        assert_eq!(list([]).text(), "");
    }

    #[test]
    fn lines_stay_within_their_bound_at_the_extremes() {
        // The widest values: the smallest i32 for coordinates, the largest
        // for sizes, radii, widths, fonts and glyphs.
        let min = Fx::from_units(i32::MIN);
        let max = Fx::from_units(i32::MAX);
        let far = Point::new(min, min);
        let widest = Rect::new(far, Size::new(max, max).unwrap());
        let mut cubic = Path::new(far);
        cubic.cubic_to(far, far, far);
        let commands = [
            Command::FillRect {
                rect: widest,
                color: Color::opaque(255, 255, 255),
            },
            Command::FillRoundedRect {
                rect: widest,
                radius: max,
                color: Color::opaque(255, 255, 255),
            },
            Command::FillPath {
                path: cubic.clone(),
                color: Color::opaque(255, 255, 255),
            },
            Command::StrokePath {
                path: cubic,
                width: max,
                join: Join::Miter,
                cap: Cap::Square,
                color: Color::opaque(255, 255, 255),
            },
            Command::Glyphs {
                font: FontId::new(u32::MAX),
                size: max,
                color: Color::opaque(255, 255, 255),
                glyphs: vec![
                    Glyph {
                        id: GlyphId::new(u32::MAX),
                        origin: far,
                    };
                    2
                ],
            },
            Command::PushClip {
                rect: widest,
                offset: far,
            },
            Command::PopClip,
        ];
        let lengths: alloc::vec::Vec<usize> = list(commands)
            .text()
            .split_inclusive('\n')
            .map(str::len)
            .collect();
        // Each line's own bytes, line feed included, and 74 for each cubic
        // segment or glyph; the move of the paths counts as an item too.
        assert_eq!(
            lengths,
            [66, 92, 20 + 26 + 74, 61 + 26 + 74, 49 + 2 * 35, 87, 9]
        );
        let items = [0, 0, 2, 2, 2, 0, 0];
        for (length, items) in lengths.iter().zip(items) {
            assert!(*length <= 92 + 74 * items, "{length} for {items} items");
        }
    }
}
