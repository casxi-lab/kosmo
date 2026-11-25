//! A one-line calculator for the integration tests. Its model holds the
//! region: a line of `kosmo-math-model`, the context and settings
//! versions, the latest accepted submission and the result slot. Its
//! controls handle the commands that the test gives them, and its view is
//! either a line under a root, or a chain of four controls.

#![allow(dead_code, reason = "each test file uses part of the support")]

use kosmo_math_model::version::{ContextVersion, SettingsVersion, Versions};
use kosmo_math_model::{EditBuffer, Item};
use kosmo_types::{
    Epoch, FocusGen, HidUsage, KeyAction, KeyEvent, KeyMeaning, ModSet, MonoTime, NamedKey,
};
use kosmo_ui::{
    App, Binding, CANCEL_JOB, Command, CommandId, Context, Control, DELETE_BACKWARD, Delivered,
    DrawContext, EVALUATE_SELECTION, Event, Handling, INSERT_TEXT, Input, JobPort, Key, Kind,
    MOVE_LEFT, MOVE_RIGHT, Message, Out, Output, Phase, Receipt, ResultSlot, Runtime, Stamp,
    SurfaceId, Tagged, TimerLabel, View, execute,
};

/// A control: its name, which is also its kind, the commands that it
/// handles and its binding table. Its presentation state counts the
/// commands that reached it, handled or not.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Part {
    pub name: &'static str,
    pub handles: Vec<CommandId>,
    pub bindings: &'static [Binding],
}

impl Part {
    pub fn new(name: &'static str, handles: &[CommandId]) -> Part {
        Part {
            name,
            handles: handles.to_vec(),
            bindings: &[],
        }
    }
}

/// The application's message: who handled which command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Note {
    pub by: &'static str,
    pub command: Command,
}

impl Control<Calc> for Part {
    type State = u32;

    fn kind(&self) -> Kind {
        Kind::new(self.name)
    }

    fn bindings(&self) -> &'static [Binding] {
        self.bindings
    }

    fn handle(&self, _: Phase, command: &Command, cx: &mut Context<'_, Calc, u32>) -> Handling {
        *cx.state_mut() += 1;
        if self.handles.contains(&command.id) {
            cx.message(Note {
                by: self.name,
                command: *command,
            });
            Handling::Handled
        } else {
            Handling::Unhandled
        }
    }

    fn draw(&self, _: &mut DrawContext<'_, u32>) {}
}

/// A job request: the line's text and the versions it was taken at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub text: String,
    pub versions: Versions,
}

/// The execution port of the tests: it numbers accepted submissions from
/// 1, and refuses the next submission when told to.
#[derive(Default)]
pub struct Port {
    pub submitted: u64,
    pub refuse_next: bool,
}

impl JobPort for Port {
    type Request = Request;
    type Submission = u64;
    type Rejection = &'static str;
    type CancelAnswer = &'static str;
    type Report = Tagged<String, u64>;

    fn submit(&mut self, _: Request) -> Result<u64, &'static str> {
        if std::mem::take(&mut self.refuse_next) {
            return Err("too many live jobs");
        }
        self.submitted += 1;
        Ok(self.submitted)
    }

    fn cancel(&mut self, _: &u64) -> &'static str {
        "cancel accepted"
    }

    fn take_reports(&mut self, _: &mut Vec<Tagged<String, u64>>) -> usize {
        0
    }
}

/// The shape of the view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    /// A root, which is the region's root, with the focused line below it.
    Line,
    /// A root, the region's root below it, a parent below that and the
    /// focused control at the bottom.
    Chain,
}

/// The calculator: the region's line, versions and latest submission, the
/// result slot and what was delivered to it, the commands that the
/// application handles, the controls of the view, a timer to request on
/// each submission, and a log of the other messages.
pub struct Calc {
    pub line: EditBuffer,
    pub context: u64,
    pub settings: u64,
    pub latest: Option<u64>,
    pub slot: ResultSlot<String, u64>,
    pub delivered: Vec<Delivered<String, u64>>,
    pub handles: Vec<CommandId>,
    pub shape: Shape,
    pub parts: [Part; 4],
    pub timer_on_submit: Option<MonoTime>,
    pub log: Vec<String>,
}

impl Calc {
    /// A calculator with the shape `shape` whose controls handle nothing.
    pub fn new(shape: Shape) -> Calc {
        Calc {
            line: EditBuffer::new(),
            context: 0,
            settings: 0,
            latest: None,
            slot: ResultSlot::new(),
            delivered: Vec::new(),
            handles: Vec::new(),
            shape,
            parts: [
                Part::new("root", &[]),
                Part::new("region", &[]),
                Part::new("parent", &[]),
                Part::new("focused", &[]),
            ],
            timer_on_submit: None,
            log: Vec::new(),
        }
    }

    /// The region's current versions.
    pub fn versions(&self) -> Versions {
        Versions {
            source: self.line.version(),
            context: ContextVersion::new(self.context),
            settings: SettingsVersion::new(self.settings),
        }
    }

    /// The region's current versions as numbers.
    pub fn numbers(&self) -> (u64, u64, u64) {
        let versions = self.versions();
        (
            versions.source.get(),
            versions.context.get(),
            versions.settings.get(),
        )
    }

    /// The characters of the line.
    pub fn text(&self) -> String {
        self.line
            .items()
            .iter()
            .filter_map(|item| match item {
                Item::Char(c) => Some(*c),
                _ => None,
            })
            .collect()
    }

    fn edit(&mut self, command: Command) {
        let edited = match (command.id, command.text) {
            (INSERT_TEXT, Some(c)) => self.line.insert_char(c),
            (DELETE_BACKWARD, _) => self.line.delete_backward(),
            (MOVE_LEFT, _) => {
                self.line.move_left();
                Ok(())
            }
            (MOVE_RIGHT, _) => {
                self.line.move_right();
                Ok(())
            }
            _ => Ok(()),
        };
        if let Err(rejected) = edited {
            self.log.push(format!("edit rejected: {rejected}"));
        }
    }
}

impl App for Calc {
    type Message = Note;
    type Control = Part;
    type Port = Port;

    fn update(&mut self, message: Message<Self>, out: &mut Out<Self>) {
        match message {
            Message::Own(note) => {
                self.log.push(format!("{} by {}", note.command, note.by));
                if note.command.id == EVALUATE_SELECTION {
                    let text = self.text();
                    out.submit(Request {
                        text,
                        versions: self.versions(),
                    });
                    if let Some(deadline) = self.timer_on_submit {
                        out.timer(deadline, TimerLabel::new(1));
                    }
                } else if note.command.id == CANCEL_JOB {
                    if let Some(latest) = self.latest {
                        out.cancel(latest);
                    }
                } else {
                    self.edit(note.command);
                }
            }
            Message::Accepted(submission) => {
                // Only an acceptance changes the latest submission.
                self.latest = Some(submission);
                self.log.push(format!("accepted {submission}"));
            }
            Message::Rejected(why) => self.log.push(format!("rejected: {why}")),
            Message::Report(result) => {
                let delivered = self
                    .slot
                    .deliver(result, self.versions(), self.latest.as_ref());
                self.delivered.push(delivered);
            }
            Message::Timer(label) => self.log.push(format!("timer {}", label.get())),
            _ => self.log.push("another message".to_string()),
        }
    }

    fn view(&self) -> View<Part> {
        let [root, region, parent, focused] = self.parts.clone();
        match self.shape {
            Shape::Line => View::new(Key::new(0), root)
                .region()
                .child(View::new(Key::new(3), focused).focused()),
            Shape::Chain => {
                View::new(Key::new(0), root).child(View::new(Key::new(1), region).region().child(
                    View::new(Key::new(2), parent).child(View::new(Key::new(3), focused).focused()),
                ))
            }
        }
    }

    fn command(&self, command: &Command) -> Option<Note> {
        self.handles.contains(&command.id).then_some(Note {
            by: "app",
            command: *command,
        })
    }
}

/// The surface of the tests.
pub const SURFACE: SurfaceId = SurfaceId::new(1);

/// The stamp of `epoch` and `generation`.
pub fn stamp(epoch: u64, generation: u64) -> Stamp {
    Stamp::new(Epoch::new(epoch), FocusGen::new(generation))
}

/// A key event of `meaning` with `action` and the modifiers `mods`.
pub fn key(meaning: KeyMeaning, action: KeyAction, mods: u16) -> KeyEvent {
    KeyEvent {
        usage: HidUsage::new(0x07, 0x04),
        meaning,
        mods: ModSet::from_bits(mods),
        action,
        time: MonoTime::ZERO,
        synthetic: false,
    }
}

/// A key `event` on the test surface with `stamp`.
pub fn key_on(stamp: Stamp, event: KeyEvent) -> Event {
    Event::Key {
        surface: SURFACE,
        stamp,
        event,
    }
}

/// A press of the named key `named` with `stamp`.
pub fn press(named: NamedKey, stamp: Stamp) -> Event {
    key_on(stamp, key(KeyMeaning::Named(named), KeyAction::Press, 0))
}

/// Focus in with `stamp`.
pub fn focus_in(stamp: Stamp) -> Event {
    Event::FocusIn {
        surface: SURFACE,
        stamp,
    }
}

/// Focus out with `stamp`.
pub fn focus_out(stamp: Stamp) -> Event {
    Event::FocusOut {
        surface: SURFACE,
        stamp,
    }
}

/// A runtime, a calculator and a port, driven round by round.
pub struct Driver {
    pub runtime: Runtime<Calc>,
    pub app: Calc,
    pub port: Port,
    pub receipts: Vec<Receipt<Port>>,
    pub time: u64,
}

impl Driver {
    pub fn new(app: Calc) -> Driver {
        Driver {
            runtime: Runtime::new(),
            app,
            port: Port::default(),
            receipts: Vec::new(),
            time: 0,
        }
    }

    /// Runs one round, 1 ms after the last, with `events`, `reports` and
    /// the receipts of the last round, and hands its effects to the port.
    pub fn round(&mut self, events: Vec<Event>, reports: Vec<Tagged<String, u64>>) -> Output {
        self.time += 1_000_000;
        let mut input = Input::new(MonoTime::from_nanos(self.time));
        input.receipts = std::mem::take(&mut self.receipts);
        input.events = events;
        input.reports = reports;
        let mut round = self.runtime.round(&mut self.app, input);
        self.receipts = execute(&mut self.port, round.take_effects());
        round.finish(&self.app)
    }

    /// Runs one round with `events` only.
    pub fn events(&mut self, events: Vec<Event>) -> Output {
        self.round(events, Vec::new())
    }

    /// The outcomes of the key lines of `output`: what follows their time.
    pub fn outcomes(output: &Output) -> Vec<String> {
        output
            .record
            .lines()
            .iter()
            .filter(|line| line.starts_with("key "))
            .filter_map(|line| line.split_once(" time "))
            .filter_map(|(_, after)| after.split_once(' '))
            .map(|(_, outcome)| outcome.to_string())
            .collect()
    }
}

/// The result `value` of `submission`, computed at `versions`, as a
/// report carries it.
pub fn tagged(value: &str, submission: u64, versions: Versions) -> Tagged<String, u64> {
    Tagged {
        value: value.to_string(),
        versions,
        submission,
    }
}

/// A press of the character `c` with `stamp` and no modifiers.
pub fn char_press(c: char, stamp: Stamp) -> Event {
    key_on(stamp, key(KeyMeaning::Char(c), KeyAction::Press, 0))
}

/// The binding table of a control in the key-map tests: one binding that no
/// test key matches.
pub const UNRELATED: &[Binding] = &[Binding {
    trigger: kosmo_ui::Trigger::Char('q'),
    mods: kosmo_ui::Mods::CTRL,
    command: CommandId::new("calc", "Quit", false),
}];

/// Every command of the minimal key map.
pub const ALL: [CommandId; 6] = [
    MOVE_LEFT,
    MOVE_RIGHT,
    DELETE_BACKWARD,
    EVALUATE_SELECTION,
    CANCEL_JOB,
    INSERT_TEXT,
];
