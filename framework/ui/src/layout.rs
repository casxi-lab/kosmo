//! Layout: the layout nodes that controls give, and the pure function
//! that lays a control tree out with constraints into a layout tree.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use kosmo_text::{FontSet, MONO, Paragraph, SANS, TextStyle};
use kosmo_types::{Fx, Point, Rect, Size};

use crate::app::{App, Control};
use crate::hit::{LayoutBox, LayoutError, LayoutResult};
use crate::tokens::{ColorToken, FontSize, Space, Tokens};
use crate::tree::{Key, Kind, Tree, WidgetId};

/// The largest layout input: 2^18 pixels, 2^24 units. The sides of a
/// constraint and of a leaf and the absolute value of a scroll request are
/// at most this; [`layout`] refuses a larger one with
/// [`LayoutError::InputOutOfRange`]. It is the largest width that
/// `kosmo-text` lays out, so a text leaf always gets a width it accepts.
pub const MAX_LAYOUT_INPUT: Fx = Fx::from_units(1 << 24);

const _: () = assert!(MAX_LAYOUT_INPUT.units() == kosmo_text::MAX_WIDTH.units());

/// One half, as a factor: `x.checked_mul(HALF)` is half of `x`, rounded to
/// a unit by `kosmo_types::round`.
const HALF: Fx = Fx::from_units(32);

/// The smallest scroll request, `-MAX_LAYOUT_INPUT`.
const MIN_REQUEST: Fx = Fx::from_units(-(1 << 24));

/// What a parent passes to a child: the smallest and the largest size that
/// the child may take.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Constraint {
    /// The smallest size.
    pub min: Size,
    /// The largest size.
    pub max: Size,
}

impl Constraint {
    /// The constraint from `min` to `max`.
    pub const fn new(min: Size, max: Size) -> Constraint {
        Constraint { min, max }
    }

    /// The constraint that allows only `size`.
    pub const fn tight(size: Size) -> Constraint {
        Constraint {
            min: size,
            max: size,
        }
    }

    /// `natural` brought into the constraint, axis by axis: at least the
    /// smallest size and at most the largest.
    fn clamp(self, natural: Size) -> Result<Size, LayoutError> {
        let width = natural.width().max(self.min.width()).min(self.max.width());
        let height = natural
            .height()
            .max(self.min.height())
            .min(self.max.height());
        Size::new(width, height).ok_or(LayoutError::Overflow)
    }

    /// Which axes of `natural` exceed the largest size.
    fn overflow(self, natural: Size) -> Overflow {
        Overflow {
            x: natural.width() > self.max.width(),
            y: natural.height() > self.max.height(),
        }
    }
}

/// An axis of the surface.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Axis {
    /// The horizontal axis, `x` growing to the right.
    X,
    /// The vertical axis, `y` growing down.
    Y,
}

/// Where the children of a column lie across it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Align {
    /// At the start: the left edge.
    #[default]
    Start,
    /// In the centre.
    Center,
    /// At the end: the right edge.
    End,
}

/// Where the children of a row lie across it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum RowAlign {
    /// At the start: the top edge.
    #[default]
    Start,
    /// In the centre.
    Center,
    /// At the end: the bottom edge.
    End,
    /// With their baselines at one height; a child without a baseline
    /// puts its bottom edge there.
    Baseline,
}

/// A row: its children from left to right, each at its own size.
///
/// The spacing between the children and the padding around them are
/// spacing tokens, or none. A value in pixels does not compile:
///
/// ```compile_fail
/// use kosmo_types::Fx;
/// use kosmo_ui::Row;
///
/// let row = Row::new().spacing(Fx::from_units(512));
/// ```
///
/// ```
/// use kosmo_ui::{Row, RowAlign, Space};
///
/// let row = Row::new().spacing(Space::M).padding(Space::L).align(RowAlign::Baseline);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Row {
    spacing: Option<Space>,
    padding: Option<Space>,
    align: RowAlign,
}

impl Row {
    /// A row without spacing or padding, with its children at the top.
    pub const fn new() -> Row {
        Row {
            spacing: None,
            padding: None,
            align: RowAlign::Start,
        }
    }

    /// The row with `spacing` between its children.
    pub const fn spacing(self, spacing: Space) -> Row {
        Row {
            spacing: Some(spacing),
            ..self
        }
    }

    /// The row with `padding` around its children, on all four sides.
    pub const fn padding(self, padding: Space) -> Row {
        Row {
            padding: Some(padding),
            ..self
        }
    }

    /// The row with its children aligned across it by `align`.
    pub const fn align(self, align: RowAlign) -> Row {
        Row { align, ..self }
    }
}

/// A column: its children from top to bottom, each at its own size, with
/// spacing and padding of spacing tokens, as a [`Row`] has.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Column {
    spacing: Option<Space>,
    padding: Option<Space>,
    align: Align,
}

impl Column {
    /// A column without spacing or padding, with its children at the left.
    pub const fn new() -> Column {
        Column {
            spacing: None,
            padding: None,
            align: Align::Start,
        }
    }

    /// The column with `spacing` between its children.
    pub const fn spacing(self, spacing: Space) -> Column {
        Column {
            spacing: Some(spacing),
            ..self
        }
    }

    /// The column with `padding` around its children, on all four sides.
    pub const fn padding(self, padding: Space) -> Column {
        Column {
            padding: Some(padding),
            ..self
        }
    }

    /// The column with its children aligned across it by `align`.
    pub const fn align(self, align: Align) -> Column {
        Column { align, ..self }
    }
}

/// A scroll container: a viewport onto its one child, the content, which
/// is moved by the scroll offset along one axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Scroll {
    axis: Axis,
    request: Fx,
}

impl Scroll {
    /// A scroll container along `axis` that asks for the offset `request`,
    /// which a control reads from its presentation state. The layout gives
    /// the offset brought into its range.
    pub const fn new(axis: Axis, request: Fx) -> Scroll {
        Scroll { axis, request }
    }
}

/// The font of a text leaf: one of the two fonts of `kosmo-text`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Font {
    /// Fira Sans, `kosmo_text::SANS`.
    Sans,
    /// Fira Mono, `kosmo_text::MONO`.
    Mono,
}

/// A text leaf: a paragraph in one font at a font size token, drawn in a
/// colour token.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Text<'a> {
    text: &'a str,
    font: Font,
    size: FontSize,
    color: ColorToken,
}

impl<'a> Text<'a> {
    /// The paragraph `text` in `font` at `size`, in [`ColorToken::Text`].
    pub const fn new(text: &'a str, font: Font, size: FontSize) -> Text<'a> {
        Text {
            text,
            font,
            size,
            color: ColorToken::Text,
        }
    }

    /// The text leaf in `color`.
    pub const fn color(self, color: ColorToken) -> Text<'a> {
        Text { color, ..self }
    }
}

/// The layout node of a control, which [`Control::layout`] gives.
///
/// Rows, columns and scroll containers lay out the control's children;
/// leaves have none of their own. A leaf's children, and the children of
/// a scroll container after its first, are laid out with the tight
/// constraint of size zero and placed at the origin, so they take no room.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Node<'a> {
    /// A leaf of size zero that draws nothing and has no baseline: the
    /// node of a control that gives none.
    #[default]
    Empty,
    /// A leaf of this size, which draws and has no baseline.
    Fixed(Size),
    /// A text leaf, which draws and has the baseline of its first line.
    Text(Text<'a>),
    /// A row.
    Row(Row),
    /// A column.
    Column(Column),
    /// A scroll container.
    Scroll(Scroll),
}

/// Which axes of a box overflow: its natural size along the axis exceeds
/// the largest size of its constraint, so it took the largest size, and
/// its content reaches beyond it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Overflow {
    /// Whether the box overflows along `x`.
    pub x: bool,
    /// Whether the box overflows along `y`.
    pub y: bool,
}

/// The text of a text leaf as the layout laid it out.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct LaidText {
    /// The paragraph, broken at the largest width of the leaf's
    /// constraint, in the coordinates of the leaf's box.
    pub paragraph: Paragraph,
    /// The colour token of the text.
    pub color: ColorToken,
}

/// One box of a [`LayoutTree`]: a control as the layout placed it.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TreeBox {
    /// The control.
    pub widget: WidgetId,
    /// The index of the parent's box, which comes earlier; `None` only for
    /// the first box, the root.
    pub parent: Option<usize>,
    /// The kind of the control.
    pub kind: Kind,
    /// The key of the control.
    pub key: Key,
    /// The constraint that the parent passed.
    pub constraint: Constraint,
    /// The position of the box in the content coordinates of its parent:
    /// its parent's position minus the parent's scroll offset, or the
    /// surface's origin for the root.
    pub position: Point,
    /// The size of the box, within its constraint.
    pub size: Size,
    /// The baseline, from the top of the box, if the box has one.
    pub baseline: Option<Fx>,
    /// The scroll offset of a scroll container, along its axis; zero for
    /// every other box.
    pub scroll: Point,
    /// Which axes overflow.
    pub overflow: Overflow,
    /// Whether the control draws: true for text and fixed-size leaves.
    pub draws: bool,
    /// The laid-out text of a text leaf.
    pub text: Option<LaidText>,
}

/// The layout tree of a surface: a box for each control, in preorder,
/// which is the drawing order.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct LayoutTree {
    boxes: Vec<TreeBox>,
}

impl LayoutTree {
    /// The boxes in preorder.
    pub fn boxes(&self) -> &[TreeBox] {
        &self.boxes
    }

    /// The layout result that hit testing and the drawing pass read: for
    /// each box its control, its parent, its position and size, its scroll
    /// offset and whether it draws.
    pub fn result(&self) -> Result<LayoutResult, LayoutError> {
        let mut result = LayoutResult::new();
        for laid in &self.boxes {
            result.push(LayoutBox {
                widget: laid.widget,
                parent: laid.parent,
                rect: Rect::new(laid.position, laid.size),
                scroll: laid.scroll,
                draws: laid.draws,
            })?;
        }
        Ok(result)
    }

    /// The tree as text, in UTF-8: the line `layout-tree 1`, the format
    /// and its version, and then one line for each box in preorder, each
    /// line followed by a line feed. A box's line holds, separated by
    /// spaces, its index from 0 and then these fields, each named before
    /// its value:
    ///
    /// - `parent`, the index of the parent's box, or `-` for the root;
    /// - `kind`, the name of the control's kind in Rust's debug form, in
    ///   double quotes with escapes;
    /// - `key`, the control's key;
    /// - `min` and `max`, the width and height of the constraint;
    /// - `at`, the position, `x` and `y`;
    /// - `size`, the width and height;
    /// - `baseline`, the baseline, or `-` without one;
    /// - `scroll`, the scroll offset, `x` and `y`;
    /// - `overflow`, `none`, `x`, `y` or `xy`;
    /// - `draws`, `yes` or `no`.
    ///
    /// Every number is an integer: a count, a key, or a value in 1/64
    /// pixel. The same tree gives the same text byte for byte.
    pub fn text(&self) -> String {
        let mut text = String::from("layout-tree 1\n");
        for (index, laid) in self.boxes.iter().enumerate() {
            let parent = laid
                .parent
                .map_or_else(|| "-".to_string(), |parent| parent.to_string());
            let baseline = laid
                .baseline
                .map_or_else(|| "-".to_string(), |baseline| baseline.units().to_string());
            let overflow = match (laid.overflow.x, laid.overflow.y) {
                (false, false) => "none",
                (true, false) => "x",
                (false, true) => "y",
                (true, true) => "xy",
            };
            let draws = if laid.draws { "yes" } else { "no" };
            // Writing into a `String` does not fail.
            let _ = writeln!(
                text,
                "{index} parent {parent} kind {:?} key {} min {} {} max {} {} at {} {} \
                 size {} {} baseline {baseline} scroll {} {} overflow {overflow} draws {draws}",
                laid.kind.name(),
                laid.key.get(),
                laid.constraint.min.width().units(),
                laid.constraint.min.height().units(),
                laid.constraint.max.width().units(),
                laid.constraint.max.height().units(),
                laid.position.x.units(),
                laid.position.y.units(),
                laid.size.width().units(),
                laid.size.height().units(),
                laid.scroll.x.units(),
                laid.scroll.y.units(),
            );
        }
        text
    }
}

/// Lays the controls of `tree` out under `constraint`, the root's
/// constraint, with the values of `tokens` and the text metrics of
/// `fonts`, and returns the layout tree.
///
/// Each control gives its node through [`Control::layout`], from its value
/// and presentation state. A parent passes each child a constraint, the
/// child reports its size and an optional baseline, and the parent places
/// it:
///
/// - A row passes each child a width from 0 to [`MAX_LAYOUT_INPUT`] and a
///   height from 0 to its own largest height less its padding above and
///   below; it puts the children side by side from its left padding, with
///   its spacing between them, and aligns them across it. Its natural
///   size is the size of the children with the spacing and padding. A
///   column does the same along `y`.
/// - Aligned by baseline, the children of a row have their baselines at
///   one height; a child without one puts its bottom edge there. The
///   row's content is then as tall as the largest height above the
///   baseline and the largest height below it together. A row or column
///   has the baseline of its first child that has one, moved by the
///   child's position.
/// - A text leaf is laid out by `fonts` at the largest width of its
///   constraint, breaking lines there and at the line breaks of its text;
///   its natural size is the size of the paragraph, and its baseline that
///   of its first line.
/// - A scroll container passes its first child, the content, a length
///   from 0 to [`MAX_LAYOUT_INPUT`] along its axis and up to its own
///   largest size across it, and takes the content's size within its
///   constraint, without overflow. Its offset is the request brought into
///   0 to the content's length less its own, or 0 when the content is
///   shorter.
/// - Every box takes its natural size brought into its constraint. Where
///   the natural size exceeds the largest size, the box takes the largest
///   size and marks the overflow of that axis; its children keep their
///   natural sizes and positions.
///
/// The layout reads only its input: the controls with their presentation
/// states, `constraint`, `tokens` and the text metrics of `fonts`, which
/// changes only its context of text layout. Every coordinate is an `Fx`,
/// and a half, for a centred child, is rounded by `kosmo_types::round`.
/// It returns [`LayoutError::InputOutOfRange`] and no tree for a
/// constraint whose smallest size exceeds its largest, for a constraint,
/// a fixed-size leaf, the size of a paragraph or a scroll request beyond
/// [`MAX_LAYOUT_INPUT`], and for a table of tokens with a negative spacing,
/// a spacing beyond [`MAX_LAYOUT_INPUT`] or a font size that `kosmo-text`
/// does not lay out; and [`LayoutError::Overflow`] when a sum of lengths
/// does not fit the numeric representation. It never panics.
pub fn layout<A: App>(
    tree: &Tree<A>,
    constraint: Constraint,
    tokens: &Tokens,
    fonts: &mut FontSet,
) -> Result<LayoutTree, LayoutError> {
    relayout(tree, constraint, tokens, fonts, 0, None).map(|(kept, _)| kept.tree)
}

/// A layout node as a layout keeps it for the next: the node with its text
/// owned and, for a scroll container, without its request, which changes
/// only the container's offset.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Shape {
    Empty,
    Fixed(Size),
    Text {
        text: String,
        font: Font,
        size: FontSize,
        color: ColorToken,
    },
    Row(Row),
    Column(Column),
    Scroll(Axis),
}

impl Shape {
    /// The shape of `node` and its scroll request, zero for a node that is
    /// not a scroll container.
    fn of(node: Node<'_>) -> (Shape, Fx) {
        match node {
            Node::Empty => (Shape::Empty, Fx::ZERO),
            Node::Fixed(size) => (Shape::Fixed(size), Fx::ZERO),
            Node::Text(text) => {
                let shape = Shape::Text {
                    text: text.text.to_string(),
                    font: text.font,
                    size: text.size,
                    color: text.color,
                };
                (shape, Fx::ZERO)
            }
            Node::Row(row) => (Shape::Row(row), Fx::ZERO),
            Node::Column(column) => (Shape::Column(column), Fx::ZERO),
            Node::Scroll(scroll) => (Shape::Scroll(scroll.axis), scroll.request),
        }
    }
}

/// What the layout pass of a round keeps of a surface's layout for the
/// next: the layout tree, the shape of each box's control, where each
/// box's subtree ends in preorder, the box of each control, and the
/// constraint, tokens and font set it was laid out with.
#[derive(Debug)]
pub(crate) struct Kept {
    tree: LayoutTree,
    shapes: Vec<Shape>,
    ends: Vec<usize>,
    index: BTreeMap<WidgetId, usize>,
    constraint: Constraint,
    tokens: Tokens,
    fonts: u64,
}

impl Kept {
    /// The layout tree.
    pub(crate) fn tree(&self) -> &LayoutTree {
        &self.tree
    }

    /// Each scroll container of the tree with its offset along its axis.
    pub(crate) fn offsets(&self) -> impl Iterator<Item = (WidgetId, Fx)> + '_ {
        self.tree
            .boxes
            .iter()
            .zip(&self.shapes)
            .filter_map(|(laid, shape)| match shape {
                Shape::Scroll(axis) => Some((laid.widget, main_of(laid.scroll, *axis))),
                _ => None,
            })
    }

    /// Whether the children of the box `at` are the controls `children`,
    /// in this order.
    fn children_are(&self, at: usize, children: &[WidgetId]) -> bool {
        let mut kept = self.children(at);
        children.iter().all(|&child| {
            kept.next()
                .and_then(|index| self.tree.boxes.get(index))
                .is_some_and(|laid| laid.widget == child)
        }) && kept.next().is_none()
    }

    /// The indices of the children of the box `at`, in order.
    fn children(&self, at: usize) -> impl Iterator<Item = usize> + '_ {
        let end = self.end(at);
        let mut next = at + 1;
        std::iter::from_fn(move || {
            (next < end).then(|| {
                let child = next;
                next = self.end(child).max(child + 1);
                child
            })
        })
    }

    /// Where the subtree of the box `at` ends in preorder.
    fn end(&self, at: usize) -> usize {
        self.ends.get(at).copied().unwrap_or(at)
    }
}

/// How much a layout did: the boxes whose size and children's positions it
/// computed, and the text leaves it laid out with the font set. The boxes
/// that it took from the layout before do not count.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Work {
    pub(crate) laid: usize,
    pub(crate) texts: usize,
}

/// Lays the controls of `tree` out as [`layout`] does, with the font set
/// `fonts` named by the number `fonts_id`, and keeps the result for the
/// next layout, which takes `kept`, this layout's, as the one before.
///
/// When `kept` was laid out under the same constraint, with the same
/// tokens and the same font set, only dirty subtrees are laid out again. A
/// control is dirty when it is new, when its layout node differs from the
/// one before other than in a scroll request, or when its children differ.
/// A subtree without a dirty control under the constraint of the layout
/// before is taken from it whole. A box whose control is not dirty and
/// whose children came out with the sizes and baselines of the layout
/// before keeps its size and its children's positions; otherwise its parent
/// computes them again, so the work goes up from a dirty control only while
/// sizes or baselines change, and stops at the latest at a box under a
/// tight constraint whose baseline stays, a layout boundary. A scroll
/// container's offset always follows its current request, so a change of
/// the request alone moves only the container's content. With another
/// constraint, other tokens or another font set the whole tree is laid out
/// again. The result is that of [`layout`] byte for byte.
pub(crate) fn relayout<A: App>(
    tree: &Tree<A>,
    constraint: Constraint,
    tokens: &Tokens,
    fonts: &mut FontSet,
    fonts_id: u64,
    kept: Option<&Kept>,
) -> Result<(Kept, Work), LayoutError> {
    check_tokens(tokens)?;
    check_constraint(constraint)?;
    let kept = kept.filter(|kept| {
        kept.constraint == constraint && kept.tokens == *tokens && kept.fonts == fonts_id
    });
    let mut marks = marks(tree, kept)?;
    let mut pass = Pass {
        tree,
        tokens,
        fonts,
        marks: &marks,
        kept,
        boxes: Vec::with_capacity(tree.len()),
        work: Work::default(),
    };
    if let Some(root) = tree.root() {
        pass.lay(root, None, constraint)?;
    }
    let (boxes, work) = (pass.boxes, pass.work);
    let mut ends: Vec<usize> = (1..=boxes.len()).collect();
    for (index, laid) in boxes.iter().enumerate().rev() {
        let end = ends.get(index).copied().unwrap_or(index);
        if let Some(parent_end) = laid.parent.and_then(|parent| ends.get_mut(parent)) {
            *parent_end = (*parent_end).max(end);
        }
    }
    let mut shapes = Vec::with_capacity(boxes.len());
    let mut index = BTreeMap::new();
    for (at, laid) in boxes.iter().enumerate() {
        let mark = marks.remove(&laid.widget).ok_or(LayoutError::Order)?;
        shapes.push(mark.shape);
        index.insert(laid.widget, at);
    }
    let kept = Kept {
        tree: LayoutTree { boxes },
        shapes,
        ends,
        index,
        constraint,
        tokens: *tokens,
        fonts: fonts_id,
    };
    Ok((kept, work))
}

/// A control's layout node in this layout and whether it or its subtree
/// changed since the layout before.
struct Mark {
    /// The shape of the control's node.
    shape: Shape,
    /// The scroll request of a scroll container; zero for other nodes.
    request: Fx,
    /// The control is dirty: new, with another shape or other children.
    dirty: bool,
    /// The control or one of its descendants is dirty.
    below: bool,
}

/// The mark of each control of `tree` against `kept`, the layout before:
/// each control's node is taken once, children before their parents.
fn marks<A: App>(
    tree: &Tree<A>,
    kept: Option<&Kept>,
) -> Result<BTreeMap<WidgetId, Mark>, LayoutError> {
    let mut marks: BTreeMap<WidgetId, Mark> = BTreeMap::new();
    for id in tree.preorder().into_iter().rev() {
        let node = tree.node(id).ok_or(LayoutError::Order)?;
        let (shape, request) = Shape::of(node.control.layout(&node.state));
        let before = kept.and_then(|kept| kept.index.get(&id).map(|&at| (kept, at)));
        let dirty = !before.is_some_and(|(kept, at)| {
            kept.shapes.get(at) == Some(&shape) && kept.children_are(at, &node.children)
        });
        let below = dirty
            || node
                .children
                .iter()
                .any(|child| marks.get(child).is_some_and(|mark| mark.below));
        let mark = Mark {
            shape,
            request,
            dirty,
            below,
        };
        marks.insert(id, mark);
    }
    Ok(marks)
}

/// A layout in progress: the boxes laid out so far, in preorder, the marks
/// of the controls, the layout before, if any, and the work so far.
struct Pass<'t, 'f, A: App> {
    tree: &'t Tree<A>,
    tokens: &'t Tokens,
    fonts: &'f mut FontSet,
    marks: &'t BTreeMap<WidgetId, Mark>,
    kept: Option<&'t Kept>,
    boxes: Vec<TreeBox>,
    work: Work,
}

/// What a node gives its box once its children are placed.
struct Laid {
    size: Size,
    baseline: Option<Fx>,
    scroll: Point,
    overflow: Overflow,
    draws: bool,
    text: Option<LaidText>,
}

impl Laid {
    /// A leaf of natural size `natural` under `constraint`.
    fn leaf(natural: Size, constraint: Constraint, draws: bool) -> Result<Laid, LayoutError> {
        Ok(Laid {
            size: constraint.clamp(natural)?,
            baseline: None,
            scroll: Point::default(),
            overflow: constraint.overflow(natural),
            draws,
            text: None,
        })
    }
}

/// A row or a column, along its main axis.
struct Lane {
    axis: Axis,
    spacing: Option<Space>,
    padding: Option<Space>,
    align: RowAlign,
}

impl Lane {
    /// The lane of `row`.
    fn row(row: &Row) -> Lane {
        Lane {
            axis: Axis::X,
            spacing: row.spacing,
            padding: row.padding,
            align: row.align,
        }
    }

    /// The lane of `column`.
    fn column(column: &Column) -> Lane {
        let align = match column.align {
            Align::Start => RowAlign::Start,
            Align::Center => RowAlign::Center,
            Align::End => RowAlign::End,
        };
        Lane {
            axis: Axis::Y,
            spacing: column.spacing,
            padding: column.padding,
            align,
        }
    }
}

impl<A: App> Pass<'_, '_, A> {
    /// Lays out the control `id` under `constraint`, as a child of the box
    /// `parent`, with its subtree, and returns the index of its box. The
    /// box is at the origin; its parent places it.
    fn lay(
        &mut self,
        id: WidgetId,
        parent: Option<usize>,
        constraint: Constraint,
    ) -> Result<usize, LayoutError> {
        let (tree, marks) = (self.tree, self.marks);
        let node = tree.node(id).ok_or(LayoutError::Order)?;
        let mark = marks.get(&id).ok_or(LayoutError::Order)?;
        let before = self.before(id, constraint);
        if let Some(at) = before
            && !mark.below
        {
            return self.copy(at, parent);
        }
        let index = self.boxes.len();
        self.boxes.push(TreeBox {
            widget: id,
            parent,
            kind: node.control.kind(),
            key: node.key,
            constraint,
            position: Point::default(),
            size: Size::ZERO,
            baseline: None,
            scroll: Point::default(),
            overflow: Overflow::default(),
            draws: false,
            text: None,
        });
        let children = node.children.as_slice();
        // The children that the node places, laid out first.
        let placed = match &mark.shape {
            Shape::Row(row) => self.lay_lane(index, children, constraint, &Lane::row(row))?,
            Shape::Column(column) => {
                self.lay_lane(index, children, constraint, &Lane::column(column))?
            }
            Shape::Scroll(axis) => {
                check_request(mark.request)?;
                let inner = Constraint::new(
                    Size::ZERO,
                    size(*axis, MAX_LAYOUT_INPUT, cross(constraint.max, *axis))?,
                );
                match children.split_first() {
                    Some((&first, rest)) => {
                        let content = self.lay(first, Some(index), inner)?;
                        self.unplaced(index, rest)?;
                        vec![content]
                    }
                    None => Vec::new(),
                }
            }
            Shape::Fixed(size) => {
                check_size(*size)?;
                self.unplaced(index, children)?;
                Vec::new()
            }
            Shape::Empty | Shape::Text { .. } => {
                self.unplaced(index, children)?;
                Vec::new()
            }
        };
        let kept = before.filter(|&at| !mark.dirty && self.same_results(at, &placed));
        let laid = match kept {
            Some(at) => self.keep(at, &placed, mark)?,
            None => {
                self.work.laid += 1;
                self.compute(&mark.shape, mark.request, &placed, constraint)?
            }
        };
        let laid_box = self.boxes.get_mut(index).ok_or(LayoutError::Order)?;
        laid_box.size = laid.size;
        laid_box.baseline = laid.baseline;
        laid_box.scroll = laid.scroll;
        laid_box.overflow = laid.overflow;
        laid_box.draws = laid.draws;
        laid_box.text = laid.text;
        Ok(index)
    }

    /// The box of the control `id` in the layout before, when that layout
    /// passed it `constraint` too.
    fn before(&self, id: WidgetId, constraint: Constraint) -> Option<usize> {
        let kept = self.kept?;
        let at = *kept.index.get(&id)?;
        let laid = kept.tree.boxes.get(at)?;
        (laid.constraint == constraint).then_some(at)
    }

    /// Takes the box `at` of the layout before with its subtree, as a child
    /// of the box `parent`, and returns the index of its box. The offset of
    /// each scroll container in it follows its current request.
    fn copy(&mut self, at: usize, parent: Option<usize>) -> Result<usize, LayoutError> {
        let kept = self.kept.ok_or(LayoutError::Order)?;
        let base = self.boxes.len();
        for from in at..kept.end(at) {
            let mut laid = kept.tree.boxes.get(from).ok_or(LayoutError::Order)?.clone();
            laid.parent = match laid.parent {
                _ if from == at => parent,
                Some(old) => Some(
                    old.checked_sub(at)
                        .and_then(|offset| base.checked_add(offset))
                        .ok_or(LayoutError::Order)?,
                ),
                None => return Err(LayoutError::Order),
            };
            if let Some(Mark {
                shape: Shape::Scroll(axis),
                request,
                ..
            }) = self.marks.get(&laid.widget)
            {
                check_request(*request)?;
                let content = kept.children(from).next();
                let content = content
                    .and_then(|child| kept.tree.boxes.get(child))
                    .map_or(Size::ZERO, |child| child.size);
                laid.scroll = offset(*axis, *request, content, laid.size);
            }
            self.boxes.push(laid);
        }
        Ok(base)
    }

    /// Whether the boxes `placed` came out with the sizes and baselines of
    /// the first children of the box `at` in the layout before.
    fn same_results(&self, at: usize, placed: &[usize]) -> bool {
        let Some(kept) = self.kept else {
            return false;
        };
        let mut children = kept.children(at);
        placed.iter().all(|&index| {
            let old = children.next().and_then(|child| kept.tree.boxes.get(child));
            let new = self.boxes.get(index);
            matches!((old, new), (Some(old), Some(new))
                if old.size == new.size && old.baseline == new.baseline)
        })
    }

    /// The result of the box `at` of the layout before, whose control is not
    /// dirty and whose children `placed` came out as before: the children
    /// take their positions of the layout before, and a scroll container's
    /// offset follows its request.
    fn keep(&mut self, at: usize, placed: &[usize], mark: &Mark) -> Result<Laid, LayoutError> {
        let kept = self.kept.ok_or(LayoutError::Order)?;
        let old = kept.tree.boxes.get(at).ok_or(LayoutError::Order)?;
        for (&index, child) in placed.iter().zip(kept.children(at)) {
            let position = kept
                .tree
                .boxes
                .get(child)
                .ok_or(LayoutError::Order)?
                .position;
            self.place(index, position)?;
        }
        let scroll = match mark.shape {
            Shape::Scroll(axis) => {
                let content = placed.first().map_or(Ok(Size::ZERO), |&index| {
                    self.child(index).map(|content| content.size)
                })?;
                offset(axis, mark.request, content, old.size)
            }
            _ => old.scroll,
        };
        Ok(Laid {
            size: old.size,
            baseline: old.baseline,
            scroll,
            overflow: old.overflow,
            draws: old.draws,
            text: old.text.clone(),
        })
    }

    /// Computes the result of a box of `shape` with the scroll request
    /// `request` under `constraint`, whose children `placed` are laid out,
    /// and places them.
    fn compute(
        &mut self,
        shape: &Shape,
        request: Fx,
        placed: &[usize],
        constraint: Constraint,
    ) -> Result<Laid, LayoutError> {
        match shape {
            Shape::Empty => Laid::leaf(Size::ZERO, constraint, false),
            Shape::Fixed(size) => Laid::leaf(*size, constraint, true),
            Shape::Text {
                text,
                font,
                size,
                color,
            } => self.text(Text::new(text, *font, *size).color(*color), constraint),
            Shape::Row(row) => self.lane(placed, constraint, &Lane::row(row)),
            Shape::Column(column) => self.lane(placed, constraint, &Lane::column(column)),
            Shape::Scroll(axis) => {
                let content = placed.first().map_or(Ok(Size::ZERO), |&index| {
                    self.child(index).map(|content| content.size)
                })?;
                let taken = constraint.clamp(content)?;
                Ok(Laid {
                    size: taken,
                    baseline: None,
                    scroll: offset(*axis, request, content, taken),
                    overflow: Overflow::default(),
                    draws: false,
                    text: None,
                })
            }
        }
    }

    /// Lays out `children` of the box `parent`, which its node does not lay
    /// out, with the tight constraint of size zero at the origin.
    fn unplaced(&mut self, parent: usize, children: &[WidgetId]) -> Result<(), LayoutError> {
        for &child in children {
            self.lay(child, Some(parent), Constraint::tight(Size::ZERO))?;
        }
        Ok(())
    }

    /// The box `index`.
    fn child(&self, index: usize) -> Result<&TreeBox, LayoutError> {
        self.boxes.get(index).ok_or(LayoutError::Order)
    }

    /// Places the box `index` at `position`.
    fn place(&mut self, index: usize, position: Point) -> Result<(), LayoutError> {
        let laid_box = self.boxes.get_mut(index).ok_or(LayoutError::Order)?;
        laid_box.position = position;
        Ok(())
    }

    /// The value of the spacing token `space`, or zero without one.
    fn space(&self, space: Option<Space>) -> Fx {
        space.map_or(Fx::ZERO, |space| self.tokens.space(space))
    }

    /// Lays out a text leaf under `constraint`.
    fn text(&mut self, text: Text<'_>, constraint: Constraint) -> Result<Laid, LayoutError> {
        let font = match text.font {
            Font::Sans => SANS,
            Font::Mono => MONO,
        };
        // The font sizes of the tokens were checked on entry.
        let style = TextStyle::new(font, self.tokens.font_size(text.size))
            .map_err(|_| LayoutError::InputOutOfRange)?;
        // The width is at most `MAX_LAYOUT_INPUT`, which the text layout
        // accepts, so its only error is a position that does not fit.
        self.work.texts += 1;
        let paragraph = self
            .fonts
            .layout(text.text, style, Some(constraint.max.width()))
            .map_err(|_| LayoutError::Overflow)?;
        let natural =
            Size::new(paragraph.width(), paragraph.height()).ok_or(LayoutError::Overflow)?;
        check_size(natural)?;
        let mut laid = Laid::leaf(natural, constraint, true)?;
        laid.baseline = paragraph.lines().first().map(|line| line.baseline());
        laid.text = Some(LaidText {
            paragraph,
            color: text.color,
        });
        Ok(laid)
    }

    /// Lays out the `children` of the row or column `index` under the
    /// constraint that `lane` passes them within `constraint`, and returns
    /// their boxes.
    fn lay_lane(
        &mut self,
        index: usize,
        children: &[WidgetId],
        constraint: Constraint,
        lane: &Lane,
    ) -> Result<Vec<usize>, LayoutError> {
        let axis = lane.axis;
        let padding = self.space(lane.padding);
        let room = less(cross(constraint.max, axis), add(padding, padding)?);
        let inner = Constraint::new(Size::ZERO, size(axis, MAX_LAYOUT_INPUT, room)?);
        let mut laid = Vec::with_capacity(children.len());
        for &child in children {
            laid.push(self.lay(child, Some(index), inner)?);
        }
        Ok(laid)
    }

    /// Places the children `laid` of a row or a column under `constraint`
    /// and gives its result.
    fn lane(
        &mut self,
        laid: &[usize],
        constraint: Constraint,
        lane: &Lane,
    ) -> Result<Laid, LayoutError> {
        let axis = lane.axis;
        let padding = self.space(lane.padding);
        let spacing = self.space(lane.spacing);
        let both = add(padding, padding)?;
        // Along the main axis: the children one after the other.
        let mut along = Vec::with_capacity(laid.len());
        let mut end = padding;
        for (number, &child) in laid.iter().enumerate() {
            if number > 0 {
                end = add(end, spacing)?;
            }
            along.push(end);
            end = add(end, main(self.child(child)?.size, axis))?;
        }
        let natural_main = add(end, padding)?;
        // Across: the children aligned, or their baselines at one height.
        let mut across = Vec::with_capacity(laid.len());
        let natural_cross = if lane.align == RowAlign::Baseline {
            let mut above = Fx::ZERO;
            let mut below = Fx::ZERO;
            for &child in laid {
                let child = self.child(child)?;
                let height = cross(child.size, axis);
                let baseline = child.baseline.unwrap_or(height);
                above = above.max(baseline);
                below = below.max(sub(height, baseline)?);
            }
            for &child in laid {
                let child = self.child(child)?;
                let baseline = child.baseline.unwrap_or(cross(child.size, axis));
                across.push(add(padding, sub(above, baseline)?)?);
            }
            add(add(above, below)?, both)?
        } else {
            let content = laid.iter().try_fold(Fx::ZERO, |content, &child| {
                Ok::<Fx, LayoutError>(content.max(cross(self.child(child)?.size, axis)))
            })?;
            let natural = add(content, both)?;
            let taken = cross(constraint.clamp(size(axis, natural_main, natural)?)?, axis);
            // The children align in the box less its padding, or in their
            // own extent where the box is smaller.
            let extent = less(taken, both).max(content);
            for &child in laid {
                let free = sub(extent, cross(self.child(child)?.size, axis))?;
                let offset = match lane.align {
                    RowAlign::Start | RowAlign::Baseline => Fx::ZERO,
                    RowAlign::Center => free.checked_mul(HALF).ok_or(LayoutError::Overflow)?,
                    RowAlign::End => free,
                };
                across.push(add(padding, offset)?);
            }
            natural
        };
        let mut baseline = None;
        for ((&child, &main_at), &cross_at) in laid.iter().zip(&along).zip(&across) {
            let position = point(axis, main_at, cross_at);
            self.place(child, position)?;
            if baseline.is_none()
                && let Some(child_baseline) = self.child(child)?.baseline
            {
                baseline = Some(add(child_baseline, position.y)?);
            }
        }
        let natural = size(axis, natural_main, natural_cross)?;
        Ok(Laid {
            size: constraint.clamp(natural)?,
            baseline,
            scroll: Point::default(),
            overflow: constraint.overflow(natural),
            draws: false,
            text: None,
        })
    }
}

/// Checks that a scroll request is within `-MAX_LAYOUT_INPUT` to
/// [`MAX_LAYOUT_INPUT`].
fn check_request(request: Fx) -> Result<(), LayoutError> {
    if request < MIN_REQUEST || request > MAX_LAYOUT_INPUT {
        return Err(LayoutError::InputOutOfRange);
    }
    Ok(())
}

/// The offset of a scroll container along `axis` with `request`, `content`
/// and the size `viewport`: the request brought into 0 to the content's
/// length less the viewport's, or 0 when the content is shorter.
fn offset(axis: Axis, request: Fx, content: Size, viewport: Size) -> Point {
    let range = less(main(content, axis), main(viewport, axis));
    point(axis, request.max(Fx::ZERO).min(range), Fx::ZERO)
}

/// Checks that each spacing of `tokens` is from zero to
/// [`MAX_LAYOUT_INPUT`] and each font size one that `kosmo-text` lays out.
fn check_tokens(tokens: &Tokens) -> Result<(), LayoutError> {
    let spaces = [Space::Xs, Space::S, Space::M, Space::L];
    let sizes = [FontSize::Small, FontSize::Body, FontSize::Large];
    let spaces_fit = spaces.iter().all(|&space| {
        let value = tokens.space(space);
        Fx::ZERO <= value && value <= MAX_LAYOUT_INPUT
    });
    let sizes_fit = sizes.iter().all(|&font_size| {
        let value = tokens.font_size(font_size);
        kosmo_text::MIN_SIZE <= value && value <= kosmo_text::MAX_SIZE
    });
    if spaces_fit && sizes_fit {
        Ok(())
    } else {
        Err(LayoutError::InputOutOfRange)
    }
}

/// Checks that the smallest size of `constraint` does not exceed its
/// largest and that the largest is within [`MAX_LAYOUT_INPUT`].
fn check_constraint(constraint: Constraint) -> Result<(), LayoutError> {
    let Constraint { min, max } = constraint;
    check_size(max)?;
    if min.width() > max.width() || min.height() > max.height() {
        return Err(LayoutError::InputOutOfRange);
    }
    Ok(())
}

/// Checks that both sides of `size` are within [`MAX_LAYOUT_INPUT`].
fn check_size(size: Size) -> Result<(), LayoutError> {
    if size.width() > MAX_LAYOUT_INPUT || size.height() > MAX_LAYOUT_INPUT {
        return Err(LayoutError::InputOutOfRange);
    }
    Ok(())
}

/// `a + b`, or [`LayoutError::Overflow`].
fn add(a: Fx, b: Fx) -> Result<Fx, LayoutError> {
    a.checked_add(b).ok_or(LayoutError::Overflow)
}

/// `a - b`, or [`LayoutError::Overflow`].
fn sub(a: Fx, b: Fx) -> Result<Fx, LayoutError> {
    a.checked_sub(b).ok_or(LayoutError::Overflow)
}

/// `a - b` for lengths `a` and `b`, or zero when `b` is larger.
fn less(a: Fx, b: Fx) -> Fx {
    a.checked_sub(b).map_or(Fx::ZERO, |rest| rest.max(Fx::ZERO))
}

/// The length of `size` along `axis`.
fn main(size: Size, axis: Axis) -> Fx {
    match axis {
        Axis::X => size.width(),
        Axis::Y => size.height(),
    }
}

/// The length of `size` across `axis`.
fn cross(size: Size, axis: Axis) -> Fx {
    match axis {
        Axis::X => size.height(),
        Axis::Y => size.width(),
    }
}

/// The size `along` long on `axis` and `across` long across it.
fn size(axis: Axis, along: Fx, across: Fx) -> Result<Size, LayoutError> {
    match axis {
        Axis::X => Size::new(along, across),
        Axis::Y => Size::new(across, along),
    }
    .ok_or(LayoutError::Overflow)
}

/// The point `along` on `axis` and `across` across it.
fn point(axis: Axis, along: Fx, across: Fx) -> Point {
    match axis {
        Axis::X => Point::new(along, across),
        Axis::Y => Point::new(across, along),
    }
}

/// The coordinate of `point` along `axis`.
fn main_of(point: Point, axis: Axis) -> Fx {
    match axis {
        Axis::X => point.x,
        Axis::Y => point.y,
    }
}

#[cfg(test)]
mod tests;
