//! The bounds of a display list: the command after the last allowed one,
//! and a command that brings the path elements and glyphs beyond their
//! bound, are refused, and the list stays as it was.

use kosmo_render::{
    Color, Command, FontId, Glyph, GlyphId, ListBuilder, ListError, MAX_COMMANDS, MAX_ITEMS, Path,
};
use kosmo_types::{Fx, Point, Rect};

fn fill() -> Command {
    Command::FillRect {
        rect: Rect::default(),
        color: Color::opaque(1, 2, 3),
    }
}

/// A path of `elements` elements.
fn path(elements: usize) -> Path {
    let mut path = Path::new(Point::default());
    for _ in 1..elements {
        path.line_to(Point::default());
    }
    path
}

#[test]
fn the_command_after_the_last_allowed_one_is_refused_and_the_list_stays() {
    let mut builder = ListBuilder::new();
    for _ in 0..MAX_COMMANDS {
        builder.push(fill()).unwrap();
    }
    let full = builder.clone();
    assert_eq!(builder.push(fill()), Err(ListError::TooManyCommands));
    assert_eq!(
        builder.push(Command::PushClip {
            rect: Rect::default(),
            offset: Point::default()
        }),
        Err(ListError::TooManyCommands)
    );
    assert_eq!(builder, full);
    assert_eq!(builder.finish().unwrap().commands().len(), MAX_COMMANDS);
}

#[test]
fn a_command_beyond_the_items_is_refused_and_the_list_stays() {
    let mut builder = ListBuilder::new();
    builder
        .push(Command::FillPath {
            path: path(MAX_ITEMS - 2),
            color: Color::TRANSPARENT,
        })
        .unwrap();
    let glyphs = |count| Command::Glyphs {
        font: FontId::new(0),
        size: Fx::from_units(768),
        color: Color::TRANSPARENT,
        glyphs: vec![
            Glyph {
                id: GlyphId::new(0),
                origin: Point::default()
            };
            count
        ],
    };
    let before = builder.clone();
    assert_eq!(builder.push(glyphs(3)), Err(ListError::TooManyItems));
    assert_eq!(
        builder.push(Command::StrokePath {
            path: path(3),
            width: Fx::ZERO,
            join: kosmo_render::Join::Miter,
            cap: kosmo_render::Cap::Butt,
            color: Color::TRANSPARENT,
        }),
        Err(ListError::TooManyItems)
    );
    assert_eq!(builder, before);
    builder.push(glyphs(2)).unwrap();
    assert_eq!(builder.items(), MAX_ITEMS);
    // Commands without items still fit.
    builder.push(fill()).unwrap();
    assert_eq!(builder.finish().unwrap().items(), MAX_ITEMS);
}
