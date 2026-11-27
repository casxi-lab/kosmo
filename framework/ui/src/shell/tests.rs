use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::rc::Rc;

use kosmo_platform::accesskit::{Node, NodeId, Role, TreeId, TreeInfo, TreeUpdate};
use kosmo_platform::{
    AccessibilitySink, Capabilities, Clock, EventSource, PixelBufferMut, PlatformCaps,
    ResourceProvider, SurfacePort, SurfaceRequest,
};
use kosmo_render::{Color, Command, FontId, Glyph, GlyphId, ListBuilder, RenderError};
use kosmo_types::{DamageRect, Fx, KeyAction, KeyMeaning, NamedKey, Point, Rect, Size, convert};

use super::*;
use crate::command::{EVALUATE_SELECTION, MOVE_LEFT};
use crate::testing::{SURFACE, TestApp, TestPort, Widget, key_event, node, stamp};
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
    /// are all `0x11`, the resource `font` and its clock at 7.
    fn new(log: &Log, width: u32, height: u32, format: PixelFormat) -> Fake {
        let stride = width * format.bytes_per_pixel();
        let desc = BufferDesc::new(width, height, stride, format).unwrap();
        Fake {
            log: Rc::clone(log),
            batches: VecDeque::new(),
            disconnect: false,
            clock: MonoTime::from_nanos(7),
            resources: BTreeMap::from([(FONT, vec![1, 2, 3])]),
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

const FONT: ResourceId = ResourceId::new("font.ttf");

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
    Shell::new(app, port, &[FONT])
}

fn focus_in() -> PlatformEvent {
    PlatformEvent::FocusIn {
        surface: SURFACE,
        stamp: stamp(1, 1),
    }
}

fn execute_key() -> PlatformEvent {
    PlatformEvent::Key {
        surface: SURFACE,
        stamp: stamp(1, 1),
        event: key_event(KeyMeaning::Named(NamedKey::EXECUTE), KeyAction::Press),
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
/// shell presents on it again.
fn release(shell: &mut Shell<TestApp>, fake: &mut Fake) {
    fake.in_flight = false;
    fake.batches
        .push_back(vec![PlatformEvent::Released { surface: SURFACE }]);
    shell.run_round(fake, None).unwrap();
}

#[test]
fn a_round_waits_reads_the_clock_takes_reports_and_runs_both_steps_in_order() {
    let log = Log::default();
    let mut fake = Fake::new(&log, 4, 2, PixelFormat::Rgba8Premul);
    fake.batches.push_back(vec![focus_in(), execute_key()]);
    let mut shell = calculator(&log, 0);

    let first = shell.run_round(&mut fake, shell.next_deadline()).unwrap();
    // The resource before the first wait, then the wait, the clock, the
    // reports and the effect of the first step.
    assert_eq!(
        taken(&log),
        [
            "resource font.ttf",
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
    // the resources are not fetched again.
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
    let missing = ResourceId::new("missing.ttf");
    let mut shell = Shell::new(TestApp::default(), TestPort::default(), &[FONT, missing]);
    assert_eq!(
        shell.run_round(&mut fake, None),
        Err(ShellError::MissingResource(missing))
    );
    assert_eq!(taken(&log), ["resource font.ttf", "resource missing.ttf"]);
    // Nothing ran, so the next round fetches them again.
    fake.resources.insert(missing, Vec::new());
    shell.run_round(&mut fake, None).unwrap();
    assert_eq!(
        taken(&log),
        [
            "resource font.ttf",
            "resource missing.ttf",
            "wait none",
            "now"
        ]
    );
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

#[test]
fn a_frame_of_an_output_for_a_presented_buffer_stops_the_delivery_with_busy() {
    let log = Log::default();
    let mut fake = Fake::new(&log, 4, 2, PixelFormat::Rgba8Premul);
    let mut shell = calculator(&log, 0);
    let mut output = shell.run_round(&mut fake, None).unwrap().output;
    let whole = damage(&fake, &[(0, 0, 4, 2)]);
    output
        .frames
        .push((SURFACE, fill(0, 0, 4, 2, Color::opaque(1, 2, 3)), whole));
    shell.deliver(&mut fake, &output).unwrap();
    taken(&log);
    // The buffer is presented and not released, so the same output's frame
    // is refused, and the delivery returns the error.
    assert_eq!(
        shell.deliver(&mut fake, &output),
        Err(ShellError::Platform(PlatformError::Busy))
    );
    assert!(taken(&log).is_empty());
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

#[test]
fn a_frame_that_cannot_be_drawn_changes_nothing() {
    let log = Log::default();
    let mut fake = Fake::new(&log, 4, 2, PixelFormat::Rgba8Premul);
    let mut shell = calculator(&log, 0);
    let whole = damage(&fake, &[(0, 0, 4, 2)]);
    let mut builder = ListBuilder::new();
    builder
        .push(Command::Glyphs {
            font: FontId::new(0),
            size: Fx::from_units(640),
            color: Color::opaque(0, 0, 0),
            glyphs: vec![Glyph {
                id: GlyphId::new(3),
                origin: Point::default(),
            }],
        })
        .unwrap();
    let glyphs = builder.finish().unwrap();
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
    let configure = |width, height| PlatformEvent::Configure {
        surface: SURFACE,
        size: PixelSize::new(width, height).unwrap(),
    };
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
    let status = run(&mut fake, app, port, &[FONT]);
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
            "resource font.ttf".to_string(),
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
    let missing = ResourceId::new("missing.ttf");
    let status = run(
        &mut fake,
        TestApp::default(),
        TestPort::default(),
        &[missing],
    );
    assert_eq!(status.reason, ExitReason::MissingResource(missing));
    assert_eq!(status.shutdown, Ok(()));
    assert_eq!(taken(&log), ["resource missing.ttf", "shutdown"]);
}
