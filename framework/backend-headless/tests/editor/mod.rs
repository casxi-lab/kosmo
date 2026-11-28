//! A test application for script runs and the driver that runs it.
//!
//! The application edits one line: it inserts characters at its cursor,
//! moves the cursor, deletes backwards, and on Execute submits the line as
//! a job. Its execution port is the job layer with the controllable
//! backend, whose release plan the driver keeps: each step releases a gate
//! of a job, named by its submission order, once the event handling record
//! holds a number of key events. The plan never looks at virtual time.
//!
//! The driver runs the application in the shell of `kosmo-ui` on the
//! headless backend. After each round whose view changed it lays the
//! single control out over the main surface, builds the display list with
//! the drawing pass and presents it; rounds do not draw yet.

#![allow(
    dead_code,
    reason = "each test file that uses the editor uses a part of it"
)]

use std::collections::{BTreeMap, BTreeSet};

use kosmo_backend_headless::HeadlessBackend;
use kosmo_jobs::controllable::{ControllableBackend, Gate, Plan, Release};
use kosmo_jobs::machine::CancelAnswer;
use kosmo_jobs::{
    EchoedVersions, ErrorClass, JobClock, JobId, JobLayer, Operation, OutcomeKind, Rejected,
    Request, RequestReport, SessionId, Submission, TerminalState,
};
use kosmo_math_model::version::Versions;
use kosmo_platform::{
    PixelSize, PlatformCaps, PlatformError, ResourceId, Stamp, SurfaceId, WaitEnd,
};
use kosmo_render::{Command, DisplayList};
use kosmo_testkit::script::Driver;
use kosmo_text::{MONO_FILE, SANS_FILE};
use kosmo_types::{BufferDesc, Damage, Fx, MonoTime, PixelFormat, Point, Rect, Size};
use kosmo_ui::{
    App, CANCEL_JOB, ColorToken, Control, DELETE_BACKWARD, DrawContext, EVALUATE_SELECTION,
    INSERT_TEXT, Key, Kind, LayoutBox, LayoutResult, MOVE_LEFT, MOVE_RIGHT, Message, Out, Shell,
    ShellError, TOKENS, View, ViewPass, paint,
};

/// The main surface of a script on a new headless backend, which numbers
/// its surfaces from 1.
pub const MAIN: SurfaceId = SurfaceId::new(1);

/// The most characters of the line.
const MAX_LINE: usize = 12;

/// The line editor.
#[derive(Debug, Default)]
pub struct Editor {
    line: Vec<char>,
    cursor: usize,
    focused: bool,
    job: JobView,
}

/// What the editor shows of its job.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
enum JobView {
    #[default]
    None,
    Submitted,
    Refused,
    Running(JobId),
    Ended(Done),
}

/// A message of the editor.
#[derive(Clone, Copy, Debug)]
pub enum Edit {
    /// A command that a key gave.
    Command(kosmo_ui::Command),
    /// The surface gained focus, or lost it.
    Focus(bool),
}

/// The editor's only control: the line's cells, its cursor and the state
/// of its job, as the view gives them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field {
    cells: Vec<char>,
    cursor: usize,
    focused: bool,
    job: u8,
}

impl Control<Editor> for Field {
    type State = ();

    fn kind(&self) -> Kind {
        Kind::new("field")
    }

    fn focus(&self, holds: bool, cx: &mut kosmo_ui::Context<'_, Editor, ()>) {
        cx.message(Edit::Focus(holds));
    }

    fn draw(&self, cx: &mut DrawContext<'_, ()>) {
        let background = if self.focused {
            ColorToken::Text
        } else {
            ColorToken::TextSecondary
        };
        let job = match self.job {
            0 => ColorToken::TextSecondary,
            1 | 2 => ColorToken::Accent,
            3 => ColorToken::Background,
            _ => ColorToken::Error,
        };
        let mut fills = vec![
            (pixels(0, 0, 64, 8), background),
            (pixels(58, 1, 5, 6), job),
        ];
        // Digits and other characters in two colours.
        for (at, cell) in (0..).zip(&self.cells) {
            let color = if cell.is_ascii_digit() {
                ColorToken::Background
            } else {
                ColorToken::Error
            };
            fills.push((pixels(1 + at * 4, 2, 3, 4), color));
        }
        let cursor = i32::try_from(self.cursor).unwrap_or(0);
        fills.push((pixels(cursor * 4, 1, 1, 6), ColorToken::Accent));
        for (rect, token) in fills {
            if let Some(rect) = rect {
                let color = cx.color(token);
                cx.draw(Command::FillRect { rect, color });
            }
        }
    }
}

/// The rectangle of `width` by `height` pixels at (`x`, `y`).
fn pixels(x: i32, y: i32, width: i32, height: i32) -> Option<Rect> {
    let px = |pixels: i32| Fx::from_pixels(pixels);
    Some(Rect::new(
        Point::new(px(x)?, px(y)?),
        Size::new(px(width)?, px(height)?)?,
    ))
}

impl App for Editor {
    type Message = Edit;
    type Control = Field;
    type Port = Jobs;

    fn update(&mut self, message: Message<Self>, out: &mut Out<Self>) {
        match message {
            Message::Own(Edit::Focus(holds)) => self.focused = holds,
            Message::Own(Edit::Command(command)) => self.command(command, out),
            Message::Accepted(id) => self.job = JobView::Running(id),
            Message::Rejected(_) => self.job = JobView::Refused,
            Message::Report(done) => self.job = JobView::Ended(done),
            _ => {}
        }
    }

    fn view(&self) -> View<Field> {
        let job = match self.job {
            JobView::None => 0,
            JobView::Submitted => 1,
            JobView::Running(_) => 2,
            JobView::Ended(Done {
                state: TerminalState::Completed,
                ..
            }) => 3,
            JobView::Refused | JobView::Ended(_) => 4,
        };
        let field = Field {
            cells: self.line.clone(),
            cursor: self.cursor,
            focused: self.focused,
            job,
        };
        View::new(Key::new(0), field).region().focused()
    }

    fn command(&self, command: &kosmo_ui::Command) -> Option<Edit> {
        let known = [
            INSERT_TEXT,
            MOVE_LEFT,
            MOVE_RIGHT,
            DELETE_BACKWARD,
            EVALUATE_SELECTION,
            CANCEL_JOB,
        ];
        known
            .contains(&command.id)
            .then_some(Edit::Command(*command))
    }
}

impl Editor {
    fn command(&mut self, command: kosmo_ui::Command, out: &mut Out<Self>) {
        match command.id {
            INSERT_TEXT => {
                if let Some(text) = command.text
                    && self.line.len() < MAX_LINE
                {
                    self.line.insert(self.cursor, text);
                    self.cursor += 1;
                }
            }
            MOVE_LEFT => self.cursor = self.cursor.saturating_sub(1),
            MOVE_RIGHT => self.cursor = (self.cursor + 1).min(self.line.len()),
            DELETE_BACKWARD => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                    self.line.remove(self.cursor);
                }
            }
            EVALUATE_SELECTION => {
                out.submit(self.line.iter().collect());
                self.job = JobView::Submitted;
            }
            CANCEL_JOB => {
                if let JobView::Running(id) = self.job {
                    out.cancel(id);
                }
            }
            _ => {}
        }
    }

    /// The view model record: the region, the line, the cursor and the
    /// job, one entry per line.
    pub fn record(&self) -> Vec<String> {
        let line: String = self.line.iter().collect();
        let job = match &self.job {
            JobView::None => "job none".to_string(),
            JobView::Submitted => "job submitted".to_string(),
            JobView::Refused => "job refused".to_string(),
            JobView::Running(id) => format!("job {} running", id.get()),
            JobView::Ended(done) => {
                format!("job {} {:?} {}", done.id.get(), done.state, done.result)
            }
        };
        vec![
            format!(
                "region main focus {}",
                if self.focused { "yes" } else { "no" }
            ),
            format!("line {line:?}"),
            format!("cursor {}", self.cursor),
            job,
        ]
    }
}

/// The terminal state of a job as a report of the execution port: its
/// identity, its state and the value of its last line, the last field of
/// the line without its line feed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Done {
    id: JobId,
    state: TerminalState,
    result: String,
}

/// The clock of the job layer. The job layer reads it only for a time
/// limit, and the editor's submissions have none, so it is never read.
struct NoTimeLimits;

impl JobClock for NoTimeLimits {
    fn now(&mut self) -> u64 {
        0
    }

    fn fork(&self) -> Box<dyn JobClock> {
        Box::new(NoTimeLimits)
    }
}

/// The editor's execution port: the job layer with the controllable
/// backend.
pub struct Jobs {
    layer: JobLayer<ControllableBackend>,
    session: Option<SessionId>,
    /// The accepted jobs whose terminal states have not been taken.
    open: BTreeSet<JobId>,
    /// The reports of the backend that the job layer has applied.
    applied: usize,
}

impl Jobs {
    /// The job layer with a controllable backend that runs each job of
    /// `plans`, by submission order, as its plan says.
    pub fn new(plans: Vec<Plan>) -> Jobs {
        let mut backend = ControllableBackend::new();
        for (order, plan) in plans.into_iter().enumerate() {
            backend.plan(order, plan);
        }
        let mut layer = JobLayer::new(backend, Box::new(NoTimeLimits));
        let session = layer.open_session().ok();
        Jobs {
            layer,
            session,
            open: BTreeSet::new(),
            applied: 0,
        }
    }

    /// Whether the backend made reports that the job layer has not
    /// applied: reports that the release plan let through.
    fn released(&self) -> bool {
        self.layer.backend().sent().len() > self.applied
    }
}

/// The plan of a job that completes with the value `value`, and waits
/// before it finishes until the release plan releases it.
pub fn completes_with(value: &str) -> Plan {
    let line = format!("value\t0\t0\t0\t{value}\n").into_bytes();
    let report = RequestReport::new(
        OutcomeKind::Value,
        ErrorClass::NoError,
        Some(line),
        EchoedVersions::new(0, 0, 0),
    );
    Plan::new()
        .request(report)
        .gate(Gate::Finished, Release::Manual)
}

impl kosmo_ui::JobPort for Jobs {
    type Request = String;
    type Submission = JobId;
    type Rejection = Option<Rejected>;
    type CancelAnswer = CancelAnswer;
    type Report = Done;

    fn submit(&mut self, text: String) -> Result<JobId, Option<Rejected>> {
        let session = self.session.ok_or(None)?;
        let request = Request::new(Operation::Eval, "", text);
        let id = self
            .layer
            .submit(Submission::new(session, Versions::default(), request))
            .map_err(Some)?;
        self.open.insert(id);
        Ok(id)
    }

    fn cancel(&mut self, id: &JobId) -> CancelAnswer {
        self.layer.cancel(*id)
    }

    fn take_reports(&mut self, out: &mut Vec<Done>) -> usize {
        let ready = self.layer.poll();
        self.applied = self.layer.backend().sent().len();
        for id in ready {
            if let Some(terminal) = self.layer.take(id) {
                self.open.remove(&id);
                let record = terminal.record();
                let result = record
                    .lines()
                    .last()
                    .map(|line| String::from_utf8_lossy(line).into_owned())
                    .and_then(|line| line.trim_end().rsplit('\t').next().map(str::to_string))
                    .unwrap_or_default();
                out.push(Done {
                    id,
                    state: record.state(),
                    result,
                });
            }
        }
        self.open.len()
    }
}

/// A step of a release plan: release `gate` of the job of submission order
/// `job` once the event handling record holds `keys` key events.
#[derive(Clone, Copy, Debug)]
pub struct Step {
    pub job: usize,
    pub gate: Gate,
    pub keys: usize,
}

/// The driver of the editor for the script runner.
pub struct EditorDriver {
    shell: Shell<Editor>,
    plan: Vec<(Step, bool)>,
    /// The key events of the event handling record so far.
    keys: usize,
    /// The event handling record of every round so far.
    record: Vec<String>,
    /// The view changed since the last presentation.
    dirty: bool,
    /// The display list presented last.
    presented: Option<DisplayList>,
}

impl EditorDriver {
    /// A driver of a new editor whose jobs run as `plans` say, released as
    /// `release` says.
    pub fn new(plans: Vec<Plan>, release: Vec<Step>) -> EditorDriver {
        EditorDriver {
            shell: Shell::new(Editor::default(), Jobs::new(plans)),
            plan: release.into_iter().map(|step| (step, false)).collect(),
            keys: 0,
            record: Vec::new(),
            dirty: false,
            presented: None,
        }
    }

    /// The event handling record of every round so far, line by line.
    pub fn record(&self) -> &[String] {
        &self.record
    }

    /// Draws the editor over the main surface and presents it, unless its
    /// buffer has not been released yet.
    fn present(&mut self, platform: &mut HeadlessBackend) -> Result<(), String> {
        let Some(size) = self.shell.size(MAIN) else {
            return Ok(());
        };
        let tree = self.shell.runtime().tree();
        let Some(root) = tree.root() else {
            return Ok(());
        };
        let (width, height) = (size.width(), size.height());
        let whole = |pixels: u32| i32::try_from(pixels).map_err(|error| error.to_string());
        let rect = self::pixels(0, 0, whole(width)?, whole(height)?).ok_or("no rectangle")?;
        let mut layout = LayoutResult::new();
        layout
            .push(LayoutBox {
                widget: root,
                parent: None,
                rect,
                scroll: Point::default(),
                draws: true,
            })
            .map_err(|error| format!("{error:?}"))?;
        let frame = BufferDesc::new(width, height, width * 4, PixelFormat::Rgba8Premul)
            .map_err(|error| format!("{error:?}"))?;
        let mut damage = Damage::new(frame);
        damage.add(0, 0, width, height);
        let list = paint(tree, &layout, &damage, &TOKENS).map_err(|error| format!("{error:?}"))?;
        match self.shell.present(platform, MAIN, &list, &damage) {
            Ok(_) => {
                self.presented = Some(list);
                self.dirty = false;
                Ok(())
            }
            Err(ShellError::Platform(PlatformError::Busy)) => Ok(()),
            Err(error) => Err(error.to_string()),
        }
    }
}

impl Driver<HeadlessBackend> for EditorDriver {
    type Error = String;

    fn round(&mut self, platform: &mut HeadlessBackend) -> Result<WaitEnd, String> {
        let deadline = self.shell.next_deadline();
        let end = self
            .shell
            .run_round(platform, deadline)
            .map_err(|error| error.to_string())?;
        let lines = end.output.record.lines();
        self.keys += lines.iter().filter(|line| line.starts_with("key ")).count();
        self.record.extend(lines.iter().cloned());
        if end.output.view == ViewPass::Rebuilt {
            self.dirty = true;
        }
        let keys = self.keys;
        let layer = &mut self.shell.port_mut().layer;
        for (step, done) in &mut self.plan {
            if !*done && keys >= step.keys && layer.controllable().received() > step.job {
                layer.controllable().release(step.job, step.gate);
                *done = true;
            }
        }
        if self.dirty {
            self.present(platform)?;
        }
        Ok(end.wait)
    }

    fn pending(&self) -> bool {
        self.shell.has_receipts() || self.shell.port().released()
    }

    fn next_deadline(&self) -> Option<MonoTime> {
        self.shell.next_deadline()
    }

    fn focus_stamp(&self, surface: SurfaceId) -> Option<Stamp> {
        self.shell.runtime().focus(surface).map(|(stamp, _)| stamp)
    }

    fn view(&self, out: &mut Vec<String>) {
        out.extend(self.shell.app().record());
    }

    fn display(&self, surface: SurfaceId, out: &mut String) {
        if surface == MAIN
            && let Some(list) = &self.presented
        {
            out.push_str(&list.text());
        }
    }
}

/// A headless backend for a 64 by 8 output in `Xrgb8888` or
/// `Rgba8Premul`, with the two fonts of `kosmo-text` from its asset
/// directory as resources, which the shell fetches before its first round.
#[allow(clippy::unwrap_used, reason = "a fixed size that is valid")]
pub fn backend() -> HeadlessBackend {
    let output = PixelSize::new(64, 8).unwrap();
    let caps = PlatformCaps::new(
        vec![PixelFormat::Xrgb8888, PixelFormat::Rgba8Premul],
        output,
    );
    let sans = include_bytes!("../../../text/assets/fonts/firasans/FiraSans-Regular.ttf");
    let mono = include_bytes!("../../../text/assets/fonts/firamono/FiraMono-Regular.ttf");
    let fonts = BTreeMap::from([
        (ResourceId::new(SANS_FILE), sans.to_vec()),
        (ResourceId::new(MONO_FILE), mono.to_vec()),
    ]);
    HeadlessBackend::new(caps, fonts)
}

/// A driver of a new editor whose first job completes with 3 once the
/// event handling record holds `keys` key events.
pub fn driver(keys: usize) -> EditorDriver {
    let release = Step {
        job: 0,
        gate: Gate::Finished,
        keys,
    };
    EditorDriver::new(vec![completes_with("3")], vec![release])
}

/// The lines of `section` from the one that starts with `head` up to the
/// next head of a part, `view`, `display` or `pixels`.
pub fn part<'a>(section: &'a str, head: &str) -> Vec<&'a str> {
    let mut lines = section.lines().skip_while(|line| !line.starts_with(head));
    let first = lines.next().into_iter();
    let parts = ["view ", "display ", "pixels "];
    first
        .chain(lines.take_while(|line| !parts.iter().any(|part| line.starts_with(part))))
        .collect()
}
