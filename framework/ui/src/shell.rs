//! The shell: the single-round entry, which drives the round function
//! through the platform interface, the presentation of frames, and `run`.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;

use kosmo_platform::{
    PixelSize, Platform, PlatformError, PlatformEvent, Presenter, ResourceId, SurfaceId, WaitEnd,
};
use kosmo_render::{
    DisplayList, ImageStore, NoGlyphs, Rasterizer, RenderError, RenderTarget, VelloRasterizer,
    WorkCount,
};
use kosmo_types::{BufferDesc, ConvertError, Damage, MonoTime, PixelFormat};

use crate::app::App;
use crate::focus::Event;
use crate::port::{JobPort, Receipt, execute};
use crate::round::{Input, Output, Runtime};

/// The shell of an application: its [`Runtime`], the application and its
/// execution port, the receipts that wait for the next round, and the
/// previous frame of each surface, which it keeps for presentation.
///
/// [`Shell::run_round`], the single-round entry, runs one round through a
/// platform, and [`run`] runs rounds until the platform disconnects. The
/// shell holds no platform: each call borrows it, so a driver keeps its
/// backend between rounds. The shell creates no surface; the application's
/// assembly or the driver creates them through `SurfacePort`.
pub struct Shell<A: App> {
    runtime: Runtime<A>,
    app: A,
    port: A::Port,
    resources: Vec<ResourceId>,
    started: bool,
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
/// An error of [`Shell::run_round`] comes from fetching the resources
/// before the first round, from the wait, from a port of the later steps or
/// from presentation. A round whose resources or wait failed runs no later
/// step; one that failed later has run both steps of the round.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShellError {
    /// A port of the platform returned this error.
    Platform(PlatformError),
    /// A resource that the shell was built with does not exist: the
    /// platform returned `NotFound` for it before the first round.
    MissingResource(ResourceId),
    /// The rasterizer drew nothing: the display list or the damage region
    /// does not fit the surface's previous frame.
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
    /// control tree and no receipts, which fetches `resources` before its
    /// first round.
    pub fn new(app: A, port: A::Port, resources: &[ResourceId]) -> Shell<A> {
        Shell {
            runtime: Runtime::new(),
            app,
            port,
            resources: resources.to_vec(),
            started: false,
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
    /// Before its first round the shell fetches each resource that it was
    /// built with through `ResourceProvider`; a resource that does not
    /// exist gives [`ShellError::MissingResource`], and no round runs. The
    /// bytes go nowhere yet: the font set that will take them is planned
    /// in `kosmo-text`. Then the round:
    ///
    /// 1. waits once for platform events with `deadline`; a backend that
    ///    never blocks, such as the headless backend, returns at once;
    /// 2. reads the clock once, the round's time;
    /// 3. takes the job reports through the execution port;
    /// 4. runs the first step of the round, with the receipts of the last
    ///    round, the events and the reports: `Key`, `FocusIn` and
    ///    `FocusOut` become the round's [`Event`]s in their order, while
    ///    the shell handles `Configure`, which sets the surface's size and
    ///    drops a previous frame of another size, and `Released`, which
    ///    makes the surface's buffer available again; kinds of events that
    ///    later versions add are ignored;
    /// 5. hands the effects to the execution port, which keeps the
    ///    receipts for the next round;
    /// 6. runs the second step;
    /// 7. pushes the output's accessibility updates through
    ///    `AccessibilitySink`, in their order;
    /// 8. presents each frame of the output with [`Shell::present`].
    ///
    /// Rounds have no accessibility pass and do not run the drawing pass
    /// yet, so their outputs hold no accessibility updates and no frames;
    /// a driver presents the frames that it draws itself with
    /// [`Shell::present`].
    ///
    /// With the feature `call-probes`, which only the command line turns
    /// on, each call records the call probe `kosmo-ui::run_round` of
    /// `kosmo-testkit`; without it no probe code is compiled.
    ///
    /// # Errors
    ///
    /// A port's error, [`PlatformError::Disconnected`] among them, and the
    /// errors of presentation, as [`ShellError`] states.
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
                    known.size = Some(size);
                    let other = |frame: &RenderTarget| {
                        (frame.desc().width(), frame.desc().height())
                            != (size.width(), size.height())
                    };
                    if known.frame.as_ref().is_some_and(other) {
                        known.frame = None;
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
        let output = round.finish(&self.app);
        self.next_deadline = if self.receipts.is_empty() {
            output.next_deadline
        } else {
            Some(time)
        };
        self.deliver(platform, &output)?;
        Ok(RoundEnd { wait, output })
    }

    /// Fetches the resources once, before the first round.
    fn start<P: Platform>(&mut self, platform: &P) -> Result<(), ShellError> {
        if self.started {
            return Ok(());
        }
        for &id in &self.resources {
            match platform.resource(id) {
                Ok(_) => {}
                Err(PlatformError::NotFound) => return Err(ShellError::MissingResource(id)),
                Err(error) => return Err(error.into()),
            }
        }
        self.started = true;
        Ok(())
    }

    /// Pushes the accessibility updates of `output` and presents its frames.
    fn deliver<P: Platform>(
        &mut self,
        platform: &mut P,
        output: &Output,
    ) -> Result<(), ShellError> {
        for (surface, update) in &output.accessibility {
            platform.update(*surface, update.clone())?;
        }
        for (surface, list, damage) in &output.frames {
            self.present(platform, *surface, list, damage)?;
        }
        Ok(())
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
    /// buffer of another size, starts as transparent black. Until a wait
    /// returns `Released` for the surface, it is not presented again.
    ///
    /// An empty `damage` presents nothing and returns no work. Glyph runs
    /// fail with a missing glyph: the glyph cache of `kosmo-text` is
    /// planned.
    ///
    /// # Errors
    ///
    /// [`PlatformError::Busy`] for a buffer presented and not released,
    /// before anything changes; the errors of `buffer` and `present`; and
    /// [`ShellError::Render`], [`ShellError::Convert`] and
    /// [`ShellError::FrameTooLarge`]. After an error of `buffer`, of the
    /// frame or of the rasterizer, the frame and the buffer are as they
    /// were; after a later error, the frame holds the drawing.
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
        let buffer = platform.buffer(surface)?;
        let desc = buffer.desc;
        let known = self.surfaces.entry(surface).or_default();
        let mut fresh = None;
        let frame = match known.frame.as_mut() {
            Some(frame)
                if frame.desc().width() == desc.width()
                    && frame.desc().height() == desc.height() =>
            {
                frame
            }
            _ => fresh.insert(new_frame(desc)?),
        };
        let work = self
            .rasterizer
            .render(list, damage, &NoGlyphs, &ImageStore::EMPTY, frame)?;
        let converted = frame.convert_into(damage, desc, buffer.bytes);
        if let Some(fresh) = fresh {
            known.frame = Some(fresh);
        }
        converted?;
        platform.present(surface, damage.rects())?;
        known.presented = true;
        Ok(work)
    }
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
    /// The resource did not exist before the first round.
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
/// Before the first round it fetches `resources` through
/// `ResourceProvider`, as [`Shell::run_round`] states; when one does not
/// exist, it shuts the platform down at once and returns
/// [`ExitReason::MissingResource`]. Then it runs rounds with
/// [`Shell::run_round`], each waiting with the [`Shell::next_deadline`]
/// that the round before left. When a port returns
/// [`PlatformError::Disconnected`] it returns
/// [`ExitReason::Disconnected`]; any other error ends it with
/// [`ExitReason::Failed`]. `run` runs on the thread that calls it and
/// creates no threads. With a backend that never blocks, such as the
/// headless backend, a wait without events returns at once, so `run`
/// keeps running rounds until the platform disconnects; a driver of such
/// a backend runs rounds with [`Shell::run_round`] instead.
pub fn run<P: Platform, A: App>(
    platform: &mut P,
    app: A,
    port: A::Port,
    resources: &[ResourceId],
) -> ExitStatus {
    let mut shell = Shell::new(app, port, resources);
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
