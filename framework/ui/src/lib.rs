//! The core of kosmo's framework: the control tree with its passes, the
//! round function of the single-threaded event loop and its shell,
//! versioned result slots, timers, the dispatch of key events with focus
//! checks, command tables with the minimal key map, layout with its design
//! tokens, hit testing and the drawing pass.
//!
//! An application is a model that implements [`App`]: `update` handles
//! [`Message`]s one at a time and is the only place that changes the model;
//! it outputs [`Effect`]s and timer requests. `view` reads the model and
//! returns a [`View`] tree. Neither reaches the platform, the execution
//! port or the control tree. The framework keeps the control [`Tree`]: it
//! compares each view tree with the tree by key and kind and applies only
//! additions, removals and changes, so a control keeps its [`WidgetId`]
//! while its parent, key and kind stay the same. Controls implement
//! [`Control`] and hold only presentation state.
//!
//! The [`Runtime`] runs one round at a time in two steps, without reading a
//! clock, waiting or calling a port: [`Runtime::round`] takes the round's
//! [`Input`] (its time, receipts, platform events and job reports) and runs
//! the event and update passes, and [`Round::finish`] runs the view pass
//! and gives the next deadline and the event handling [`Record`] of the
//! round. [`Round::finish_with`] runs the layout and drawing passes after
//! the view pass, with the font set, the table of design tokens and the
//! surface sizes of its [`LayoutInput`]: it lays out each surface under its
//! size against the layout that the runtime kept from the round before,
//! laying out again only dirty subtrees, and [`Runtime::layout`] gives each
//! surface's [`LayoutTree`]; then it draws each surface whose layout tree
//! changed since its latest frame, every surface after a view rebuilt the
//! tree or a handler of a control took its presentation state through
//! [`Context::state_mut`], and the pending region of each surface, and the
//! [`Output`] holds the frames, each a display list with its damage
//! region. Between the two steps, whoever drives the rounds hands
//! the effects to the execution port, a [`JobPort`], for example with
//! [`execute`], and passes the receipts to the next round.
//!
//! ```
//! use kosmo_types::{Epoch, FocusGen, HidUsage, KeyAction, KeyEvent, KeyMeaning, ModSet, MonoTime};
//! use kosmo_ui::{
//!     App, Command, Context, Control, DrawContext, EVALUATE_SELECTION, Event, Input, JobPort,
//!     Key, Kind, Message, Out, Receipt, Runtime, Stamp, SurfaceId, View, execute,
//! };
//!
//! // A port that accepts every submission and numbers it.
//! #[derive(Default)]
//! struct Port { next: u64 }
//! impl JobPort for Port {
//!     type Request = &'static str;
//!     type Submission = u64;
//!     type Rejection = ();
//!     type CancelAnswer = ();
//!     type Report = (u64, &'static str);
//!     fn submit(&mut self, _: &'static str) -> Result<u64, ()> { self.next += 1; Ok(self.next) }
//!     fn cancel(&mut self, _: &u64) {}
//!     fn take_reports(&mut self, _: &mut Vec<(u64, &'static str)>) -> usize { 0 }
//! }
//!
//! #[derive(Clone, PartialEq, Debug)]
//! struct Line;
//! impl Control<Calc> for Line {
//!     type State = ();
//!     fn kind(&self) -> Kind { Kind::new("line") }
//!     fn draw(&self, _: &mut DrawContext<'_, ()>) {}
//! }
//!
//! // The application submits a job on Execute and keeps the identity it gets.
//! #[derive(Default)]
//! struct Calc { submitted: Option<u64> }
//! impl App for Calc {
//!     type Message = ();
//!     type Control = Line;
//!     type Port = Port;
//!     fn update(&mut self, message: Message<Self>, out: &mut Out<Self>) {
//!         match message {
//!             Message::Own(()) => out.submit("2*x"),
//!             Message::Accepted(id) => self.submitted = Some(id),
//!             _ => {}
//!         }
//!     }
//!     fn view(&self) -> View<Line> { View::new(Key::new(0), Line).focused() }
//!     fn command(&self, command: &Command) -> Option<()> {
//!         (command.id == EVALUATE_SELECTION).then_some(())
//!     }
//! }
//!
//! let surface = SurfaceId::new(1);
//! let stamp = Stamp::new(Epoch::new(1), FocusGen::new(1));
//! let execute_key = KeyEvent {
//!     usage: HidUsage::new(0x07, 0x28),
//!     meaning: KeyMeaning::Named(kosmo_types::NamedKey::EXECUTE),
//!     mods: ModSet::EMPTY,
//!     action: KeyAction::Press,
//!     time: MonoTime::from_nanos(5),
//!     synthetic: false,
//! };
//! let (mut runtime, mut app, mut port) = (Runtime::new(), Calc::default(), Port::default());
//!
//! // Round 1: focus comes in and Execute is pressed; the effect goes to the port.
//! let mut input = Input::new(MonoTime::from_nanos(5));
//! input.events.push(Event::FocusIn { surface, stamp });
//! input.events.push(Event::Key { surface, stamp, event: execute_key });
//! let mut round = runtime.round(&mut app, input);
//! let receipts: Vec<Receipt<Port>> = execute(&mut port, round.take_effects());
//! let first = round.finish(&app);
//! assert!(first.record.text().contains("handled kosmo.EvaluateSelection by app"));
//!
//! // Round 2: the receipt comes back as the first message of the round.
//! let mut input = Input::new(MonoTime::from_nanos(6));
//! input.receipts = receipts;
//! let second = runtime.round(&mut app, input).finish(&app);
//! assert_eq!(app.submitted, Some(1));
//! assert_eq!(second.record.lines()[1], "receipt accepted 1");
//! ```
//!
//! The [`Shell`] drives rounds through the platform interface of
//! `kosmo-platform`. Before its first round it fetches the two fonts of
//! `kosmo-text` through `ResourceProvider` and builds its font set from
//! them. [`Shell::run_round`], the single-round entry, waits once for
//! platform events, reads the clock, takes job reports, runs both steps
//! with the execution port between them, handing its font set, the table
//! of design tokens and the size of each configured surface to the layout
//! and drawing passes, and pushes the round's accessibility updates and
//! presents its frames, keeping the region of a frame for a surface whose
//! buffer is still presented for the round after its release;
//! [`Shell::present`] prepares the coverage masks of a display list's glyph
//! runs in the glyph cache of the font set, draws the list into a surface's
//! previous frame and presents its damage.
//! [`run`] runs rounds until the platform disconnects and returns an
//! [`ExitStatus`]. With the feature `call-probes`, off by default and
//! turned on only from the command line, the single-round entry records
//! the call probe `kosmo-ui::run_round` of `kosmo-testkit` for tests of
//! call relations; ordinary builds and tests compile no probe code.
//!
//! The control tree holds at most [`MAX_WIDGETS`] controls and is at most
//! [`MAX_DEPTH`] deep; a view beyond either leaves the tree as it was and
//! the round reports a [`RebuildError`]. The timer table holds at most
//! [`TIMER_CAPACITY`] timers. [`hit_test`] finds the control at a point of
//! a [`LayoutResult`], [`paint`] builds the display list of a damage region
//! from one, as each round does from the layout of each surface, and
//! [`ResultSlot`] holds the current result of a region against the
//! versions of `kosmo-math-model`.
//!
//! [`layout()`] lays a control tree out under the [`Constraint`] of its
//! root into a [`LayoutTree`], a pure function of the controls with their
//! presentation states, the constraint, a table of design tokens and the
//! text metrics of a font set. Each control gives a [`Node`] through
//! [`Control::layout`]: a [`Row`], a [`Column`] or a [`Scroll`] container,
//! which lays out the control's children, a [`Text`] leaf, a fixed-size
//! leaf, or by default an empty leaf. A parent passes each child the
//! smallest and the largest size it may take, the child reports its size
//! and an optional baseline, and the parent places it; rows can align
//! their children by baseline. A box whose natural size exceeds its
//! largest size takes the largest size and marks the overflow of that
//! axis, and a scroll container brings the offset that its control
//! requests into the range of its content. Coordinates are `Fx` values; an
//! input beyond [`MAX_LAYOUT_INPUT`] gives
//! [`LayoutError::InputOutOfRange`], and a length beyond the numeric
//! representation [`LayoutError::Overflow`], for the whole layout.
//! [`LayoutTree::result`] gives the [`LayoutResult`] of the tree, and
//! [`LayoutTree::text`] writes the tree as text, one line for each box.
//! After the layout pass of a round, the control of each scroll container
//! learns its offset through [`Control::scrolled`]; a control that keeps
//! its request in its presentation state writes the offset back there,
//! and by default a control writes nothing.
//!
//! Rows and columns take their spacing and padding only as [`Space`]
//! tokens, and text leaves their size as a [`FontSize`] token and their
//! colour as a [`ColorToken`], so a length in pixels does not compile in
//! their place. [`TOKENS`], a [`Tokens`] table, holds the value of each:
//! spacings of 2, 4, 8 and 16 pixels, font sizes of 12, 16 and 24 pixels
//! and five opaque colours. Controls draw with the colours that
//! [`DrawContext::color`] gives for colour tokens, and the drawing pass
//! refuses a colour or a glyph size that is not the value of a token with
//! [`PaintError::Untokened`].

#![forbid(unsafe_code)]

mod app;
mod command;
mod focus;
mod hit;
mod layout;
mod paint;
mod port;
mod record;
mod round;
mod shell;
mod slot;
mod timer;
mod tokens;
mod tree;

#[cfg(test)]
mod random;
#[cfg(test)]
mod testing;

pub use app::{App, Context, Control, DrawContext, Handling, Message, Out, Phase};
pub use command::{
    Binding, CANCEL_JOB, Command, CommandId, Conflict, DELETE_BACKWARD, EVALUATE_SELECTION,
    INSERT_TEXT, MINIMAL_KEY_MAP, MOVE_LEFT, MOVE_RIGHT, Mods, Trigger, check_table, resolve,
};
pub use focus::{Event, Stamp, SurfaceId};
pub use hit::{LayoutBox, LayoutError, LayoutResult, hit_test};
pub use layout::{
    Align, Axis, Column, Constraint, Font, LaidText, LayoutTree, MAX_LAYOUT_INPUT, Node, Overflow,
    Row, RowAlign, Scroll, Text, TreeBox, layout,
};
pub use paint::{PaintError, paint};
pub use port::{Effect, JobPort, Receipt, execute};
pub use record::Record;
pub use round::{Counts, Input, LayoutInput, Output, Round, Runtime, ViewPass};
pub use shell::{ExitReason, ExitStatus, RoundEnd, Shell, ShellError, run};
pub use slot::{Delivered, ResultSlot, Shown, Tagged};
pub use timer::{TIMER_CAPACITY, TimerLabel};
pub use tokens::{ColorToken, FontSize, Space, TOKENS, Tokens};
pub use tree::{Key, Kind, MAX_DEPTH, MAX_WIDGETS, RebuildError, Tree, View, WidgetId};

/// The longest that the event loop waits while jobs that it submitted have
/// not ended, in nanoseconds: 16 ms, one frame at 60 Hz rounded down. While
/// the execution port reports such jobs, the next deadline of a round is at
/// most its time plus this interval.
pub const JOB_POLL_INTERVAL: u64 = 16_000_000;
