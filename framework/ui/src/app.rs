//! The application and control interfaces: what `update`, `view`, the
//! event handlers of controls and the drawing entry receive.

use kosmo_render::{Color, ListBuilder, ListError};
use kosmo_types::{Fx, MonoTime, Rect, Size};

use crate::command::{Binding, Command};
use crate::layout::{LaidText, Node};
use crate::paint::PaintError;
use crate::port::{Effect, JobPort};
use crate::timer::{Requester, TimerLabel, TimerRequest};
use crate::tokens::{ColorToken, Tokens};
use crate::tree::{Kind, View, WidgetId};

/// The request type of an application's execution port.
pub(crate) type RequestOf<A> = <<A as App>::Port as JobPort>::Request;
/// The submission identity of an application's execution port.
pub(crate) type SubmissionOf<A> = <<A as App>::Port as JobPort>::Submission;
/// The presentation state of an application's controls.
pub(crate) type StateOf<A> = <<A as App>::Control as Control<A>>::State;

/// An application: a model with `update` and `view`.
///
/// `update` is called once for each message of a round, in order, and is
/// the only place that changes the model. It writes effects and timer
/// requests into its [`Out`]. `view` reads the model and returns the view
/// tree; the framework calls it once in a round that handled a message,
/// and in each round until a view tree has been applied. Neither reaches
/// the platform, the execution port or the control tree.
///
/// The application is also the last handler of a command that no control
/// handled, and its binding table comes after the tables of the focused
/// control and its ancestors.
pub trait App: Sized {
    /// The application's own messages, which controls and
    /// [`App::command`] output.
    type Message;
    /// The controls of the application's view trees.
    type Control: Control<Self>;
    /// The execution port whose requests, submission identities, receipts
    /// and reports the application uses. The application never holds it.
    type Port: JobPort;

    /// Handles one message.
    fn update(&mut self, message: Message<Self>, out: &mut Out<Self>);

    /// The view tree of the model.
    fn view(&self) -> View<Self::Control>;

    /// The application's binding table, constant data that the framework
    /// looks up after the tables of the focus chain and before the minimal
    /// key map. None by default.
    fn bindings(&self) -> &'static [Binding] {
        &[]
    }

    /// Handles a command that no control handled: a message when the
    /// application handles it, `None` when it does not. Handles none by
    /// default.
    fn command(&self, command: &Command) -> Option<Self::Message> {
        let _ = command;
        None
    }
}

/// A control of a view tree: the value that the view gives it, compared
/// by its kind and key with the control in the tree.
///
/// The framework keeps the control and its presentation state; the
/// control's methods get the state through their context. Controls receive
/// commands, never raw key events. The drawing entry [`Control::draw`]
/// takes only the control and a [`DrawContext`], which holds only the
/// control's identity and presentation state: it reaches neither the model,
/// nor the execution port, nor the platform.
///
/// The three examples that follow fail to compile because the drawing
/// entry tries to reach the model, the execution port and a platform port
/// through its parameters; the three after them are the same code without
/// that line, and compile.
///
/// ```compile_fail
/// # use kosmo_ui::{App, Control, DrawContext, JobPort, Key, Kind, Message, Out, View};
/// # struct Port;
/// # impl JobPort for Port {
/// #     type Request = u32;
/// #     type Submission = u32;
/// #     type Rejection = ();
/// #     type CancelAnswer = ();
/// #     type Report = ();
/// #     fn submit(&mut self, request: u32) -> Result<u32, ()> { Ok(request) }
/// #     fn cancel(&mut self, _: &u32) {}
/// #     fn take_reports(&mut self, _: &mut Vec<()>) -> usize { 0 }
/// # }
/// struct Model { count: u32 }
/// # impl App for Model {
/// #     type Message = ();
/// #     type Control = Label;
/// #     type Port = Port;
/// #     fn update(&mut self, _: Message<Self>, _: &mut Out<Self>) {}
/// #     fn view(&self) -> View<Label> { View::new(Key::new(0), Label) }
/// # }
/// #[derive(Clone)]
/// struct Label;
/// impl Control<Model> for Label {
///     type State = ();
///     fn kind(&self) -> Kind { Kind::new("label") }
///     fn draw(&self, cx: &mut DrawContext<'_, ()>) {
///         let _ = cx.id();
///         cx.model_mut().count += 1;
///     }
/// }
/// ```
///
/// ```compile_fail
/// # use kosmo_ui::{App, Control, DrawContext, JobPort, Key, Kind, Message, Out, View};
/// # struct Model;
/// # impl App for Model {
/// #     type Message = ();
/// #     type Control = Label;
/// #     type Port = Port;
/// #     fn update(&mut self, _: Message<Self>, _: &mut Out<Self>) {}
/// #     fn view(&self) -> View<Label> { View::new(Key::new(0), Label) }
/// # }
/// struct Port;
/// impl JobPort for Port {
///     type Request = u32;
///     type Submission = u32;
///     type Rejection = ();
///     type CancelAnswer = ();
///     type Report = ();
///     fn submit(&mut self, request: u32) -> Result<u32, ()> { Ok(request) }
///     fn cancel(&mut self, _: &u32) {}
///     fn take_reports(&mut self, _: &mut Vec<()>) -> usize { 0 }
/// }
/// #[derive(Clone)]
/// struct Label;
/// impl Control<Model> for Label {
///     type State = ();
///     fn kind(&self) -> Kind { Kind::new("label") }
///     fn draw(&self, cx: &mut DrawContext<'_, ()>) {
///         let _ = cx.id();
///         let _ = JobPort::submit(cx.job_port(), 7);
///     }
/// }
/// ```
///
/// ```compile_fail
/// # use kosmo_ui::{App, Control, DrawContext, JobPort, Key, Kind, Message, Out, View};
/// # struct Port;
/// # impl JobPort for Port {
/// #     type Request = u32;
/// #     type Submission = u32;
/// #     type Rejection = ();
/// #     type CancelAnswer = ();
/// #     type Report = ();
/// #     fn submit(&mut self, request: u32) -> Result<u32, ()> { Ok(request) }
/// #     fn cancel(&mut self, _: &u32) {}
/// #     fn take_reports(&mut self, _: &mut Vec<()>) -> usize { 0 }
/// # }
/// # struct Model;
/// # impl App for Model {
/// #     type Message = ();
/// #     type Control = Label;
/// #     type Port = Port;
/// #     fn update(&mut self, _: Message<Self>, _: &mut Out<Self>) {}
/// #     fn view(&self) -> View<Label> { View::new(Key::new(0), Label) }
/// # }
/// // A stand-in for a port of the platform interface.
/// trait SurfacePort { fn create_surface(&mut self) -> u32; }
/// #[derive(Clone)]
/// struct Label;
/// impl Control<Model> for Label {
///     type State = ();
///     fn kind(&self) -> Kind { Kind::new("label") }
///     fn draw(&self, cx: &mut DrawContext<'_, ()>) {
///         let _ = cx.id();
///         let _ = SurfacePort::create_surface(cx.platform());
///     }
/// }
/// ```
///
/// ```
/// # use kosmo_ui::{App, Control, DrawContext, JobPort, Key, Kind, Message, Out, View};
/// # struct Port;
/// # impl JobPort for Port {
/// #     type Request = u32;
/// #     type Submission = u32;
/// #     type Rejection = ();
/// #     type CancelAnswer = ();
/// #     type Report = ();
/// #     fn submit(&mut self, request: u32) -> Result<u32, ()> { Ok(request) }
/// #     fn cancel(&mut self, _: &u32) {}
/// #     fn take_reports(&mut self, _: &mut Vec<()>) -> usize { 0 }
/// # }
/// struct Model { count: u32 }
/// # impl App for Model {
/// #     type Message = ();
/// #     type Control = Label;
/// #     type Port = Port;
/// #     fn update(&mut self, _: Message<Self>, _: &mut Out<Self>) {}
/// #     fn view(&self) -> View<Label> { View::new(Key::new(0), Label) }
/// # }
/// #[derive(Clone)]
/// struct Label;
/// impl Control<Model> for Label {
///     type State = ();
///     fn kind(&self) -> Kind { Kind::new("label") }
///     fn draw(&self, cx: &mut DrawContext<'_, ()>) {
///         let _ = cx.id();
///     }
/// }
/// ```
///
/// ```
/// # use kosmo_ui::{App, Control, DrawContext, JobPort, Key, Kind, Message, Out, View};
/// # struct Model;
/// # impl App for Model {
/// #     type Message = ();
/// #     type Control = Label;
/// #     type Port = Port;
/// #     fn update(&mut self, _: Message<Self>, _: &mut Out<Self>) {}
/// #     fn view(&self) -> View<Label> { View::new(Key::new(0), Label) }
/// # }
/// struct Port;
/// impl JobPort for Port {
///     type Request = u32;
///     type Submission = u32;
///     type Rejection = ();
///     type CancelAnswer = ();
///     type Report = ();
///     fn submit(&mut self, request: u32) -> Result<u32, ()> { Ok(request) }
///     fn cancel(&mut self, _: &u32) {}
///     fn take_reports(&mut self, _: &mut Vec<()>) -> usize { 0 }
/// }
/// #[derive(Clone)]
/// struct Label;
/// impl Control<Model> for Label {
///     type State = ();
///     fn kind(&self) -> Kind { Kind::new("label") }
///     fn draw(&self, cx: &mut DrawContext<'_, ()>) {
///         let _ = cx.id();
///     }
/// }
/// ```
///
/// ```
/// # use kosmo_ui::{App, Control, DrawContext, JobPort, Key, Kind, Message, Out, View};
/// # struct Port;
/// # impl JobPort for Port {
/// #     type Request = u32;
/// #     type Submission = u32;
/// #     type Rejection = ();
/// #     type CancelAnswer = ();
/// #     type Report = ();
/// #     fn submit(&mut self, request: u32) -> Result<u32, ()> { Ok(request) }
/// #     fn cancel(&mut self, _: &u32) {}
/// #     fn take_reports(&mut self, _: &mut Vec<()>) -> usize { 0 }
/// # }
/// # struct Model;
/// # impl App for Model {
/// #     type Message = ();
/// #     type Control = Label;
/// #     type Port = Port;
/// #     fn update(&mut self, _: Message<Self>, _: &mut Out<Self>) {}
/// #     fn view(&self) -> View<Label> { View::new(Key::new(0), Label) }
/// # }
/// // A stand-in for a port of the platform interface.
/// trait SurfacePort { fn create_surface(&mut self) -> u32; }
/// #[derive(Clone)]
/// struct Label;
/// impl Control<Model> for Label {
///     type State = ();
///     fn kind(&self) -> Kind { Kind::new("label") }
///     fn draw(&self, cx: &mut DrawContext<'_, ()>) {
///         let _ = cx.id();
///     }
/// }
/// ```
pub trait Control<A: App>: Clone {
    /// The control's presentation state, such as a scroll offset. A new
    /// control starts with the default, and the state stays while the
    /// control keeps its identity.
    type State: Default;

    /// The kind of the control. A control of the tree keeps its identity
    /// only while the view gives a control of the same kind with the same
    /// key under the same parent.
    fn kind(&self) -> Kind;

    /// The control's binding table, constant data that the framework looks
    /// up when the control is the focused control or one of its ancestors.
    /// None by default.
    fn bindings(&self) -> &'static [Binding] {
        &[]
    }

    /// Handles a command in a phase of dispatch. A control that does not
    /// handle the command, or that does not know the phase, returns
    /// [`Handling::Unhandled`]; that is the default.
    fn handle(
        &self,
        phase: Phase,
        command: &Command,
        cx: &mut Context<'_, A, Self::State>,
    ) -> Handling {
        let _ = (phase, command, cx);
        Handling::Unhandled
    }

    /// Learns that the surface gained focus, when `holds` is true, or lost
    /// it. Only the region's root receives this, and only the changes that
    /// arrive while it is the region's root, of every surface alike; a
    /// region's root that a later view creates is not told that its surface
    /// already holds focus. Does nothing by default.
    fn focus(&self, holds: bool, cx: &mut Context<'_, A, Self::State>) {
        let _ = (holds, cx);
    }

    /// Receives a timer that the control requested and that is due. Does
    /// nothing by default.
    fn timer(&self, label: TimerLabel, cx: &mut Context<'_, A, Self::State>) {
        let _ = (label, cx);
    }

    /// The control's layout node, from its value and its presentation
    /// state `state`: a row, a column or a scroll container, which lays
    /// out the control's children, or a leaf. [`layout`](crate::layout())
    /// calls it once for each control it lays out. By default the control
    /// is [`Node::Empty`], a leaf of size zero without a baseline, which
    /// draws nothing.
    fn layout(&self, state: &Self::State) -> Node<'_> {
        let _ = state;
        Node::Empty
    }

    /// Learns the offset that the layout pass of a round gave the
    /// control's scroll container along its axis: the request of its node
    /// brought into the range of its content. A control that keeps its
    /// request in its presentation state writes the offset there, so that
    /// requests beyond the range do not add up. When several surfaces lay
    /// the container out, the offset is the smallest of theirs, which is
    /// within the range of each. Does nothing by default.
    fn scrolled(&self, offset: Fx, state: &mut Self::State) {
        let _ = (offset, state);
    }

    /// The drawing entry. The drawing pass calls it in preorder; its
    /// context holds only the control's identity and presentation state,
    /// the size of its box, the text that the layout pass laid out for a
    /// text leaf, the colours of the design tokens and the display list
    /// that it draws into.
    fn draw(&self, cx: &mut DrawContext<'_, Self::State>);
}

/// A message of a round, which `update` handles.
///
/// The event pass queues the messages of a round in this order: the
/// receipts of the previous round's effects, the messages that key events
/// give through dispatch, the messages of other platform events, the job
/// reports and the due timers.
#[non_exhaustive]
pub enum Message<A: App> {
    /// A message that a control or [`App::command`] output.
    Own(A::Message),
    /// The execution port accepted a submission and gave it this identity.
    Accepted(SubmissionOf<A>),
    /// The execution port refused a submission. A refused submission is
    /// never the region's latest submission.
    Rejected(<A::Port as JobPort>::Rejection),
    /// A job report, which `update` hands to the region's result slot.
    Report(<A::Port as JobPort>::Report),
    /// A timer that the application requested is due.
    Timer(TimerLabel),
}

/// Where `update` writes its effects and timer requests.
pub struct Out<A: App> {
    pub(crate) effects: Vec<Effect<A::Port>>,
    pub(crate) timers: Vec<TimerRequest>,
}

impl<A: App> Out<A> {
    pub(crate) fn new() -> Out<A> {
        Out {
            effects: Vec::new(),
            timers: Vec::new(),
        }
    }

    /// Submits a job with `request`.
    pub fn submit(&mut self, request: RequestOf<A>) {
        self.effects.push(Effect::Submit(request));
    }

    /// Cancels the job of `submission`, the identity of its acceptance.
    pub fn cancel(&mut self, submission: SubmissionOf<A>) {
        self.effects.push(Effect::Cancel(submission));
    }

    /// Requests a timer that fires once, in the first round whose time is
    /// not earlier than `deadline`, as a [`Message::Timer`] with `label`.
    /// A request to a full timer table is refused and recorded, and
    /// `update` receives no message for the refusal.
    pub fn timer(&mut self, deadline: MonoTime, label: TimerLabel) {
        self.timers.push(TimerRequest {
            deadline,
            label,
            requester: Requester::App,
        });
    }
}

/// What the event handlers of a control reach: its identity and
/// presentation state, and the messages, effects and timer requests that it
/// outputs as values.
///
/// A handler reads the presentation state with [`Context::state`] and
/// changes it through [`Context::state_mut`]; only taking it through
/// `state_mut` counts as a change of the presentation state, after which
/// the next drawing pass of the rounds draws every surface whole.
pub struct Context<'a, A: App, S> {
    pub(crate) id: WidgetId,
    pub(crate) state: &'a mut S,
    pub(crate) messages: &'a mut Vec<Message<A>>,
    pub(crate) out: &'a mut Out<A>,
    /// Whether the handler took the presentation state through
    /// [`Context::state_mut`].
    pub(crate) state_taken: bool,
}

impl<A: App, S> Context<'_, A, S> {
    /// The control's identity.
    pub fn id(&self) -> WidgetId {
        self.id
    }

    /// The control's presentation state. Reading it is no change of it.
    pub fn state(&self) -> &S {
        self.state
    }

    /// The control's presentation state, to change it.
    ///
    /// Taking it counts as a change of the presentation state, also when
    /// the handler leaves its value as it was, since states are not
    /// compared: the next drawing pass of the rounds draws every surface
    /// that has a layout whole. A control that changes its state through
    /// interior mutability without taking it here gives no frame.
    pub fn state_mut(&mut self) -> &mut S {
        self.state_taken = true;
        self.state
    }

    /// Outputs a message for `update`, queued after the messages already
    /// output in this round.
    pub fn message(&mut self, message: A::Message) {
        self.messages.push(Message::Own(message));
    }

    /// Submits a job with `request`.
    pub fn submit(&mut self, request: RequestOf<A>) {
        self.out.submit(request);
    }

    /// Cancels the job of `submission`.
    pub fn cancel(&mut self, submission: SubmissionOf<A>) {
        self.out.cancel(submission);
    }

    /// Requests a timer for the control, which its [`Control::timer`]
    /// receives when it is due; it is dropped if the control no longer
    /// exists then. A request to a full timer table is refused and
    /// recorded, and the control receives nothing for the refusal.
    pub fn timer(&mut self, deadline: MonoTime, label: TimerLabel) {
        self.out.timers.push(TimerRequest {
            deadline,
            label,
            requester: Requester::Control(self.id),
        });
    }
}

/// What the drawing entry of a control reaches: the control's identity and
/// its presentation state, which it can only read, the size of its box, the
/// text that the layout laid out for it, the colours of the design tokens,
/// and the display list of the drawing pass, to which it appends commands.
pub struct DrawContext<'a, S> {
    id: WidgetId,
    state: &'a S,
    size: Size,
    text: Option<&'a LaidText>,
    canvas: Canvas<'a>,
}

impl<'a, S> DrawContext<'a, S> {
    /// The context for drawing the control `id` with `state` into `canvas`,
    /// with a box of `size` and the laid-out `text` of a text leaf.
    pub(crate) fn new(
        id: WidgetId,
        state: &'a S,
        size: Size,
        text: Option<&'a LaidText>,
        canvas: Canvas<'a>,
    ) -> DrawContext<'a, S> {
        DrawContext {
            id,
            state,
            size,
            text,
            canvas,
        }
    }

    /// The control's identity.
    pub fn id(&self) -> WidgetId {
        self.id
    }

    /// The control's presentation state.
    pub fn state(&self) -> &S {
        self.state
    }

    /// The size of the control's box.
    pub fn size(&self) -> Size {
        self.size
    }

    /// The text of the control's text leaf as the layout pass of a round
    /// laid it out, in the coordinates of its box, with its colour token.
    /// It is `None` for any other control, and for every control that
    /// [`paint`](crate::paint()) draws from a layout result, which holds no
    /// text.
    pub fn text(&self) -> Option<&'a LaidText> {
        self.text
    }

    /// The colour of the colour token `token` in the table of design
    /// tokens that the drawing pass checks the commands against.
    pub fn color(&self, token: ColorToken) -> Color {
        self.canvas.tokens.color(token)
    }

    /// Appends `command` to the display list, relative to the control's box:
    /// (0, 0) is the box's top-left corner, and drawing is clipped to the
    /// box. A [`PopClip`](kosmo_render::Command::PopClip) ends only a group that the control
    /// started, and the control ends each group that it starts. Its colours
    /// are values of the table of design tokens, which
    /// [`DrawContext::color`] gives, and the size of a glyph run is the
    /// value of a font size token. A command with another colour or size,
    /// a command that the list refuses, or a group left open, makes the
    /// drawing pass return the error, and the control's later commands are
    /// dropped.
    pub fn draw(&mut self, command: kosmo_render::Command) {
        self.canvas.push(command);
    }

    /// The canvas, to end the control's drawing.
    pub(crate) fn into_canvas(self) -> Canvas<'a> {
        self.canvas
    }
}

/// The display list as the drawing entry of one control reaches it: the
/// control's commands go into a group that clips them to its box and moves
/// them to the box's position, started with the control's first command.
pub(crate) struct Canvas<'a> {
    list: &'a mut ListBuilder,
    /// The table of design tokens that the commands are checked against.
    tokens: &'a Tokens,
    /// The control.
    id: WidgetId,
    /// The control's box, in the coordinates of the group around it.
    rect: Rect,
    /// Whether the group of the box is started.
    started: bool,
    /// The groups that the control started and has not ended.
    depth: usize,
    /// The first command that the check or the list refused.
    error: Option<PaintError>,
}

impl<'a> Canvas<'a> {
    /// The canvas of the control `id` whose box is `rect` in the
    /// coordinates of the group around it, checked against `tokens`.
    pub(crate) fn new(
        list: &'a mut ListBuilder,
        tokens: &'a Tokens,
        id: WidgetId,
        rect: Rect,
    ) -> Canvas<'a> {
        Canvas {
            list,
            tokens,
            id,
            rect,
            started: false,
            depth: 0,
            error: None,
        }
    }

    fn push(&mut self, command: kosmo_render::Command) {
        if self.error.is_some() {
            return;
        }
        if !self.tokens.admits(&command) {
            self.error = Some(PaintError::Untokened(self.id));
            return;
        }
        if let Err(error) = self.append(command) {
            self.error = Some(PaintError::List(error));
        }
    }

    fn append(&mut self, command: kosmo_render::Command) -> Result<(), ListError> {
        let depth = match command {
            kosmo_render::Command::PushClip { .. } => self.depth + 1,
            kosmo_render::Command::PopClip => self
                .depth
                .checked_sub(1)
                .ok_or(ListError::UnpairedPopClip)?,
            _ => self.depth,
        };
        if !self.started {
            self.list.push(kosmo_render::Command::PushClip {
                rect: self.rect,
                offset: self.rect.origin(),
            })?;
            self.started = true;
        }
        self.list.push(command)?;
        self.depth = depth;
        Ok(())
    }

    /// Ends the control's drawing: the first error, a group that the
    /// control left open, or the end of the group of its box.
    pub(crate) fn finish(self) -> Result<(), PaintError> {
        if let Some(error) = self.error {
            return Err(error);
        }
        if self.depth > 0 {
            return Err(ListError::UnclosedClip.into());
        }
        if self.started {
            self.list.push(kosmo_render::Command::PopClip)?;
        }
        Ok(())
    }
}

/// A phase of dispatch. A new phase is a new variant; a control that does
/// not know a phase returns [`Handling::Unhandled`] for it.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Phase {
    /// The command reaches the focused control.
    Target,
    /// The command reaches an ancestor of the focused control, up to the
    /// region's root, after the controls below it did not handle it.
    Bubble,
}

/// Whether a handler handled a command. The first handler that handles it
/// ends dispatch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Handling {
    /// The handler handled the command.
    Handled,
    /// The handler did not handle the command.
    Unhandled,
}
