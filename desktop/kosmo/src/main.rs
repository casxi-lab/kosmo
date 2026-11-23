//! The `kosmo` program: without arguments, the line-mode terminal;
//! `kosmo --version` prints the program name and version and exits with 0;
//! any other arguments print the usage to standard error and exit with 2.
//!
//! `main` binds only the standard streams, the arguments and the exit code;
//! the library's `kosmo::program` does the rest.

#![forbid(unsafe_code)]

use std::ffi::OsString;
use std::io;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let code = kosmo::program(
        &args,
        &mut io::stdin().lock(),
        &mut io::stdout().lock(),
        &mut io::stderr().lock(),
    );
    ExitCode::from(code)
}
