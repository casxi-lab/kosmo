//! The script runner: [`Script::run`] replays a script on a platform with
//! its test hooks, advances the framework round by round through a
//! [`Driver`], and writes a [`Record`] at each checkpoint.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use kosmo_platform::{
    PlatformError, PlatformEvent, PlatformTestHooks, Stamp, SurfaceId, SurfaceRequest, SurfaceRole,
    WaitEnd,
};
use kosmo_types::MonoTime;

use super::{Header, Line, Problem, Script, ScriptError};
use crate::pixels::{PixelDigest, format_name};

/// The most rounds that the runner runs for one line of a script: 65,536.
/// A line that needs more fails with [`Problem::NotIdle`].
pub const MAX_ROUNDS_PER_LINE: usize = 65_536;

/// What the runner drives: the framework with its shell, an application
/// and whatever the test arranges around them, such as the release plan
/// of a controllable job backend. The test implements it; the runner
/// reaches the framework only through it, and the platform only through
/// the platform interface with its test hooks.
pub trait Driver<P: PlatformTestHooks> {
    /// The error of a round.
    type Error: fmt::Display;

    /// Runs one round of the framework through `platform`, through the
    /// shell's single-round entry with the shell's deadline, and returns
    /// how the round's wait ended. A driver that arranges more between
    /// rounds, such as releasing a job when a key has been handled, does it
    /// here, after the round.
    ///
    /// # Errors
    ///
    /// The error of the round, which stops the run.
    fn round(&mut self, platform: &mut P) -> Result<WaitEnd, Self::Error>;

    /// Whether something waits for the next round without a platform
    /// event: receipts of the last round's effects, or job reports that the
    /// test released and the framework has not taken.
    fn pending(&self) -> bool;

    /// The deadline that the next round waits with, `None` for none.
    fn next_deadline(&self) -> Option<MonoTime>;

    /// The focus stamp of the latest focus in of `surface` that the
    /// framework took, `None` before one.
    fn focus_stamp(&self, surface: SurfaceId) -> Option<Stamp>;

    /// Appends the lines of the view model record to `out`: what the
    /// application shows, as it defines it, one entry per line, without
    /// line feeds, tabs or other control characters.
    fn view(&self, out: &mut Vec<String>);

    /// Appends the text of the display list that was last presented on
    /// `surface` to `out`: one command per line, each followed by a line
    /// feed, as `kosmo-render` writes display lists, or nothing before the
    /// surface presented one.
    fn display(&self, surface: SurfaceId, out: &mut String);
}

/// The records of a run: one section for each checkpoint, in order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Record {
    checkpoints: Vec<String>,
}

impl Record {
    /// The section of each checkpoint, each line followed by a line feed.
    pub fn checkpoints(&self) -> &[String] {
        &self.checkpoints
    }

    /// The sections of all checkpoints, one after the other.
    pub fn text(&self) -> String {
        self.checkpoints.concat()
    }
}

impl Script {
    /// Replays the script on `platform`, driving the framework with
    /// `driver`, and returns the record of each checkpoint.
    ///
    /// - The header creates the main surface, of the role `Main`, with its
    ///   size and format, which must fit the output size of the platform's
    ///   capabilities and be one of their formats. The framework then runs
    ///   until it is idle.
    /// - Before the clock moves to a later time, the framework runs until
    ///   it is idle; then the clock stops at each deadline of the driver
    ///   that comes before the time, and the framework runs until it is
    ///   idle there. A key line and a time line move the clock this way to
    ///   their time; lines of one time do not run the framework between
    ///   them.
    /// - A key line injects its key event for the main surface with the
    ///   stamp that it states; without one, with the stamp of the latest
    ///   focus line that entered the main surface, or before one, the stamp
    ///   of the surface's focus that the driver reports.
    /// - A focus line injects `FocusIn` or `FocusOut` for its surface, one
    ///   that the script created.
    /// - A checkpoint line runs the framework until it is idle and writes a
    ///   section of the record.
    ///
    /// The framework is idle when a round's wait returned `Idle`, the driver
    /// has nothing pending and its next deadline is later than the clock.
    /// The runner advances the clock only through the test hooks and reads
    /// no other clock.
    ///
    /// A section is the line `checkpoint <n> at <time>`, the line
    /// `view <count>` followed by the driver's view lines, for each surface
    /// in the order of their identities the line `display <surface> <count>`
    /// followed by the lines of the display list, and for each surface the
    /// line `pixels <surface> <digest>`, the read-back as
    /// [`PixelDigest`](crate::pixels::PixelDigest) writes it, or
    /// `pixels <surface> none` before the surface presented a buffer.
    ///
    /// With the feature `call-probes`, which only the command line turns
    /// on, each call records the call probe `kosmo-testkit::run`.
    ///
    /// # Errors
    ///
    /// The first problem, with the number of the line that the runner ran:
    /// a header outside the capabilities, a platform error, a driver
    /// error, a focus line for another surface, a key line without a stamp
    /// to take, a line that does not let the framework become idle within
    /// [`MAX_ROUNDS_PER_LINE`] rounds, or view lines or display text that
    /// break their form.
    pub fn run<P: PlatformTestHooks, D: Driver<P>>(
        &self,
        platform: &mut P,
        driver: &mut D,
    ) -> Result<Record, ScriptError> {
        #[cfg(feature = "call-probes")]
        crate::probe!("kosmo-testkit::run");
        let mut runner = Runner {
            platform,
            driver,
            rounds: 0,
            main: SurfaceId::new(0),
            surfaces: BTreeSet::new(),
            stamps: BTreeMap::new(),
            record: Record::default(),
        };
        let at = |line| move |problem| ScriptError { line, problem };
        runner.start(self.header).map_err(at(1))?;
        for &(line, kind) in &self.lines {
            runner.rounds = 0;
            runner.run(kind).map_err(at(line))?;
        }
        Ok(runner.record)
    }
}

/// The state of a run.
struct Runner<'a, P, D> {
    platform: &'a mut P,
    driver: &'a mut D,
    /// The rounds that the line that runs ran.
    rounds: usize,
    main: SurfaceId,
    /// The surfaces that the script created.
    surfaces: BTreeSet<SurfaceId>,
    /// The stamp of the latest focus line that entered each surface.
    stamps: BTreeMap<SurfaceId, Stamp>,
    record: Record,
}

impl<P: PlatformTestHooks, D: Driver<P>> Runner<'_, P, D> {
    fn start(&mut self, header: Header) -> Result<(), Problem> {
        let caps = self.platform.caps();
        let fits = header.size.width() <= caps.output().width()
            && header.size.height() <= caps.output().height();
        if !fits || !caps.accepts(header.format) {
            return Err(Problem::OutsideCaps(describe(header)));
        }
        let request = SurfaceRequest {
            role: SurfaceRole::Main,
            size: header.size,
            format: header.format,
        };
        self.main = self.platform.create_surface(request).map_err(platform)?;
        self.surfaces.insert(self.main);
        self.settle()
    }

    fn run(&mut self, line: Line) -> Result<(), Problem> {
        match line {
            Line::Key { event, stamp } => {
                self.advance(event.time)?;
                let stamp = stamp
                    .or_else(|| self.stamps.get(&self.main).copied())
                    .or_else(|| self.driver.focus_stamp(self.main))
                    .ok_or(Problem::NoStamp)?;
                let key = PlatformEvent::Key {
                    surface: self.main,
                    stamp,
                    event,
                };
                self.platform.inject(key).map_err(platform)
            }
            Line::Time(time) => self.advance(time),
            Line::Focus {
                enter,
                surface,
                stamp,
            } => {
                if !self.surfaces.contains(&surface) {
                    return Err(Problem::NoSurface(surface.get()));
                }
                let event = if enter {
                    self.stamps.insert(surface, stamp);
                    PlatformEvent::FocusIn { surface, stamp }
                } else {
                    PlatformEvent::FocusOut { surface, stamp }
                };
                self.platform.inject(event).map_err(platform)
            }
            Line::Checkpoint => {
                self.settle()?;
                self.checkpoint()
            }
        }
    }

    /// Moves the clock to `time`, as [`Script::run`] states.
    fn advance(&mut self, time: MonoTime) -> Result<(), Problem> {
        let clock = self.platform.now();
        if time < clock {
            return Err(Problem::Earlier { time, clock });
        }
        if time == clock {
            return Ok(());
        }
        self.settle()?;
        while let Some(deadline) = self.driver.next_deadline() {
            if deadline <= self.platform.now() || deadline >= time {
                break;
            }
            self.platform.advance_to(deadline).map_err(platform)?;
            self.settle()?;
        }
        self.platform.advance_to(time).map_err(platform)
    }

    /// Runs rounds until the framework is idle.
    fn settle(&mut self) -> Result<(), Problem> {
        loop {
            if self.rounds >= MAX_ROUNDS_PER_LINE {
                return Err(Problem::NotIdle);
            }
            self.rounds += 1;
            let wait = self
                .driver
                .round(self.platform)
                .map_err(|error| Problem::Driver(error.to_string()))?;
            let clock = self.platform.now();
            let due = self
                .driver
                .next_deadline()
                .is_some_and(|deadline| deadline <= clock);
            if wait == WaitEnd::Idle && !self.driver.pending() && !due {
                return Ok(());
            }
        }
    }

    /// Writes the section of the next checkpoint.
    fn checkpoint(&mut self) -> Result<(), Problem> {
        let number = self.record.checkpoints.len() + 1;
        let mut text = format!(
            "checkpoint {number} at {}\n",
            self.platform.now().as_nanos()
        );
        let mut view = Vec::new();
        self.driver.view(&mut view);
        text.push_str(&format!("view {}\n", view.len()));
        for line in view {
            if line.chars().any(char::is_control) {
                return Err(Problem::View(line));
            }
            text.push_str(&line);
            text.push('\n');
        }
        for &surface in &self.surfaces {
            let mut list = String::new();
            self.driver.display(surface, &mut list);
            let lines = list.matches('\n').count();
            let control = list.chars().any(|c| c.is_control() && c != '\n');
            if control || !(list.is_empty() || list.ends_with('\n')) {
                return Err(Problem::Display(surface.get()));
            }
            text.push_str(&format!("display {} {lines}\n", surface.get()));
            text.push_str(&list);
        }
        for &surface in &self.surfaces {
            let pixels = match self.platform.read_back(surface).map_err(platform)? {
                None => "none".to_string(),
                Some(read) => PixelDigest::of(read.desc, read.bytes)
                    .ok_or(Problem::Platform(PlatformError::Unsupported))?
                    .to_string(),
            };
            text.push_str(&format!("pixels {} {pixels}\n", surface.get()));
        }
        self.record.checkpoints.push(text);
        Ok(())
    }
}

fn platform(error: PlatformError) -> Problem {
    Problem::Platform(error)
}

/// The header as the problem of a header outside the capabilities names
/// it.
fn describe(header: Header) -> String {
    format!(
        "{}x{} {}",
        header.size.width(),
        header.size.height(),
        format_name(header.format).unwrap_or("unknown")
    )
}
