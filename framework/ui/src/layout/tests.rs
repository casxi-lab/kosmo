use kosmo_text::FontSet;
use kosmo_types::{Fx, Point, Size};

use super::*;
use crate::app::{App, Control, DrawContext, Message, Out};
use crate::hit::hit_test;
use crate::layout::{Kept, Work, relayout};
use crate::testing::{TestApp, TestPort, node};
use crate::tokens::{ColorToken, FontSize, Space, TOKENS, Tokens};
use crate::tree::{Key, Kind, Tree, View};

mod cases;
mod random;

use cases::*;

/// The bytes of the two fonts, from the asset directory of `kosmo-text`.
const SANS_BYTES: &[u8] =
    include_bytes!("../../../text/assets/fonts/firasans/FiraSans-Regular.ttf");
const MONO_BYTES: &[u8] =
    include_bytes!("../../../text/assets/fonts/firamono/FiraMono-Regular.ttf");

/// A font set of the two fonts.
fn fonts() -> FontSet {
    FontSet::new(SANS_BYTES, MONO_BYTES).unwrap()
}

/// A control of the layout tests: a node of each kind. A scroll
/// container's request is its presentation state, which `build` sets from
/// the value that the view gives.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Part {
    Row(Row),
    Column(Column),
    Scroll(Axis, Fx),
    Text(String, Font, FontSize, ColorToken),
    Fixed(Size),
}

impl Control<Layouts> for Part {
    type State = Fx;

    fn kind(&self) -> Kind {
        Kind::new(match self {
            Part::Row(_) => "row",
            Part::Column(_) => "column",
            Part::Scroll(..) => "scroll",
            Part::Text(..) => "text",
            Part::Fixed(_) => "fixed",
        })
    }

    fn layout(&self, request: &Fx) -> Node<'_> {
        match self {
            Part::Row(row) => Node::Row(*row),
            Part::Column(column) => Node::Column(*column),
            Part::Scroll(axis, _) => Node::Scroll(Scroll::new(*axis, *request)),
            Part::Text(text, font, size, color) => {
                Node::Text(Text::new(text, *font, *size).color(*color))
            }
            Part::Fixed(size) => Node::Fixed(*size),
        }
    }

    fn draw(&self, _: &mut DrawContext<'_, Fx>) {}
}

/// The application of the layout tests, whose trees the tests build
/// directly.
struct Layouts;

impl App for Layouts {
    type Message = ();
    type Control = Part;
    type Port = TestPort;

    fn update(&mut self, _: Message<Self>, _: &mut Out<Self>) {}

    fn view(&self) -> View<Part> {
        View::new(Key::new(0), Part::Fixed(Size::ZERO))
    }
}

/// A node of a tree to lay out, with its children.
#[derive(Clone, Debug)]
struct Spec {
    part: Part,
    children: Vec<Spec>,
}

/// The view of `spec` with `key`; its children have their indices as
/// keys.
fn view(spec: &Spec, key: u64) -> View<Part> {
    let mut view = View::new(Key::new(key), spec.part.clone());
    for (index, child) in spec.children.iter().enumerate() {
        view.push(self::view(child, index as u64));
    }
    view
}

/// The control tree of `spec`, with the request of each scroll container
/// in its presentation state.
fn build(spec: &Spec) -> Tree<Layouts> {
    let mut tree = Tree::new();
    tree.rebuild(&view(spec, 0)).unwrap();
    for id in tree.preorder() {
        let node = tree.node_mut(id).unwrap();
        if let Part::Scroll(_, request) = &node.control {
            node.state = *request;
        }
    }
    tree
}

/// Applies the view of `spec` to `tree`, which keeps the identities of the
/// controls that stay, and sets the request of each scroll container from
/// its value, as `build` does.
fn rebuild(tree: &mut Tree<Layouts>, spec: &Spec) {
    tree.rebuild(&view(spec, 0)).unwrap();
    for id in tree.preorder() {
        let node = tree.node_mut(id).unwrap();
        if let Part::Scroll(_, request) = &node.control {
            node.state = *request;
        }
    }
}

fn px(pixels: i32) -> Fx {
    Fx::from_pixels(pixels).unwrap()
}

fn size_px(width: i32, height: i32) -> Size {
    Size::new(px(width), px(height)).unwrap()
}

/// The constraint from `min_width` × `min_height` to `max_width` ×
/// `max_height` pixels.
fn constraint(min_width: i32, min_height: i32, max_width: i32, max_height: i32) -> Constraint {
    Constraint::new(
        size_px(min_width, min_height),
        size_px(max_width, max_height),
    )
}

fn row(row: Row, children: Vec<Spec>) -> Spec {
    Spec {
        part: Part::Row(row),
        children,
    }
}

fn column(column: Column, children: Vec<Spec>) -> Spec {
    Spec {
        part: Part::Column(column),
        children,
    }
}

fn scroll(axis: Axis, request: i32, content: Spec) -> Spec {
    Spec {
        part: Part::Scroll(axis, px(request)),
        children: vec![content],
    }
}

fn text(text: &str, font: Font, size: FontSize) -> Spec {
    Spec {
        part: Part::Text(text.to_string(), font, size, ColorToken::Text),
        children: Vec::new(),
    }
}

/// A fixed-size leaf of `width` × `height` pixels.
fn fixed(width: i32, height: i32) -> Spec {
    Spec {
        part: Part::Fixed(size_px(width, height)),
        children: Vec::new(),
    }
}

/// A tree and the constraint of its root.
struct Case {
    spec: Spec,
    constraint: Constraint,
}

fn lay_out(case: &Case, fonts: &mut FontSet) -> LayoutTree {
    layout(&build(&case.spec), case.constraint, &TOKENS, fonts).unwrap()
}

/// The position and size of a box, in units: `[x, y, width, height]`.
fn rect(laid: &TreeBox) -> [i32; 4] {
    [
        laid.position.x.units(),
        laid.position.y.units(),
        laid.size.width().units(),
        laid.size.height().units(),
    ]
}

/// `[x, y, width, height]` in pixels, as units.
fn px4(x: i32, y: i32, width: i32, height: i32) -> [i32; 4] {
    [x * 64, y * 64, width * 64, height * 64]
}

/// The rectangles of the boxes, in preorder.
fn rects(tree: &LayoutTree) -> Vec<[i32; 4]> {
    tree.boxes().iter().map(rect).collect()
}

/// The overflow of each box, in preorder, as `(x, y)`.
fn overflows(tree: &LayoutTree) -> Vec<(bool, bool)> {
    tree.boxes()
        .iter()
        .map(|laid| (laid.overflow.x, laid.overflow.y))
        .collect()
}

/// Where the baseline of box `index` lies in the coordinates of its
/// parent's content.
fn baseline_at(tree: &LayoutTree, index: usize) -> i32 {
    let laid = &tree.boxes()[index];
    laid.position.y.units() + laid.baseline.unwrap().units()
}

#[test]
fn a_row_puts_its_spacing_between_its_children() {
    let tree = lay_out(&row_spacing(), &mut fonts());
    assert_eq!(
        rects(&tree),
        [
            px4(0, 0, 76, 20),
            px4(0, 0, 20, 10),
            px4(28, 0, 30, 20),
            px4(66, 0, 10, 5),
        ]
    );
    assert_eq!(overflows(&tree), [(false, false); 4]);
    assert!(tree.boxes().iter().all(|laid| laid.baseline.is_none()));
}

#[test]
fn rows_align_their_children_at_the_start_centre_or_end() {
    let tree = lay_out(&row_cross(), &mut fonts());
    let rects = rects(&tree);
    assert_eq!(rects[0], px4(0, 0, 40, 98));
    assert_eq!(
        [rects[1], rects[4], rects[7]],
        [px4(0, 0, 40, 30), px4(0, 34, 40, 30), px4(0, 68, 40, 30)]
    );
    assert_eq!(
        [rects[2], rects[5], rects[8]],
        [px4(0, 0, 20, 10), px4(0, 10, 20, 10), px4(0, 20, 20, 10)]
    );
    assert_eq!([rects[3], rects[6], rects[9]], [px4(20, 0, 20, 30); 3]);
    assert_eq!(overflows(&tree), [(false, false); 10]);
}

#[test]
fn a_centred_child_takes_half_its_free_room_rounded_up() {
    let odd = Spec {
        part: Part::Fixed(Size::new(px(20), Fx::from_units(10 * 64 + 1)).unwrap()),
        children: Vec::new(),
    };
    let case = Case {
        constraint: constraint(0, 0, 100, 100),
        spec: row(Row::new().align(RowAlign::Center), vec![odd, fixed(20, 10)]),
    };
    let tree = lay_out(&case, &mut fonts());
    // One unit of free room: half a unit, which rounds up to one.
    assert_eq!(tree.boxes()[2].position.y.units(), 1);
}

#[test]
fn a_row_keeps_its_padding_around_its_children() {
    let tree = lay_out(&row_padding(), &mut fonts());
    assert_eq!(
        rects(&tree),
        [px4(0, 0, 56, 42), px4(16, 16, 10, 10), px4(30, 16, 10, 10)]
    );
    // The children may be as tall as the row's largest height less its
    // padding.
    assert_eq!(tree.boxes()[1].constraint.max.height(), px(100 - 32));
}

#[test]
fn a_row_wider_than_its_largest_width_takes_it_and_marks_overflow_in_x() {
    let tree = lay_out(&row_overflow(), &mut fonts());
    assert_eq!(
        rects(&tree),
        [px4(0, 0, 200, 10), px4(0, 0, 120, 10), px4(128, 0, 120, 10)]
    );
    assert_eq!(
        overflows(&tree),
        [(true, false), (false, false), (false, false)]
    );
    // The second child keeps its size and reaches beyond the row.
    let second = &tree.boxes()[2];
    assert!(second.position.x.units() + second.size.width().units() > px(200).units());
}

#[test]
fn a_row_under_a_tight_constraint_takes_its_size() {
    let tree = lay_out(&row_tight(), &mut fonts());
    assert_eq!(rects(&tree), [px4(0, 0, 300, 40), px4(0, 0, 20, 10)]);
    assert_eq!(overflows(&tree), [(false, false); 2]);
}

#[test]
fn a_column_puts_its_spacing_between_its_children() {
    let tree = lay_out(&column_spacing(), &mut fonts());
    assert_eq!(
        rects(&tree),
        [
            px4(0, 0, 40, 51),
            px4(0, 0, 40, 10),
            px4(0, 18, 20, 20),
            px4(0, 46, 30, 5),
        ]
    );
    assert_eq!(overflows(&tree), [(false, false); 4]);
}

#[test]
fn columns_align_their_children_at_the_start_centre_or_end() {
    let tree = lay_out(&column_cross(), &mut fonts());
    let rects = rects(&tree);
    assert_eq!(rects[0], px4(0, 0, 152, 20));
    assert_eq!(
        [rects[1], rects[4], rects[7]],
        [px4(0, 0, 40, 20), px4(56, 0, 40, 20), px4(112, 0, 40, 20)]
    );
    assert_eq!([rects[2], rects[5], rects[8]], [px4(0, 0, 40, 10); 3]);
    assert_eq!(
        [rects[3], rects[6], rects[9]],
        [px4(0, 10, 20, 10), px4(10, 10, 20, 10), px4(20, 10, 20, 10)]
    );
}

#[test]
fn children_align_in_a_box_larger_than_they_are() {
    // Under the tight constraint the column is 100 pixels wide, wider than
    // its child: the child aligns in the column less its padding of 4
    // pixels on each side.
    for (align, x) in [(Align::Start, 4), (Align::Center, 40), (Align::End, 76)] {
        let case = Case {
            constraint: constraint(100, 50, 100, 50),
            spec: column(
                Column::new().padding(Space::S).align(align),
                vec![fixed(20, 10)],
            ),
        };
        let tree = lay_out(&case, &mut fonts());
        assert_eq!(tree.boxes()[1].position.x, px(x));
    }
}

#[test]
fn text_in_a_column_breaks_at_the_columns_width() {
    let mut fonts = fonts();
    let source = "The result is 1/3 and the error is 0";
    let lines_of = |tree: &LayoutTree| -> Vec<String> {
        let laid = tree.boxes()[1].text.as_ref().unwrap();
        laid.paragraph
            .lines()
            .iter()
            .map(|line| source[line.text()].to_string())
            .collect()
    };
    let tree = lay_out(&column_wrap(), &mut fonts);
    assert_eq!(
        lines_of(&tree),
        ["The result is 1/3 ", "and the error is ", "0"]
    );
    // The column takes the size of the paragraph, at most 120 pixels wide.
    assert_eq!(rects(&tree), [[0, 0, 7391, 3686], [0, 0, 7391, 3686]]);
    assert_eq!(tree.boxes()[1].constraint.max.width(), px(120));
    assert_eq!(overflows(&tree), [(false, false); 2]);
    assert_eq!(tree.boxes()[0].baseline, tree.boxes()[1].baseline);
    assert_eq!(
        tree.boxes()[1].text.as_ref().unwrap().color,
        ColorToken::Text
    );
    // At 160 pixels the same text takes two lines.
    let wider = Case {
        constraint: constraint(0, 0, 160, 200),
        ..column_wrap()
    };
    let tree = lay_out(&wider, &mut fonts);
    assert_eq!(
        lines_of(&tree),
        ["The result is 1/3 and ", "the error is 0"]
    );
}

#[test]
fn a_text_leaf_without_a_colour_takes_the_text_token() {
    assert_eq!(
        Text::new("a", Font::Sans, FontSize::Body).color,
        ColorToken::Text
    );
}

#[test]
fn a_column_taller_than_its_largest_height_takes_it_and_marks_overflow_in_y() {
    let tree = lay_out(&column_overflow(), &mut fonts());
    assert_eq!(
        rects(&tree),
        [px4(0, 0, 10, 100), px4(0, 0, 10, 80), px4(0, 80, 10, 80)]
    );
    assert_eq!(
        overflows(&tree),
        [(false, true), (false, false), (false, false)]
    );
}

#[test]
fn a_vertical_scroll_container_starts_at_offset_zero() {
    let tree = lay_out(&scroll_y(), &mut fonts());
    let rects = rects(&tree);
    assert_eq!(rects[0], px4(0, 0, 100, 50));
    assert_eq!(rects[1], px4(0, 0, 100, 120));
    let rows: Vec<[i32; 4]> = (0..6).map(|row| px4(0, 20 * row, 100, 20)).collect();
    assert_eq!(rects[2..], rows[..]);
    assert_eq!(tree.boxes()[0].scroll, Point::default());
    // The offset may go up to the content's height less the viewport's.
    let range = tree.boxes()[1].size.height().units() - tree.boxes()[0].size.height().units();
    assert_eq!(range, px(70).units());
    assert_eq!(overflows(&tree), [(false, false); 8]);
}

#[test]
fn an_offset_within_its_range_changes_only_the_scroll_offset() {
    let mut fonts = fonts();
    let at_zero = lay_out(&scroll_y(), &mut fonts);
    let moved = lay_out(&scroll_y_offset(), &mut fonts);
    assert_eq!(moved.boxes()[0].scroll, Point::new(Fx::ZERO, px(30)));
    assert_eq!(moved.boxes()[1..], at_zero.boxes()[1..]);
    let mut back = moved.boxes()[0].clone();
    back.scroll = Point::default();
    assert_eq!(back, at_zero.boxes()[0]);
}

#[test]
fn a_horizontal_scroll_container_lets_its_row_grow_past_the_viewport() {
    let tree = lay_out(&scroll_x(), &mut fonts());
    let rects = rects(&tree);
    assert_eq!(rects[0], px4(0, 0, 120, 20));
    assert_eq!(rects[1], [0, 0, 206 * 64, 20 * 64]);
    assert_eq!(
        rects[2..],
        [
            px4(0, 0, 50, 20),
            px4(52, 0, 50, 20),
            px4(104, 0, 50, 20),
            px4(156, 0, 50, 20)
        ]
    );
    assert_eq!(tree.boxes()[0].scroll, Point::default());
    let range = tree.boxes()[1].size.width().units() - tree.boxes()[0].size.width().units();
    assert_eq!(range, px(86).units());
}

#[test]
fn content_wider_than_a_vertical_viewport_marks_overflow_in_x() {
    let tree = lay_out(&scroll_cross(), &mut fonts());
    assert_eq!(rects(&tree), [px4(0, 0, 100, 50), px4(0, 0, 100, 200)]);
    assert_eq!(overflows(&tree), [(false, false), (true, false)]);
    let range = tree.boxes()[1].size.height().units() - tree.boxes()[0].size.height().units();
    assert_eq!(range, px(150).units());
}

#[test]
fn text_of_two_sizes_shares_one_baseline() {
    let tree = lay_out(&baseline_sizes(), &mut fonts());
    assert_eq!(
        rects(&tree),
        [
            [0, 0, 3054, 1843],
            [0, 0, 1679, 1843],
            [1935, 479, 1119, 1229]
        ]
    );
    assert_eq!(baseline_at(&tree, 1), baseline_at(&tree, 2));
    // The row's baseline is the larger height above a baseline.
    let larger = tree.boxes()[1].baseline.unwrap().units();
    assert_eq!(tree.boxes()[0].baseline.map(Fx::units), Some(larger));
    assert_eq!(larger, 1436);
}

#[test]
fn text_of_two_fonts_shares_one_baseline() {
    let tree = lay_out(&baseline_fonts(), &mut fonts());
    assert_eq!(
        rects(&tree),
        [
            [0, 0, 4642, 1843],
            [0, 479, 2458, 1229],
            [2458, 0, 2184, 1843]
        ]
    );
    assert_eq!(baseline_at(&tree, 1), baseline_at(&tree, 2));
    assert_eq!(baseline_at(&tree, 1), 1436);
}

#[test]
fn a_box_without_a_baseline_puts_its_bottom_on_the_baseline() {
    let tree = lay_out(&baseline_box(), &mut fonts());
    assert_eq!(
        rects(&tree),
        [
            [0, 0, 2655, 1229],
            [0, 0, 1119, 1229],
            [1375, 317, 1280, 640]
        ]
    );
    let fixed = &tree.boxes()[2];
    assert_eq!(fixed.baseline, None);
    assert_eq!(
        fixed.position.y.units() + fixed.size.height().units(),
        baseline_at(&tree, 1)
    );
    assert_eq!(tree.boxes()[0].baseline.map(Fx::units), Some(957));
}

#[test]
fn a_column_reports_the_baseline_of_its_first_child() {
    let tree = lay_out(&baseline_nested(), &mut fonts());
    assert_eq!(
        rects(&tree),
        [
            [0, 0, 2961, 3065],
            [0, 479, 507, 2586],
            [0, 0, 443, 1229],
            [0, 1357, 507, 1229],
            [1019, 0, 1942, 1843]
        ]
    );
    // The column's baseline is that of "1", which shares the row's
    // baseline with "= 3".
    assert_eq!(tree.boxes()[1].baseline, tree.boxes()[2].baseline);
    assert_eq!(baseline_at(&tree, 1), baseline_at(&tree, 4));
    assert_eq!(baseline_at(&tree, 1), 1436);
}

#[test]
fn text_of_several_lines_aligns_its_first_baseline() {
    let tree = lay_out(&baseline_lines(), &mut fonts());
    assert_eq!(
        rects(&tree),
        [[0, 0, 1201, 2082], [0, 239, 456, 1843], [712, 0, 489, 1229]]
    );
    let lines = tree.boxes()[1].text.as_ref().unwrap().paragraph.lines();
    assert_eq!(lines.len(), 2);
    assert_eq!(tree.boxes()[1].baseline, Some(lines[0].baseline()));
    assert_eq!(baseline_at(&tree, 1), baseline_at(&tree, 2));
}

#[test]
fn every_case_laid_out_twice_writes_the_same_bytes() {
    let mut fonts = fonts();
    for (name, case) in CASES {
        let first = lay_out(&case(), &mut fonts);
        let second = lay_out(&case(), &mut fonts);
        assert_eq!(first.text().as_bytes(), second.text().as_bytes(), "{name}");
        assert_eq!(first, second, "{name}");
    }
}

#[test]
fn the_text_of_a_tree_has_a_line_for_each_box_in_preorder() {
    let mut fonts = fonts();
    assert_eq!(
        lay_out(&row_spacing(), &mut fonts).text(),
        "layout-tree 1\n\
         0 parent - kind \"row\" key 0 min 0 0 max 12800 6400 at 0 0 size 4864 1280 \
         baseline - scroll 0 0 overflow none draws no\n\
         1 parent 0 kind \"fixed\" key 0 min 0 0 max 16777216 6400 at 0 0 size 1280 640 \
         baseline - scroll 0 0 overflow none draws yes\n\
         2 parent 0 kind \"fixed\" key 1 min 0 0 max 16777216 6400 at 1792 0 size 1920 1280 \
         baseline - scroll 0 0 overflow none draws yes\n\
         3 parent 0 kind \"fixed\" key 2 min 0 0 max 16777216 6400 at 4224 0 size 640 320 \
         baseline - scroll 0 0 overflow none draws yes\n"
    );
    let text = lay_out(&scroll_y_offset(), &mut fonts).text();
    assert_eq!(
        text.lines().nth(1),
        Some(
            "0 parent - kind \"scroll\" key 0 min 6400 3200 max 6400 3200 at 0 0 \
             size 6400 3200 baseline - scroll 0 1920 overflow none draws no"
        )
    );
    let text = lay_out(&baseline_box(), &mut fonts).text();
    assert_eq!(
        text.lines().nth(2),
        Some(
            "1 parent 0 kind \"text\" key 0 min 0 0 max 16777216 6400 at 0 0 \
             size 1119 1229 baseline 957 scroll 0 0 overflow none draws yes"
        )
    );
    let text = lay_out(&column_overflow(), &mut fonts).text();
    assert!(
        text.lines()
            .nth(1)
            .unwrap()
            .ends_with(" overflow y draws no")
    );
    // A kind is written in its debug form, so every box stays on one line.
    let mut tree = Tree::<TestApp>::new();
    tree.rebuild(&node(7, "a \"kind\"\non two lines", 0))
        .unwrap();
    let laid = layout(&tree, constraint(0, 0, 10, 10), &TOKENS, &mut fonts).unwrap();
    assert_eq!(
        laid.text(),
        "layout-tree 1\n\
         0 parent - kind \"a \\\"kind\\\"\\non two lines\" key 7 min 0 0 max 640 640 at 0 0 \
         size 0 0 baseline - scroll 0 0 overflow none draws no\n"
    );
}

#[test]
fn a_control_without_a_layout_is_an_empty_leaf() {
    let mut tree = Tree::<TestApp>::new();
    let view = node(0, "root", 0)
        .child(node(0, "first", 1))
        .child(node(1, "second", 2).child(node(0, "third", 3)));
    tree.rebuild(&view).unwrap();
    let laid = layout(&tree, constraint(10, 20, 100, 100), &TOKENS, &mut fonts()).unwrap();
    let kinds: Vec<&str> = laid.boxes().iter().map(|laid| laid.kind.name()).collect();
    assert_eq!(kinds, ["root", "first", "second", "third"]);
    // The root takes the smallest size of its constraint; its children
    // have the tight constraint of size zero.
    assert_eq!(rects(&laid), [px4(0, 0, 10, 20), [0; 4], [0; 4], [0; 4]]);
    assert!(laid.boxes().iter().all(|laid| laid.baseline.is_none()
        && !laid.draws
        && laid.overflow == Overflow::default()
        && laid.scroll == Point::default()));
    let parents: Vec<Option<usize>> = laid.boxes().iter().map(|laid| laid.parent).collect();
    assert_eq!(parents, [None, Some(0), Some(0), Some(2)]);
}

#[test]
fn children_that_their_node_does_not_lay_out_take_no_room() {
    let spec = row(
        Row::new(),
        vec![
            Spec {
                part: Part::Fixed(size_px(10, 10)),
                children: vec![fixed(5, 5)],
            },
            Spec {
                part: Part::Scroll(Axis::Y, Fx::ZERO),
                children: vec![fixed(20, 30), fixed(40, 40)],
            },
        ],
    );
    let case = Case {
        constraint: constraint(0, 0, 100, 100),
        spec,
    };
    let tree = lay_out(&case, &mut fonts());
    assert_eq!(
        rects(&tree),
        [
            px4(0, 0, 30, 30),
            px4(0, 0, 10, 10),
            [0; 4],
            px4(10, 0, 20, 30),
            px4(0, 0, 20, 30),
            [0; 4]
        ]
    );
    let zero = Constraint::tight(Size::ZERO);
    assert_eq!(tree.boxes()[2].constraint, zero);
    assert_eq!(tree.boxes()[5].constraint, zero);
    assert_eq!(overflows(&tree)[2], (true, true));
    assert_eq!(overflows(&tree)[5], (true, true));
}

#[test]
fn a_scroll_request_is_brought_into_the_range_of_its_content() {
    let mut fonts = fonts();
    let limit = MAX_LAYOUT_INPUT.units();
    for (request, offset) in [
        (-limit, 0),
        (-30 * 64, 0),
        (0, 0),
        (30 * 64, 30 * 64),
        (70 * 64, 70 * 64),
        (70 * 64 + 1, 70 * 64),
        (limit, 70 * 64),
    ] {
        let mut case = scroll_y();
        case.spec.part = Part::Scroll(Axis::Y, Fx::from_units(request));
        let tree = lay_out(&case, &mut fonts);
        assert_eq!(
            tree.boxes()[0].scroll.y.units(),
            offset,
            "request {request}"
        );
    }
    // Content shorter than the viewport does not scroll.
    let short = Case {
        constraint: constraint(100, 50, 100, 50),
        spec: scroll(Axis::Y, 30, fixed(100, 20)),
    };
    assert_eq!(
        lay_out(&short, &mut fonts).boxes()[0].scroll,
        Point::default()
    );
}

#[test]
fn inputs_beyond_their_range_are_refused_distinctly() {
    let mut fonts = fonts();
    let limit = MAX_LAYOUT_INPUT;
    let past = Fx::from_units(limit.units() + 1);
    let size = |width, height| Size::new(width, height).unwrap();
    let leaf = |side: Size| Spec {
        part: Part::Fixed(side),
        children: Vec::new(),
    };
    let up_to = |max| Constraint::new(Size::ZERO, max);
    let mut lay = |spec: &Spec, constraint, tokens: &Tokens| {
        layout(&build(spec), constraint, tokens, &mut fonts)
    };
    // Inputs at their limits are laid out.
    let at_limit = leaf(size(limit, limit));
    assert!(lay(&at_limit, up_to(size(limit, limit)), &TOKENS).is_ok());
    for request in [limit, Fx::from_units(-limit.units())] {
        let spec = Spec {
            part: Part::Scroll(Axis::X, request),
            children: vec![fixed(10, 10)],
        };
        assert!(lay(&spec, up_to(size(limit, limit)), &TOKENS).is_ok());
    }
    let refused = Err(LayoutError::InputOutOfRange);
    // A constraint beyond the limit, or whose smallest size exceeds its
    // largest.
    let small = fixed(1, 1);
    for constraint in [
        up_to(size(past, Fx::ZERO)),
        up_to(size(Fx::ZERO, past)),
        Constraint::new(size_px(2, 0), size_px(1, 1)),
        Constraint::new(size_px(0, 2), size_px(1, 1)),
    ] {
        assert_eq!(lay(&small, constraint, &TOKENS), refused);
    }
    // A fixed-size leaf beyond the limit, at the root or deeper.
    for side in [size(past, Fx::ZERO), size(Fx::ZERO, past)] {
        assert_eq!(lay(&leaf(side), up_to(size_px(1, 1)), &TOKENS), refused);
        let deeper = column(Column::new(), vec![fixed(1, 1), leaf(side)]);
        assert_eq!(lay(&deeper, up_to(size_px(1, 1)), &TOKENS), refused);
    }
    // A scroll request beyond its range.
    for request in [past, Fx::from_units(-past.units())] {
        let spec = Spec {
            part: Part::Scroll(Axis::Y, request),
            children: vec![fixed(10, 10)],
        };
        assert_eq!(lay(&spec, up_to(size_px(5, 5)), &TOKENS), refused);
    }
    // A table of tokens with a negative spacing or a font size that the
    // text layout does not take.
    for tokens in [
        Tokens {
            space_s: Fx::from_units(-1),
            ..TOKENS
        },
        Tokens {
            space_l: past,
            ..TOKENS
        },
        Tokens {
            font_body: Fx::ZERO,
            ..TOKENS
        },
    ] {
        assert_eq!(lay(&small, up_to(size_px(1, 1)), &tokens), refused);
    }
}

#[test]
fn a_paragraph_wider_than_the_largest_input_is_refused() {
    // A row lays its text out at the largest width, and a word does not
    // break: 20,000 letters W at the large size are wider than 2^18
    // pixels.
    let word = "W".repeat(20_000);
    let spec = row(Row::new(), vec![text(&word, Font::Sans, FontSize::Large)]);
    let up_to = Constraint::new(Size::ZERO, size_px(100, 100));
    let laid = layout(&build(&spec), up_to, &TOKENS, &mut fonts());
    assert_eq!(laid, Err(LayoutError::InputOutOfRange));
}

#[test]
fn a_length_beyond_the_numeric_representation_fails_the_whole_layout() {
    let mut fonts = fonts();
    let limit = MAX_LAYOUT_INPUT;
    let side = Spec {
        part: Part::Fixed(Size::new(limit, Fx::ZERO).unwrap()),
        children: Vec::new(),
    };
    let up_to = Constraint::new(Size::ZERO, Size::new(limit, limit).unwrap());
    // 127 leaves of 2^18 pixels fit in an `Fx`; the row overflows.
    let fits = row(Row::new(), vec![side.clone(); 127]);
    let tree = layout(&build(&fits), up_to, &TOKENS, &mut fonts).unwrap();
    assert_eq!(tree.boxes()[0].overflow, Overflow { x: true, y: false });
    // 128 are 2^25 pixels, beyond the largest `Fx`.
    let beyond = row(Row::new(), vec![side; 128]);
    assert_eq!(
        layout(&build(&beyond), up_to, &TOKENS, &mut fonts),
        Err(LayoutError::Overflow)
    );
}

#[test]
fn the_layout_result_holds_the_boxes_of_the_tree() {
    let tree = lay_out(&scroll_y_offset(), &mut fonts());
    let result = tree.result().unwrap();
    assert_eq!(result.boxes().len(), tree.boxes().len());
    for (laid, result_box) in tree.boxes().iter().zip(result.boxes()) {
        assert_eq!(result_box.widget, laid.widget);
        assert_eq!(result_box.parent, laid.parent);
        assert_eq!(
            result_box.rect,
            kosmo_types::Rect::new(laid.position, laid.size)
        );
        assert_eq!(result_box.scroll, laid.scroll);
        assert_eq!(result_box.draws, laid.draws);
    }
    // 10 pixels into the viewport, scrolled by 30, is the third row.
    let path = hit_test(&result, Point::new(px(50), px(10)))
        .unwrap()
        .unwrap();
    assert_eq!(path.last(), Some(&tree.boxes()[4].widget));
    assert_eq!(path.len(), 3);
}

/// A row of a column of two text leaves, the first `first`, and of a
/// vertical scroll container that asks for `request` pixels, with a column
/// of six rows of 100 × 40 pixels, the first `first_width` pixels wide, as
/// its content; under the tight constraint of 200 × 100 pixels, the
/// container is 100 pixels high and may scroll by 140.
fn dashboard(first: &str, first_width: i32, request: i32) -> Case {
    let mut rows = vec![fixed(100, 40); 6];
    rows[0] = fixed(first_width, 40);
    Case {
        constraint: constraint(200, 100, 200, 100),
        spec: row(
            Row::new().spacing(Space::S),
            vec![
                column(
                    Column::new(),
                    vec![
                        text(first, Font::Sans, FontSize::Body),
                        text("a", Font::Sans, FontSize::Body),
                    ],
                ),
                scroll(Axis::Y, request, column(Column::new(), rows)),
            ],
        ),
    }
}

/// Lays out `tree` against `kept` and as a whole, checks that both give
/// the same tree, and returns the first with its work.
fn again(
    tree: &Tree<Layouts>,
    constraint: Constraint,
    kept: &Kept,
    fonts: &mut FontSet,
) -> (Kept, Work) {
    let (again, work) = relayout(tree, constraint, &TOKENS, fonts, 0, Some(kept)).unwrap();
    let (whole, _) = relayout(tree, constraint, &TOKENS, fonts, 0, None).unwrap();
    assert_eq!(again.tree.text(), whole.tree.text());
    assert_eq!(again.tree, whole.tree);
    (again, work)
}

#[test]
fn a_layout_against_the_layout_before_lays_out_only_what_changed() {
    let mut fonts = fonts();
    let case = dashboard("1", 100, 0);
    let mut tree = build(&case.spec);
    let (first, work) = relayout(&tree, case.constraint, &TOKENS, &mut fonts, 0, None).unwrap();
    // Every box, and both text leaves, the first time.
    assert_eq!(first.tree.boxes().len(), 12);
    assert_eq!(work, Work { laid: 12, texts: 2 });
    // Nothing changed: the whole tree is taken from the layout before.
    let (kept, work) = again(&tree, case.constraint, &first, &mut fonts);
    assert_eq!(work, Work::default());
    assert_eq!(kept.tree, first.tree);
    // A longer first text: the leaf, its column, whose width grows, and
    // the root, whose size its tight constraint keeps; the other leaf and
    // the scroll container with its content are taken as they were.
    rebuild(&mut tree, &dashboard("1234", 100, 0).spec);
    let (kept, work) = again(&tree, case.constraint, &kept, &mut fonts);
    assert_eq!(work, Work { laid: 3, texts: 1 });
    // A narrower first row in the content, which keeps the width of the
    // content, and another request: the row and the content's column are
    // laid out, and the scroll container keeps its size and follows its
    // request.
    rebuild(&mut tree, &dashboard("1234", 50, 30).spec);
    let (kept, work) = again(&tree, case.constraint, &kept, &mut fonts);
    assert_eq!(work, Work { laid: 2, texts: 0 });
    let scroll = &kept.tree.boxes()[4];
    assert_eq!(scroll.scroll, Point::new(Fx::ZERO, px(30)));
    // Another request alone moves only the scroll container's content.
    rebuild(&mut tree, &dashboard("1234", 50, 45).spec);
    let (moved, work) = again(&tree, case.constraint, &kept, &mut fonts);
    assert_eq!(work, Work::default());
    let changed: Vec<usize> = (0..12)
        .filter(|&index| moved.tree.boxes()[index] != kept.tree.boxes()[index])
        .collect();
    assert_eq!(changed, [4]);
    assert_eq!(moved.tree.boxes()[4].scroll, Point::new(Fx::ZERO, px(45)));
}

#[test]
fn a_request_beyond_its_range_in_a_subtree_taken_whole_fails_as_the_whole_layout() {
    let mut fonts = fonts();
    let spec = column(
        Column::new(),
        vec![
            scroll(Axis::Y, 0, column(Column::new(), vec![fixed(10, 80)])),
            fixed(10, 10),
        ],
    );
    let up_to = constraint(0, 0, 100, 50);
    let mut tree = build(&spec);
    let (kept, _) = relayout(&tree, up_to, &TOKENS, &mut fonts, 0, None).unwrap();
    // Only the request changes, so no control is dirty and the whole tree
    // would be taken from the layout before.
    let scroll = tree.preorder()[1];
    tree.node_mut(scroll).unwrap().state = Fx::from_units((1 << 24) + 1);
    let refused = Some(LayoutError::InputOutOfRange);
    let again = relayout(&tree, up_to, &TOKENS, &mut fonts, 0, Some(&kept));
    assert_eq!(again.err(), refused);
    let whole = relayout(&tree, up_to, &TOKENS, &mut fonts, 0, None);
    assert_eq!(whole.err(), refused);
}

#[test]
fn another_constraint_table_of_tokens_or_font_set_lays_out_the_whole_tree() {
    let mut fonts = fonts();
    let case = dashboard("1", 100, 0);
    let tree = build(&case.spec);
    let (first, _) = relayout(&tree, case.constraint, &TOKENS, &mut fonts, 0, None).unwrap();
    let whole = Work { laid: 12, texts: 2 };
    let wider = constraint(240, 100, 240, 100);
    let lay = |constraint, tokens: &Tokens, fonts_id, fonts: &mut FontSet| {
        relayout(&tree, constraint, tokens, fonts, fonts_id, Some(&first))
            .unwrap()
            .1
    };
    assert_eq!(lay(wider, &TOKENS, 0, &mut fonts), whole);
    let tokens = Tokens {
        space_s: px(5),
        ..TOKENS
    };
    assert_eq!(lay(case.constraint, &tokens, 0, &mut fonts), whole);
    // A colour too, though the layout does not read colours.
    let tokens = Tokens {
        accent: kosmo_render::Color::opaque(0xC0, 0, 0),
        ..TOKENS
    };
    assert_eq!(lay(case.constraint, &tokens, 0, &mut fonts), whole);
    let mut other = self::fonts();
    assert_eq!(lay(case.constraint, &TOKENS, 1, &mut other), whole);
    // The same three take the whole tree from the layout before.
    assert_eq!(
        lay(case.constraint, &TOKENS, 0, &mut fonts),
        Work::default()
    );
}
