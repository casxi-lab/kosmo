//! The drawing pass of a round: each surface with a layout is drawn whole
//! when its layout or its controls changed since its latest frame, a
//! pending region is drawn by itself, and a drawing pass that fails gives
//! no frame and is recorded.

use kosmo_platform::PixelSize;
use kosmo_render::{Color, Command, DisplayList};
use kosmo_text::FontSet;
use kosmo_types::{BufferDesc, DamageRect, Fx, MonoTime, NamedKey, PixelFormat, Point, Rect, Size};

use super::super::*;
use crate::app::{Context, DrawContext, Handling, Message, Out, Phase};
use crate::command::{Command as KeyCommand, MOVE_LEFT};
use crate::layout::{Font, Node, Row, Text};
use crate::testing::{SURFACE, TestPort, font_set, press, stamp};
use crate::tokens::{ColorToken, FontSize, TOKENS};
use crate::tree::{Key, Kind, View};

fn px(pixels: i32) -> Fx {
    Fx::from_pixels(pixels).unwrap()
}

/// A control of the scene: a row; a fixed-size leaf of 4 by 4 pixels
/// that fills its box with the colour of a token and handles `MoveLeft`
/// by counting it in its presentation state; a text leaf that draws the
/// text that the layout laid out; or a fixed-size leaf that fills its box
/// with a colour given as a value.
#[derive(Clone, Debug, PartialEq)]
enum Part {
    Row,
    Swatch(ColorToken),
    Label(&'static str),
    Literal(Color),
}

impl Control<Scene> for Part {
    type State = u32;

    fn kind(&self) -> Kind {
        Kind::new(match self {
            Part::Row => "row",
            Part::Swatch(_) | Part::Literal(_) => "swatch",
            Part::Label(_) => "label",
        })
    }

    fn handle(&self, _: Phase, command: &KeyCommand, cx: &mut Context<'_, Scene, u32>) -> Handling {
        if matches!(self, Part::Swatch(_)) && command.id == MOVE_LEFT {
            *cx.state_mut() += 1;
            Handling::Handled
        } else {
            Handling::Unhandled
        }
    }

    fn layout(&self, _: &u32) -> Node<'_> {
        match self {
            Part::Row => Node::Row(Row::new()),
            Part::Swatch(_) | Part::Literal(_) => Node::Fixed(Size::new(px(4), px(4)).unwrap()),
            Part::Label(text) => Node::Text(Text::new(text, Font::Sans, FontSize::Body)),
        }
    }

    fn draw(&self, cx: &mut DrawContext<'_, u32>) {
        let rect = Rect::new(Point::default(), cx.size());
        match self {
            Part::Row => {}
            Part::Swatch(token) => {
                let color = cx.color(*token);
                cx.draw(Command::FillRect { rect, color });
            }
            Part::Label(_) => {
                if let Some(text) = cx.text() {
                    let color = cx.color(text.color);
                    cx.draw(text.paragraph.run(Point::default(), color).unwrap());
                }
            }
            Part::Literal(color) => cx.draw(Command::FillRect {
                rect,
                color: *color,
            }),
        }
    }
}

/// The scene: its view is the one that the test gives.
struct Scene {
    view: View<Part>,
}

impl App for Scene {
    type Message = ();
    type Control = Part;
    type Port = TestPort;

    fn update(&mut self, _: Message<Self>, _: &mut Out<Self>) {}

    fn view(&self) -> View<Part> {
        self.view.clone()
    }
}

/// The second surface of the tests.
const SECOND: SurfaceId = SurfaceId::new(2);

fn size(width: u32, height: u32) -> PixelSize {
    PixelSize::new(width, height).unwrap()
}

/// A row of a swatch in the accent, which is the focused control, and
/// the label "Ag".
fn swatch_and_label() -> View<Part> {
    View::new(Key::new(0), Part::Row)
        .region()
        .child(View::new(Key::new(1), Part::Swatch(ColorToken::Accent)).focused())
        .child(View::new(Key::new(2), Part::Label("Ag")))
}

/// Runs a round with `events` whose layout and drawing passes read
/// `input`, and returns its output.
fn drawn(
    runtime: &mut Runtime<Scene>,
    app: &mut Scene,
    events: Vec<Event>,
    input: LayoutInput<'_>,
) -> Output {
    let mut round = Input::new(MonoTime::ZERO);
    round.events = events;
    runtime.round(app, round).finish_with(app, input)
}

/// The input of the two surfaces at `first` and `second`.
fn both(fonts: &mut FontSet, first: PixelSize, second: PixelSize) -> LayoutInput<'_> {
    LayoutInput::new(fonts, 0, TOKENS)
        .surface(SURFACE, first)
        .surface(SECOND, second)
}

/// The damage region of a surface of `size` with `rects`.
fn damage(size: PixelSize, rects: &[(u32, u32, u32, u32)]) -> Damage {
    let desc = BufferDesc::new(
        size.width(),
        size.height(),
        size.width() * 4,
        PixelFormat::Rgba8Premul,
    )
    .unwrap();
    let mut damage = Damage::new(desc);
    for &(x, y, width, height) in rects {
        damage.add(x, y, width, height);
    }
    damage
}

/// The surfaces and damage regions of the frames of `output`.
fn regions(output: &Output) -> Vec<(SurfaceId, Vec<DamageRect>)> {
    output
        .frames
        .iter()
        .map(|(surface, _, damage)| (*surface, damage.rects().to_vec()))
        .collect()
}

fn rect(x: u32, y: u32, width: u32, height: u32) -> DamageRect {
    DamageRect::new(x, y, width, height).unwrap()
}

/// The display list of the swatch and the label on `surface`: the group of
/// the row's children, the swatch's group with its fill in the accent, and
/// the label's group with the glyph run of the paragraph that the layout
/// pass laid out with the round's font set, in the colour of text.
fn expected(runtime: &Runtime<Scene>, surface: SurfaceId) -> Vec<Command> {
    let laid = runtime.layout(surface).unwrap();
    let boxes = laid.boxes();
    let place = |index: usize| Rect::new(boxes[index].position, boxes[index].size);
    let text = boxes[2].text.as_ref().unwrap();
    assert_eq!(text.paragraph.style().size(), TOKENS.font_body);
    vec![
        Command::PushClip {
            rect: place(0),
            offset: Point::default(),
        },
        Command::PushClip {
            rect: place(1),
            offset: place(1).origin(),
        },
        Command::FillRect {
            rect: Rect::new(Point::default(), place(1).size()),
            color: TOKENS.accent,
        },
        Command::PopClip,
        Command::PushClip {
            rect: place(2),
            offset: place(2).origin(),
        },
        text.paragraph.run(Point::default(), TOKENS.text).unwrap(),
        Command::PopClip,
        Command::PopClip,
    ]
}

fn commands(output: &Output, index: usize) -> &[Command] {
    let list: &DisplayList = &output.frames[index].1;
    list.commands()
}

#[test]
fn each_surface_is_drawn_whole_when_its_layout_or_its_controls_changed() {
    let mut fonts = font_set();
    let mut app = Scene {
        view: swatch_and_label(),
    };
    let mut runtime = Runtime::new();
    let (first, second) = (size(40, 20), size(30, 20));

    // The first frame of each surface is whole, in the order of the
    // surfaces, and its controls draw from the layout of their surface.
    let output = drawn(
        &mut runtime,
        &mut app,
        Vec::new(),
        both(&mut fonts, first, second),
    );
    assert_eq!(
        regions(&output),
        [
            (SURFACE, vec![rect(0, 0, 40, 20)]),
            (SECOND, vec![rect(0, 0, 30, 20)])
        ]
    );
    assert_eq!(commands(&output, 0), expected(&runtime, SURFACE));
    assert_eq!(commands(&output, 1), expected(&runtime, SECOND));
    assert!(output.paint_errors.is_empty());

    // Nothing changed: no frame.
    let output = drawn(
        &mut runtime,
        &mut app,
        Vec::new(),
        both(&mut fonts, first, second),
    );
    assert!(output.frames.is_empty());

    // A handler of a control ran: its presentation state may have changed,
    // so both surfaces are drawn whole.
    let events = vec![
        Event::FocusIn {
            surface: SURFACE,
            stamp: stamp(1, 1),
        },
        press(NamedKey::LEFT, stamp(1, 1)),
    ];
    let output = drawn(
        &mut runtime,
        &mut app,
        events,
        both(&mut fonts, first, second),
    );
    assert_eq!(output.frames.len(), 2);
    assert_eq!(
        runtime.tree().state(runtime.tree().focused().unwrap()),
        Some(&1)
    );

    // Another size changes the layout of one surface, which alone is drawn
    // whole.
    let wider = size(48, 20);
    let output = drawn(
        &mut runtime,
        &mut app,
        Vec::new(),
        both(&mut fonts, wider, second),
    );
    assert_eq!(regions(&output), [(SURFACE, vec![rect(0, 0, 48, 20)])]);

    // A pending region of a surface is drawn by itself.
    let mut input = both(&mut fonts, wider, second);
    input
        .pending
        .insert(SECOND, damage(second, &[(1, 2, 3, 4)]));
    let output = drawn(&mut runtime, &mut app, Vec::new(), input);
    assert_eq!(regions(&output), [(SECOND, vec![rect(1, 2, 3, 4)])]);

    // A view that the round rebuilds may change any control's value, so
    // each surface is drawn whole; a surface that the input leaves out has
    // no layout and is not drawn.
    app.view = app
        .view
        .clone()
        .child(View::new(Key::new(3), Part::Label("x")));
    let mut input = Input::new(MonoTime::ZERO);
    input.receipts = vec![Receipt::Accepted(1)];
    let output = runtime.round(&mut app, input).finish_with(
        &app,
        LayoutInput::new(&mut fonts, 0, TOKENS).surface(SECOND, second),
    );
    assert_eq!(output.view, ViewPass::Rebuilt);
    assert_eq!(regions(&output), [(SECOND, vec![rect(0, 0, 30, 20)])]);
}

#[test]
fn a_drawing_pass_that_fails_gives_no_frame_and_is_recorded() {
    let mut fonts = font_set();
    let untokened = Part::Literal(Color::opaque(1, 2, 3));
    let mut app = Scene {
        view: View::new(Key::new(0), Part::Row)
            .child(View::new(Key::new(1), Part::Swatch(ColorToken::Accent)))
            .child(View::new(Key::new(2), untokened)),
    };
    let mut runtime = Runtime::new();
    // A surface whose frame, 4 bytes per pixel, would exceed the buffer
    // limits is not drawn either.
    let input = both(&mut fonts, size(40, 20), size(8_192, 8_192));
    let output = drawn(&mut runtime, &mut app, Vec::new(), input);
    let literal = runtime.tree().children(runtime.tree().root().unwrap())[1];
    assert!(output.frames.is_empty());
    assert_eq!(
        output.paint_errors,
        [
            (SURFACE, PaintError::Untokened(literal)),
            (SECOND, PaintError::Layout(LayoutError::InputOutOfRange))
        ]
    );
    let painted: Vec<&str> = output
        .record
        .lines()
        .iter()
        .map(String::as_str)
        .filter(|line| line.starts_with("paint "))
        .collect();
    assert_eq!(
        painted,
        [
            format!("paint surface 1 failed untokened control {literal}"),
            "paint surface 2 failed input-out-of-range".to_string(),
        ]
    );
    // The failed surface has no frame to keep, so the next round draws it
    // whole again, though nothing changed.
    let output = drawn(
        &mut runtime,
        &mut app,
        Vec::new(),
        LayoutInput::new(&mut fonts, 0, TOKENS).surface(SURFACE, size(40, 20)),
    );
    assert_eq!(
        output.paint_errors,
        [(SURFACE, PaintError::Untokened(literal))]
    );
}
