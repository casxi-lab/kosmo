//! The layout cases of the layout tree goldens: a control of each kind of
//! layout node, an application whose view is a case's tree, and its layout
//! through the font set of the two bundled fonts.
//!
//! The cases themselves are those of the unit tests of the layout, in
//! `src/layout/tests/cases.rs`, which this module includes, so the goldens
//! and the reference values of the unit tests describe the same trees.

use kosmo_text::FontSet;
use kosmo_types::{Fx, MonoTime, Size};
use kosmo_ui::{
    Align, App, Axis, ColorToken, Column, Constraint, Control, DrawContext, Font, FontSize, Input,
    JobPort, Key, Kind, LayoutTree, Message, Node, Out, Row, RowAlign, Runtime, Scroll, Space,
    TOKENS, Text, View, ViewPass, layout,
};

#[path = "../../src/layout/tests/cases.rs"]
mod cases;

pub use cases::CASES;

/// The result of a helper; the error says what went wrong.
pub type Checked<T = ()> = Result<T, String>;

/// `error` as the error of a helper.
pub fn failed(error: impl std::fmt::Debug) -> String {
    format!("{error:?}")
}

/// The bytes of Fira Sans from the asset directory of `kosmo-text`.
pub const SANS_BYTES: &[u8] =
    include_bytes!("../../../text/assets/fonts/firasans/FiraSans-Regular.ttf");

/// The bytes of Fira Mono from the asset directory of `kosmo-text`.
pub const MONO_BYTES: &[u8] =
    include_bytes!("../../../text/assets/fonts/firamono/FiraMono-Regular.ttf");

/// A font set of the two bundled fonts.
pub fn font_set() -> Checked<FontSet> {
    FontSet::new(SANS_BYTES, MONO_BYTES).map_err(failed)
}

/// A control of the cases: a node of each kind. A scroll container asks
/// for the offset that its value holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Part {
    Row(Row),
    Column(Column),
    Scroll(Axis, Fx),
    Text(String, Font, FontSize, ColorToken),
    Fixed(Size),
}

impl Control<Cases> for Part {
    type State = ();

    fn kind(&self) -> Kind {
        Kind::new(match self {
            Part::Row(_) => "row",
            Part::Column(_) => "column",
            Part::Scroll(..) => "scroll",
            Part::Text(..) => "text",
            Part::Fixed(_) => "fixed",
        })
    }

    fn layout(&self, _: &()) -> Node<'_> {
        match self {
            Part::Row(row) => Node::Row(*row),
            Part::Column(column) => Node::Column(*column),
            Part::Scroll(axis, request) => Node::Scroll(Scroll::new(*axis, *request)),
            Part::Text(text, font, size, color) => {
                Node::Text(Text::new(text, *font, *size).color(*color))
            }
            Part::Fixed(size) => Node::Fixed(*size),
        }
    }

    fn draw(&self, _: &mut DrawContext<'_, ()>) {}
}

/// A port that takes no request: the application of the cases submits no
/// job.
pub struct NoJobs;

impl JobPort for NoJobs {
    type Request = ();
    type Submission = ();
    type Rejection = ();
    type CancelAnswer = ();
    type Report = ();

    fn submit(&mut self, _: ()) -> Result<(), ()> {
        Err(())
    }

    fn cancel(&mut self, _: &()) {}

    fn take_reports(&mut self, _: &mut Vec<()>) -> usize {
        0
    }
}

/// The application whose view is the tree of a case.
pub struct Cases {
    view: View<Part>,
}

impl App for Cases {
    type Message = ();
    type Control = Part;
    type Port = NoJobs;

    fn update(&mut self, _: Message<Self>, _: &mut Out<Self>) {}

    fn view(&self) -> View<Part> {
        self.view.clone()
    }
}

/// A node of a case's tree, with its children.
#[derive(Clone, Debug)]
pub struct Spec {
    pub part: Part,
    pub children: Vec<Spec>,
}

/// A tree and the constraint of its root.
pub struct Case {
    pub spec: Spec,
    pub constraint: Constraint,
}

/// The view of `spec` with `key`; its children have their indices as
/// keys.
pub fn view(spec: &Spec, key: u64) -> View<Part> {
    let mut view = View::new(Key::new(key), spec.part.clone());
    for (index, child) in (0..).zip(&spec.children) {
        view.push(self::view(child, index));
    }
    view
}

/// The layout tree of `case`: a runtime builds the control tree from the
/// case's view in its first round, and `fonts` lays it out with the table
/// of design tokens under the case's constraint.
pub fn lay_out(case: &Case, fonts: &mut FontSet) -> Checked<LayoutTree> {
    let mut app = Cases {
        view: view(&case.spec, 0),
    };
    let mut runtime = Runtime::new();
    let output = runtime
        .round(&mut app, Input::new(MonoTime::ZERO))
        .finish(&app);
    if output.view != ViewPass::Rebuilt {
        return Err(format!("the view was not applied: {:?}", output.view));
    }
    layout(runtime.tree(), case.constraint, &TOKENS, fonts).map_err(failed)
}

/// The layout tree of the case `name`, laid out by `fonts`.
pub fn lay_out_case(name: &str, fonts: &mut FontSet) -> Checked<LayoutTree> {
    let (_, case) = CASES
        .iter()
        .find(|(case, _)| *case == name)
        .ok_or_else(|| format!("no case is named {name}"))?;
    lay_out(&case(), fonts)
}

/// `pixels` as an `Fx`.
#[allow(
    clippy::expect_used,
    reason = "the lengths of the cases are small constants"
)]
fn px(pixels: i32) -> Fx {
    Fx::from_pixels(pixels).expect("a length of the cases fits")
}

/// The size of `width` × `height` pixels.
#[allow(
    clippy::expect_used,
    reason = "the sizes of the cases are small, positive constants"
)]
fn size_px(width: i32, height: i32) -> Size {
    Size::new(px(width), px(height)).expect("a size of the cases is not negative")
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

/// A scroll container along `axis` that asks for `request` pixels, with
/// `content`.
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
