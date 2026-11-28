//! The shell: the single-round entry, which drives the round function
//! through the platform interface, the presentation of frames, and `run`.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;

use kosmo_platform::{
    PixelSize, Platform, PlatformError, PlatformEvent, Presenter, ResourceId, SurfaceId, WaitEnd,
};
use kosmo_render::{
    DisplayList, GlyphCoverage, ImageStore, NoGlyphs, Rasterizer, RenderError, RenderTarget,
    VelloRasterizer, WorkCount,
};
use kosmo_text::{FontSet, FontSetError, GlyphError, MONO_FILE, SANS_FILE};
use kosmo_types::{BufferDesc, ConvertError, Damage, MonoTime, PixelFormat};

use crate::app::App;
use crate::focus::Event;
use crate::port::{JobPort, Receipt, execute};
use crate::round::{Input, LayoutInput, Output, Runtime};
use crate::tokens::TOKENS;

/// Fira Sans, the first resource that the shell fetches before its first
/// round, named by its file in the asset manifest of `kosmo-text`.
const SANS_RESOURCE: ResourceId = ResourceId::new(SANS_FILE);

/// Fira Mono, the second resource that the shell fetches before its first
/// round, named by its file in the asset manifest of `kosmo-text`.
const MONO_RESOURCE: ResourceId = ResourceId::new(MONO_FILE);

/// The number that names the shell's font set in the layout input of its
/// rounds. A shell builds one font set and keeps it, so the number never
/// changes.
const FONTS_ID: u64 = 0;

/// The shell of an application: its [`Runtime`], the application and its
/// execution port, the receipts that wait for the next round, the font set
/// of `kosmo-text`, which it builds before its first round and hands to the
/// layout pass of each round, and the size, the previous frame and the
/// pending region of each surface, which it keeps for layout, drawing and
/// presentation.
///
/// [`Shell::run_round`], the single-round entry, runs one round through a
/// platform, and [`run`] runs rounds until the platform disconnects. The
/// shell holds no platform: each call borrows it, so a driver keeps its
/// backend between rounds. The shell creates and destroys no surface; the
/// application's assembly or the driver creates them through `SurfacePort`.
pub struct Shell<A: App> {
    runtime: Runtime<A>,
    app: A,
    port: A::Port,
    /// The font set, from the first round on.
    fonts: Option<FontSet>,
    receipts: Vec<Receipt<A::Port>>,
    next_deadline: Option<MonoTime>,
    surfaces: BTreeMap<SurfaceId, Surface>,
    rasterizer: VelloRasterizer,
}

/// What the shell knows of a surface.
#[derive(Debug, Default)]
struct Surface {
    /// The size of its latest `Configure`.
    size: Option<PixelSize>,
    /// The previous frame: a tightly packed `Rgba8Premul` buffer of the
    /// size of the surface's buffer.
    frame: Option<RenderTarget>,
    /// Its buffer is presented and not released yet.
    presented: bool,
    /// The region of the frames of rounds that came while its buffer was
    /// presented, and the whole surface after a new size or a dropped
    /// frame, which the next round after the release draws again.
    pending: Option<Damage>,
}

/// How a round of [`Shell::run_round`] ended.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq)]
pub struct RoundEnd {
    /// How the round's wait ended.
    pub wait: WaitEnd,
    /// The output of the round's second step.
    pub output: Output,
}

/// Why the shell stopped.
///
/// An error of [`Shell::run_round`] comes from fetching the fonts before the
/// first round, from the wait, from a port of the later steps or from
/// presentation. A round whose fonts or wait failed runs no later step; one
/// that failed later has run both steps of the round.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShellError {
    /// A port of the platform returned this error.
    Platform(PlatformError),
    /// A font that the shell fetches before the first round does not
    /// exist: the platform returned `NotFound` for it.
    MissingResource(ResourceId),
    /// The bytes that the platform returned for the fonts before the first
    /// round were refused by the font set of `kosmo-text`: one of them is
    /// not one TrueType font, or both are of one family. The shell does not
    /// check which fonts they are; whoever builds the platform's resource
    /// table keeps them those of the asset directory.
    Fonts(FontSetError),
    /// The glyph cache could not prepare the coverage masks of the glyph
    /// runs of a frame, so nothing was drawn: a font or a size outside the
    /// font set, a glyph without an outline, or masks of the frame beyond
    /// the cache.
    Glyphs(GlyphError),
    /// The rasterizer drew nothing and returned this error: a damage region
    /// that does not fit the surface's previous frame, a glyph without
    /// coverage, a command that it does not draw, offsets out of range or
    /// an input beyond its limits.
    Render(RenderError),
    /// The buffer that the platform handed out did not take the frame: its
    /// bytes are not as many as its description says.
    Convert(ConvertError),
    /// A previous frame of the size of the surface's buffer, 4 bytes per
    /// pixel, would exceed the buffer limits of `kosmo-types`.
    FrameTooLarge,
}

impl From<PlatformError> for ShellError {
    fn from(error: PlatformError) -> ShellError {
        ShellError::Platform(error)
    }
}

impl From<RenderError> for ShellError {
    fn from(error: RenderError) -> ShellError {
        ShellError::Render(error)
    }
}

impl From<ConvertError> for ShellError {
    fn from(error: ConvertError) -> ShellError {
        ShellError::Convert(error)
    }
}

impl fmt::Display for ShellError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ShellError::Platform(error) => write!(f, "the platform failed: {error}"),
            ShellError::MissingResource(id) => {
                write!(f, "the resource {} does not exist", id.name())
            }
            ShellError::Fonts(error) => write!(f, "the fonts were not taken: {error}"),
            ShellError::Glyphs(error) => {
                write!(f, "the glyphs of the frame were not prepared: {error}")
            }
            ShellError::Render(error) => write!(f, "the frame was not drawn: {error:?}"),
            ShellError::Convert(error) => {
                write!(f, "the frame was not written into the buffer: {error:?}")
            }
            ShellError::FrameTooLarge => f.write_str("the previous frame would be too large"),
        }
    }
}

impl Error for ShellError {}

impl<A: App> Shell<A> {
    /// The shell of `app` with its execution port `port`, with an empty
    /// control tree and no receipts, which fetches the fonts before its
    /// first round.
    pub fn new(app: A, port: A::Port) -> Shell<A> {
        Shell {
            runtime: Runtime::new(),
            app,
            port,
            fonts: None,
            receipts: Vec::new(),
            next_deadline: Some(MonoTime::ZERO),
            surfaces: BTreeMap::new(),
            rasterizer: VelloRasterizer::new(),
        }
    }

    /// The runtime, with the control tree.
    pub fn runtime(&self) -> &Runtime<A> {
        &self.runtime
    }

    /// The application.
    pub fn app(&self) -> &A {
        &self.app
    }

    /// The execution port.
    pub fn port(&self) -> &A::Port {
        &self.port
    }

    /// The execution port, for a driver that controls the jobs behind it.
    pub fn port_mut(&mut self) -> &mut A::Port {
        &mut self.port
    }

    /// Whether receipts of the last round's effects wait for the next round.
    pub fn has_receipts(&self) -> bool {
        !self.receipts.is_empty()
    }

    /// The deadline for the next round's wait: the round's time while
    /// receipts wait, so that the wait does not block; otherwise the next
    /// deadline of the last round's output. Before the first round it is
    /// `MonoTime::ZERO`, so the first round runs at once and builds the
    /// view.
    pub fn next_deadline(&self) -> Option<MonoTime> {
        self.next_deadline
    }

    /// The size of the surface's latest `Configure`, or `None` before one.
    pub fn size(&self, surface: SurfaceId) -> Option<PixelSize> {
        self.surfaces.get(&surface).and_then(|known| known.size)
    }

    /// The single-round entry: runs one round through `platform`.
    ///
    /// Before its first round the shell fetches the two fonts through
    /// `ResourceProvider`, first Fira Sans and then Fira Mono, named by
    /// their files `SANS_FILE` and `MONO_FILE` in the asset manifest of
    /// `kosmo-text`, and builds its font set from their bytes with
    /// `FontSet::new`. A font that does not exist gives
    /// [`ShellError::MissingResource`], and bytes that the font set refuses
    /// give [`ShellError::Fonts`]; then no round runs, and the next
    /// call fetches the fonts again. Then the round:
    ///
    /// 1. waits once for platform events with `deadline`; a backend that
    ///    never blocks, such as the headless backend, returns at once;
    /// 2. reads the clock once, the round's time;
    /// 3. takes the job reports through the execution port;
    /// 4. runs the first step of the round, with the receipts of the last
    ///    round, the events and the reports: `Key`, `FocusIn` and
    ///    `FocusOut` become the round's [`Event`]s in their order, while
    ///    the shell handles `Configure`, which sets the surface's size,
    ///    drops a previous frame of another size and, when the size
    ///    changes or it drops the frame, makes the whole surface its
    ///    pending region, and
    ///    `Released`, which makes the surface's buffer available again;
    ///    kinds of events that later versions add are ignored;
    /// 5. hands the effects to the execution port, which keeps the
    ///    receipts for the next round;
    /// 6. runs the second step with the layout and drawing passes, to
    ///    which it hands its own font set, the table
    ///    [`TOKENS`](crate::TOKENS), the size of each surface whose
    ///    `Configure` it took, so that each such surface is laid out under
    ///    its size and drawn, and the pending region of each such surface
    ///    whose buffer is not presented; the round builds no font set of
    ///    its own, and [`Runtime::layout`] gives the layouts;
    /// 7. pushes the output's accessibility updates through
    ///    `AccessibilitySink`, in their order;
    /// 8. presents each frame of the output with [`Shell::present`], in
    ///    their order, except a frame for a surface whose buffer is
    ///    presented and not released: its damage region joins the pending
    ///    region of the surface, at most 16 rectangles and beyond them
    ///    their bounding rectangle, and the first round after the release
    ///    draws that region together with its own update region and
    ///    presents it. A presented frame ends the pending region. A frame
    ///    that fails to present, and the frames after it, join the pending
    ///    regions of their surfaces, so that the next round draws them
    ///    again.
    ///
    /// Rounds have no accessibility pass yet, so their outputs hold no
    /// accessibility updates.
    ///
    /// With the feature `call-probes`, which only the command line turns
    /// on, each call records the call probe `kosmo-ui::run_round` of
    /// `kosmo-testkit`; without it no probe code is compiled.
    ///
    /// # Errors
    ///
    /// A port's error, [`PlatformError::Disconnected`] among them, and the
    /// errors of presentation, as [`ShellError`] states; an error of
    /// presentation comes after both steps, and the frames that it leaves
    /// unpresented wait in the pending regions of their surfaces. A frame
    /// for a surface whose buffer is presented and not released gives no
    /// error.
    pub fn run_round<P: Platform>(
        &mut self,
        platform: &mut P,
        deadline: Option<MonoTime>,
    ) -> Result<RoundEnd, ShellError> {
        #[cfg(feature = "call-probes")]
        kosmo_testkit::probe!("kosmo-ui::run_round");
        self.start(platform)?;
        let mut events = Vec::new();
        let wait = platform.wait(deadline, &mut events)?;
        let time = platform.now();
        let mut input = Input::new(time);
        input.unfinished_jobs = self.port.take_reports(&mut input.reports);
        input.receipts = std::mem::take(&mut self.receipts);
        for event in events {
            match event {
                PlatformEvent::Key {
                    surface,
                    stamp,
                    event,
                } => input.events.push(Event::Key {
                    surface,
                    stamp,
                    event,
                }),
                PlatformEvent::FocusIn { surface, stamp } => {
                    input.events.push(Event::FocusIn { surface, stamp });
                }
                PlatformEvent::FocusOut { surface, stamp } => {
                    input.events.push(Event::FocusOut { surface, stamp });
                }
                PlatformEvent::Configure { surface, size } => {
                    let known = self.surfaces.entry(surface).or_default();
                    let resized = known.size.is_some_and(|old| old != size);
                    known.size = Some(size);
                    let other = |frame: &RenderTarget| {
                        (frame.desc().width(), frame.desc().height())
                            != (size.width(), size.height())
                    };
                    let dropped = known.frame.as_ref().is_some_and(other);
                    if dropped {
                        known.frame = None;
                    }
                    // A region of another size is clipped wrongly, and a
                    // dropped one is drawn by no round: the next frame
                    // after a new size is whole.
                    if resized || dropped {
                        known.pending = whole(size);
                    }
                }
                PlatformEvent::Released { surface } => {
                    self.surfaces.entry(surface).or_default().presented = false;
                }
                _ => {}
            }
        }
        let mut round = self.runtime.round(&mut self.app, input);
        self.receipts = execute(&mut self.port, round.take_effects());
        // The fonts exist once `start` has returned.
        let output = match self.fonts.as_mut() {
            Some(fonts) => {
                let mut layout = LayoutInput::new(fonts, FONTS_ID, TOKENS);
                for (&surface, known) in &self.surfaces {
                    if let Some(size) = known.size {
                        layout = layout.surface(surface, size);
                        if let (Some(pending), false) = (known.pending, known.presented) {
                            layout.pending.insert(surface, pending);
                        }
                    }
                }
                round.finish_with(&self.app, layout)
            }
            None => round.finish(&self.app),
        };
        self.next_deadline = if self.receipts.is_empty() {
            output.next_deadline
        } else {
            Some(time)
        };
        self.deliver(platform, &output)?;
        Ok(RoundEnd { wait, output })
    }

    /// Fetches the fonts and builds the font set once, before the first
    /// round.
    fn start<P: Platform>(&mut self, platform: &P) -> Result<(), ShellError> {
        if self.fonts.is_some() {
            return Ok(());
        }
        let fetch = |id| {
            platform.resource(id).map_err(|error| match error {
                PlatformError::NotFound => ShellError::MissingResource(id),
                error => ShellError::Platform(error),
            })
        };
        let (sans, mono) = (fetch(SANS_RESOURCE)?, fetch(MONO_RESOURCE)?);
        self.fonts = Some(FontSet::new(sans, mono).map_err(ShellError::Fonts)?);
        Ok(())
    }

    /// Pushes the accessibility updates of `output` and presents its
    /// frames. A frame for a surface whose buffer is presented and not
    /// released is not presented: its damage joins the surface's pending
    /// region, which the next round after the release draws again. When a
    /// frame fails to present, its damage and that of the frames after it
    /// join the pending regions of their surfaces before the error
    /// returns, so that the next round draws them again.
    fn deliver<P: Platform>(
        &mut self,
        platform: &mut P,
        output: &Output,
    ) -> Result<(), ShellError> {
        for (surface, update) in &output.accessibility {
            platform.update(*surface, update.clone())?;
        }
        for (index, (surface, list, damage)) in output.frames.iter().enumerate() {
            if self
                .surfaces
                .get(surface)
                .is_some_and(|known| known.presented)
            {
                self.defer(*surface, damage);
                continue;
            }
            if let Err(error) = self.present(platform, *surface, list, damage) {
                for (surface, _, damage) in output.frames.get(index..).unwrap_or(&[]) {
                    self.defer(*surface, damage);
                }
                return Err(error);
            }
            // The frame drew the pending region too.
            if let Some(known) = self.surfaces.get_mut(surface) {
                known.pending = None;
            }
        }
        Ok(())
    }

    /// Joins `damage` to the pending region of `surface`, at most 16
    /// rectangles and beyond them their bounding rectangle.
    fn defer(&mut self, surface: SurfaceId, damage: &Damage) {
        let known = self.surfaces.entry(surface).or_default();
        match &mut known.pending {
            Some(pending) => {
                for rect in damage.rects() {
                    pending.add(rect.x(), rect.y(), rect.width(), rect.height());
                }
            }
            None => known.pending = Some(*damage),
        }
    }

    /// Presents `list` on `surface` inside `damage`, and returns the work
    /// of drawing it.
    ///
    /// The shell keeps a previous frame for each surface, a tightly packed
    /// `Rgba8Premul` buffer of the size of the surface's buffer. It draws
    /// `list` into that frame inside `damage` with `kosmo-render`'s
    /// rasterizer, converts the rectangles of `damage` into the buffer that
    /// `Presenter::buffer` hands out with
    /// [`RenderTarget::convert_into`], and presents them. The first frame
    /// of a surface, and the first after a `Configure` with a new size or a
    /// buffer of another size, starts as transparent black and is drawn,
    /// converted and presented whole, whatever `damage` holds, so that the
    /// buffer is the conversion of the frame, with X of `Xrgb8888` always
    /// `0xFF`. Until a wait returns `Released` for the surface, it is not
    /// presented again.
    ///
    /// Before it asks for the buffer, the shell prepares the coverage masks
    /// of the glyph runs of `list` in the glyph cache of its font set, as
    /// one frame of the cache, and the rasterizer draws the glyphs from the
    /// cache. Before a round has fetched the fonts, the shell has no font
    /// set, and glyph runs fail with a missing glyph. The frames of rounds
    /// lay their text out with the shell's font set; a frame that a driver
    /// builds from text laid out with a font set of its own matches the
    /// glyph cache only when that font set holds the same two fonts in the
    /// same places. An empty `damage` presents nothing, prepares nothing
    /// and returns no work.
    ///
    /// # Errors
    ///
    /// [`PlatformError::Busy`] for a buffer presented and not released, and
    /// [`ShellError::Glyphs`] for glyph runs that the glyph cache cannot
    /// prepare, both before anything changes; the errors of `buffer` and
    /// `present`; and [`ShellError::Render`], [`ShellError::Convert`] and
    /// [`ShellError::FrameTooLarge`]. After an error of `buffer`, of the
    /// frame, of the rasterizer or of the conversion of a new frame, the
    /// frame and the buffer are as they were; after a later error, the
    /// frame holds the drawing.
    pub fn present<P: Presenter>(
        &mut self,
        platform: &mut P,
        surface: SurfaceId,
        list: &DisplayList,
        damage: &Damage,
    ) -> Result<WorkCount, ShellError> {
        if damage.is_empty() {
            return Ok(WorkCount::default());
        }
        if self
            .surfaces
            .get(&surface)
            .is_some_and(|known| known.presented)
        {
            return Err(PlatformError::Busy.into());
        }
        if let Some(fonts) = &mut self.fonts {
            fonts
                .prepare(list, &mut self.rasterizer)
                .map_err(ShellError::Glyphs)?;
        }
        let buffer = platform.buffer(surface)?;
        let desc = buffer.desc;
        let known = self.surfaces.entry(surface).or_default();
        let mut fresh = None;
        // A new frame is drawn, converted and presented whole, so that the
        // buffer holds the conversion of the frame everywhere.
        let (frame, whole) = match known.frame.as_mut() {
            Some(frame)
                if frame.desc().width() == desc.width()
                    && frame.desc().height() == desc.height() =>
            {
                (frame, None)
            }
            _ => {
                let frame = fresh.insert(new_frame(desc)?);
                let whole = frame.whole();
                (frame, Some(whole))
            }
        };
        let damage = whole.as_ref().unwrap_or(damage);
        let glyphs: &dyn GlyphCoverage = match &self.fonts {
            Some(fonts) => fonts.glyphs(),
            None => &NoGlyphs,
        };
        let work = self
            .rasterizer
            .render(list, damage, glyphs, &ImageStore::EMPTY, frame)?;
        frame.convert_into(damage, desc, buffer.bytes)?;
        if let Some(fresh) = fresh {
            known.frame = Some(fresh);
        }
        platform.present(surface, damage.rects())?;
        known.presented = true;
        Ok(work)
    }
}

/// The whole of a surface of `size` as a damage region of a frame of 4
/// bytes per pixel; `None` beyond the buffer limits.
fn whole(size: PixelSize) -> Option<Damage> {
    let stride = size.width().checked_mul(4)?;
    let desc = BufferDesc::new(
        size.width(),
        size.height(),
        stride,
        PixelFormat::Rgba8Premul,
    )
    .ok()?;
    let mut damage = Damage::new(desc);
    damage.add(0, 0, size.width(), size.height());
    Some(damage)
}

/// A new previous frame for a buffer of `desc`: transparent black, tightly
/// packed `Rgba8Premul` of the same width and height.
fn new_frame(desc: BufferDesc) -> Result<RenderTarget, ShellError> {
    let tight = BufferDesc::new(
        desc.width(),
        desc.height(),
        desc.width().saturating_mul(4),
        PixelFormat::Rgba8Premul,
    )
    .map_err(|_| ShellError::FrameTooLarge)?;
    RenderTarget::new(tight).ok_or(ShellError::FrameTooLarge)
}

/// Why [`run`] returned.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExitReason {
    /// A port returned [`PlatformError::Disconnected`]: the platform is
    /// gone. Applications of the first release do not request exit, so
    /// this is how they end.
    Disconnected,
    /// A font did not exist before the first round.
    MissingResource(ResourceId),
    /// The shell stopped with this error, of another kind.
    Failed(ShellError),
}

/// The exit status of [`run`]: why it returned, and what the platform's
/// `shutdown` returned. Exit codes exist only at the entry of a program,
/// which maps the status to one.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExitStatus {
    /// Why the shell stopped.
    pub reason: ExitReason,
    /// What `shutdown` returned.
    pub shutdown: Result<(), PlatformError>,
}

/// Runs `app` with its execution port `port` on `platform` until the shell
/// stops, then calls `shutdown` once and returns the exit status.
///
/// Before the first round it fetches the two fonts through
/// `ResourceProvider`, as [`Shell::run_round`] states; when one does not
/// exist, it shuts the platform down at once and returns
/// [`ExitReason::MissingResource`], and when the font set refuses their
/// bytes, [`ExitReason::Failed`] with [`ShellError::Fonts`]. Then it runs
/// rounds with [`Shell::run_round`], each waiting with the
/// [`Shell::next_deadline`] that the round before left. When a port returns
/// [`PlatformError::Disconnected`] it returns
/// [`ExitReason::Disconnected`]; any other error ends it with
/// [`ExitReason::Failed`]. `run` runs on the thread that calls it and
/// creates no threads. With a backend that never blocks, such as the
/// headless backend, a wait without events returns at once, so `run`
/// keeps running rounds until the platform disconnects; a driver of such
/// a backend runs rounds with [`Shell::run_round`] instead. `run` destroys
/// no surface, so the surfaces of the application that it drives are not
/// destroyed before `shutdown`.
pub fn run<P: Platform, A: App>(platform: &mut P, app: A, port: A::Port) -> ExitStatus {
    let mut shell = Shell::new(app, port);
    let reason = loop {
        let deadline = shell.next_deadline();
        match shell.run_round(platform, deadline) {
            Ok(_) => {}
            Err(ShellError::Platform(PlatformError::Disconnected)) => {
                break ExitReason::Disconnected;
            }
            Err(ShellError::MissingResource(id)) => break ExitReason::MissingResource(id),
            Err(error) => break ExitReason::Failed(error),
        }
    };
    ExitStatus {
        reason,
        shutdown: platform.shutdown(),
    }
}

#[cfg(test)]
mod tests;
