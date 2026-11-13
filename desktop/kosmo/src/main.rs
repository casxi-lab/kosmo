//! The `kosmo` program. So far it only reports its version:
//! `kosmo --version` prints the program name and version and exits with 0;
//! any other arguments print the usage to standard error and exit with 2.

#![forbid(unsafe_code)]

use std::ffi::OsString;
use std::io::Write;
use std::process::ExitCode;

const USAGE: &str = "Usage: kosmo --version";

/// What the program does for its arguments.
#[derive(Debug, PartialEq, Eq)]
enum Reply {
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
        [only] if *only == "--version" => Reply::Version,
        _ => Reply::Usage,
    }
}

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    match reply(&args) {
        Reply::Version => {
            let line = format!("{} {}\n", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
            match std::io::stdout().write_all(line.as_bytes()) {
                Ok(()) => ExitCode::SUCCESS,
                Err(_) => ExitCode::FAILURE,
            }
        }
        Reply::Usage => {
            // Exits with 2 whether or not the usage can be written.
            let _ = writeln!(std::io::stderr(), "{USAGE}");
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    #[test]
    fn only_the_single_version_flag_prints_the_version() {
        assert_eq!(reply(&args(&["--version"])), Reply::Version);
        assert_eq!(reply(&args(&[])), Reply::Usage);
        assert_eq!(reply(&args(&["--version", "--version"])), Reply::Usage);
        assert_eq!(reply(&args(&["-V"])), Reply::Usage);
    }

    #[cfg(unix)]
    #[test]
    fn an_argument_that_is_not_utf8_gets_the_usage() {
        use std::os::unix::ffi::OsStringExt;
        let invalid = OsString::from_vec(vec![0xff]);
        assert_eq!(reply(std::slice::from_ref(&invalid)), Reply::Usage);
        assert_eq!(reply(&[OsString::from("--version"), invalid]), Reply::Usage);
    }
}
