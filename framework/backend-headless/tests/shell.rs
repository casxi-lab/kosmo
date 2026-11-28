//! The shell of the framework core on the headless backend: `run` returns
//! distinct exit statuses when the platform disconnects, when a font is
//! missing at start-up and when another error stops it, shutting the
//! backend down in each case; the frame that a round draws is what the
//! backend reads back; and text that a round lays out and draws with the
//! fonts that the shell fetched from the backend's resources has the
//! coverage of the pixel golden of `kosmo-text`.

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
use kosmo_testkit::pixels::PixelDigest;
use kosmo_text::{MONO_FILE, SANS_FILE};
use kosmo_types::{BufferDesc, Damage, DamageRect, Fx, PixelFormat, Point, Rect, Size};
use kosmo_ui::{
    App, ColorToken, Column, Control, DrawContext, ExitReason, Font, FontSize, JobPort, Key, Kind,
    Message, Node, Out, Row, Shell, ShellError, Space, TOKENS, Text, View, run,
};

/// The two fonts as resources, named by their files in the asset manifest
/// of `kosmo-text`, and their bytes from its asset directory.
const SANS_RESOURCE: ResourceId = ResourceId::new(SANS_FILE);
const MONO_RESOURCE: ResourceId = ResourceId::new(MONO_FILE);
const SANS_BYTES: &[u8] = include_bytes!("../../text/assets/fonts/firasans/FiraSans-Regular.ttf");
const MONO_BYTES: &[u8] = include_bytes!("../../text/assets/fonts/firamono/FiraMono-Regular.ttf");

/// The pixel golden of the ASCII string of `kosmo-text` and the manifest
/// of its goldens.
const TEXT_ASCII: &[u8] = include_bytes!("../../text/tests/golden/text-ascii.px");
const MANIFEST: &str = include_str!("../../text/tests/golden/MANIFEST");

/// The resource table of the two fonts.
fn fonts() -> BTreeMap<ResourceId, Vec<u8>> {
    BTreeMap::from([
        (SANS_RESOURCE, SANS_BYTES.to_vec()),
        (MONO_RESOURCE, MONO_BYTES.to_vec()),
    ])
}

/// An application of a row of two boxes of 4 by 4 pixels, which fill
/// themselves with the colour of their tokens: the accent and the colour
/// of errors.
struct Boxes;

#[derive(Clone, Debug, PartialEq)]
enum Part {
    Row,
    Fill(ColorToken),
}

impl Control<Boxes> for Part {
    type State = ();

    fn kind(&self) -> Kind {
        Kind::new(match self {
            Part::Row => "row",
            Part::Fill(_) => "fill",
        })
    }

    fn layout(&self, _: &()) -> Node<'_> {
        match self {
            Part::Row => Node::Row(Row::new()),
            Part::Fill(_) => Node::Fixed(Size::new(SIDE, SIDE).unwrap_or(Size::ZERO)),
        }
    }

    fn draw(&self, cx: &mut DrawContext<'_, ()>) {
        if let Part::Fill(token) = self {
            cx.draw(Command::FillRect {
                rect: Rect::new(Point::default(), cx.size()),
                color: cx.color(*token),
            });
        }
    }
}

/// The side of the boxes: 4 pixels.
const SIDE: Fx = Fx::from_units(4 * 64);

impl App for Boxes {
    type Message = ();
    type Control = Part;
    type Port = NoJobs;

    fn update(&mut self, _: Message<Self>, _: &mut Out<Self>) {}

    fn view(&self) -> View<Part> {
        View::new(Key::new(0), Part::Row)
            .child(View::new(Key::new(1), Part::Fill(ColorToken::Accent)))
            .child(View::new(Key::new(2), Part::Fill(ColorToken::Error)))
    }
}

/// The string of the pixel golden of `kosmo-text`.
const FOX: &str = "The quick brown fox jumps over the lazy dog.";

/// An application of a column with a padding of 2 pixels, which holds the
/// text leaf of the string of the golden of `kosmo-text` in Fira Sans at
/// 16 pixels per em; the text leaf draws the text that the layout laid
/// out.
struct Words;

#[derive(Clone, Debug, PartialEq)]
enum Word {
    Column,
    Text,
}

impl Control<Words> for Word {
    type State = ();

    fn kind(&self) -> Kind {
        Kind::new(match self {
            Word::Column => "column",
            Word::Text => "text",
        })
    }

    fn layout(&self, _: &()) -> Node<'_> {
        match self {
            Word::Column => Node::Column(Column::new().padding(Space::Xs)),
            Word::Text => Node::Text(Text::new(FOX, Font::Sans, FontSize::Body)),
        }
    }

    fn draw(&self, cx: &mut DrawContext<'_, ()>) {
        if let Some(text) = cx.text()
            && let Ok(run) = text.paragraph.run(Point::default(), cx.color(text.color))
        {
            cx.draw(run);
        }
    }
}

impl App for Words {
    type Message = ();
    type Control = Word;
    type Port = NoJobs;

    fn update(&mut self, _: Message<Self>, _: &mut Out<Self>) {}

    fn view(&self) -> View<Word> {
        View::new(Key::new(0), Word::Column).child(View::new(Key::new(1), Word::Text))
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

/// A headless backend for an 8 by 4 output in `Xrgb8888`, with the two
/// fonts as resources.
#[allow(clippy::unwrap_used, reason = "a fixed size that is valid")]
fn backend() -> HeadlessBackend {
    let caps = PlatformCaps::new(vec![PixelFormat::Xrgb8888], PixelSize::new(8, 4).unwrap());
    HeadlessBackend::new(caps, fonts())
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
    let status = run(&mut backend, Boxes, NoJobs);
    assert_eq!(status.reason, ExitReason::Disconnected);
    assert_eq!(status.shutdown, Ok(()));
    // The backend is shut down: its ports report the disconnection.
    assert_eq!(
        backend.resource(SANS_RESOURCE),
        Err(PlatformError::Disconnected)
    );
}

#[test]
fn run_returns_another_status_when_a_resource_is_missing_at_start_up() {
    let mut backend = backend();
    backend
        .inject_fault(FaultPort::Resource, PlatformError::NotFound)
        .unwrap();
    let status = run(&mut backend, Boxes, NoJobs);
    assert_eq!(status.reason, ExitReason::MissingResource(SANS_RESOURCE));
    assert_ne!(status.reason, ExitReason::Disconnected);
    assert_eq!(status.shutdown, Ok(()));
    assert_eq!(
        backend.resource(SANS_RESOURCE),
        Err(PlatformError::Disconnected)
    );

    // A font that the table does not hold is missing too.
    let mut table = fonts();
    table.remove(&MONO_RESOURCE);
    let caps = PlatformCaps::new(vec![PixelFormat::Xrgb8888], PixelSize::new(8, 4).unwrap());
    let status = run(&mut HeadlessBackend::new(caps, table), Boxes, NoJobs);
    assert_eq!(status.reason, ExitReason::MissingResource(MONO_RESOURCE));
    assert_eq!(status.shutdown, Ok(()));
}

#[test]
fn run_stops_at_any_other_error_with_a_failure_and_shuts_down() {
    // A resource that fails otherwise than as missing. The shell runs no
    // further round, so the disconnection that waits at the wait is never
    // reached.
    let mut resource = backend();
    resource
        .inject_fault(FaultPort::Resource, PlatformError::Backend)
        .unwrap();
    resource
        .inject_fault(FaultPort::Wait, PlatformError::Disconnected)
        .unwrap();
    let status = run(&mut resource, Boxes, NoJobs);
    let failed = ExitReason::Failed(ShellError::Platform(PlatformError::Backend));
    assert_eq!(status.reason, failed);
    assert_ne!(status.reason, ExitReason::MissingResource(SANS_RESOURCE));
    assert_eq!(status.shutdown, Ok(()));
    assert_eq!(
        resource.resource(SANS_RESOURCE),
        Err(PlatformError::Disconnected)
    );

    // A wait that fails.
    let mut wait = backend();
    wait.inject_fault(FaultPort::Wait, PlatformError::Backend)
        .unwrap();
    let status = run(&mut wait, Boxes, NoJobs);
    assert_eq!(status.reason, failed);
    assert_eq!(status.shutdown, Ok(()));
    assert_eq!(
        wait.resource(SANS_RESOURCE),
        Err(PlatformError::Disconnected)
    );
}

#[test]
fn the_frame_that_a_round_draws_is_what_the_backend_reads_back() {
    let mut backend = backend();
    let size = PixelSize::new(8, 4).unwrap();
    let surface = backend
        .create_surface(SurfaceRequest {
            role: SurfaceRole::Main,
            size,
            format: PixelFormat::Xrgb8888,
        })
        .unwrap();
    let mut shell = Shell::new(Boxes, NoJobs);
    // The first round takes the backend's Configure and FocusIn, builds the
    // view, lays it out over the surface, draws it whole and presents the
    // frame.
    let first = shell
        .run_round(&mut backend, shell.next_deadline())
        .unwrap();
    assert_eq!(first.wait, WaitEnd::Events);
    assert_eq!(shell.size(surface), Some(size));
    let frame = BufferDesc::new(8, 4, 32, PixelFormat::Rgba8Premul).unwrap();
    let mut whole = Damage::new(frame);
    whole.add(0, 0, 8, 4);
    assert_eq!(first.output.frames.len(), 1);
    let (drawn_on, list, damage) = &first.output.frames[0];
    assert_eq!((*drawn_on, *damage), (surface, whole));

    // The same list drawn into a frame of its own and converted.
    let mut drawn = RenderTarget::new(frame).unwrap();
    VelloRasterizer::new()
        .render(list, &whole, &NoGlyphs, &ImageStore::EMPTY, &mut drawn)
        .unwrap();
    let shown = BufferDesc::new(8, 4, 32, PixelFormat::Xrgb8888).unwrap();
    let mut expected = vec![0; 128];
    drawn.convert_into(&whole, shown, &mut expected).unwrap();
    let read = backend.read_back(surface).unwrap().unwrap();
    assert_eq!(read.desc, shown);
    assert_eq!(read.bytes, expected);
    assert_eq!(read.damage, whole.rects());
    // B, G, R, X: the accent at (0, 0), the colour of errors at (4, 1).
    assert_eq!(&read.bytes[..4], &[0xBF, 0x5F, 0x1F, 0xFF]);
    assert_eq!(&read.bytes[48..52], &[0x1E, 0x26, 0xB3, 0xFF]);

    // Until a wait returns the release, a driver does not present the
    // surface again; the round that takes the release, in which nothing
    // changed, draws no frame.
    assert_eq!(
        shell.present(&mut backend, surface, list, &whole),
        Err(ShellError::Platform(PlatformError::Busy))
    );
    let released = shell
        .run_round(&mut backend, shell.next_deadline())
        .unwrap();
    assert_eq!(released.wait, WaitEnd::Events);
    assert!(released.output.frames.is_empty());

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

#[test]
fn a_new_previous_frame_is_presented_whole_whatever_its_damage() {
    let mut backend = backend();
    let mut shell = Shell::new(Boxes, NoJobs);
    // The first round fetches the fonts. The surface comes after it, so no
    // round has drawn it and the shell keeps no frame of it.
    shell
        .run_round(&mut backend, shell.next_deadline())
        .unwrap();
    let surface = backend
        .create_surface(SurfaceRequest {
            role: SurfaceRole::Main,
            size: PixelSize::new(8, 4).unwrap(),
            format: PixelFormat::Xrgb8888,
        })
        .unwrap();
    // A red box at (2, 1) over transparent black, presented first with the
    // damage of the top-left 2 by 1 pixels only.
    let mut builder = ListBuilder::new();
    builder
        .push(Command::FillRect {
            rect: pixels(2, 1, 4, 2),
            color: Color::opaque(255, 0, 0),
        })
        .unwrap();
    let list = builder.finish().unwrap();
    let frame = BufferDesc::new(8, 4, 32, PixelFormat::Rgba8Premul).unwrap();
    let mut corner = Damage::new(frame);
    corner.add(0, 0, 2, 1);
    let work = shell
        .present(&mut backend, surface, &list, &corner)
        .unwrap();
    assert_eq!(work.pixels, 32);

    // The whole list drawn into a frame of its own and converted whole.
    let mut drawn = RenderTarget::new(frame).unwrap();
    let whole = drawn.whole();
    VelloRasterizer::new()
        .render(&list, &whole, &NoGlyphs, &ImageStore::EMPTY, &mut drawn)
        .unwrap();
    let shown = BufferDesc::new(8, 4, 32, PixelFormat::Xrgb8888).unwrap();
    let mut expected = vec![0; 128];
    drawn.convert_into(&whole, shown, &mut expected).unwrap();
    let read = backend.read_back(surface).unwrap().unwrap();
    assert_eq!(read.bytes, expected);
    assert_eq!(read.damage, whole.rects());
    // B, G, R, X: black at (0, 0), red at (2, 1), and X 0xFF everywhere.
    assert_eq!(&read.bytes[..4], &[0, 0, 0, 255]);
    assert_eq!(&read.bytes[40..44], &[0, 0, 255, 255]);
    assert!(read.bytes.chunks(4).all(|pixel| pixel[3] == 0xFF));
}

#[test]
fn text_that_a_round_draws_has_the_coverage_of_the_golden_of_kosmo_text() {
    // A surface of the golden's canvas, whose width and height its
    // description gives.
    let field = |at: usize| u32::from_le_bytes(TEXT_ASCII[at..at + 4].try_into().unwrap());
    let (width, height) = (field(0), field(4));
    let size = PixelSize::new(width, height).unwrap();
    let caps = PlatformCaps::new(vec![PixelFormat::Rgba8Premul], size);
    let mut backend = HeadlessBackend::new(caps, fonts());
    let surface = backend
        .create_surface(SurfaceRequest {
            role: SurfaceRole::Main,
            size,
            format: PixelFormat::Rgba8Premul,
        })
        .unwrap();
    // The first round fetches the fonts from the resource table, lays the
    // string out with them at 16 pixels per em, 2 pixels from the edges,
    // inside the golden's margin, and draws it in the colour of text.
    let mut shell = Shell::new(Words, NoJobs);
    let first = shell
        .run_round(&mut backend, shell.next_deadline())
        .unwrap();
    assert_eq!(first.output.frames.len(), 1);

    // The golden is drawn in opaque black, so its alpha is the coverage of
    // the glyphs, and its digest is the one that its manifest lists.
    let frame = BufferDesc::new(width, height, width * 4, PixelFormat::Rgba8Premul).unwrap();
    let golden = &TEXT_ASCII[16..];
    let digest = PixelDigest::of(frame, golden).unwrap().hex();
    assert!(
        MANIFEST.contains(&format!("{digest}  text-ascii\n")),
        "{digest}"
    );
    // The buffer read back has that coverage byte for byte, in the opaque
    // colour of text: transparent black where the golden is, that colour
    // where the golden is opaque black.
    let read = backend.read_back(surface).unwrap().unwrap();
    assert_eq!(read.desc, frame);
    let alpha = |bytes: &[u8]| bytes.chunks(4).map(|pixel| pixel[3]).collect::<Vec<u8>>();
    assert!(alpha(read.bytes) == alpha(golden), "the coverage differs");
    let text = TOKENS.color(ColorToken::Text).to_bytes();
    for (pixel, golden) in read.bytes.chunks(4).zip(golden.chunks(4)) {
        match golden[3] {
            0 => assert_eq!(pixel, [0, 0, 0, 0]),
            255 => assert_eq!(pixel, text),
            _ => {}
        }
    }
    assert!(alpha(golden).contains(&255));
}
