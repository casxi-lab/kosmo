use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::rc::Rc;

use kosmo_platform::accesskit::{Node, NodeId, Role, TreeId, TreeInfo, TreeUpdate};
use kosmo_platform::{
    AccessibilitySink, Capabilities, Clock, EventSource, PixelBufferMut, PlatformCaps,
    ResourceProvider, SurfacePort, SurfaceRequest,
};
use kosmo_render::{Color, Command, FontId, Glyph, GlyphId, ListBuilder, RenderError};
use kosmo_text::{GlyphError, MONO, SANS};
use kosmo_types::{DamageRect, Fx, KeyAction, KeyMeaning, NamedKey, Point, Rect, Size, convert};

use super::*;
use crate::app::{Context, Control, DrawContext, Handling, Message, Out, Phase};
use crate::command::{EVALUATE_SELECTION, MOVE_LEFT};
use crate::layout::{self, Column, Constraint, Font, Row, Text};
use crate::testing::{SURFACE, TestApp, TestPort, Widget, key_event, node, stamp};
use crate::tokens::{ColorToken, FontSize};
use crate::tree::{Key, View};

type Log = Rc<RefCell<Vec<String>>>;

/// A platform that logs its calls, in the log that the test port shares,
/// and returns the event batches that the test gives it, one for each
/// wait, then `Disconnected` once they are gone and `disconnect` is set.
/// Its clock stays at `clock`, and it has one buffer for every surface.
struct Fake {
    log: Log,
    batches: VecDeque<Vec<PlatformEvent>>,
    disconnect: bool,
    clock: MonoTime,
    resources: BTreeMap<ResourceId, Vec<u8>>,
    desc: BufferDesc,
    bytes: Vec<u8>,
    in_flight: bool,
}

impl Fake {
    /// A platform with a `width` by `height` buffer of `format` whose bytes
    /// are all `0x11`, the two fonts as resources and its clock at 7.
    fn new(log: &Log, width: u32, height: u32, format: PixelFormat) -> Fake {
        let stride = width * format.bytes_per_pixel();
        let desc = BufferDesc::new(width, height, stride, format).unwrap();
        Fake {
            log: Rc::clone(log),
            batches: VecDeque::new(),
            disconnect: false,
            clock: MonoTime::from_nanos(7),
            resources: BTreeMap::from([
                (SANS_RESOURCE, SANS_BYTES.to_vec()),
                (MONO_RESOURCE, MONO_BYTES.to_vec()),
            ]),
            desc,
            bytes: vec![0x11; desc.byte_len() as usize],
            in_flight: false,
        }
    }

    fn log(&self, call: String) {
        self.log.borrow_mut().push(call);
    }
}

impl EventSource for Fake {
    fn wait(
        &mut self,
        deadline: Option<MonoTime>,
        out: &mut Vec<PlatformEvent>,
    ) -> Result<WaitEnd, PlatformError> {
        self.log(match deadline {
            Some(deadline) => format!("wait {}", deadline.as_nanos()),
            None => "wait none".to_string(),
        });
        match self.batches.pop_front() {
            Some(batch) if !batch.is_empty() => {
                out.extend(batch);
                Ok(WaitEnd::Events)
            }
            None if self.disconnect => Err(PlatformError::Disconnected),
            _ if deadline.is_some_and(|deadline| deadline <= self.clock) => Ok(WaitEnd::Deadline),
            _ => Ok(WaitEnd::Idle),
        }
    }
}

impl Clock for Fake {
    fn now(&self) -> MonoTime {
        self.log("now".to_string());
        self.clock
    }
}

impl SurfacePort for Fake {
    fn create_surface(&mut self, _: SurfaceRequest) -> Result<SurfaceId, PlatformError> {
        Err(PlatformError::Unsupported)
    }

    fn destroy_surface(&mut self, _: SurfaceId) -> Result<(), PlatformError> {
        Err(PlatformError::Unsupported)
    }
}

impl Presenter for Fake {
    fn buffer(&mut self, s: SurfaceId) -> Result<PixelBufferMut<'_>, PlatformError> {
        self.log(format!("buffer {}", s.get()));
        if self.in_flight {
            return Err(PlatformError::Busy);
        }
        Ok(PixelBufferMut {
            desc: self.desc,
            bytes: &mut self.bytes,
        })
    }

    fn present(&mut self, s: SurfaceId, damage: &[DamageRect]) -> Result<(), PlatformError> {
        let rects: Vec<String> = damage
            .iter()
            .map(|r| format!("{},{} {}x{}", r.x(), r.y(), r.width(), r.height()))
            .collect();
        self.log(format!("present {} {}", s.get(), rects.join(" ")));
        self.in_flight = true;
        Ok(())
    }
}

impl Capabilities for Fake {
    fn caps(&self) -> PlatformCaps {
        let size = PixelSize::new(self.desc.width(), self.desc.height()).unwrap();
        PlatformCaps::new(vec![self.desc.format()], size)
    }
}

impl ResourceProvider for Fake {
    fn resource(&self, id: ResourceId) -> Result<&[u8], PlatformError> {
        self.log(format!("resource {}", id.name()));
        self.resources
            .get(&id)
            .map(Vec::as_slice)
            .ok_or(PlatformError::NotFound)
    }
}

impl AccessibilitySink for Fake {
    fn update(&mut self, surface: SurfaceId, _: TreeUpdate) -> Result<(), PlatformError> {
        self.log(format!("update {}", surface.get()));
        Ok(())
    }
}

impl Platform for Fake {
    fn shutdown(&mut self) -> Result<(), PlatformError> {
        self.log("shutdown".to_string());
        Ok(())
    }
}

/// The bytes of the two fonts, from the asset directory of `kosmo-text`.
const SANS_BYTES: &[u8] =
    include_bytes!("../../../text/assets/fonts/firasans/FiraSans-Regular.ttf");
const MONO_BYTES: &[u8] =
    include_bytes!("../../../text/assets/fonts/firamono/FiraMono-Regular.ttf");

/// The calls of the shell that fetch the fonts, as the platform logs them.
const FETCHES: [&str; 2] = [
    "resource firasans/FiraSans-Regular.ttf",
    "resource firamono/FiraMono-Regular.ttf",
];

/// An application whose root submits a job on `Execute` and whose focused
/// line handles `MoveLeft`, and its port, which shares `log` and reports
/// `unfinished` jobs.
fn calculator(log: &Log, unfinished: usize) -> Shell<TestApp> {
    let line = View::new(
        Key::new(1),
        Widget {
            handles: Some(MOVE_LEFT),
            ..node(1, "line", 0).control().clone()
        },
    )
    .focused();
    let app = TestApp {
        view: Some(node(0, "root", 0).region().child(line)),
        handles: Some(EVALUATE_SELECTION),
        submits: vec!["job"],
        ..TestApp::default()
    };
    let port = TestPort {
        unfinished,
        calls: Some(Rc::clone(log)),
        ..TestPort::default()
    };
    Shell::new(app, port)
}

fn focus_in() -> PlatformEvent {
    PlatformEvent::FocusIn {
        surface: SURFACE,
        stamp: stamp(1, 1),
    }
}

fn execute_key() -> PlatformEvent {
    key(NamedKey::EXECUTE)
}

/// A press of the key `named` on the surface.
fn key(named: NamedKey) -> PlatformEvent {
    PlatformEvent::Key {
        surface: SURFACE,
        stamp: stamp(1, 1),
        event: key_event(KeyMeaning::Named(named), KeyAction::Press),
    }
}

/// A `Configure` of the surface to `width` by `height` pixels.
fn configure(width: u32, height: u32) -> PlatformEvent {
    PlatformEvent::Configure {
        surface: SURFACE,
        size: PixelSize::new(width, height).unwrap(),
    }
}

fn taken(log: &Log) -> Vec<String> {
    std::mem::take(&mut *log.borrow_mut())
}

/// A list that fills a `width` by `height` rectangle at (`x`, `y`), in
/// pixels, with `color`.
fn fill(x: i32, y: i32, width: i32, height: i32, color: Color) -> DisplayList {
    let px = |pixels| Fx::from_pixels(pixels).unwrap();
    let rect = Rect::new(
        Point::new(px(x), px(y)),
        Size::new(px(width), px(height)).unwrap(),
    );
    let mut builder = ListBuilder::new();
    builder.push(Command::FillRect { rect, color }).unwrap();
    builder.finish().unwrap()
}

fn damage(fake: &Fake, rects: &[(u32, u32, u32, u32)]) -> Damage {
    let mut damage = Damage::new(fake.desc);
    for &(x, y, width, height) in rects {
        damage.add(x, y, width, height);
    }
    damage
}

/// Returns the release of the surface's buffer through a round, so that the
/// shell presents on it again, and returns how that round ended.
fn release<A: App>(shell: &mut Shell<A>, fake: &mut Fake) -> RoundEnd {
    fake.in_flight = false;
    fake.batches
        .push_back(vec![PlatformEvent::Released { surface: SURFACE }]);
    shell.run_round(fake, None).unwrap()
}

#[test]
fn a_round_waits_reads_the_clock_takes_reports_and_runs_both_steps_in_order() {
    let log = Log::default();
    let mut fake = Fake::new(&log, 4, 2, PixelFormat::Rgba8Premul);
    fake.batches.push_back(vec![focus_in(), execute_key()]);
    let mut shell = calculator(&log, 0);

    let first = shell.run_round(&mut fake, shell.next_deadline()).unwrap();
    // The fonts before the first wait, then the wait, the clock, the
    // reports and the effect of the first step.
    assert_eq!(
        taken(&log),
        [
            FETCHES[0],
            FETCHES[1],
            "wait 0",
            "now",
            "take_reports",
            "submit job"
        ]
    );
    assert_eq!(first.wait, WaitEnd::Events);
    assert_eq!(first.output.record.lines()[0], "round 1 time 7");
    assert!(
        first
            .output
            .record
            .text()
            .contains("handled kosmo.EvaluateSelection by app")
    );
    assert_eq!(first.output.view, crate::round::ViewPass::Rebuilt);

    // The receipt waits for the next round, so its wait does not block;
    // the fonts are not fetched again.
    assert!(shell.has_receipts());
    assert_eq!(shell.next_deadline(), Some(MonoTime::from_nanos(7)));
    let second = shell.run_round(&mut fake, shell.next_deadline()).unwrap();
    assert_eq!(taken(&log), ["wait 7", "now", "take_reports"]);
    assert_eq!(second.wait, WaitEnd::Deadline);
    assert_eq!(second.output.record.lines()[1], "receipt accepted 1");
    assert!(!shell.has_receipts());
    assert_eq!(shell.next_deadline(), None);
}

#[test]
fn while_jobs_run_the_next_wait_has_the_polling_deadline() {
    let log = Log::default();
    let mut fake = Fake::new(&log, 4, 2, PixelFormat::Rgba8Premul);
    let mut shell = calculator(&log, 1);
    let end = shell.run_round(&mut fake, None).unwrap();
    assert_eq!(end.wait, WaitEnd::Idle);
    let polling = MonoTime::from_nanos(7 + crate::JOB_POLL_INTERVAL);
    assert_eq!(end.output.next_deadline, Some(polling));
    assert_eq!(shell.next_deadline(), Some(polling));
}

#[test]
fn platform_events_become_round_events_and_the_shell_handles_the_others() {
    let log = Log::default();
    let mut fake = Fake::new(&log, 4, 2, PixelFormat::Rgba8Premul);
    let size = PixelSize::new(4, 2).unwrap();
    let left = PlatformEvent::Key {
        surface: SURFACE,
        stamp: stamp(1, 1),
        event: key_event(KeyMeaning::Named(NamedKey::LEFT), KeyAction::Press),
    };
    let mut shell = calculator(&log, 0);
    // The first round builds the view, so that the region's root exists.
    shell.run_round(&mut fake, None).unwrap();
    fake.batches.push_back(vec![
        PlatformEvent::Configure {
            surface: SURFACE,
            size,
        },
        focus_in(),
        left,
        PlatformEvent::Released { surface: SURFACE },
        PlatformEvent::FocusOut {
            surface: SURFACE,
            stamp: stamp(1, 1),
        },
    ]);
    assert_eq!(shell.size(SURFACE), None);
    let end = shell.run_round(&mut fake, None).unwrap();
    let lines: Vec<&str> = end
        .output
        .record
        .lines()
        .iter()
        .map(String::as_str)
        .filter(|line| line.starts_with("focus") || line.starts_with("key"))
        .collect();
    assert_eq!(
        lines,
        [
            "focus-in surface 1 epoch 1 generation 1",
            "key surface 1 epoch 1 generation 1 named 1 press mods 0x0000 time 0 handled kosmo.MoveLeft by control 1:0",
            "focus-out surface 1 epoch 1 generation 1",
        ]
    );
    assert_eq!(shell.size(SURFACE), Some(size));
}

#[test]
fn a_missing_resource_stops_the_first_round_before_its_wait() {
    let log = Log::default();
    let mut fake = Fake::new(&log, 4, 2, PixelFormat::Rgba8Premul);
    let mono = fake.resources.remove(&MONO_RESOURCE).unwrap();
    let mut shell = Shell::new(TestApp::default(), TestPort::default());
    assert_eq!(
        shell.run_round(&mut fake, None),
        Err(ShellError::MissingResource(MONO_RESOURCE))
    );
    assert_eq!(taken(&log), FETCHES);
    // Nothing ran, so the next round fetches them again.
    fake.resources.insert(MONO_RESOURCE, mono);
    shell.run_round(&mut fake, None).unwrap();
    assert_eq!(taken(&log), [FETCHES[0], FETCHES[1], "wait none", "now"]);
}

#[test]
fn bytes_that_are_not_the_fonts_stop_the_first_round_before_its_wait() {
    let log = Log::default();
    let mut fake = Fake::new(&log, 4, 2, PixelFormat::Rgba8Premul);
    // Fira Mono in the place of Fira Sans: the font set takes each family
    // only once.
    fake.resources.insert(SANS_RESOURCE, MONO_BYTES.to_vec());
    let mut shell = Shell::new(TestApp::default(), TestPort::default());
    assert_eq!(
        shell.run_round(&mut fake, None),
        Err(ShellError::Fonts(FontSetError::SameFamily))
    );
    fake.resources.insert(SANS_RESOURCE, vec![1, 2, 3]);
    assert_eq!(
        shell.run_round(&mut fake, None),
        Err(ShellError::Fonts(FontSetError::NotTrueType(SANS)))
    );
    assert_eq!(taken(&log), [FETCHES, FETCHES].concat());
    assert!(shell.fonts.is_none());
}

#[test]
fn the_updates_and_frames_of_an_output_go_to_the_platform_in_order() {
    let log = Log::default();
    let mut fake = Fake::new(&log, 4, 2, PixelFormat::Rgba8Premul);
    let mut shell = calculator(&log, 0);
    let mut output = shell.run_round(&mut fake, None).unwrap().output;
    taken(&log);
    let root = NodeId(1);
    let update = TreeUpdate {
        nodes: vec![(root, Node::new(Role::Window))],
        tree: Some(TreeInfo::new(root)),
        tree_id: TreeId::ROOT,
        focus: root,
    };
    output.accessibility.push((SURFACE, update));
    let whole = damage(&fake, &[(0, 0, 4, 2)]);
    output
        .frames
        .push((SURFACE, fill(0, 0, 4, 2, Color::opaque(1, 2, 3)), whole));
    shell.deliver(&mut fake, &output).unwrap();
    assert_eq!(taken(&log), ["update 1", "buffer 1", "present 1 0,0 4x2"]);
    assert_eq!(&fake.bytes[..4], &[1, 2, 3, 255]);
}

/// The calls of `log` that ask for a buffer or present one.
fn presentations(log: &Log) -> Vec<String> {
    taken(log)
        .into_iter()
        .filter(|call| call.starts_with("buffer") || call.starts_with("present"))
        .collect()
}

#[test]
fn a_frame_for_a_presented_buffer_is_drawn_again_after_the_release() {
    let log = Log::default();
    let mut fake = Fake::new(&log, 32, 2, PixelFormat::Rgba8Premul);
    let mut shell = calculator(&log, 0);
    fake.batches.push_back(vec![configure(32, 2), focus_in()]);
    // The first frame of the configured surface is presented whole.
    let first = shell.run_round(&mut fake, None).unwrap();
    assert_eq!(first.output.frames.len(), 1);
    assert_eq!(presentations(&log), ["buffer 1", "present 1 0,0 32x2"]);

    // While the buffer is presented, the frames of an output are not
    // presented: their regions join the pending region, whose 17th
    // rectangle merges all of them into their bounding rectangle.
    let mut output = first.output.clone();
    output.frames.clear();
    for x in 0..17 {
        let list = DisplayList::default();
        output
            .frames
            .push((SURFACE, list, damage(&fake, &[(x, 0, 1, 1)])));
    }
    shell.deliver(&mut fake, &output).unwrap();
    assert!(presentations(&log).is_empty());
    let pending = shell.surfaces[&SURFACE].pending.unwrap();
    assert_eq!(pending.rects(), [DamageRect::new(0, 0, 17, 1).unwrap()]);

    // The round after the release, in which nothing changed, draws the
    // pending region and presents it.
    release(&mut shell, &mut fake);
    assert_eq!(presentations(&log), ["buffer 1", "present 1 0,0 17x1"]);
    assert!(shell.surfaces[&SURFACE].pending.is_none());

    // A round whose view changes while the buffer is presented gives a
    // whole frame, which waits: the round returns no error.
    fake.batches.push_back(vec![execute_key()]);
    let busy = shell.run_round(&mut fake, None).unwrap();
    assert_eq!(busy.output.frames.len(), 1);
    assert!(presentations(&log).is_empty());
    // A driver that presents directly is still refused.
    let list = DisplayList::default();
    let whole = damage(&fake, &[(0, 0, 32, 2)]);
    assert_eq!(
        shell.present(&mut fake, SURFACE, &list, &whole),
        Err(ShellError::Platform(PlatformError::Busy))
    );
    release(&mut shell, &mut fake);
    assert_eq!(presentations(&log), ["buffer 1", "present 1 0,0 32x2"]);
    // Then nothing is pending, and nothing is presented.
    release(&mut shell, &mut fake);
    assert!(presentations(&log).is_empty());
}

/// A control of the tiles: a row, or a leaf of 8 by 2 pixels that counts
/// the `MoveLeft` commands it handles in its presentation state and fills
/// its box in the accent before the first and in the colour of errors
/// after it.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Tile {
    Row,
    Leaf,
}

impl Control<Tiles> for Tile {
    type State = u32;

    fn kind(&self) -> crate::tree::Kind {
        crate::tree::Kind::new(match self {
            Tile::Row => "row",
            Tile::Leaf => "tile",
        })
    }

    fn handle(
        &self,
        _: Phase,
        command: &crate::command::Command,
        cx: &mut Context<'_, Tiles, u32>,
    ) -> Handling {
        if *self == Tile::Leaf && command.id == MOVE_LEFT {
            *cx.state_mut() += 1;
            Handling::Handled
        } else {
            Handling::Unhandled
        }
    }

    fn layout(&self, _: &u32) -> layout::Node<'_> {
        let px = |pixels| Fx::from_pixels(pixels).unwrap();
        match self {
            Tile::Row => layout::Node::Row(Row::new()),
            Tile::Leaf => layout::Node::Fixed(Size::new(px(8), px(2)).unwrap()),
        }
    }

    fn draw(&self, cx: &mut DrawContext<'_, u32>) {
        if *self == Tile::Leaf {
            let token = match cx.state() {
                0 => ColorToken::Accent,
                _ => ColorToken::Error,
            };
            let color = cx.color(token);
            cx.draw(Command::FillRect {
                rect: Rect::new(Point::default(), cx.size()),
                color,
            });
        }
    }
}

/// An application whose view is a row of four tiles, the first of them
/// focused, which fill a surface of 32 by 2 pixels.
struct Tiles;

impl App for Tiles {
    type Message = ();
    type Control = Tile;
    type Port = TestPort;

    fn update(&mut self, _: Message<Self>, _: &mut Out<Self>) {}

    fn view(&self) -> View<Tile> {
        let mut row = View::new(Key::new(0), Tile::Row)
            .region()
            .child(View::new(Key::new(1), Tile::Leaf).focused());
        for key in 2..5 {
            row = row.child(View::new(Key::new(key), Tile::Leaf));
        }
        row
    }
}

/// The bytes of a pixel in the colour of `token` in an `Rgba8Premul`
/// buffer.
fn tile(token: ColorToken) -> [u8; 4] {
    TOKENS.color(token).to_bytes()
}

/// Whether `damage` holds rectangles, each the whole surface of `width`
/// by 2 pixels.
fn is_whole(damage: &Damage, width: u32) -> bool {
    let whole = DamageRect::new(0, 0, width, 2).unwrap();
    !damage.is_empty() && damage.rects().iter().all(|rect| *rect == whole)
}

/// A `Configure` while the buffer is presented, and a second one to the
/// size of the buffer again, make the whole surface the pending region, so
/// that the frame after the release, a new previous frame, draws every
/// tile and not only those inside the region of the smaller size.
#[test]
fn a_second_size_while_the_buffer_is_presented_draws_the_next_frame_whole() {
    let log = Log::default();
    let mut fake = Fake::new(&log, 32, 2, PixelFormat::Rgba8Premul);
    let mut shell = Shell::new(Tiles, TestPort::default());
    fake.batches.push_back(vec![configure(32, 2), focus_in()]);
    shell.run_round(&mut fake, None).unwrap();
    // The third tile, at x = 20.
    assert_eq!(&fake.bytes[80..84], &tile(ColorToken::Accent));
    // Smaller, then the size of the buffer again, while presented.
    fake.batches.push_back(vec![configure(16, 2)]);
    let small = shell.run_round(&mut fake, None).unwrap();
    assert_eq!(small.output.frames.len(), 1);
    fake.batches.push_back(vec![configure(32, 2)]);
    let back = shell.run_round(&mut fake, None).unwrap();
    assert_eq!(back.output.frames.len(), 1);
    assert!(is_whole(&shell.surfaces[&SURFACE].pending.unwrap(), 32));
    let after = release(&mut shell, &mut fake);
    assert!(is_whole(&after.output.frames[0].2, 32));
    assert_eq!(&fake.bytes[0..4], &tile(ColorToken::Accent));
    assert_eq!(&fake.bytes[80..84], &tile(ColorToken::Accent));
}

/// A `Configure` to another size and back to the old one in one batch,
/// while the buffer is presented, keeps the change that waits for the
/// release: the layout is as before, and the round after the release
/// still draws the first tile in the colour of its new state.
#[test]
fn a_size_and_back_while_the_buffer_is_presented_draws_the_change() {
    let log = Log::default();
    let mut fake = Fake::new(&log, 32, 2, PixelFormat::Rgba8Premul);
    let mut shell = Shell::new(Tiles, TestPort::default());
    fake.batches.push_back(vec![configure(32, 2), focus_in()]);
    shell.run_round(&mut fake, None).unwrap();
    assert_eq!(&fake.bytes[0..4], &tile(ColorToken::Accent));
    // The first tile handles MoveLeft while the buffer is presented, so
    // its whole frame waits.
    fake.batches.push_back(vec![key(NamedKey::LEFT)]);
    let changed = shell.run_round(&mut fake, None).unwrap();
    assert_eq!(changed.output.frames.len(), 1);
    assert!(shell.surfaces[&SURFACE].pending.is_some());
    fake.batches
        .push_back(vec![configure(16, 2), configure(32, 2)]);
    let resized = shell.run_round(&mut fake, None).unwrap();
    assert!(resized.output.frames.is_empty());
    assert!(shell.surfaces[&SURFACE].pending.is_some());
    let after = release(&mut shell, &mut fake);
    assert_eq!(after.output.frames.len(), 1);
    let first = shell.runtime().tree().focused().unwrap();
    assert_eq!(shell.runtime().tree().state(first), Some(&1));
    assert_eq!(&fake.bytes[0..4], &tile(ColorToken::Error));
}

/// A frame that fails to present, here because the platform still holds a
/// buffer that the shell saw released, waits in the pending region of its
/// surface, and the next round, in which nothing else changed, draws it
/// again and presents it; the frames after it in the output wait in the
/// pending regions of their surfaces too.
#[test]
fn frames_that_fail_to_present_are_drawn_again_by_the_next_round() {
    let log = Log::default();
    let mut fake = Fake::new(&log, 32, 2, PixelFormat::Rgba8Premul);
    let mut shell = Shell::new(Tiles, TestPort::default());
    fake.batches.push_back(vec![configure(32, 2), focus_in()]);
    shell.run_round(&mut fake, None).unwrap();
    release(&mut shell, &mut fake);
    taken(&log);
    // The first tile handles MoveLeft, and the platform refuses the buffer.
    fake.in_flight = true;
    fake.batches.push_back(vec![key(NamedKey::LEFT)]);
    assert_eq!(
        shell.run_round(&mut fake, None).unwrap_err(),
        ShellError::Platform(PlatformError::Busy)
    );
    assert_eq!(presentations(&log), ["buffer 1"]);
    assert!(is_whole(&shell.surfaces[&SURFACE].pending.unwrap(), 32));
    fake.in_flight = false;
    let next = shell.run_round(&mut fake, None).unwrap();
    assert!(is_whole(&next.output.frames[0].2, 32));
    assert_eq!(presentations(&log), ["buffer 1", "present 1 0,0 32x2"]);
    assert_eq!(&fake.bytes[0..4], &tile(ColorToken::Error));
    assert!(shell.surfaces[&SURFACE].pending.is_none());

    // The frame of the second surface comes after the one that fails.
    release(&mut shell, &mut fake);
    fake.in_flight = true;
    let second = SurfaceId::new(2);
    let mut output = next.output.clone();
    output.frames = vec![
        (
            SURFACE,
            DisplayList::default(),
            damage(&fake, &[(0, 0, 1, 1)]),
        ),
        (
            second,
            DisplayList::default(),
            damage(&fake, &[(2, 0, 3, 1)]),
        ),
    ];
    taken(&log);
    assert_eq!(
        shell.deliver(&mut fake, &output),
        Err(ShellError::Platform(PlatformError::Busy))
    );
    assert_eq!(presentations(&log), ["buffer 1"]);
    let pending = |surface| shell.surfaces[&surface].pending.unwrap();
    assert_eq!(
        pending(SURFACE).rects(),
        [DamageRect::new(0, 0, 1, 1).unwrap()]
    );
    assert_eq!(
        pending(second).rects(),
        [DamageRect::new(2, 0, 3, 1).unwrap()]
    );
}

#[test]
fn presenting_converts_only_the_damage_of_the_frame_into_the_buffer() {
    let log = Log::default();
    let mut fake = Fake::new(&log, 4, 2, PixelFormat::Xrgb8888);
    let mut shell = calculator(&log, 0);
    // The first frame is presented whole, in blue.
    let whole = damage(&fake, &[(0, 0, 4, 2)]);
    let blue = fill(0, 0, 4, 2, Color::opaque(0, 0, 255));
    shell.present(&mut fake, SURFACE, &blue, &whole).unwrap();
    release(&mut shell, &mut fake);
    // The buffer no longer holds the first frame, so a conversion of more
    // than the damage would show.
    fake.bytes.fill(0x11);
    taken(&log);
    let damage = damage(&fake, &[(1, 0, 2, 1), (3, 1, 1, 1)]);
    let work = shell
        .present(
            &mut fake,
            SURFACE,
            &fill(0, 0, 4, 2, Color::opaque(200, 100, 50)),
            &damage,
        )
        .unwrap();
    assert_eq!(
        work,
        WorkCount::of(&fill(0, 0, 4, 2, Color::TRANSPARENT), &damage)
    );
    assert_eq!(work.pixels, 3);
    assert_eq!(taken(&log), ["buffer 1", "present 1 1,0 2x1 3,1 1x1"]);
    // B, G, R, X at (1, 0), (2, 0) and (3, 1); every other byte as the
    // buffer held it.
    let mut expected = vec![0x11; 32];
    for pixel in [1, 2, 7] {
        expected[pixel * 4..pixel * 4 + 4].copy_from_slice(&[50, 100, 200, 0xFF]);
    }
    assert_eq!(fake.bytes, expected);
}

#[test]
fn a_presented_buffer_is_not_presented_again_until_it_is_released() {
    let log = Log::default();
    let mut fake = Fake::new(&log, 4, 2, PixelFormat::Rgba8Premul);
    let mut shell = calculator(&log, 0);
    let whole = damage(&fake, &[(0, 0, 4, 2)]);
    let red = fill(0, 0, 4, 2, Color::opaque(255, 0, 0));
    shell.present(&mut fake, SURFACE, &red, &whole).unwrap();
    taken(&log);
    // The shell knows that the buffer is presented and asks for none.
    assert_eq!(
        shell.present(&mut fake, SURFACE, &red, &whole),
        Err(ShellError::Platform(PlatformError::Busy))
    );
    assert!(taken(&log).is_empty());
    // A wait returns the release, and the next presentation goes ahead.
    fake.in_flight = false;
    fake.batches
        .push_back(vec![PlatformEvent::Released { surface: SURFACE }]);
    shell.run_round(&mut fake, None).unwrap();
    taken(&log);
    shell.present(&mut fake, SURFACE, &red, &whole).unwrap();
    assert_eq!(taken(&log), ["buffer 1", "present 1 0,0 4x2"]);
}

#[test]
fn an_empty_damage_region_presents_nothing() {
    let log = Log::default();
    let mut fake = Fake::new(&log, 4, 2, PixelFormat::Rgba8Premul);
    let mut shell = calculator(&log, 0);
    let empty = damage(&fake, &[]);
    let red = fill(0, 0, 4, 2, Color::opaque(255, 0, 0));
    assert_eq!(
        shell.present(&mut fake, SURFACE, &red, &empty),
        Ok(WorkCount::default())
    );
    assert!(taken(&log).is_empty());
    assert!(fake.bytes.iter().all(|&byte| byte == 0x11));
}

/// A list of one glyph run of glyph 3 of `font` at 10 pixels per em, on
/// the baseline 2 pixels down.
fn glyph_run(font: FontId) -> DisplayList {
    let mut builder = ListBuilder::new();
    builder
        .push(Command::Glyphs {
            font,
            size: Fx::from_units(640),
            color: Color::opaque(0, 0, 0),
            glyphs: vec![Glyph {
                id: GlyphId::new(3),
                origin: Point::new(Fx::ZERO, Fx::from_units(128)),
            }],
        })
        .unwrap();
    builder.finish().unwrap()
}

#[test]
fn glyph_runs_are_drawn_from_the_glyph_cache_of_the_fetched_fonts() {
    let log = Log::default();
    let mut fake = Fake::new(&log, 4, 2, PixelFormat::Rgba8Premul);
    let mut shell = calculator(&log, 0);
    shell.run_round(&mut fake, None).unwrap();
    taken(&log);
    let whole = damage(&fake, &[(0, 0, 4, 2)]);
    // A font outside the font set: the glyph cache refuses the frame
    // before the shell asks for the buffer.
    assert_eq!(
        shell.present(&mut fake, SURFACE, &glyph_run(FontId::new(2)), &whole),
        Err(ShellError::Glyphs(GlyphError::UnknownFont(FontId::new(2))))
    );
    assert!(taken(&log).is_empty());
    assert!(fake.bytes.iter().all(|&byte| byte == 0x11));
    // Fira Sans: the cache holds the glyph's mask, and it is drawn as the
    // rasterizer draws it from a font set of the same bytes.
    let list = glyph_run(SANS);
    shell.present(&mut fake, SURFACE, &list, &whole).unwrap();
    assert_eq!(taken(&log), ["buffer 1", "present 1 0,0 4x2"]);
    let fonts = shell.fonts.as_ref().unwrap();
    assert_eq!(fonts.glyphs().entries(), 1);
    let mut set = FontSet::new(SANS_BYTES, MONO_BYTES).unwrap();
    let mut rasterizer = VelloRasterizer::new();
    set.prepare(&list, &mut rasterizer).unwrap();
    let mut expected = new_frame(fake.desc).unwrap();
    rasterizer
        .render(
            &list,
            &whole,
            set.glyphs(),
            &ImageStore::EMPTY,
            &mut expected,
        )
        .unwrap();
    assert_eq!(fake.bytes, expected.bytes());
    assert!(fake.bytes.iter().any(|&byte| byte != 0));
}

#[test]
fn an_empty_or_refused_presentation_leaves_the_glyph_cache_alone() {
    let log = Log::default();
    let mut fake = Fake::new(&log, 4, 2, PixelFormat::Rgba8Premul);
    let mut shell = calculator(&log, 0);
    shell.run_round(&mut fake, None).unwrap();
    let cache = |shell: &Shell<TestApp>| {
        let glyphs = shell.fonts.as_ref().unwrap().glyphs();
        (glyphs.frame(), glyphs.entries())
    };
    // An empty damage region prepares nothing.
    let list = glyph_run(SANS);
    let empty = damage(&fake, &[]);
    shell.present(&mut fake, SURFACE, &list, &empty).unwrap();
    assert_eq!(cache(&shell), (0, 0));
    // A buffer presented and not released refuses the next frame before
    // its glyph runs are prepared.
    let whole = damage(&fake, &[(0, 0, 4, 2)]);
    shell.present(&mut fake, SURFACE, &list, &whole).unwrap();
    assert_eq!(cache(&shell), (1, 1));
    assert_eq!(
        shell.present(&mut fake, SURFACE, &glyph_run(MONO), &whole),
        Err(ShellError::Platform(PlatformError::Busy))
    );
    assert_eq!(cache(&shell), (1, 1));
}

#[test]
fn a_frame_that_cannot_be_drawn_changes_nothing() {
    let log = Log::default();
    let mut fake = Fake::new(&log, 4, 2, PixelFormat::Rgba8Premul);
    let mut shell = calculator(&log, 0);
    let whole = damage(&fake, &[(0, 0, 4, 2)]);
    // No round has fetched the fonts, so the shell has no glyph cache.
    let glyphs = glyph_run(SANS);
    assert!(matches!(
        shell.present(&mut fake, SURFACE, &glyphs, &whole),
        Err(ShellError::Render(RenderError::MissingGlyph(_)))
    ));
    assert_eq!(taken(&log), ["buffer 1"]);
    assert!(fake.bytes.iter().all(|&byte| byte == 0x11));
    assert!(
        shell
            .surfaces
            .get(&SURFACE)
            .is_none_or(|known| known.frame.is_none())
    );
    // The surface is not busy: a frame that can be drawn is presented.
    let red = fill(0, 0, 4, 2, Color::opaque(255, 0, 0));
    shell.present(&mut fake, SURFACE, &red, &whole).unwrap();
}

#[test]
fn a_buffer_that_does_not_take_the_frame_is_not_presented() {
    let log = Log::default();
    let mut fake = Fake::new(&log, 4, 2, PixelFormat::Rgba8Premul);
    // One byte fewer than its description says.
    fake.bytes.truncate(31);
    let mut shell = calculator(&log, 0);
    let whole = damage(&fake, &[(0, 0, 4, 2)]);
    let red = fill(0, 0, 4, 2, Color::opaque(255, 0, 0));
    assert_eq!(
        shell.present(&mut fake, SURFACE, &red, &whole),
        Err(ShellError::Convert(ConvertError::TargetLength))
    );
    assert_eq!(taken(&log), ["buffer 1"]);
    assert!(fake.bytes.iter().all(|&byte| byte == 0x11));
    // The new frame is not kept, so the next one is new again.
    assert!(
        shell
            .surfaces
            .get(&SURFACE)
            .is_none_or(|known| known.frame.is_none())
    );
    // Nothing was presented, so the surface is not busy; the new frame is
    // presented whole whatever the damage.
    fake.bytes = vec![0x11; 32];
    let corner = damage(&fake, &[(0, 0, 1, 1)]);
    shell.present(&mut fake, SURFACE, &red, &corner).unwrap();
    assert_eq!(taken(&log), ["buffer 1", "present 1 0,0 4x2"]);
    assert_eq!(fake.bytes, [255, 0, 0, 255].repeat(8));
}

#[test]
fn a_configure_with_a_new_size_drops_the_previous_frame() {
    let log = Log::default();
    let mut fake = Fake::new(&log, 4, 2, PixelFormat::Rgba8Premul);
    let mut shell = calculator(&log, 0);
    let whole = damage(&fake, &[(0, 0, 4, 2)]);
    let red = fill(0, 0, 4, 2, Color::opaque(255, 0, 0));
    shell.present(&mut fake, SURFACE, &red, &whole).unwrap();
    let frame = |shell: &Shell<TestApp>| {
        shell.surfaces[&SURFACE]
            .frame
            .as_ref()
            .map(|frame| frame.bytes()[..4].to_vec())
    };
    assert_eq!(frame(&shell), Some(vec![255, 0, 0, 255]));
    // The same size keeps the frame; another size drops it.
    fake.batches.push_back(vec![configure(4, 2)]);
    shell.run_round(&mut fake, None).unwrap();
    assert_eq!(frame(&shell), Some(vec![255, 0, 0, 255]));
    fake.batches.push_back(vec![configure(5, 2)]);
    shell.run_round(&mut fake, None).unwrap();
    assert_eq!(frame(&shell), None);
}

#[test]
fn a_buffer_of_another_size_without_a_configure_gets_a_new_frame() {
    let log = Log::default();
    let mut fake = Fake::new(&log, 4, 2, PixelFormat::Rgba8Premul);
    let mut shell = calculator(&log, 0);
    let red = fill(0, 0, 5, 3, Color::opaque(255, 0, 0));
    // Another width, then another height; no Configure comes between.
    for (width, height) in [(4, 2), (5, 2), (5, 3)] {
        fake.desc = BufferDesc::new(width, height, width * 4, PixelFormat::Rgba8Premul).unwrap();
        fake.bytes = vec![0x11; fake.desc.byte_len() as usize];
        // A new frame is presented whole whatever the damage.
        let corner = damage(&fake, &[(0, 0, 1, 1)]);
        taken(&log);
        shell.present(&mut fake, SURFACE, &red, &corner).unwrap();
        assert_eq!(
            taken(&log),
            [
                "buffer 1".to_string(),
                format!("present 1 0,0 {width}x{height}")
            ]
        );
        let frame = shell.surfaces[&SURFACE].frame.as_ref().unwrap();
        assert_eq!(
            (frame.desc().width(), frame.desc().height()),
            (width, height)
        );
        assert_eq!(
            fake.bytes,
            [255, 0, 0, 255].repeat((width * height) as usize)
        );
        release(&mut shell, &mut fake);
    }
}

#[test]
fn a_frame_beyond_the_buffer_limits_is_refused() {
    // An RGB565 buffer of 8,192 by 4,096 pixels holds 64 MiB, the limit;
    // its previous frame, 4 bytes per pixel, would hold twice as many.
    let log = Log::default();
    let mut fake = Fake::new(&log, 8_192, 4_096, PixelFormat::Rgb565);
    let mut shell = calculator(&log, 0);
    let whole = damage(&fake, &[(0, 0, 8_192, 4_096)]);
    assert_eq!(
        shell.present(&mut fake, SURFACE, &DisplayList::default(), &whole),
        Err(ShellError::FrameTooLarge)
    );
    assert_eq!(taken(&log), ["buffer 1"]);
}

#[test]
fn an_rgb565_buffer_takes_the_conversion_of_a_frame_of_4_bytes_per_pixel() {
    let log = Log::default();
    let mut fake = Fake::new(&log, 4, 2, PixelFormat::Rgb565);
    let mut shell = calculator(&log, 0);
    let whole = damage(&fake, &[(0, 0, 4, 2)]);
    let orange = fill(0, 0, 4, 2, Color::opaque(200, 100, 50));
    shell.present(&mut fake, SURFACE, &orange, &whole).unwrap();
    // The buffer has 2 bytes per pixel, its previous frame 4.
    let frame = shell.surfaces[&SURFACE].frame.as_ref().unwrap();
    assert_eq!(frame.desc().stride(), 16);
    let source = BufferDesc::new(4, 2, 16, PixelFormat::Rgba8Premul).unwrap();
    let mut expected = vec![0; 16];
    convert(
        source,
        &[200, 100, 50, 255].repeat(8),
        DamageRect::new(0, 0, 4, 2).unwrap(),
        fake.desc,
        &mut expected,
        0,
        0,
    )
    .unwrap();
    assert_eq!(fake.bytes, expected);
    // R 24, G 25 and B 6 in a little-endian word.
    assert_eq!(&fake.bytes[..2], &[0x26, 0xC3]);
}

#[test]
fn run_waits_with_the_deadline_that_each_round_leaves_and_shuts_down() {
    let log = Log::default();
    let mut fake = Fake::new(&log, 4, 2, PixelFormat::Rgba8Premul);
    fake.batches.push_back(vec![focus_in(), execute_key()]);
    fake.batches.push_back(Vec::new());
    fake.disconnect = true;
    let Shell { app, port, .. } = calculator(&log, 1);
    let status = run(&mut fake, app, port);
    assert_eq!(
        status,
        ExitStatus {
            reason: ExitReason::Disconnected,
            shutdown: Ok(())
        }
    );
    let polling = 7 + crate::JOB_POLL_INTERVAL;
    assert_eq!(
        taken(&log),
        [
            FETCHES[0].to_string(),
            FETCHES[1].to_string(),
            "wait 0".to_string(),
            "now".to_string(),
            "take_reports".to_string(),
            "submit job".to_string(),
            "wait 7".to_string(),
            "now".to_string(),
            "take_reports".to_string(),
            format!("wait {polling}"),
            "shutdown".to_string(),
        ]
    );
}

#[test]
fn run_shuts_down_and_reports_a_missing_resource_before_any_wait() {
    let log = Log::default();
    let mut fake = Fake::new(&log, 4, 2, PixelFormat::Rgba8Premul);
    fake.resources.remove(&SANS_RESOURCE);
    let status = run(&mut fake, TestApp::default(), TestPort::default());
    assert_eq!(status.reason, ExitReason::MissingResource(SANS_RESOURCE));
    assert_eq!(status.shutdown, Ok(()));
    assert_eq!(taken(&log), [FETCHES[0], "shutdown"]);
    // Bytes that are not the fonts end it with a failure.
    let mut fake = Fake::new(&log, 4, 2, PixelFormat::Rgba8Premul);
    fake.resources.insert(MONO_RESOURCE, Vec::new());
    let status = run(&mut fake, TestApp::default(), TestPort::default());
    let fonts = ShellError::Fonts(FontSetError::NotTrueType(MONO));
    assert_eq!(status.reason, ExitReason::Failed(fonts));
    assert_eq!(taken(&log), [FETCHES[0], FETCHES[1], "shutdown"]);
}

#[test]
fn glyph_runs_are_drawn_from_the_glyph_cache_into_a_kept_frame() {
    let log = Log::default();
    let mut fake = Fake::new(&log, 4, 2, PixelFormat::Rgba8Premul);
    let mut shell = calculator(&log, 0);
    shell.run_round(&mut fake, None).unwrap();
    let whole = damage(&fake, &[(0, 0, 4, 2)]);
    // The first frame of the surface is new; the next is drawn into the
    // frame that the shell keeps, and its glyphs come from the cache too.
    let clear = fill(0, 0, 4, 2, Color::TRANSPARENT);
    shell.present(&mut fake, SURFACE, &clear, &whole).unwrap();
    release(&mut shell, &mut fake);
    let list = glyph_run(SANS);
    shell.present(&mut fake, SURFACE, &list, &whole).unwrap();
    let mut set = FontSet::new(SANS_BYTES, MONO_BYTES).unwrap();
    let mut rasterizer = VelloRasterizer::new();
    set.prepare(&list, &mut rasterizer).unwrap();
    let mut expected = new_frame(fake.desc).unwrap();
    rasterizer
        .render(
            &list,
            &whole,
            set.glyphs(),
            &ImageStore::EMPTY,
            &mut expected,
        )
        .unwrap();
    assert_eq!(fake.bytes, expected.bytes());
    assert!(fake.bytes.iter().any(|&byte| byte != 0));
}

/// A control of the label: a column, or a text leaf in Fira Sans.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Label {
    Column,
    Text(&'static str),
}

impl Control<Labels> for Label {
    type State = ();

    fn kind(&self) -> crate::tree::Kind {
        crate::tree::Kind::new(match self {
            Label::Column => "column",
            Label::Text(_) => "text",
        })
    }

    fn layout(&self, _: &()) -> layout::Node<'_> {
        match self {
            Label::Column => layout::Node::Column(Column::new()),
            Label::Text(text) => layout::Node::Text(Text::new(text, Font::Sans, FontSize::Body)),
        }
    }

    fn draw(&self, _: &mut DrawContext<'_, ()>) {}
}

/// An application whose view is a column with the text "Ag".
struct Labels;

impl App for Labels {
    type Message = ();
    type Control = Label;
    type Port = TestPort;

    fn update(&mut self, _: Message<Self>, _: &mut Out<Self>) {}

    fn view(&self) -> View<Label> {
        View::new(Key::new(0), Label::Column).child(View::new(Key::new(0), Label::Text("Ag")))
    }
}

/// The layout lines of the record of `end`.
fn layout_lines(end: &RoundEnd) -> Vec<String> {
    let lines = end.output.record.lines();
    lines
        .iter()
        .filter(|line| line.starts_with("layout "))
        .cloned()
        .collect()
}

#[test]
fn each_round_lays_out_the_configured_surfaces_with_the_shells_font_set() {
    let log = Log::default();
    let mut fake = Fake::new(&log, 64, 8, PixelFormat::Rgba8Premul);
    let mut shell = Shell::new(Labels, TestPort::default());
    // Before a `Configure` the surface has no size, and nothing is laid
    // out.
    let first = shell.run_round(&mut fake, None).unwrap();
    assert!(layout_lines(&first).is_empty());
    assert!(shell.runtime().layout(SURFACE).is_none());
    let size = PixelSize::new(64, 8).unwrap();
    fake.batches.push_back(vec![PlatformEvent::Configure {
        surface: SURFACE,
        size,
    }]);
    let second = shell.run_round(&mut fake, None).unwrap();
    assert_eq!(
        layout_lines(&second),
        ["layout surface 1 boxes 2 laid 2 texts 1"]
    );
    let laid = shell.runtime().layout(SURFACE).unwrap();
    let px = |pixels| Fx::from_pixels(pixels).unwrap();
    let surface = Size::new(px(64), px(8)).unwrap();
    assert_eq!(laid.boxes()[0].constraint, Constraint::tight(surface));
    assert!(laid.boxes()[1].text.is_some());
    // The fonts are not fetched again, and an unchanged tree is not laid
    // out again.
    taken(&log);
    let third = shell.run_round(&mut fake, None).unwrap();
    assert_eq!(taken(&log), ["wait none", "now"]);
    assert_eq!(
        layout_lines(&third),
        ["layout surface 1 boxes 2 laid 0 texts 0"]
    );
}

#[test]
fn rounds_lay_text_out_with_the_font_set_that_the_shell_fetched() {
    // A resource table with the two fonts swapped: the shell builds its
    // font set from them, with Fira Mono in the place of Fira Sans, and the
    // round lays the text out with that font set, not with one of its own.
    let log = Log::default();
    let mut fake = Fake::new(&log, 64, 8, PixelFormat::Rgba8Premul);
    fake.resources.insert(SANS_RESOURCE, MONO_BYTES.to_vec());
    fake.resources.insert(MONO_RESOURCE, SANS_BYTES.to_vec());
    fake.batches.push_back(vec![PlatformEvent::Configure {
        surface: SURFACE,
        size: PixelSize::new(64, 8).unwrap(),
    }]);
    let mut shell = Shell::new(Labels, TestPort::default());
    shell.run_round(&mut fake, None).unwrap();
    let laid = shell.runtime().layout(SURFACE).unwrap();
    let width = laid.boxes()[1].text.as_ref().unwrap().paragraph.width();
    let width_in = |sans: &[u8], mono: &[u8]| {
        let mut fonts = FontSet::new(sans, mono).unwrap();
        let style = kosmo_text::TextStyle::new(SANS, Fx::from_pixels(16).unwrap()).unwrap();
        let limit = Some(Fx::from_pixels(64).unwrap());
        fonts.layout("Ag", style, limit).unwrap().width()
    };
    assert_eq!(width, width_in(MONO_BYTES, SANS_BYTES));
    assert_ne!(width, width_in(SANS_BYTES, MONO_BYTES));
}
