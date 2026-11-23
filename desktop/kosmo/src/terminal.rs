//! The loop of the line-mode terminal.

use std::io::{self, BufRead, Write};

use kosmo_sdk::kosmo_jobs::stub::{HostStub, WorkerNotStarted};
use kosmo_sdk::kosmo_jobs::{JobBackend, JobClock, JobLayer, Limits, Operation, SessionId};
use kosmo_sdk::kosmo_math_kalyx::{KalyxPreparer, KalyxRunner};
use kosmo_sdk::kosmo_math_model::display::Freshness;
use kosmo_sdk::kosmo_math_model::version::Judgement;
use kosmo_sdk::kosmo_scratchpad::Scratchpad;

use crate::command::{Command, parse};
use crate::enter::enter;
use crate::line::{MAX_LINE_BYTES, Read, read_line};
use crate::output::{self, write_line};

/// The job layer of the terminal: the host stub with the kalyx runner.
pub type Layer = JobLayer<HostStub<KalyxPreparer>>;

/// The job layer of the terminal on `clock`: the host stub, whose worker
/// thread runs from now on, with the kalyx runner, which reads a fork of the
/// clock. The job layer reads the clock when it accepts a submission with a
/// time limit.
///
/// # Errors
///
/// [`WorkerNotStarted`] when the worker thread cannot be created.
pub fn job_layer(clock: Box<dyn JobClock>) -> Result<Layer, WorkerNotStarted> {
    let stub = HostStub::new(KalyxPreparer::new(), KalyxRunner::new(clock.fork()))?;
    Ok(JobLayer::new(stub, clock))
}

/// How the terminal ended.
#[must_use]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Ended {
    /// The input ended, or `:quit` was read.
    Done,
    /// Reading the input, or writing or flushing the output, failed.
    Failed,
}

impl Ended {
    /// The exit code of the program: 0 for [`Ended::Done`] and 1 for
    /// [`Ended::Failed`].
    pub const fn exit_code(self) -> u8 {
        match self {
            Ended::Done => 0,
            Ended::Failed => 1,
        }
    }
}

/// Runs the line-mode terminal: reads the lines of `input` and writes the
/// lines for each to `output`, with the current line and the history of
/// `pad` and the jobs of `layer` in `session`.
///
/// It reads one whole line at a time, of at most [`MAX_LINE_BYTES`] bytes,
/// and answers it before it reads the next. The line syntax gives what a
/// line asks for: an expression is entered into the current line one
/// character at a time through the insert operation, as [`enter`] does,
/// prepared with the default limits and `time_limit`, in nanoseconds when
/// it is given, submitted on `layer`, and waited for until its job reaches
/// its terminal state, which is delivered to `pad` and written. So at most
/// one job is live at a time. After the line for an expression is written,
/// the current line is cleared when it is not empty, which is an edit of its
/// own. Every line written ends with a line feed, holds only the bytes 0x20
/// to 0x7E, and is flushed before the next line is read.
///
/// It ends at the end of the input or at `:quit`, and when reading or
/// writing fails; then it shuts `layer` down, so the host stub's worker
/// thread has ended when it returns. An error of a line is written and
/// changes neither how the terminal goes on nor how it ends.
pub fn run<B: JobBackend>(
    input: &mut dyn BufRead,
    output: &mut dyn Write,
    pad: &mut Scratchpad,
    layer: &mut JobLayer<B>,
    session: SessionId,
    time_limit: Option<u64>,
) -> Ended {
    let mut terminal = Terminal {
        output,
        pad,
        layer,
        session,
        time_limit,
    };
    let ended = match terminal.lines(input) {
        Ok(()) => Ended::Done,
        Err(_) => Ended::Failed,
    };
    terminal.layer.shutdown();
    ended
}

/// Whether the terminal reads another line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Next {
    Read,
    Quit,
}

/// What the terminal works with while it runs.
struct Terminal<'a, B: JobBackend> {
    output: &'a mut dyn Write,
    pad: &'a mut Scratchpad,
    layer: &'a mut JobLayer<B>,
    session: SessionId,
    time_limit: Option<u64>,
}

impl<B: JobBackend> Terminal<'_, B> {
    /// Answers each line of `input` until the input ends or `:quit`.
    fn lines(&mut self, input: &mut dyn BufRead) -> io::Result<()> {
        let mut line = Vec::with_capacity(MAX_LINE_BYTES);
        loop {
            let next = match read_line(input, &mut line)? {
                Read::End => Next::Quit,
                Read::TooLong => {
                    write_line(self.output, "input error: LineTooLong")?;
                    Next::Read
                }
                Read::Line => self.answer(&line)?,
            };
            self.output.flush()?;
            if next == Next::Quit {
                return Ok(());
            }
        }
    }

    /// Answers one line.
    fn answer(&mut self, line: &[u8]) -> io::Result<Next> {
        match parse(line) {
            Command::Submit {
                operation,
                variable,
                expression,
            } => self.submit(operation, variable, expression)?,
            Command::History => {
                for entry in self.pad.history() {
                    write_line(self.output, &output::history_entry(entry))?;
                }
            }
            Command::Quit => return Ok(Next::Quit),
            Command::Unknown => write_line(self.output, "input error: UnknownCommand")?,
            Command::Malformed => write_line(self.output, "input error: MalformedCommand")?,
        }
        Ok(Next::Read)
    }

    /// Enters `expression`, submits it and writes its line, then clears the
    /// current line.
    fn submit(
        &mut self,
        operation: Operation,
        variable: &str,
        expression: &[u8],
    ) -> io::Result<()> {
        let written = match enter(self.pad, expression) {
            Ok(()) => self.compute(operation, variable),
            Err(not) => output::not_entered(not),
        };
        write_line(self.output, &written)?;
        if self.pad.line().items().is_empty() {
            return Ok(());
        }
        match self.pad.clear() {
            Ok(()) => Ok(()),
            Err(rejected) => write_line(
                self.output,
                &format!("internal error: the current line cannot be cleared: {rejected}"),
            ),
        }
    }

    /// Prepares the current line, runs its job to its terminal state and
    /// gives the line to write for it.
    fn compute(&mut self, operation: Operation, variable: &str) -> String {
        let prepared = self.pad.prepare(
            self.session,
            operation,
            variable,
            Limits::new(),
            self.time_limit,
        );
        let (submission, pending) = match prepared {
            Ok(prepared) => prepared,
            Err(not) => return output::not_prepared(&not),
        };
        let answer = self.layer.submit(submission);
        let id = match self.pad.accept(pending, answer) {
            Ok(id) => id,
            Err(not) => return format!("internal error: {not}"),
        };
        let freshness = match self.layer.wait(id) {
            Some(terminal) => match self.pad.deliver(&terminal).judgement {
                Judgement::Current => Freshness::Current,
                Judgement::Superseded | Judgement::Stale => Freshness::Stale,
            },
            // Without a terminal state the entry has no outcome, so its line
            // is the internal error that the history writes for it too.
            None => Freshness::Current,
        };
        match self.pad.entry(id) {
            Some(entry) => output::outcome(entry, freshness),
            None => format!("internal error: {id} has no entry in the history"),
        }
    }
}
