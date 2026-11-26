//! The shell of the framework core on the headless backend: `run` returns
//! distinct exit statuses when the platform disconnects and when a resource
//! is missing at start-up, shutting the backend down in both cases, and a
//! frame that the shell presents is what the backend reads back.

// These tests need the test hooks: the check builds this package once
// more with them, and a build without them compiles this file empty.
#![cfg(feature = "test-hooks")]

use std::collections::BTreeMap;

use kosmo_backend_headless::HeadlessBackend;
use kosmo_platform::{
    FaultPort, PixelSize, PlatformCaps, PlatformError, PlatformTestHooks, ResourceId,
    ResourceProvider, SurfacePort, SurfaceRequest, SurfaceRole, WaitEnd,
};
use kosmo_render::{
    Color, Command, ImageStore, ListBuilder, NoGlyphs, Rasterizer, RenderTarget, VelloRasterizer,
};
use kosmo_types::{BufferDesc, Damage, DamageRect, Fx, PixelFormat, Point, Rect, Size};
use kosmo_ui::{
    App, Control, DrawContext, ExitReason, JobPort, Key, Kind, LayoutBox, LayoutResult, Message,
    Out, Shell, ShellError, View, paint, run,
};

/// The resource of the application.
const FONT: ResourceId = ResourceId::new("font.ttf");

const BLUE: Color = Color::opaque(0, 0, 255);
const RED: Color = Color::opaque(255, 0, 0);

/// An application of two controls, a root and its child, each of which
/// fills its box with its color.
struct Boxes;

#[derive(Clone, Debug, PartialEq)]
struct Fill(Color);

impl Control<Boxes> for Fill {
    type State = ();

    fn kind(&self) -> Kind {
        Kind::new("fill")
    }

    fn draw(&self, cx: &mut DrawContext<'_, ()>) {
        cx.draw(Command::FillRect {
            rect: Rect::new(Point::default(), cx.size()),
            color: self.0,
        });
    }
}

impl App for Boxes {
    type Message = ();
    type Control = Fill;
    type Port = NoJobs;

    fn update(&mut self, _: Message<Self>, _: &mut Out<Self>) {}

    fn view(&self) -> View<Fill> {
        View::new(Key::new(0), Fill(BLUE)).child(View::new(Key::new(1), Fill(RED)))
    }
}

/// An execution port that runs no jobs.
struct NoJobs;

impl JobPort for NoJobs {
    type Request = ();
    type Submission = u64;
    type Rejection = ();
    type CancelAnswer = ();
    type Report = ();

    fn submit(&mut self, _: ()) -> Result<u64, ()> {
        Err(())
    }

    fn cancel(&mut self, _: &u64) {}

    fn take_reports(&mut self, _: &mut Vec<()>) -> usize {
        0
    }
}

/// A headless backend for an 8 by 4 output in `Xrgb8888`, with the
/// resource `font.ttf`.
#[allow(clippy::unwrap_used, reason = "a fixed size that is valid")]
fn backend() -> HeadlessBackend {
    let caps = PlatformCaps::new(vec![PixelFormat::Xrgb8888], PixelSize::new(8, 4).unwrap());
    HeadlessBackend::new(caps, BTreeMap::from([(FONT, vec![0, 1, 0, 0])]))
}

/// The rectangle of `width` by `height` pixels at (`x`, `y`).
#[allow(clippy::unwrap_used, reason = "small sizes that fit")]
fn pixels(x: i32, y: i32, width: i32, height: i32) -> Rect {
    let px = |pixels| Fx::from_pixels(pixels).unwrap();
    Rect::new(
        Point::new(px(x), px(y)),
        Size::new(px(width), px(height)).unwrap(),
    )
}

#[test]
fn run_shuts_down_and_returns_when_the_platform_disconnects() {
    let mut backend = backend();
    backend
        .inject_fault(FaultPort::Wait, PlatformError::Disconnected)
        .unwrap();
    let status = run(&mut backend, Boxes, NoJobs, &[FONT]);
    assert_eq!(status.reason, ExitReason::Disconnected);
    assert_eq!(status.shutdown, Ok(()));
    // The backend is shut down: its ports report the disconnection.
    assert_eq!(backend.resource(FONT), Err(PlatformError::Disconnected));
}

#[test]
fn run_returns_another_status_when_a_resource_is_missing_at_start_up() {
    let mut backend = backend();
    backend
        .inject_fault(FaultPort::Resource, PlatformError::NotFound)
        .unwrap();
    let status = run(&mut backend, Boxes, NoJobs, &[FONT]);
    assert_eq!(status.reason, ExitReason::MissingResource(FONT));
    assert_ne!(status.reason, ExitReason::Disconnected);
    assert_eq!(status.shutdown, Ok(()));
    assert_eq!(backend.resource(FONT), Err(PlatformError::Disconnected));

    // A resource that the table does not hold is missing too.
    let absent = ResourceId::new("absent.ttf");
    let status = run(&mut self::backend(), Boxes, NoJobs, &[FONT, absent]);
    assert_eq!(status.reason, ExitReason::MissingResource(absent));
    assert_eq!(status.shutdown, Ok(()));
}

#[test]
fn a_frame_that_the_shell_presents_is_what_the_backend_reads_back() {
    let mut backend = backend();
    let size = PixelSize::new(8, 4).unwrap();
    let surface = backend
        .create_surface(SurfaceRequest {
            role: SurfaceRole::Main,
            size,
            format: PixelFormat::Xrgb8888,
        })
        .unwrap();
    let mut shell = Shell::new(Boxes, NoJobs, &[FONT]);
    // The first round takes the backend's Configure and FocusIn and builds
    // the view.
    let first = shell
        .run_round(&mut backend, shell.next_deadline())
        .unwrap();
    assert_eq!(first.wait, WaitEnd::Events);
    assert_eq!(shell.size(surface), Some(size));

    // The layout result that the layout pass will give: the root covers
    // the surface, and its child is a 4 by 2 box at (2, 1).
    let tree = shell.runtime().tree();
    let root = tree.root().unwrap();
    let mut layout = LayoutResult::new();
    for (widget, parent, rect) in [
        (root, None, pixels(0, 0, 8, 4)),
        (tree.children(root)[0], Some(0), pixels(2, 1, 4, 2)),
    ] {
        layout
            .push(LayoutBox {
                widget,
                parent,
                rect,
                scroll: Point::default(),
                draws: true,
            })
            .unwrap();
    }
    let frame = BufferDesc::new(8, 4, 32, PixelFormat::Rgba8Premul).unwrap();
    let mut whole = Damage::new(frame);
    whole.add(0, 0, 8, 4);
    let list = paint(tree, &layout, &whole).unwrap();
    let work = shell.present(&mut backend, surface, &list, &whole).unwrap();
    assert_eq!(work.pixels, 32);

    // The same list drawn into a frame of its own and converted.
    let mut drawn = RenderTarget::new(frame).unwrap();
    VelloRasterizer::new()
        .render(&list, &whole, &NoGlyphs, &ImageStore::EMPTY, &mut drawn)
        .unwrap();
    let shown = BufferDesc::new(8, 4, 32, PixelFormat::Xrgb8888).unwrap();
    let mut expected = vec![0; 128];
    drawn.convert_into(&whole, shown, &mut expected).unwrap();
    let read = backend.read_back(surface).unwrap().unwrap();
    assert_eq!(read.desc, shown);
    assert_eq!(read.bytes, expected);
    assert_eq!(read.damage, whole.rects());
    // B, G, R, X: blue at (0, 0), red at (2, 1).
    assert_eq!(&read.bytes[..4], &[255, 0, 0, 255]);
    assert_eq!(&read.bytes[40..44], &[0, 0, 255, 255]);

    // Until a wait returns the release, the surface is not presented again.
    assert_eq!(
        shell.present(&mut backend, surface, &list, &whole),
        Err(ShellError::Platform(PlatformError::Busy))
    );
    let released = shell
        .run_round(&mut backend, shell.next_deadline())
        .unwrap();
    assert_eq!(released.wait, WaitEnd::Events);

    // A frame of the top-left 2 by 1 pixels in green changes only them.
    let mut builder = ListBuilder::new();
    builder
        .push(Command::FillRect {
            rect: pixels(0, 0, 8, 4),
            color: Color::opaque(0, 255, 0),
        })
        .unwrap();
    let green = builder.finish().unwrap();
    let mut corner = Damage::new(frame);
    corner.add(0, 0, 2, 1);
    shell
        .present(&mut backend, surface, &green, &corner)
        .unwrap();
    let read = backend.read_back(surface).unwrap().unwrap();
    for pixel in [0, 1] {
        expected[pixel * 4..pixel * 4 + 4].copy_from_slice(&[0, 255, 0, 255]);
    }
    assert_eq!(read.bytes, expected);
    assert_eq!(read.damage, [DamageRect::new(0, 0, 2, 1).unwrap()]);
}
