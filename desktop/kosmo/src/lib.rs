//! The `kosmo` program and its line-mode terminal.
//!
//! Without arguments, `kosmo` is the line-mode terminal: it reads whole
//! lines from standard input, computes each one through the Scratchpad and
//! the job layer of the framework, and writes what it gives to standard
//! output. `kosmo --version` prints the program name and version; any other
//! arguments print the usage to standard error and exit with 2.
//!
//! The terminal is the library function [`run`], which reads a `BufRead`
//! and writes a `Write`, with a Scratchpad and a job layer that its caller
//! holds, so the caller can read the history and the job layer's worker
//! threads after it returns. [`program`] is the entry that `main` calls with
//! the standard streams and the arguments: it builds the real clock, the job
//! layer of [`job_layer`] and a Scratchpad, and gives the exit code.
//!
//! The program depends on the framework only through the facade
//! `kosmo-sdk`. It uses only the standard library: it starts no thread of
//! its own, uses no interface of the operating system beyond the standard
//! streams, the arguments, the exit code and the monotonic clock, and
//! neither reads nor changes terminal attributes.
//!
//! ```
//! use kosmo::{Ended, job_layer, run};
//! use kosmo_sdk::kosmo_jobs::JobClock;
//! use kosmo_sdk::kosmo_scratchpad::Scratchpad;
//!
//! struct Still;
//! impl JobClock for Still {
//!     fn now(&mut self) -> u64 { 0 }
//!     fn fork(&self) -> Box<dyn JobClock> { Box::new(Still) }
//! }
//!
//! let mut layer = job_layer(Box::new(Still))?;
//! let session = layer.open_session()?;
//! let mut pad = Scratchpad::new();
//! let mut output = Vec::new();
//! let input = b":diff x x^2\n:history\n";
//! let ended = run(&mut &input[..], &mut output, &mut pad, &mut layer, session, None);
//! assert_eq!(ended, Ended::Done);
//! assert_eq!(
//!     output,
//!     b"value (source 3, settings 0): 2*x\n\
//!       job 1: :diff x x^2 => value (source 3, settings 0): 2*x\n"
//! );
//! assert_eq!(pad.history().len(), 1);
//! assert_eq!(layer.backend().live_threads(), 0);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

#![forbid(unsafe_code)]

mod clock;
mod command;
mod enter;
mod line;
mod output;
mod terminal;

use std::ffi::OsString;
use std::io::{BufRead, Write};

use kosmo_sdk::kosmo_scratchpad::Scratchpad;

pub use enter::{NotEntered, enter};
pub use line::MAX_LINE_BYTES;
pub use terminal::{Ended, Layer, job_layer, run};

/// The usage that the program writes to standard error for arguments that
/// it does not take.
pub const USAGE: &str = "Usage: kosmo [--version]";

/// What the program does for its arguments.
#[derive(Debug, PartialEq, Eq)]
enum Reply {
    /// Run the line-mode terminal.
    Terminal,
    /// Print the program name and version.
    Version,
    /// Print the usage and exit with 2.
    Usage,
}

/// The reply to the arguments after the program name. They are compared
/// as the operating system passes them, so an argument that is not UTF-8
/// gets the usage.
fn reply(args: &[OsString]) -> Reply {
    match args {
        [] => Reply::Terminal,
        [only] if *only == "--version" => Reply::Version,
        _ => Reply::Usage,
    }
}

/// The program for the arguments after its name, as `main` runs it with
/// the standard streams, and its exit code.
///
/// Without arguments it runs the terminal on `input` and `output`, with the
/// monotonic clock of the host and no time limit, so each job is bounded
/// only by the default limits of its request. The exit code is then 0 when
/// the input ended or `:quit` was read and 1 when reading or writing
/// failed. With the single argument `--version` it writes the program name
/// and version to `output`, with the exit code 0, or 1 when that fails. Any
/// other arguments write [`USAGE`] to `errors`, with the exit code 2.
/// `errors` gets nothing else but the reason why the terminal could not
/// start, with the exit code 1.
pub fn program(
    args: &[OsString],
    input: &mut dyn BufRead,
    output: &mut dyn Write,
    errors: &mut dyn Write,
) -> u8 {
    match reply(args) {
        Reply::Terminal => terminal(input, output, errors),
        Reply::Version => {
            let line = format!("{} {}\n", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
            match output
                .write_all(line.as_bytes())
                .and_then(|()| output.flush())
            {
                Ok(()) => 0,
                Err(_) => 1,
            }
        }
        Reply::Usage => {
            // Exits with 2 whether or not the usage can be written.
            let _ = writeln!(errors, "{USAGE}");
            2
        }
    }
}

/// Starts the terminal with the host's clock and runs it.
fn terminal(input: &mut dyn BufRead, output: &mut dyn Write, errors: &mut dyn Write) -> u8 {
    let mut layer = match job_layer(Box::new(clock::HostClock::new())) {
        Ok(layer) => layer,
        Err(reason) => {
            let _ = writeln!(errors, "kosmo: {reason}");
            return 1;
        }
    };
    let session = match layer.open_session() {
        Ok(session) => session,
        Err(reason) => {
            layer.shutdown();
            let _ = writeln!(errors, "kosmo: the job layer opened no session: {reason}");
            return 1;
        }
    };
    let mut pad = Scratchpad::new();
    run(input, output, &mut pad, &mut layer, session, None).exit_code()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    #[test]
    fn no_arguments_run_the_terminal_and_only_the_single_version_flag_prints_the_version() {
        assert_eq!(reply(&args(&[])), Reply::Terminal);
        assert_eq!(reply(&args(&["--version"])), Reply::Version);
        assert_eq!(reply(&args(&["--version", "--version"])), Reply::Usage);
        assert_eq!(reply(&args(&["-V"])), Reply::Usage);
        assert_eq!(reply(&args(&[""])), Reply::Usage);
    }

    #[test]
    fn the_version_goes_to_the_output_and_the_usage_to_the_errors() {
        let (mut output, mut errors) = (Vec::new(), Vec::new());
        let code = program(
            &args(&["--version"]),
            &mut &b""[..],
            &mut output,
            &mut errors,
        );
        assert_eq!(code, 0);
        assert_eq!(
            String::from_utf8(output).unwrap(),
            format!("kosmo {}\n", env!("CARGO_PKG_VERSION"))
        );
        assert!(errors.is_empty());

        let (mut output, mut errors) = (Vec::new(), Vec::new());
        let code = program(&args(&["-V"]), &mut &b""[..], &mut output, &mut errors);
        assert_eq!(code, 2);
        assert!(output.is_empty());
        assert_eq!(errors, b"Usage: kosmo [--version]\n");
    }

    #[test]
    fn the_program_runs_the_terminal_without_arguments() {
        let (mut output, mut errors) = (Vec::new(), Vec::new());
        let code = program(&[], &mut &b"1/3+1/6\n"[..], &mut output, &mut errors);
        assert_eq!(code, 0);
        assert_eq!(output, b"value (source 7, settings 0): 1/2\n");
        assert!(errors.is_empty());
    }
}
