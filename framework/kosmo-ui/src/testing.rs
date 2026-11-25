//! A test application for the unit tests: its view is whatever the test
//! sets, its controls handle the commands they are given, and it logs
//! what `update` receives.

use kosmo_types::{
    Epoch, FocusGen, HidUsage, KeyAction, KeyEvent, KeyMeaning, ModSet, MonoTime, NamedKey,
};

use crate::app::{App, Context, Control, DrawContext, Handling, Message, Out, Phase};
use crate::command::{Command, CommandId};
use crate::focus::{Event, Stamp, SurfaceId};
use crate::port::JobPort;
use crate::timer::TimerLabel;
use crate::tree::{Key, Kind, View};

/// A control: its kind, a value that the view gives it, the command that
/// it handles, if any, and what it does when it handles it. Its
/// presentation state counts the commands it handled.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Widget {
    pub(crate) kind: Kind,
    pub(crate) value: u32,
    pub(crate) handles: Option<CommandId>,
    pub(crate) reply: Reply,
}

/// What a widget does when it handles a command, besides counting it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Reply {
    /// It outputs a message.
    Message,
    /// It changes only its presentation state.
    StateOnly,
    /// It requests a timer at this time, with this time as its label.
    Timer(u64),
}

/// A widget of `kind` with `value` that handles no command.
pub(crate) fn widget(kind: &'static str, value: u32) -> Widget {
    Widget {
        kind: Kind::new(kind),
        value,
        handles: None,
        reply: Reply::Message,
    }
}

/// A view node with `key` of a widget of `kind` with `value`.
pub(crate) fn node(key: u64, kind: &'static str, value: u32) -> View<Widget> {
    View::new(Key::new(key), widget(kind, value))
}

/// What `update` received, in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Logged {
    Own(Note),
    Accepted(u64),
    Rejected(&'static str),
    Report(u64),
    Timer(u64),
}

/// The application's own messages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Note {
    /// A control handled a command.
    Handled(CommandId),
    /// The application handled a command.
    App(CommandId),
    /// A timer of a control was due.
    ControlTimer(u64),
}

/// The test application: the view that the test sets, the command that the
/// application handles, the requests that `update` submits for each of the
/// application's own messages, a log of what `update` received and the
/// number of calls of `view`.
#[derive(Default)]
pub(crate) struct TestApp {
    pub(crate) view: Option<View<Widget>>,
    pub(crate) handles: Option<CommandId>,
    pub(crate) submits: Vec<&'static str>,
    pub(crate) log: Vec<Logged>,
    pub(crate) views: std::cell::Cell<u32>,
}

impl Control<TestApp> for Widget {
    type State = u32;

    fn kind(&self) -> Kind {
        self.kind
    }

    fn handle(
        &self,
        _phase: Phase,
        command: &Command,
        cx: &mut Context<'_, TestApp, u32>,
    ) -> Handling {
        if self.handles == Some(command.id) {
            *cx.state_mut() += 1;
            match self.reply {
                Reply::Message => cx.message(Note::Handled(command.id)),
                Reply::StateOnly => {}
                Reply::Timer(at) => cx.timer(MonoTime::from_nanos(at), TimerLabel::new(at)),
            }
            Handling::Handled
        } else {
            Handling::Unhandled
        }
    }

    fn timer(&self, label: TimerLabel, cx: &mut Context<'_, TestApp, u32>) {
        cx.message(Note::ControlTimer(label.get()));
    }

    fn draw(&self, _cx: &mut DrawContext<'_, u32>) {}
}

impl App for TestApp {
    type Message = Note;
    type Control = Widget;
    type Port = TestPort;

    fn update(&mut self, message: Message<Self>, out: &mut Out<Self>) {
        if let Message::Own(Note::App(_)) = message {
            for &request in &self.submits {
                out.submit(request);
            }
        }
        self.log.push(match message {
            Message::Own(note) => Logged::Own(note),
            Message::Accepted(id) => Logged::Accepted(id),
            Message::Rejected(why) => Logged::Rejected(why),
            Message::Report(id) => Logged::Report(id),
            Message::Timer(label) => Logged::Timer(label.get()),
        });
    }

    fn view(&self) -> View<Widget> {
        self.views.set(self.views.get() + 1);
        self.view
            .clone()
            .unwrap_or_else(|| View::new(Key::new(0), widget("root", 0)))
    }

    fn command(&self, command: &Command) -> Option<Note> {
        (self.handles == Some(command.id)).then_some(Note::App(command.id))
    }
}

/// A port that numbers its submissions from 1 and refuses none.
#[derive(Default)]
pub(crate) struct TestPort {
    pub(crate) submitted: u64,
}

impl JobPort for TestPort {
    type Request = &'static str;
    type Submission = u64;
    type Rejection = &'static str;
    type CancelAnswer = &'static str;
    type Report = u64;

    fn submit(&mut self, _request: &'static str) -> Result<u64, &'static str> {
        self.submitted += 1;
        Ok(self.submitted)
    }

    fn cancel(&mut self, _submission: &u64) -> &'static str {
        "accepted"
    }

    fn take_reports(&mut self, _out: &mut Vec<u64>) -> usize {
        0
    }
}

/// The surface of the tests.
pub(crate) const SURFACE: SurfaceId = SurfaceId::new(1);

/// The stamp of epoch `epoch` and focus generation `generation`.
pub(crate) fn stamp(epoch: u64, generation: u64) -> Stamp {
    Stamp::new(Epoch::new(epoch), FocusGen::new(generation))
}

/// A key event of `meaning` and `action` at time 0, without modifiers.
pub(crate) fn key_event(meaning: KeyMeaning, action: KeyAction) -> KeyEvent {
    KeyEvent {
        usage: HidUsage::new(0x07, 0x04),
        meaning,
        mods: ModSet::EMPTY,
        action,
        time: MonoTime::ZERO,
        synthetic: false,
    }
}

/// A key press of the named key `key` with `stamp` on the test surface.
pub(crate) fn press(key: NamedKey, stamp: Stamp) -> Event {
    Event::Key {
        surface: SURFACE,
        stamp,
        event: key_event(KeyMeaning::Named(key), KeyAction::Press),
    }
}
