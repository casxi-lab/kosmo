//! The drawing pass of a round: each surface with a layout is drawn whole
//! when its layout or its controls changed since its latest frame, a
//! pending region is drawn by itself, and a drawing pass that fails, also
//! beyond the command limit of the display list, gives no frame and is
//! recorded.

use kosmo_platform::PixelSize;
use kosmo_render::{Color, Command, DisplayList, ListError, MAX_COMMANDS};
use kosmo_text::FontSet;
use kosmo_types::{BufferDesc, DamageRect, Fx, MonoTime, NamedKey, PixelFormat, Point, Rect, Size};

use super::super::*;
use crate::app::{Context, DrawContext, Handling, Message, Out, Phase};
use crate::command::{Command as KeyCommand, MOVE_LEFT};
use crate::layout::{Column, Font, Node, Row, Text};
use crate::testing::{SURFACE, TestPort, font_set, press, stamp};
use crate::tokens::{ColorToken, FontSize, TOKENS};
use crate::tree::{Key, Kind, MAX_DEPTH, MAX_WIDGETS, View};

fn px(pixels: i32) -> Fx {
    Fx::from_pixels(pixels).unwrap()
}

/// A control of the scene: a row; a column; a fixed-size leaf of 4 by 4
/// pixels that fills its box with the colour of a token and handles
/// `MoveLeft` by counting it in its presentation state; a text leaf that
/// draws the text that the layout laid out; a fixed-size leaf that fills
/// its box with a colour given as a value; or a fixed-size leaf of 1 by 1
/// pixel that fills its box a number of times.
#[derive(Clone, Debug, PartialEq)]
enum Part {
    Row,
    Column,
    Swatch(ColorToken),
    Label(&'static str),
    Literal(Color),
    Many(usize),
}

impl Control<Scene> for Part {
    type State = u32;

    fn kind(&self) -> Kind {
        Kind::new(match self {
            Part::Row => "row",
            Part::Column => "column",
            Part::Swatch(_) | Part::Literal(_) | Part::Many(_) => "swatch",
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
            Part::Column => Node::Column(Column::new()),
            Part::Swatch(_) | Part::Literal(_) => Node::Fixed(Size::new(px(4), px(4)).unwrap()),
            Part::Label(text) => Node::Text(Text::new(text, Font::Sans, FontSize::Body)),
            Part::Many(_) => Node::Fixed(Size::new(px(1), px(1)).unwrap()),
        }
    }

    fn draw(&self, cx: &mut DrawContext<'_, u32>) {
        let rect = Rect::new(Point::default(), cx.size());
        match self {
            Part::Row | Part::Column => {}
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
            Part::Many(times) => {
                let color = cx.color(ColorToken::Accent);
                for _ in 0..*times {
                    cx.draw(Command::FillRect { rect, color });
                }
            }
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

    // A handler of a control took its presentation state mutably, so both
    // surfaces are drawn whole.
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
    // each surface is drawn whole, also when only the swatch's value
    // changes and its layout stays; a surface that the input leaves out
    // has no layout and is not drawn.
    app.view = View::new(Key::new(0), Part::Row)
        .region()
        .child(View::new(Key::new(1), Part::Swatch(ColorToken::Error)).focused())
        .child(View::new(Key::new(2), Part::Label("Ag")));
    let before = runtime.layout(SECOND).unwrap().clone();
    let mut input = Input::new(MonoTime::ZERO);
    input.receipts = vec![Receipt::Accepted(1)];
    let output = runtime.round(&mut app, input).finish_with(
        &app,
        LayoutInput::new(&mut fonts, 0, TOKENS).surface(SECOND, second),
    );
    assert_eq!(output.view, ViewPass::Rebuilt);
    assert_eq!(runtime.layout(SECOND), Some(&before));
    assert_eq!(regions(&output), [(SECOND, vec![rect(0, 0, 30, 20)])]);
    assert!(commands(&output, 0).contains(&Command::FillRect {
        rect: Rect::new(Point::default(), Size::new(px(4), px(4)).unwrap()),
        color: TOKENS.error,
    }));
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

/// The command count of a frame: in a layout of the layout pass only
/// leaves draw, so 56 chains of 63 levels below a row, 3,529 controls and
/// 64 deep, hold 56 leaves that draw and 3,473 boxes with a child, the row
/// and 62 columns in each chain. Each leaf that draws its rectangle `times`
/// times adds `times` commands and a group of 2, and each box with a child a
/// group of 2: 56 × 168 + 2 × 3,473 = 16,354 commands for 166 rectangles,
/// within the limit, and 56 × 169 + 2 × 3,473 = 16,410 for 167, beyond it,
/// which gives no frame and is recorded.
#[test]
fn a_frame_beyond_the_command_limit_gives_no_frame_and_is_recorded() {
    let chains = |times: usize| {
        let mut row = View::new(Key::new(0), Part::Row);
        for chain in 0..56 {
            // The leaf and 61 columns, below the chain's first column.
            let mut node = View::new(Key::new(0), Part::Many(times));
            for _ in 0..61 {
                node = View::new(Key::new(0), Part::Column).child(node);
            }
            row.push(View::new(Key::new(chain), Part::Column).child(node));
        }
        row
    };
    let frame = |times: usize| {
        let mut fonts = font_set();
        let mut app = Scene {
            view: chains(times),
        };
        let mut runtime = Runtime::new();
        let input = LayoutInput::new(&mut fonts, 0, TOKENS).surface(SURFACE, size(64, 8));
        let output = drawn(&mut runtime, &mut app, Vec::new(), input);
        assert_eq!(runtime.tree().len(), 3_529);
        assert!(runtime.tree().len() <= MAX_WIDGETS);
        let depth = runtime.layout(SURFACE).unwrap().boxes().iter().fold(
            Vec::new(),
            |mut depths: Vec<usize>, laid| {
                depths.push(laid.parent.map_or(1, |parent| depths[parent] + 1));
                depths
            },
        );
        assert_eq!(depth.iter().max(), Some(&MAX_DEPTH));
        output
    };
    let within = frame(166);
    assert_eq!(within.frames.len(), 1);
    assert_eq!(commands(&within, 0).len(), 56 * 168 + 2 * 3_473);
    assert!(commands(&within, 0).len() <= MAX_COMMANDS);

    // One rectangle more in each of the 56 leaves exceeds the limit.
    assert!(commands(&within, 0).len() + 56 > MAX_COMMANDS);
    let beyond = frame(167);
    assert!(beyond.frames.is_empty());
    assert_eq!(
        beyond.paint_errors,
        [(SURFACE, PaintError::List(ListError::TooManyCommands))]
    );
    assert!(
        beyond
            .record
            .lines()
            .contains(&"paint surface 1 failed too-many-commands".to_string())
    );
}
