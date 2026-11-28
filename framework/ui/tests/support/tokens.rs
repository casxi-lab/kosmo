//! The scene of the design tokens: a column of text leaves and a rule that
//! together use every token of a table, laid out and drawn by a round on
//! a surface of [`WIDTH`] × [`HEIGHT`] pixels.
//!
//! The scene is a column with padding `L` and spacing `M` of the title
//! "Result" in Fira Sans at size `Large`; a row aligned by baseline with
//! spacing `Xs` of "x = 1/3" in Fira Mono at size `Body` and "stale" in
//! Fira Sans at size `Small` in the colour `TextSecondary`; a rule of
//! 208 × 2 pixels, which fills its box with the colour `Accent`; and a row
//! with spacing `S` of "error:" and "division by zero" in Fira Sans at size
//! `Body` in the colour `Error`. Text without a colour of its own has the
//! colour `Text`. The canvas is a tightly packed `Rgba8Premul` buffer of the
//! surface's size, drawn whole by vello_cpu from the coverage masks of the
//! glyph cache: a rectangle of the canvas in the colour `Background`, and
//! over it the display list of the round's drawing pass.
//!
//! The golden [`TREE`] holds the layout tree of the round's layout pass, as
//! `LayoutTree::text` writes it, and the golden [`PIXELS`] the canvas,
//! encoded as the pixel goldens of `kosmo-render` are: the description of
//! the buffer in 16 bytes, its width, height, stride and format code 1 as
//! little-endian `u32`s, followed by the bytes of the buffer.

use kosmo_platform::PixelSize;
use kosmo_render::{Command, ImageStore, ListBuilder, Rasterizer, RenderTarget, VelloRasterizer};
use kosmo_text::FontSet;
use kosmo_types::{BufferDesc, Fx, MonoTime, PixelFormat, Point, Rect, Size};
use kosmo_ui::{
    App, ColorToken, Column, Control, DrawContext, Font, FontSize, Input, Key, Kind, LayoutInput,
    LayoutTree, Message, Node, Out, Row, RowAlign, Runtime, Space, SurfaceId, Text, Tokens, View,
};

use super::layout::{Checked, NoJobs, failed};

/// The name of the layout tree golden of the scene.
pub const TREE: &str = "layout-tokens";

/// The name of the pixel golden of the scene.
pub const PIXELS: &str = "tokens";

/// The width of the surface, in pixels.
pub const WIDTH: u32 = 240;

/// The height of the surface, in pixels.
pub const HEIGHT: u32 = 160;

/// The surface of the scene.
const SURFACE: SurfaceId = SurfaceId::new(1);

/// The format code of `Rgba8Premul` in the encoding.
const RGBA8_PREMUL: u32 = 1;

/// The bytes of the encoded description.
pub const HEADER: usize = 16;

/// A control of the scene: a column, a row, a text leaf or a rule.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Piece {
    Column(Column),
    Row(Row),
    Label(&'static str, Font, FontSize, ColorToken),
    Rule(i32, i32),
}

impl Control<Scene> for Piece {
    type State = ();

    fn kind(&self) -> Kind {
        Kind::new(match self {
            Piece::Column(_) => "column",
            Piece::Row(_) => "row",
            Piece::Label(..) => "label",
            Piece::Rule(..) => "rule",
        })
    }

    #[allow(
        clippy::expect_used,
        reason = "the rule of the scene is a small, positive constant"
    )]
    fn layout(&self, _: &()) -> Node<'_> {
        match self {
            Piece::Column(column) => Node::Column(*column),
            Piece::Row(row) => Node::Row(*row),
            Piece::Label(text, font, size, color) => {
                Node::Text(Text::new(text, *font, *size).color(*color))
            }
            Piece::Rule(width, height) => {
                let size = Size::new(Fx::from_units(width * 64), Fx::from_units(height * 64));
                Node::Fixed(size.expect("the rule of the scene has a size"))
            }
        }
    }

    #[allow(
        clippy::expect_used,
        reason = "a paragraph that the layout pass laid out has positions that fit"
    )]
    fn draw(&self, cx: &mut DrawContext<'_, ()>) {
        match self {
            Piece::Column(_) | Piece::Row(_) => {}
            Piece::Label(..) => {
                if let Some(text) = cx.text() {
                    let color = cx.color(text.color);
                    let run = text.paragraph.run(Point::default(), color);
                    cx.draw(run.expect("the text of the scene has a glyph run"));
                }
            }
            Piece::Rule(..) => {
                let rect = Rect::new(Point::default(), cx.size());
                let color = cx.color(ColorToken::Accent);
                cx.draw(Command::FillRect { rect, color });
            }
        }
    }
}

/// The application whose view is the scene.
pub struct Scene;

impl App for Scene {
    type Message = ();
    type Control = Piece;
    type Port = NoJobs;

    fn update(&mut self, _: Message<Self>, _: &mut Out<Self>) {}

    fn view(&self) -> View<Piece> {
        let label = |key, text, font, size, color| {
            View::new(Key::new(key), Piece::Label(text, font, size, color))
        };
        let reading = Row::new().align(RowAlign::Baseline).spacing(Space::Xs);
        View::new(
            Key::new(0),
            Piece::Column(Column::new().padding(Space::L).spacing(Space::M)),
        )
        .child(label(
            0,
            "Result",
            Font::Sans,
            FontSize::Large,
            ColorToken::Text,
        ))
        .child(
            View::new(Key::new(1), Piece::Row(reading))
                .child(label(
                    0,
                    "x = 1/3",
                    Font::Mono,
                    FontSize::Body,
                    ColorToken::Text,
                ))
                .child(label(
                    1,
                    "stale",
                    Font::Sans,
                    FontSize::Small,
                    ColorToken::TextSecondary,
                )),
        )
        .child(View::new(Key::new(2), Piece::Rule(208, 2)))
        .child(
            View::new(Key::new(3), Piece::Row(Row::new().spacing(Space::S)))
                .child(label(
                    0,
                    "error:",
                    Font::Sans,
                    FontSize::Body,
                    ColorToken::Error,
                ))
                .child(label(
                    1,
                    "division by zero",
                    Font::Sans,
                    FontSize::Body,
                    ColorToken::Error,
                )),
        )
    }
}

/// The scene with the values of `tokens`: the first round of a new runtime
/// lays it out with `fonts` and draws it, and the result is the layout tree
/// of its layout pass and the encoding of the canvas.
pub fn scene(tokens: Tokens, fonts: &mut FontSet) -> Checked<(LayoutTree, Vec<u8>)> {
    let mut app = Scene;
    let mut runtime = Runtime::new();
    let size = PixelSize::new(WIDTH, HEIGHT).ok_or("no surface size")?;
    let input = LayoutInput::new(fonts, 0, tokens).surface(SURFACE, size);
    let output = runtime
        .round(&mut app, Input::new(MonoTime::ZERO))
        .finish_with(&app, input);
    let laid = runtime
        .layout(SURFACE)
        .cloned()
        .ok_or_else(|| format!("the scene was not laid out: {:?}", output.record.lines()))?;
    let [(_, frame, _)] = output.frames.as_slice() else {
        return Err(format!(
            "the scene was not drawn as one frame: {:?}",
            output.record.lines()
        ));
    };
    let canvas = Rect::new(
        Point::default(),
        Size::new(in_units(WIDTH)?, in_units(HEIGHT)?).ok_or("no canvas size")?,
    );
    let mut list = ListBuilder::new();
    list.push(Command::FillRect {
        rect: canvas,
        color: tokens.color(ColorToken::Background),
    })
    .map_err(failed)?;
    for command in frame.commands() {
        list.push(command.clone()).map_err(failed)?;
    }
    let list = list.finish().map_err(failed)?;
    let mut rasterizer = VelloRasterizer::new();
    fonts.prepare(&list, &mut rasterizer).map_err(failed)?;
    let desc =
        BufferDesc::new(WIDTH, HEIGHT, WIDTH * 4, PixelFormat::Rgba8Premul).map_err(failed)?;
    let mut target = RenderTarget::new(desc).ok_or("no canvas")?;
    let whole = target.whole();
    rasterizer
        .render(
            &list,
            &whole,
            fonts.glyphs(),
            &ImageStore::EMPTY,
            &mut target,
        )
        .map_err(failed)?;
    Ok((laid, encode(target.desc(), target.bytes())))
}

/// `pixels` as an `Fx`.
fn in_units(pixels: u32) -> Checked<Fx> {
    i32::try_from(pixels)
        .ok()
        .and_then(Fx::from_pixels)
        .ok_or_else(|| format!("{pixels} pixels do not fit"))
}

/// The encoding of an `Rgba8Premul` buffer of `desc` that holds `pixels`.
fn encode(desc: BufferDesc, pixels: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(HEADER + pixels.len());
    for field in [desc.width(), desc.height(), desc.stride(), RGBA8_PREMUL] {
        bytes.extend_from_slice(&field.to_le_bytes());
    }
    bytes.extend_from_slice(pixels);
    bytes
}

/// The pixel at (`x`, `y`) of the encoding `bytes` of the canvas.
pub fn pixel(bytes: &[u8], x: u32, y: u32) -> Option<[u8; 4]> {
    let at = HEADER + (y * WIDTH * 4 + x * 4) as usize;
    bytes.get(at..at + 4)?.try_into().ok()
}
