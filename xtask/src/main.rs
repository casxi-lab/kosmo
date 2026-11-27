//! Repository tasks for kosmo, run from the repository root as
//! `cargo xtask <command>`.

mod assets;
mod check;
mod commits;
mod deps;
mod docs;
mod fetch;
mod golden;
mod layout;
mod receipt;
mod repo;
mod workflows;

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;

/// A command of `cargo xtask`.
struct Command {
    name: &'static str,
    about: &'static str,
}

/// Every command that `cargo xtask` provides.
const COMMANDS: &[Command] = &[
    Command {
        name: "check",
        about: "Run the layout, golden and font asset, fetch, dependency, format, Clippy, test, documentation, commit message and workflow checks; with --receipt <path>, also write a JSON receipt of the run",
    },
    Command {
        name: "golden",
        about: "Write the pixel goldens and their MANIFEST; with --verify, draw them again into the build directory and compare each digest with MANIFEST",
    },
    Command {
        name: "help",
        about: "List the commands",
    },
];

/// The environment variable that, when it is set, makes the build scripts
/// of the icu data packages of the framework's text stack compile the
/// Unicode data of the directory it names instead of their own.
const ICU4X_DATA_DIR: &str = "ICU4X_DATA_DIR";

fn print_help() {
    println!("Usage: cargo xtask <command>\n\nCommands:");
    for command in COMMANDS {
        println!("  {:<8} {}", command.name, command.about);
    }
}

/// Whether `args` is a command that builds the product workspaces: the
/// check and both forms of `golden`.
fn builds(args: &[&str]) -> bool {
    matches!(
        args,
        ["check"] | ["check", "--receipt", _] | ["golden"] | ["golden", "--verify"]
    )
}

/// Fails, saying why, when `var`, which reads the environment, finds
/// [`ICU4X_DATA_DIR`] set to any value.
fn refuse_icu_data(var: impl Fn(&str) -> Option<OsString>) -> Result<(), String> {
    if var(ICU4X_DATA_DIR).is_none() {
        return Ok(());
    }
    Err(format!(
        "{ICU4X_DATA_DIR} is set: with it, the build scripts of the icu data packages of the framework's text stack compile the Unicode data of the directory it names, from outside the repository, instead of their own, which changes line breaking and normalization and with them the goldens; unset it and run the command again"
    ))
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    if builds(&args)
        && let Err(error) = refuse_icu_data(|name| std::env::var_os(name))
    {
        eprintln!("{error}");
        return ExitCode::FAILURE;
    }
    match args.as_slice() {
        ["check"] => check::run(repo::root(), None),
        ["check", "--receipt", path] => check::run(repo::root(), Some(PathBuf::from(path))),
        ["golden"] => golden::update(&repo::root()),
        ["golden", "--verify"] => golden::verify(&repo::root()),
        [] | ["help"] => {
            print_help();
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("unknown command: cargo xtask {}\n", args.join(" "));
            print_help();
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_and_golden_fail_first_when_icu_data_is_set() {
        let commands: [&[&str]; 4] = [
            &["check"],
            &["check", "--receipt", "receipt.json"],
            &["golden"],
            &["golden", "--verify"],
        ];
        for args in commands {
            assert!(builds(args), "{args:?}");
        }
        assert!(!builds(&[]) && !builds(&["help"]) && !builds(&["check", "--slow"]));
        assert_eq!(refuse_icu_data(|_| None), Ok(()));
        for value in ["/elsewhere/icu4x-data", ""] {
            let set = |name: &str| (name == ICU4X_DATA_DIR).then(|| OsString::from(value));
            let error = refuse_icu_data(set).unwrap_err();
            assert!(error.starts_with("ICU4X_DATA_DIR is set: "), "{error}");
        }
    }
}
