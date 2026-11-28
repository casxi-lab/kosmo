//! Scrolling and hit testing: a vertical scroll container of 100 × 50
//! pixels over a column of six rows of 100 × 20 pixels brings the offset
//! that its control requests into the range of its content, from 0 to 70
//! pixels, and hit testing on the layout result finds the row under a
//! point of the viewport, both after the layout function and after the
//! layout pass of rounds in which a bound command moves the request.

mod support;

use kosmo_platform::PixelSize;
use kosmo_render::Command;
use kosmo_text::FontSet;
use kosmo_types::{Fx, MonoTime, Point, Rect, Size};
use kosmo_ui::{
    App, Axis, Binding, ColorToken, Column, CommandId, Constraint, Context, Control, DrawContext,
    Event, Handling, Input, Key, Kind, LayoutInput, LayoutTree, Message, Mods, Node, Out, Output,
    Phase, Runtime, Scroll, TOKENS, Trigger, View, ViewPass, WidgetId, hit_test, layout,
};
use support::layout::{Checked, NoJobs, failed, font_set};
use support::{SURFACE, char_press, focus_in, stamp};

/// A command of the tests that moves a scroll container 30 pixels down.
const SCROLL_DOWN: CommandId = CommandId::new("test", "ScrollDown", true);

/// A command of the tests that moves a scroll container 500 pixels down.
const SCROLL_FAR: CommandId = CommandId::new("test", "ScrollFar", false);

/// The binding table of the container that scrolls by commands: `j` moves
/// it down by 30 pixels and `f` by 500.
const BINDINGS: &[Binding] = &[
    Binding {
        trigger: Trigger::Char('j'),
        mods: Mods::NONE,
        command: SCROLL_DOWN,
    },
    Binding {
        trigger: Trigger::Char('f'),
        mods: Mods::NONE,
        command: SCROLL_FAR,
    },
];

const fn px(pixels: i32) -> Fx {
    Fx::from_units(pixels * 64)
}

/// A control of the rows.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Piece {
    /// A vertical scroll container that asks for the offset of its value.
    Asks(Fx),
    /// A vertical scroll container that asks for the offset of its
    /// presentation state, which its bound commands move down and to which
    /// it writes back the offset that the layout pass gives it.
    Scrolls,
    /// A column.
    Column,
    /// A row of 100 × 20 pixels, which fills its box with the accent colour
    /// when it is true, for an even key, and with the background colour
    /// otherwise.
    Line(bool),
}

impl Control<Rows> for Piece {
    type State = Fx;

    fn kind(&self) -> Kind {
        Kind::new(match self {
            Piece::Asks(_) | Piece::Scrolls => "scroll",
            Piece::Column => "column",
            Piece::Line(_) => "line",
        })
    }

    fn bindings(&self) -> &'static [Binding] {
        match self {
            Piece::Scrolls => BINDINGS,
            _ => &[],
        }
    }

    fn handle(
        &self,
        _: Phase,
        command: &kosmo_ui::Command,
        cx: &mut Context<'_, Rows, Fx>,
    ) -> Handling {
        let step = match (self, command.id) {
            (Piece::Scrolls, SCROLL_DOWN) => px(30),
            (Piece::Scrolls, SCROLL_FAR) => px(500),
            _ => return Handling::Unhandled,
        };
        match cx.state().checked_add(step) {
            Some(request) => {
                *cx.state_mut() = request;
                Handling::Handled
            }
            None => Handling::Unhandled,
        }
    }

    #[allow(
        clippy::expect_used,
        reason = "the size of a row is a small, positive constant"
    )]
    fn layout(&self, request: &Fx) -> Node<'_> {
        match self {
            Piece::Asks(asked) => Node::Scroll(Scroll::new(Axis::Y, *asked)),
            Piece::Scrolls => Node::Scroll(Scroll::new(Axis::Y, *request)),
            Piece::Column => Node::Column(Column::new()),
            Piece::Line(_) => Node::Fixed(Size::new(px(100), px(20)).expect("a row has a size")),
        }
    }

    fn scrolled(&self, offset: Fx, request: &mut Fx) {
        if let Piece::Scrolls = self {
            *request = offset;
        }
    }

    fn draw(&self, cx: &mut DrawContext<'_, Fx>) {
        if let Piece::Line(even) = self {
            let token = if *even {
                ColorToken::Accent
            } else {
                ColorToken::Background
            };
            let rect = Rect::new(Point::default(), cx.size());
            let color = cx.color(token);
            cx.draw(Command::FillRect { rect, color });
        }
    }
}

/// The application whose view is the scroll container over the rows.
struct Rows {
    view: View<Piece>,
}

impl App for Rows {
    type Message = ();
    type Control = Piece;
    type Port = NoJobs;

    fn update(&mut self, _: Message<Self>, _: &mut Out<Self>) {}

    fn view(&self) -> View<Piece> {
        self.view.clone()
    }
}

/// The view of `scroll` over a column of six rows with the keys 0 to 5.
fn rows(scroll: View<Piece>) -> View<Piece> {
    let mut column = View::new(Key::new(0), Piece::Column);
    for key in 0..6 {
        column.push(View::new(Key::new(key), Piece::Line(key % 2 == 0)));
    }
    scroll.child(column)
}

/// The identities of the scroll container, the column and the six rows.
struct Ids {
    scroll: WidgetId,
    column: WidgetId,
    rows: Vec<WidgetId>,
}

impl Ids {
    fn of(runtime: &Runtime<Rows>) -> Checked<Ids> {
        let tree = runtime.tree();
        let scroll = tree.root().ok_or("no tree")?;
        let column = *tree.children(scroll).first().ok_or("no column")?;
        let rows = tree.children(column).to_vec();
        Ok(Ids {
            scroll,
            column,
            rows,
        })
    }

    /// The path of hit testing to the row `row`.
    fn path(&self, row: usize) -> Option<Vec<WidgetId>> {
        Some(vec![self.scroll, self.column, self.rows[row]])
    }
}

/// The rows below a scroll container that asks for `request` pixels, laid
/// out by the layout function under the tight constraint of 100 ×
/// `height` pixels, with the identities of their controls.
fn laid_out(request: i32, height: i32) -> Checked<(LayoutTree, Ids)> {
    let mut app = Rows {
        view: rows(View::new(Key::new(0), Piece::Asks(px(request)))),
    };
    let mut runtime = Runtime::new();
    let output = runtime
        .round(&mut app, Input::new(MonoTime::ZERO))
        .finish(&app);
    if output.view != ViewPass::Rebuilt {
        return Err(format!("the view was not applied: {:?}", output.view));
    }
    let size = Size::new(px(100), px(height)).ok_or("no size")?;
    let laid = layout(
        runtime.tree(),
        Constraint::tight(size),
        &TOKENS,
        &mut font_set()?,
    )
    .map_err(failed)?;
    Ok((laid, Ids::of(&runtime)?))
}

/// What hit testing finds at (`x`, `y`) in `laid`.
fn hit(laid: &LayoutTree, x: i32, y: i32) -> Checked<Option<Vec<WidgetId>>> {
    hit_test(&laid.result().map_err(failed)?, Point::new(px(x), px(y))).map_err(failed)
}

/// The scroll offset of the scroll container of `laid`.
fn offset(laid: &LayoutTree) -> Option<Point> {
    laid.boxes().first().map(|scroll| scroll.scroll)
}

/// The offset of `pixels` along `y`.
fn down(pixels: i32) -> Option<Point> {
    Some(Point::new(Fx::ZERO, px(pixels)))
}

#[test]
fn after_a_request_within_the_range_points_of_the_viewport_hit_the_rows_under_them() -> Checked {
    let (laid, ids) = laid_out(30, 50)?;
    assert_eq!(offset(&laid), down(30));
    // 5 pixels into the viewport is 35 pixels into the content, row 1, and
    // 45 pixels is 75, row 3.
    assert_eq!(hit(&laid, 10, 5)?, ids.path(1));
    assert_eq!(hit(&laid, 10, 45)?, ids.path(3));
    Ok(())
}

#[test]
fn a_request_beyond_the_range_scrolls_to_its_end_where_points_hit_the_last_rows() -> Checked {
    let (laid, ids) = laid_out(500, 50)?;
    // The content of 120 pixels in a viewport of 50 ends at 70.
    assert_eq!(offset(&laid), down(70));
    assert_eq!(hit(&laid, 10, 45)?, ids.path(5));
    assert_eq!(hit(&laid, 10, 0)?, ids.path(3));
    Ok(())
}

#[test]
fn a_negative_request_scrolls_to_the_start() -> Checked {
    let (laid, ids) = laid_out(-30, 50)?;
    assert_eq!(offset(&laid), down(0));
    assert_eq!(hit(&laid, 10, 5)?, ids.path(0));
    Ok(())
}

#[test]
fn content_that_fits_its_viewport_does_not_scroll() -> Checked {
    for request in [30, 500, -30] {
        let (laid, ids) = laid_out(request, 200)?;
        assert_eq!(offset(&laid), down(0));
        assert_eq!(hit(&laid, 10, 5)?, ids.path(0));
    }
    Ok(())
}

#[test]
fn a_point_below_the_viewport_hits_no_row() -> Checked {
    let (laid, ids) = laid_out(30, 50)?;
    // The bottom edge of the viewport is outside it, and the row below
    // it is clipped, while the last line of pixels inside it hits.
    assert_eq!(hit(&laid, 10, 50)?, None);
    assert_eq!(hit(&laid, 10, 49)?, ids.path(3));
    Ok(())
}

/// Runs a round with `events` that lays out and draws the test surface of
/// 100 × 50 pixels with `fonts`, and returns its output.
fn round(
    runtime: &mut Runtime<Rows>,
    app: &mut Rows,
    fonts: &mut FontSet,
    events: Vec<Event>,
) -> Checked<Output> {
    let mut input = Input::new(MonoTime::ZERO);
    input.events = events;
    let size = PixelSize::new(100, 50).ok_or("no size")?;
    let layout = LayoutInput::new(fonts, 0, TOKENS).surface(SURFACE, size);
    let output = runtime.round(app, input).finish_with(app, layout);
    if !output.paint_errors.is_empty() {
        return Err(format!("{:?}", output.paint_errors));
    }
    Ok(output)
}

#[test]
fn bound_commands_scroll_in_rounds_and_hit_testing_reads_the_layout_of_the_round() -> Checked {
    // The press of `j` asks for 30 pixels, and the press of `f` for 500.
    for (key, request, end, points) in [
        ('j', 30, 30, [(5, 1), (45, 3)]),
        ('f', 500, 70, [(45, 5), (0, 3)]),
    ] {
        let mut fonts = font_set()?;
        let mut app = Rows {
            view: rows(View::new(Key::new(0), Piece::Scrolls).focused()),
        };
        let mut runtime = Runtime::new();
        round(
            &mut runtime,
            &mut app,
            &mut fonts,
            vec![focus_in(stamp(1, 1))],
        )?;
        let output = round(
            &mut runtime,
            &mut app,
            &mut fonts,
            vec![char_press(key, stamp(1, 1))],
        )?;
        let ids = Ids::of(&runtime)?;
        assert!(output.record.text().contains(&format!(
            "handled test.{} by control {}",
            if key == 'j' {
                "ScrollDown"
            } else {
                "ScrollFar"
            },
            ids.scroll
        )));
        // The round laid the rows out with the request, wrote the offset
        // back and drew the surface whole.
        let kept = runtime.layout(SURFACE).unwrap();
        assert_eq!(offset(kept), down(end));
        assert_eq!(runtime.tree().state(ids.scroll), Some(&px(end)));
        assert_eq!(output.frames.len(), 1);
        // The layout of the round is that of the layout function for the
        // request, and hit testing finds the same rows in both.
        let (direct, direct_ids) = laid_out(request, 50)?;
        assert_eq!(kept.text(), direct.text());
        for (y, row) in points {
            assert_eq!(hit(kept, 10, y)?, ids.path(row));
            assert_eq!(hit(&direct, 10, y)?, direct_ids.path(row));
        }
        // Another press of `f` asks for 500 pixels beyond the request
        // written back, which stays at the end of the range.
        round(
            &mut runtime,
            &mut app,
            &mut fonts,
            vec![char_press('f', stamp(1, 1))],
        )?;
        assert_eq!(offset(runtime.layout(SURFACE).unwrap()), down(70));
        assert_eq!(runtime.tree().state(ids.scroll), Some(&px(70)));
        // A round in which nothing changed writes the same offset back
        // through `Control::scrolled` and draws nothing.
        let idle = round(&mut runtime, &mut app, &mut fonts, Vec::new())?;
        assert!(idle.frames.is_empty());
    }
    Ok(())
}
