//! The layout pass of a round: each surface with a size is laid out
//! under it, only dirty subtrees are laid out again, a scroll request
//! alone moves only its container, and the offsets are written back.

use kosmo_platform::PixelSize;
use kosmo_text::FontSet;
use kosmo_types::{Fx, MonoTime, NamedKey, Point, Size};

use super::super::*;
use crate::app::{Context, DrawContext, Handling, Message, Out, Phase};
use crate::command::{Command, EVALUATE_SELECTION, MOVE_LEFT, MOVE_RIGHT};
use crate::hit::hit_test;
use crate::layout::{Axis, Column, Constraint, Font, Node, Row, Scroll, Text, layout};
use crate::testing::{SURFACE, TestPort, font_set, press, stamp};
use crate::tokens::{FontSize, Space, TOKENS, Tokens};
use crate::tree::{Key, Kind, View};

fn px(pixels: i32) -> Fx {
    Fx::from_pixels(pixels).unwrap()
}

/// A control of the board: a node of each kind. A scroll container along
/// its axis scrolls forward by 30 pixels on `MoveRight` and back by 10 on
/// `MoveLeft`, from the request in its presentation state, and writes back
/// the offset that the layout gives it.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Piece {
    Row,
    Column,
    Text(&'static str),
    Fixed(i32, i32),
    Scroll(Axis),
    /// A scroll container along its axis whose control does not write
    /// the offset back.
    Still(Axis),
}

impl Control<Board> for Piece {
    type State = Fx;

    fn kind(&self) -> Kind {
        Kind::new(match self {
            Piece::Row => "row",
            Piece::Column => "column",
            Piece::Text(_) => "text",
            Piece::Fixed(..) => "fixed",
            Piece::Scroll(_) | Piece::Still(_) => "scroll",
        })
    }

    fn handle(&self, _: Phase, command: &Command, cx: &mut Context<'_, Board, Fx>) -> Handling {
        let step = match (self, command.id) {
            (Piece::Scroll(_), MOVE_RIGHT) => px(30),
            (Piece::Scroll(_), MOVE_LEFT) => px(-10),
            _ => return Handling::Unhandled,
        };
        let request = cx.state().checked_add(step).unwrap();
        *cx.state_mut() = request;
        Handling::Handled
    }

    fn layout(&self, request: &Fx) -> Node<'_> {
        match self {
            Piece::Row => Node::Row(Row::new().spacing(Space::S)),
            Piece::Column => Node::Column(Column::new()),
            Piece::Text(text) => Node::Text(Text::new(text, Font::Sans, FontSize::Body)),
            Piece::Fixed(width, height) => Node::Fixed(Size::new(px(*width), px(*height)).unwrap()),
            Piece::Scroll(axis) | Piece::Still(axis) => Node::Scroll(Scroll::new(*axis, *request)),
        }
    }

    fn scrolled(&self, offset: Fx, request: &mut Fx) {
        if let Piece::Scroll(_) = self {
            *request = offset;
        }
    }

    fn draw(&self, _: &mut DrawContext<'_, Fx>) {}
}

/// The board: its view is whatever the test sets, and it handles
/// `EvaluateSelection`, so that a press of Execute rebuilds the view.
#[derive(Default)]
struct Board {
    view: Option<View<Piece>>,
}

impl App for Board {
    type Message = ();
    type Control = Piece;
    type Port = TestPort;

    fn update(&mut self, _: Message<Self>, _: &mut Out<Self>) {}

    fn view(&self) -> View<Piece> {
        self.view
            .clone()
            .unwrap_or_else(|| View::new(Key::new(0), Piece::Column))
    }

    fn command(&self, command: &Command) -> Option<()> {
        (command.id == EVALUATE_SELECTION).then_some(())
    }
}

/// A view node with `key` of `piece`.
fn piece(key: u64, piece: Piece) -> View<Piece> {
    View::new(Key::new(key), piece)
}

/// A node with `key` of the text leaf `text`.
fn text(key: u64, text: &'static str) -> View<Piece> {
    piece(key, Piece::Text(text))
}

fn size(width: u32, height: u32) -> PixelSize {
    PixelSize::new(width, height).unwrap()
}

/// The board with `view`, its runtime, and the font set.
fn board(view: View<Piece>) -> (Runtime<Board>, Board, FontSet) {
    let app = Board { view: Some(view) };
    (Runtime::new(), app, font_set())
}

/// The focus in of the test surface, with which a round starts.
fn focus_in() -> Event {
    Event::FocusIn {
        surface: SURFACE,
        stamp: stamp(1, 1),
    }
}

/// Runs a round with `events` whose layout pass reads `layout`, and
/// returns its output.
fn laid_out(
    runtime: &mut Runtime<Board>,
    app: &mut Board,
    events: Vec<Event>,
    layout: LayoutInput<'_>,
) -> Output {
    let mut input = Input::new(MonoTime::ZERO);
    input.events = events;
    runtime.round(app, input).finish_with(app, layout)
}

/// The input that lays the test surface out at `size` with `fonts`.
fn on(fonts: &mut FontSet, size: PixelSize) -> LayoutInput<'_> {
    LayoutInput::new(fonts, 0, TOKENS).surface(SURFACE, size)
}

/// The layout lines of `output`'s record.
fn layout_lines(output: &Output) -> Vec<&str> {
    output
        .record
        .lines()
        .iter()
        .map(String::as_str)
        .filter(|line| line.starts_with("layout "))
        .collect()
}

/// A press of Execute on the test surface, which rebuilds the view.
fn execute() -> Event {
    press(NamedKey::EXECUTE, stamp(1, 1))
}

/// The tight constraint of `size`.
fn tight(size: PixelSize) -> Constraint {
    let width = px(i32::try_from(size.width()).unwrap());
    let height = px(i32::try_from(size.height()).unwrap());
    Constraint::tight(Size::new(width, height).unwrap())
}

/// Checks that the layout that the round kept for the test surface is
/// that of the whole tree at `size`.
fn assert_whole(runtime: &Runtime<Board>, fonts: &mut FontSet, size: PixelSize) {
    assert_whole_on(runtime, fonts, (SURFACE, size));
}

/// Checks that the layout that the round kept for the surface of
/// `surface` is that of the whole tree at its size.
fn assert_whole_on(runtime: &Runtime<Board>, fonts: &mut FontSet, surface: (SurfaceId, PixelSize)) {
    let whole = layout(runtime.tree(), tight(surface.1), &TOKENS, fonts).unwrap();
    let kept = runtime.layout(surface.0).unwrap();
    assert_eq!(kept.text(), whole.text());
    assert_eq!(*kept, whole);
}

#[test]
fn each_surface_with_a_size_is_laid_out_under_it_in_the_order_of_the_surfaces() {
    let view = piece(0, Piece::Column)
        .child(text(0, "x"))
        .child(piece(1, Piece::Fixed(10, 10)));
    let (mut runtime, mut app, mut fonts) = board(view);
    let second = SurfaceId::new(2);
    let input = LayoutInput::new(&mut fonts, 0, TOKENS)
        .surface(second, size(30, 20))
        .surface(SURFACE, size(40, 10));
    let output = laid_out(&mut runtime, &mut app, Vec::new(), input);
    // After the view line, one line for each surface in their order, then
    // the deadline line.
    let lines = output.record.lines();
    assert_eq!(
        lines[lines.len() - 4..],
        [
            "view rebuilt controls 3",
            "layout surface 1 boxes 3 laid 3 texts 1",
            "layout surface 2 boxes 3 laid 3 texts 1",
            "deadline none",
        ]
    );
    // The root of each takes the size of its surface as a tight
    // constraint.
    let root = |surface| runtime.layout(surface).unwrap().boxes()[0].constraint;
    assert_eq!(root(SURFACE), tight(size(40, 10)));
    assert_eq!(root(second), tight(size(30, 20)));
    assert!(runtime.layout(SurfaceId::new(3)).is_none());
    // A round without layout input keeps the layouts; a surface that the
    // next layout pass does not list loses its layout.
    let output = runtime
        .round(&mut app, Input::new(MonoTime::ZERO))
        .finish(&app);
    assert!(layout_lines(&output).is_empty());
    assert!(runtime.layout(second).is_some());
    let output = laid_out(
        &mut runtime,
        &mut app,
        Vec::new(),
        on(&mut fonts, size(40, 10)),
    );
    assert_eq!(
        layout_lines(&output),
        ["layout surface 1 boxes 3 laid 0 texts 0"]
    );
    assert!(runtime.layout(second).is_none());
}

#[test]
fn changing_one_text_leaf_lays_out_only_it_and_its_ancestors_up_to_the_boundary() {
    let view = |first| {
        piece(0, Piece::Column)
            .child(
                piece(0, Piece::Row)
                    .child(text(0, first))
                    .child(text(1, "a")),
            )
            .child(
                piece(1, Piece::Column)
                    .child(text(0, "b"))
                    .child(text(1, "c")),
            )
    };
    let (mut runtime, mut app, mut fonts) = board(view("1"));
    let surface = size(200, 100);
    let output = laid_out(
        &mut runtime,
        &mut app,
        vec![focus_in()],
        on(&mut fonts, surface),
    );
    assert_eq!(
        layout_lines(&output),
        ["layout surface 1 boxes 7 laid 7 texts 4"]
    );
    let before = runtime.layout(SURFACE).unwrap().clone();
    // A longer text: the leaf, its row, which grows, and the root, the
    // layout boundary under the surface's tight constraint, are laid out
    // again; the text "a", which moves right, and the column of "b" and
    // "c" are not.
    app.view = Some(view("1234"));
    let output = laid_out(
        &mut runtime,
        &mut app,
        vec![execute()],
        on(&mut fonts, surface),
    );
    assert_eq!(output.view, ViewPass::Rebuilt);
    assert_eq!(
        layout_lines(&output),
        ["layout surface 1 boxes 7 laid 3 texts 1"]
    );
    let after = runtime.layout(SURFACE).unwrap();
    let changed: Vec<usize> = (0..7)
        .filter(|&index| after.boxes()[index] != before.boxes()[index])
        .collect();
    assert_eq!(changed, [1, 2, 3]);
    assert_whole(&runtime, &mut fonts, surface);
}

#[test]
fn a_text_that_a_leaf_holds_lays_out_only_itself() {
    // The text leaf under the fixed-size leaf is laid out with the tight
    // constraint of size zero: its size depends only on that constraint.
    let view = |first| {
        piece(0, Piece::Row)
            .child(piece(0, Piece::Fixed(20, 10)).child(text(0, first)))
            .child(text(1, "x"))
    };
    let (mut runtime, mut app, mut fonts) = board(view("1"));
    let surface = size(200, 100);
    laid_out(
        &mut runtime,
        &mut app,
        vec![focus_in()],
        on(&mut fonts, surface),
    );
    app.view = Some(view("12"));
    let output = laid_out(
        &mut runtime,
        &mut app,
        vec![execute()],
        on(&mut fonts, surface),
    );
    // Only the text leaf: neither the fixed-size leaf above it nor the root.
    assert_eq!(
        layout_lines(&output),
        ["layout surface 1 boxes 4 laid 1 texts 1"]
    );
    assert_whole(&runtime, &mut fonts, surface);
}

#[test]
fn the_work_stops_at_a_box_whose_size_and_baseline_stay() {
    let view = |height| {
        piece(0, Piece::Column)
            .child(
                piece(0, Piece::Row)
                    .child(piece(0, Piece::Fixed(20, height)))
                    .child(piece(1, Piece::Fixed(20, 30))),
            )
            .child(text(1, "x"))
    };
    let (mut runtime, mut app, mut fonts) = board(view(10));
    let surface = size(200, 100);
    laid_out(
        &mut runtime,
        &mut app,
        vec![focus_in()],
        on(&mut fonts, surface),
    );
    // A taller first leaf, still lower than the second: the leaf and its
    // row, whose size stays, so the column above it is not laid out again.
    app.view = Some(view(20));
    let output = laid_out(
        &mut runtime,
        &mut app,
        vec![execute()],
        on(&mut fonts, surface),
    );
    assert_eq!(
        layout_lines(&output),
        ["layout surface 1 boxes 5 laid 2 texts 0"]
    );
    assert_whole(&runtime, &mut fonts, surface);
}

/// A scroll container, focused, with a column of eight text leaves; its
/// content is taller than a surface 50 pixels high.
fn scrolled_view() -> View<Piece> {
    let lines = ["0", "1", "2", "3", "4", "5", "6", "7"];
    let mut column = piece(0, Piece::Column);
    for (key, line) in (0..).zip(lines) {
        column.push(text(key, line));
    }
    piece(0, Piece::Scroll(Axis::Y)).focused().child(column)
}

/// The input that lays out two surfaces, each with its size, with `fonts`.
fn two(
    fonts: &mut FontSet,
    first: (SurfaceId, PixelSize),
    second: (SurfaceId, PixelSize),
) -> LayoutInput<'_> {
    LayoutInput::new(fonts, 0, TOKENS)
        .surface(first.0, first.1)
        .surface(second.0, second.1)
}

/// A press of `key` on the test surface.
fn key(key: NamedKey) -> Event {
    press(key, stamp(1, 1))
}

#[test]
fn a_changed_scroll_request_only_moves_its_container() {
    let (mut runtime, mut app, mut fonts) = board(scrolled_view());
    let surface = size(100, 50);
    let output = laid_out(
        &mut runtime,
        &mut app,
        vec![focus_in()],
        on(&mut fonts, surface),
    );
    assert_eq!(
        layout_lines(&output),
        ["layout surface 1 boxes 10 laid 10 texts 8"]
    );
    let before = runtime.layout(SURFACE).unwrap().clone();
    // The scroll container handles the key and changes only its request:
    // the view pass is skipped, no text is laid out, and the layout tree
    // differs only in the container's offset.
    let output = laid_out(
        &mut runtime,
        &mut app,
        vec![key(NamedKey::RIGHT)],
        on(&mut fonts, surface),
    );
    assert_eq!(output.view, ViewPass::Skipped);
    assert_eq!(
        layout_lines(&output),
        ["layout surface 1 boxes 10 laid 0 texts 0"]
    );
    let after = runtime.layout(SURFACE).unwrap();
    let changed: Vec<usize> = (0..10)
        .filter(|&index| after.boxes()[index] != before.boxes()[index])
        .collect();
    assert_eq!(changed, [0]);
    assert_eq!(after.boxes()[0].scroll, Point::new(Fx::ZERO, px(30)));
    assert_whole(&runtime, &mut fonts, surface);
    // Hit testing reads the layout that the round kept: 5 pixels into the
    // viewport, scrolled by 30, is the line whose box holds 35.
    let result = after.result().unwrap();
    let path = hit_test(&result, Point::new(px(5), px(5)))
        .unwrap()
        .unwrap();
    let line = after
        .boxes()
        .iter()
        .find(|laid| {
            laid.draws
                && laid.position.y <= px(35)
                && px(35) < laid.position.y.checked_add(laid.size.height()).unwrap()
        })
        .unwrap();
    assert_eq!(path.last(), Some(&line.widget));
}

#[test]
fn the_round_writes_offsets_back_so_that_requests_beyond_the_range_do_not_add_up() {
    let (mut runtime, mut app, mut fonts) = board(scrolled_view());
    let (low, high) = (size(100, 50), size(100, 80));
    let second = SurfaceId::new(2);
    let surfaces = [(SURFACE, low), (second, high)];
    laid_out(
        &mut runtime,
        &mut app,
        vec![focus_in()],
        two(&mut fonts, surfaces[0], surfaces[1]),
    );
    let range = |surface| {
        let boxes = runtime.layout(surface).unwrap().boxes();
        boxes[1]
            .size
            .height()
            .checked_sub(boxes[0].size.height())
            .unwrap()
    };
    // The taller surface has the smaller range.
    let (smaller, larger) = (range(second), range(SURFACE));
    assert!(smaller < larger);
    assert!(smaller > px(30) && larger < px(150));
    let scroll = runtime.tree().root().unwrap();
    for _ in 0..5 {
        laid_out(
            &mut runtime,
            &mut app,
            vec![key(NamedKey::RIGHT)],
            two(&mut fonts, surfaces[0], surfaces[1]),
        );
    }
    // Five steps of 30 pixels went beyond both ranges. Each round laid
    // the surfaces out with the request of the round, each bringing it
    // into its own range, and then wrote back the smaller offset, so the
    // request is the end of the smaller range and not 150 pixels. The
    // layouts that the round kept follow the request written back, also
    // on the surface whose range is larger, and are those of the tree.
    assert_eq!(runtime.tree().state(scroll), Some(&smaller));
    let offset =
        |runtime: &Runtime<Board>, surface| runtime.layout(surface).unwrap().boxes()[0].scroll.y;
    assert_eq!(offset(&runtime, second), smaller);
    assert_eq!(offset(&runtime, SURFACE), smaller);
    for surface in surfaces {
        assert_whole_on(&runtime, &mut fonts, surface);
    }
    // One step back moves the content of both at once.
    laid_out(
        &mut runtime,
        &mut app,
        vec![key(NamedKey::LEFT)],
        two(&mut fonts, surfaces[0], surfaces[1]),
    );
    let back = smaller.checked_sub(px(10)).unwrap();
    assert_eq!(offset(&runtime, SURFACE), back);
    assert_eq!(offset(&runtime, second), back);
    assert_eq!(runtime.tree().state(scroll), Some(&back));
    for surface in surfaces {
        assert_whole_on(&runtime, &mut fonts, surface);
    }
}

#[test]
fn every_scroll_container_on_every_surface_follows_the_requests_written_back() {
    // A vertical container at the root holds a column with a horizontal
    // container that writes its offset back and has a second child, one
    // that does not write it back, and one without content that does not.
    let row = || {
        let mut row = piece(0, Piece::Row);
        for index in 0..5 {
            row.push(piece(index, Piece::Fixed(60, 20)));
        }
        row
    };
    let column = piece(0, Piece::Column)
        .child(piece(0, Piece::Fixed(80, 40)))
        .child(
            piece(1, Piece::Scroll(Axis::X))
                .child(row())
                .child(piece(1, Piece::Fixed(10, 10))),
        )
        .child(piece(2, Piece::Still(Axis::X)).child(row()))
        .child(piece(3, Piece::Still(Axis::Y)))
        .child(piece(4, Piece::Fixed(80, 120)));
    let (mut runtime, mut app, mut fonts) = board(piece(0, Piece::Scroll(Axis::Y)).child(column));
    // The ranges on surface 2 are the smaller ones, so the layout of
    // surface 3, which comes after it, follows the requests written back.
    let surfaces = [
        (SurfaceId::new(2), size(150, 80)),
        (SurfaceId::new(3), size(100, 50)),
    ];
    laid_out(
        &mut runtime,
        &mut app,
        Vec::new(),
        two(&mut fonts, surfaces[0], surfaces[1]),
    );
    let root = runtime.tree().root().unwrap();
    let column = runtime.tree().children(root)[0];
    let containers = [
        root,
        runtime.tree().children(column)[1],
        runtime.tree().children(column)[2],
        runtime.tree().children(column)[3],
    ];
    for (requests, written) in [
        ([px(1000); 4], [px(120), px(166), px(1000), px(1000)]),
        (
            [Fx::from_units(-1), px(-50), px(-50), px(-50)],
            [Fx::ZERO, Fx::ZERO, px(-50), px(-50)],
        ),
    ] {
        for (&container, request) in containers.iter().zip(requests) {
            runtime.tree.node_mut(container).unwrap().state = request;
        }
        laid_out(
            &mut runtime,
            &mut app,
            Vec::new(),
            two(&mut fonts, surfaces[0], surfaces[1]),
        );
        let states = containers.map(|container| *runtime.tree().state(container).unwrap());
        assert_eq!(states, written);
        for surface in surfaces {
            assert_whole_on(&runtime, &mut fonts, surface);
        }
    }
}

#[test]
fn a_horizontal_scroll_container_writes_its_offset_back_along_x() {
    // A focused horizontal scroll container, 100 pixels wide, with a row
    // of five leaves 30 pixels wide as its content.
    let mut row = piece(0, Piece::Row);
    for index in 0..5 {
        row.push(piece(index, Piece::Fixed(30, 10)));
    }
    let view = piece(0, Piece::Scroll(Axis::X)).focused().child(row);
    let (mut runtime, mut app, mut fonts) = board(view);
    let surface = size(100, 20);
    laid_out(
        &mut runtime,
        &mut app,
        vec![focus_in()],
        on(&mut fonts, surface),
    );
    let boxes = runtime.layout(SURFACE).unwrap().boxes();
    let range = boxes[1]
        .size
        .width()
        .checked_sub(boxes[0].size.width())
        .unwrap();
    assert!(range > px(30) && range < px(150));
    let scroll = runtime.tree().root().unwrap();
    for _ in 0..5 {
        laid_out(
            &mut runtime,
            &mut app,
            vec![key(NamedKey::RIGHT)],
            on(&mut fonts, surface),
        );
    }
    // Five steps of 30 pixels went beyond the range along `x`: the request
    // written back is the end of the range, and the offset lies along `x`.
    assert_eq!(runtime.tree().state(scroll), Some(&range));
    assert_eq!(
        runtime.layout(SURFACE).unwrap().boxes()[0].scroll,
        Point::new(range, Fx::ZERO)
    );
    assert_whole(&runtime, &mut fonts, surface);
}

#[test]
fn a_new_size_table_of_tokens_or_font_set_lays_the_whole_tree_out_again() {
    let view = piece(0, Piece::Row).child(text(0, "a")).child(text(1, "b"));
    let (mut runtime, mut app, mut fonts) = board(view);
    let mut lay = |fonts: &mut FontSet, fonts_id, tokens, surface| {
        let input = LayoutInput::new(fonts, fonts_id, tokens).surface(SURFACE, surface);
        let output = laid_out(&mut runtime, &mut app, Vec::new(), input);
        layout_lines(&output).concat()
    };
    let whole = "layout surface 1 boxes 3 laid 3 texts 2";
    let nothing = "layout surface 1 boxes 3 laid 0 texts 0";
    let surface = size(200, 100);
    assert_eq!(lay(&mut fonts, 0, TOKENS, surface), whole);
    assert_eq!(lay(&mut fonts, 0, TOKENS, surface), nothing);
    assert_eq!(lay(&mut fonts, 0, TOKENS, size(201, 100)), whole);
    let wider = Tokens {
        space_s: px(5),
        ..TOKENS
    };
    assert_eq!(lay(&mut fonts, 0, wider, size(201, 100)), whole);
    let mut other = font_set();
    assert_eq!(lay(&mut other, 1, wider, size(201, 100)), whole);
    assert_eq!(lay(&mut other, 1, wider, size(201, 100)), nothing);
}

#[test]
fn a_failed_layout_is_recorded_and_leaves_the_surface_without_a_layout() {
    // A leaf one pixel wider than the largest layout input.
    let view = |width| piece(0, Piece::Column).child(piece(0, Piece::Fixed(width, 10)));
    let (mut runtime, mut app, mut fonts) = board(view(10));
    let surface = size(200, 100);
    laid_out(
        &mut runtime,
        &mut app,
        vec![focus_in()],
        on(&mut fonts, surface),
    );
    assert!(runtime.layout(SURFACE).is_some());
    app.view = Some(view((1 << 18) + 1));
    let output = laid_out(
        &mut runtime,
        &mut app,
        vec![execute()],
        on(&mut fonts, surface),
    );
    assert_eq!(
        layout_lines(&output),
        ["layout surface 1 failed input-out-of-range"]
    );
    assert!(runtime.layout(SURFACE).is_none());
    // The next layout, without the leaf, lays the tree out whole.
    app.view = Some(view(10));
    let output = laid_out(
        &mut runtime,
        &mut app,
        vec![execute()],
        on(&mut fonts, surface),
    );
    assert_eq!(
        layout_lines(&output),
        ["layout surface 1 boxes 2 laid 2 texts 0"]
    );
}
