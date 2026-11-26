use kosmo_render::{Color, Command, MAX_COMMANDS};
use kosmo_types::{BufferDesc, PixelFormat};

use super::*;
use crate::app::{Message, Out};
use crate::hit::LayoutBox;
use crate::testing::TestPort;
use crate::tree::{Key, Kind, View};

/// A control that fills its box with a shade of red, fills it a number of
/// times, draws nothing, or misuses groups.
#[derive(Clone, Debug, PartialEq)]
enum Painter {
    Fill(u8),
    Many(usize),
    Nothing,
    Pops,
    Opens,
}

struct Painting;

impl App for Painting {
    type Message = ();
    type Control = Painter;
    type Port = TestPort;

    fn update(&mut self, _: Message<Self>, _: &mut Out<Self>) {}

    fn view(&self) -> View<Painter> {
        View::new(Key::new(0), Painter::Nothing)
    }
}

impl Control<Painting> for Painter {
    type State = ();

    fn kind(&self) -> Kind {
        Kind::new("painter")
    }

    fn draw(&self, cx: &mut DrawContext<'_, ()>) {
        match *self {
            Painter::Fill(shade) => cx.draw(Command::FillRect {
                rect: Rect::new(Point::default(), cx.size()),
                color: Color::opaque(shade, 0, 0),
            }),
            Painter::Many(times) => {
                for _ in 0..times {
                    cx.draw(Command::FillRect {
                        rect: Rect::new(Point::default(), cx.size()),
                        color: Color::opaque(1, 0, 0),
                    });
                }
            }
            Painter::Nothing => {}
            Painter::Pops => cx.draw(Command::PopClip),
            Painter::Opens => cx.draw(Command::PushClip {
                rect: Rect::new(Point::default(), cx.size()),
                offset: Point::default(),
            }),
        }
    }
}

fn px(pixels: i32) -> Fx {
    Fx::from_pixels(pixels).unwrap()
}

fn point(x: i32, y: i32) -> Point {
    Point::new(px(x), px(y))
}

fn rect(x: i32, y: i32, width: i32, height: i32) -> Rect {
    Rect::new(point(x, y), Size::new(px(width), px(height)).unwrap())
}

/// A tree of a root with the children `a` and `b`, and `a` with the child
/// `a1`, of the controls `controls` in preorder, and their identities in
/// preorder.
fn build(controls: [Painter; 4]) -> (Tree<Painting>, [WidgetId; 4]) {
    let [root, a, a1, b] = controls;
    let view = View::new(Key::new(0), root)
        .child(View::new(Key::new(1), a).child(View::new(Key::new(2), a1)))
        .child(View::new(Key::new(3), b));
    let mut tree = Tree::new();
    tree.rebuild(&view).unwrap();
    let ids = tree.preorder();
    (tree, [ids[0], ids[1], ids[2], ids[3]])
}

fn filling() -> (Tree<Painting>, [WidgetId; 4]) {
    build([
        Painter::Fill(1),
        Painter::Fill(2),
        Painter::Fill(3),
        Painter::Fill(4),
    ])
}

/// The boxes: the root of 100 by 80 pixels, `a` at 10, 10 scrolled down by
/// 5 pixels, `a1` at the top left of `a`'s content and `b` at 60, 10.
fn boxes(ids: [WidgetId; 4]) -> [LayoutBox; 4] {
    let at = |widget, parent, rect, scroll| LayoutBox {
        widget,
        parent,
        rect,
        scroll,
        draws: true,
    };
    [
        at(ids[0], None, rect(0, 0, 100, 80), Point::default()),
        at(ids[1], Some(0), rect(10, 10, 40, 30), point(0, 5)),
        at(ids[2], Some(1), rect(0, 0, 20, 10), Point::default()),
        at(ids[3], Some(0), rect(60, 10, 30, 30), Point::default()),
    ]
}

fn layout(boxes: &[LayoutBox]) -> LayoutResult {
    let mut layout = LayoutResult::new();
    for &layout_box in boxes {
        layout.push(layout_box).unwrap();
    }
    layout
}

/// The damage region of a surface of 100 by 80 pixels with `rects`.
fn damage(rects: &[(u32, u32, u32, u32)]) -> Damage {
    let desc = BufferDesc::new(100, 80, 400, PixelFormat::Rgba8Premul).unwrap();
    let mut damage = Damage::new(desc);
    for &(x, y, width, height) in rects {
        damage.add(x, y, width, height);
    }
    damage
}

fn clip(rect: Rect, offset: Point) -> Command {
    Command::PushClip { rect, offset }
}

/// The commands of a control that fills its box `rect` with `shade`.
fn filled(rect: Rect, shade: u8) -> [Command; 3] {
    [
        clip(rect, rect.origin()),
        Command::FillRect {
            rect: Rect::new(Point::default(), rect.size()),
            color: Color::opaque(shade, 0, 0),
        },
        Command::PopClip,
    ]
}

#[test]
fn every_control_draws_in_preorder_relative_to_its_box() {
    let (tree, ids) = filling();
    let list = paint(&tree, &layout(&boxes(ids)), &damage(&[(0, 0, 100, 80)])).unwrap();
    let mut expected = Vec::new();
    expected.extend(filled(rect(0, 0, 100, 80), 1));
    expected.push(clip(rect(0, 0, 100, 80), point(0, 0)));
    expected.extend(filled(rect(10, 10, 40, 30), 2));
    // The children of `a` are moved by its position and its scroll offset.
    expected.push(clip(rect(10, 10, 40, 30), point(10, 5)));
    expected.extend(filled(rect(0, 0, 20, 10), 3));
    expected.push(Command::PopClip);
    expected.extend(filled(rect(60, 10, 30, 30), 4));
    expected.push(Command::PopClip);
    assert_eq!(list.commands(), expected);
}

#[test]
fn controls_outside_the_damage_are_skipped_with_their_descendants() {
    let (tree, ids) = filling();
    let layout = layout(&boxes(ids));
    // Only the root and `b`.
    let list = paint(&tree, &layout, &damage(&[(70, 20, 5, 5)])).unwrap();
    let mut expected = Vec::new();
    expected.extend(filled(rect(0, 0, 100, 80), 1));
    expected.push(clip(rect(0, 0, 100, 80), point(0, 0)));
    expected.extend(filled(rect(60, 10, 30, 30), 4));
    expected.push(Command::PopClip);
    assert_eq!(list.commands(), expected);
    // `a1` is visible from 10, 10 to 30, 15 on the surface; below it only
    // `a` is.
    let list = paint(&tree, &layout, &damage(&[(29, 14, 1, 1)])).unwrap();
    assert_eq!(list.commands().len(), 13);
    let list = paint(&tree, &layout, &damage(&[(20, 15, 1, 1)])).unwrap();
    assert_eq!(list.commands().len(), 8);
    assert!(
        !list
            .commands()
            .contains(&clip(rect(10, 10, 40, 30), point(10, 5)))
    );
    let list = paint(&tree, &layout, &damage(&[])).unwrap();
    assert!(list.commands().is_empty());
}

#[test]
fn a_control_that_draws_nothing_adds_no_group() {
    let (tree, ids) = build([
        Painter::Nothing,
        Painter::Fill(2),
        Painter::Nothing,
        Painter::Fill(4),
    ]);
    let mut boxes = boxes(ids);
    boxes[1].draws = false;
    let list = paint(&tree, &layout(&boxes), &damage(&[(0, 0, 100, 80)])).unwrap();
    let mut expected = vec![
        clip(rect(0, 0, 100, 80), point(0, 0)),
        clip(rect(10, 10, 40, 30), point(10, 5)),
        Command::PopClip,
    ];
    expected.extend(filled(rect(60, 10, 30, 30), 4));
    expected.push(Command::PopClip);
    // `a` does not draw, `a1` draws nothing, and the group of `a`'s
    // children holds no command.
    assert_eq!(list.commands(), expected);
}

#[test]
fn a_box_out_of_preorder_is_refused() {
    let (tree, ids) = filling();
    let [root, a, a1, b] = boxes(ids);
    let out_of_order = layout(&[
        root,
        a,
        b,
        LayoutBox {
            parent: Some(1),
            ..a1
        },
    ]);
    assert_eq!(
        paint(&tree, &out_of_order, &damage(&[(0, 0, 100, 80)])),
        Err(PaintError::Layout(LayoutError::Order))
    );
}

#[test]
fn a_box_of_a_control_that_does_not_exist_is_refused() {
    let (tree, ids) = filling();
    let mut boxes = boxes(ids);
    boxes[3].widget = WidgetId::for_test(99);
    assert_eq!(
        paint(&tree, &layout(&boxes), &damage(&[(0, 0, 100, 80)])),
        Err(PaintError::MissingControl(WidgetId::for_test(99)))
    );
}

#[test]
fn a_control_ends_only_the_groups_it_starts() {
    let (tree, ids) = build([
        Painter::Fill(1),
        Painter::Pops,
        Painter::Nothing,
        Painter::Nothing,
    ]);
    assert_eq!(
        paint(&tree, &layout(&boxes(ids)), &damage(&[(0, 0, 100, 80)])),
        Err(PaintError::List(ListError::UnpairedPopClip))
    );
    let (tree, ids) = build([
        Painter::Fill(1),
        Painter::Nothing,
        Painter::Opens,
        Painter::Nothing,
    ]);
    assert_eq!(
        paint(&tree, &layout(&boxes(ids)), &damage(&[(0, 0, 100, 80)])),
        Err(PaintError::List(ListError::UnclosedClip))
    );
}

/// The group around a control's commands counts toward the limit of the
/// display list: with the root's box alone, the start of the root's group
/// and its 16,383 commands fill the list, and the end of the group is
/// refused.
#[test]
fn the_groups_count_toward_the_limit_of_the_list() {
    let (tree, ids) = build([
        Painter::Many(MAX_COMMANDS - 1),
        Painter::Nothing,
        Painter::Nothing,
        Painter::Nothing,
    ]);
    assert_eq!(
        paint(
            &tree,
            &layout(&boxes(ids)[..1]),
            &damage(&[(0, 0, 100, 80)])
        ),
        Err(PaintError::List(ListError::TooManyCommands))
    );
}
