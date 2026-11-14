//! `cargo xtask check`: the required checks, in order, stopping at the first
//! failure.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::{Duration, Instant};

use crate::repo::{Entry, Tree};
use crate::{commits, deps, docs, fetch, layout, receipt, workflows};

/// The Cargo workspaces that the format, Clippy and test steps cover, as
/// repository-relative directories. The layout step checks that they are
/// exactly the tool workspaces and the product workspaces with crates in the
/// layout table, which the fetch and deps steps read from the table.
pub const WORKSPACES: [&str; 2] = ["xtask", "desktop"];

/// The environment variable, and its value, that keeps Cargo off the
/// network. Every Cargo command after the fetch step sets it; `cargo fmt`
/// does not accept `--offline`.
pub const OFFLINE: (&str, &str) = ("CARGO_NET_OFFLINE", "true");

/// A command run in the repository root.
#[derive(Debug, Clone)]
pub struct Cmd {
    pub program: OsString,
    pub args: Vec<String>,
    /// Environment variables set for the command, in addition to those it
    /// inherits.
    pub env: Vec<(String, String)>,
    /// The fact under which the receipt records how long the command ran,
    /// in milliseconds.
    pub timing: String,
}

impl Cmd {
    /// The command as it is shown: the variables it sets, the program and
    /// the arguments.
    pub fn shown(&self) -> String {
        let mut parts: Vec<String> = self
            .env
            .iter()
            .map(|(name, value)| format!("{name}={value}"))
            .collect();
        parts.push(self.program.to_string_lossy().into_owned());
        parts.extend(self.args.iter().cloned());
        parts.join(" ")
    }
}

/// The offline setting as a variable of a command.
pub fn offline() -> Vec<(String, String)> {
    vec![(OFFLINE.0.to_string(), OFFLINE.1.to_string())]
}

/// Named values that a step observed, in the order it observed them, for the
/// receipt. A step adds them as it goes, so they are kept when it fails.
pub type Facts = Vec<(String, String)>;

/// A check written in this crate, run with the repository root.
pub type Check = Box<dyn Fn(&Path, &mut Facts) -> Result<(), String>>;

/// What a step does.
pub enum Action {
    /// Run commands in order; each must exit with status 0.
    Commands(Vec<Cmd>),
    /// Run a check written in this crate.
    Internal(Check),
}

/// A named step of the check.
pub struct Step {
    pub name: &'static str,
    pub action: Action,
}

/// How a step ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    /// The step passed.
    Passed,
    /// The step failed, and why.
    Failed(String),
    /// An earlier step failed, so this one did not run.
    NotRun,
}

/// The result of one step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub step: &'static str,
    pub status: Status,
    pub duration: Duration,
    pub facts: Facts,
}

/// The Cargo that runs `cargo xtask`, or `cargo` when it is not known.
pub fn cargo() -> OsString {
    std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into())
}

/// The same Cargo command for each workspace, offline.
fn per_workspace(args: &[&str], trailing: &[&str]) -> Vec<Cmd> {
    WORKSPACES
        .iter()
        .map(|workspace| {
            let mut all: Vec<String> = args.iter().map(|arg| arg.to_string()).collect();
            all.push("--manifest-path".into());
            all.push(format!("{workspace}/Cargo.toml"));
            all.extend(trailing.iter().map(|arg| arg.to_string()));
            Cmd {
                program: cargo(),
                args: all,
                env: offline(),
                timing: format!("{workspace}_ms"),
            }
        })
        .collect()
}

/// The steps of `cargo xtask check`, in the order they run.
pub fn steps() -> Vec<Step> {
    vec![
        Step {
            name: "layout",
            action: Action::Internal(Box::new(layout::run)),
        },
        Step {
            name: "fetch",
            action: Action::Internal(Box::new(fetch::run)),
        },
        Step {
            name: "deps",
            action: Action::Internal(Box::new(deps::run)),
        },
        Step {
            name: "fmt",
            action: Action::Commands(per_workspace(&["fmt", "--all", "--check"], &[])),
        },
        Step {
            name: "clippy",
            action: Action::Commands(per_workspace(
                &["clippy", "--workspace", "--all-targets", "--locked"],
                &["--", "-D", "warnings"],
            )),
        },
        Step {
            name: "test",
            action: Action::Commands(per_workspace(&["test", "--workspace", "--locked"], &[])),
        },
        Step {
            name: "docs",
            action: Action::Internal(Box::new(|root, _| docs::run(root))),
        },
        Step {
            name: "commits",
            action: Action::Internal(Box::new(commits::run)),
        },
        Step {
            name: "workflows",
            action: Action::Internal(Box::new(check_workflows)),
        },
    ]
}

/// The workflows step on the tracked files; the files it read become facts.
fn check_workflows(root: &Path, facts: &mut Facts) -> Result<(), String> {
    let tree = Tree::load(root)?;
    let tracked = || {
        tree.entries().map(|(path, entry)| match entry {
            Entry::File(bytes) => (path, Some(bytes.as_slice())),
            Entry::Symlink(_) | Entry::Submodule => (path, None),
        })
    };
    let files: Vec<&str> = tracked()
        .map(|(path, _)| path)
        .filter(|path| workflows::is_workflow(path))
        .collect();
    if files.is_empty() {
        println!("workflows: no workflow files");
    } else {
        println!("workflows: {}", files.join(", "));
    }
    facts.push(("files".to_string(), files.join(", ")));
    let problems = workflows::check(tracked());
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems.join("\n"))
    }
}

/// Runs `steps` in order in `root` and stops at the first failure. Every
/// step has an outcome; those after a failure did not run.
pub fn run_steps(root: &Path, steps: &[Step]) -> Vec<Outcome> {
    let mut outcomes = Vec::new();
    let mut failed = false;
    for step in steps {
        let mut facts = Facts::new();
        if failed {
            outcomes.push(Outcome {
                step: step.name,
                status: Status::NotRun,
                duration: Duration::ZERO,
                facts,
            });
            continue;
        }
        println!("==> {}", step.name);
        let started = Instant::now();
        let result = match &step.action {
            Action::Commands(commands) => commands.iter().try_for_each(|cmd| {
                let started = Instant::now();
                let result = run_command(root, cmd);
                facts.push((
                    cmd.timing.clone(),
                    started.elapsed().as_millis().to_string(),
                ));
                result
            }),
            Action::Internal(check) => check(root, &mut facts),
        };
        let status = match result {
            Ok(()) => Status::Passed,
            Err(detail) => {
                failed = true;
                Status::Failed(detail)
            }
        };
        outcomes.push(Outcome {
            step: step.name,
            status,
            duration: started.elapsed(),
            facts,
        });
    }
    outcomes
}

/// The failed step and why it failed, if one did.
pub fn failure(outcomes: &[Outcome]) -> Option<(&'static str, &str)> {
    outcomes.iter().find_map(|outcome| match &outcome.status {
        Status::Failed(detail) => Some((outcome.step, detail.as_str())),
        _ => None,
    })
}

fn run_command(root: &Path, cmd: &Cmd) -> Result<(), String> {
    let shown = cmd.shown();
    println!("$ {shown}");
    let status = Command::new(&cmd.program)
        .args(&cmd.args)
        .envs(cmd.env.iter().map(|(name, value)| (name, value)))
        .current_dir(root)
        .status()
        .map_err(|error| format!("cannot run `{shown}`: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("`{shown}` exited with {status}"))
    }
}

/// Runs `cmd` in `root` with its output captured and returns its standard
/// output. The standard error follows on this process's standard error
/// when the command succeeds, and in the error, each line indented, when
/// it fails.
pub fn capture(root: &Path, cmd: &Cmd) -> Result<Vec<u8>, String> {
    let shown = cmd.shown();
    println!("$ {shown}");
    let output = Command::new(&cmd.program)
        .args(&cmd.args)
        .envs(cmd.env.iter().map(|(name, value)| (name, value)))
        .current_dir(root)
        .output()
        .map_err(|error| format!("cannot run `{shown}`: {error}"))?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    if output.status.success() {
        eprint!("{stderr}");
        Ok(output.stdout)
    } else {
        let mut problem = format!("`{shown}` exited with {}", output.status);
        for line in stderr.lines().filter(|line| !line.trim().is_empty()) {
            problem.push_str("\n  ");
            problem.push_str(line.trim_end());
        }
        Err(problem)
    }
}

/// Runs every command of `commands` in `root`, also after one fails, and
/// records how long each ran. Fails with every failure, one per command.
pub fn run_each(root: &Path, commands: &[Cmd], facts: &mut Facts) -> Result<(), String> {
    let mut failures = Vec::new();
    for cmd in commands {
        let started = Instant::now();
        let result = capture(root, cmd);
        facts.push((
            cmd.timing.clone(),
            started.elapsed().as_millis().to_string(),
        ));
        if let Err(failure) = result {
            failures.push(failure);
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("\n"))
    }
}

/// Entry point of `cargo xtask check`. With `receipt`, the run is recorded
/// there whether it passes or fails.
pub fn run(root: PathBuf, receipt: Option<PathBuf>) -> ExitCode {
    let start = receipt
        .as_ref()
        .map(|_| (receipt::GitState::read(&root), receipt::Tools::read(&root)));
    let outcomes = run_steps(&root, &steps());
    let mut code = match failure(&outcomes) {
        None => {
            println!("check passed");
            ExitCode::SUCCESS
        }
        Some((step, detail)) => {
            eprintln!("{detail}");
            eprintln!("check failed at step {step}");
            ExitCode::FAILURE
        }
    };
    if let (Some(path), Some((before, tools))) = (receipt, start) {
        let after = receipt::GitState::read(&root);
        let text = receipt::render(
            &before,
            &after,
            &outcomes,
            &tools,
            &receipt::Environment::read(),
        );
        match receipt::write(&path, &text) {
            Ok(()) => println!("receipt written to {}", path.display()),
            Err(error) => {
                eprintln!("cannot write the receipt {}: {error}", path.display());
                code = ExitCode::FAILURE;
            }
        }
    }
    code
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::BTreeSet;
    use std::rc::Rc;

    use super::*;

    const ORDER: [&str; 9] = [
        "layout",
        "fetch",
        "deps",
        "fmt",
        "clippy",
        "test",
        "docs",
        "commits",
        "workflows",
    ];

    #[test]
    fn steps_run_in_the_documented_order() {
        let names: Vec<&str> = steps().iter().map(|step| step.name).collect();
        assert_eq!(names, ORDER);
    }

    #[test]
    fn command_steps_cover_every_workspace() {
        for step in steps() {
            if let Action::Commands(commands) = step.action {
                let targets: Vec<&str> = commands
                    .iter()
                    .map(|cmd| {
                        let at = cmd.args.iter().position(|arg| arg == "--manifest-path");
                        cmd.args[at.expect("each command names a manifest") + 1].as_str()
                    })
                    .collect();
                assert_eq!(targets, ["xtask/Cargo.toml", "desktop/Cargo.toml"]);
            }
        }
    }

    /// The real step list with `failing` replaced by a failing action and
    /// every other step replaced by one that records that it ran and adds a
    /// fact.
    fn with_failure(failing: &str, ran: &Rc<RefCell<Vec<&'static str>>>) -> Vec<Step> {
        steps()
            .into_iter()
            .map(|step| {
                let name = step.name;
                let action = if name == failing {
                    match step.action {
                        // A real process that exits with a non-zero status.
                        Action::Commands(_) => Action::Commands(vec![Cmd {
                            program: "sh".into(),
                            args: vec!["-c".into(), "exit 3".into()],
                            env: Vec::new(),
                            timing: "sh_ms".to_string(),
                        }]),
                        Action::Internal(_) => Action::Internal(Box::new(|_, facts| {
                            facts.push(("seen".to_string(), "before failing".to_string()));
                            Err("forced failure".to_string())
                        })),
                    }
                } else {
                    let ran = Rc::clone(ran);
                    Action::Internal(Box::new(move |_, facts| {
                        ran.borrow_mut().push(name);
                        facts.push(("ran".to_string(), name.to_string()));
                        Ok(())
                    }))
                };
                Step { name, action }
            })
            .collect()
    }

    fn assert_fails_at(failing: &'static str) {
        let ran = Rc::new(RefCell::new(Vec::new()));
        let steps = with_failure(failing, &ran);
        let outcomes = run_steps(Path::new("."), &steps);
        let (step, _) = failure(&outcomes).expect("the check fails");
        assert_eq!(step, failing);
        let before: Vec<&str> = ORDER
            .iter()
            .copied()
            .take_while(|name| *name != failing)
            .collect();
        assert_eq!(
            *ran.borrow(),
            before,
            "steps after the failure must not run"
        );
        let names: Vec<&str> = outcomes.iter().map(|outcome| outcome.step).collect();
        assert_eq!(names, ORDER, "every step has an outcome");
        let failed_at = ORDER.iter().position(|name| *name == failing).unwrap();
        for (index, outcome) in outcomes.iter().enumerate() {
            match index.cmp(&failed_at) {
                std::cmp::Ordering::Less => {
                    assert_eq!(outcome.status, Status::Passed);
                    assert_eq!(
                        outcome.facts,
                        [("ran".to_string(), outcome.step.to_string())]
                    );
                }
                std::cmp::Ordering::Equal => {
                    assert!(matches!(outcome.status, Status::Failed(_)), "{outcome:?}");
                    // The failing step keeps what it recorded before it
                    // failed: the time of its command, or the fact of its
                    // check.
                    let keys: Vec<&str> =
                        outcome.facts.iter().map(|(key, _)| key.as_str()).collect();
                    assert!(keys == ["sh_ms"] || keys == ["seen"], "{outcome:?}");
                }
                std::cmp::Ordering::Greater => {
                    assert_eq!(outcome.status, Status::NotRun);
                    assert_eq!(outcome.duration, Duration::ZERO);
                    assert!(outcome.facts.is_empty());
                }
            }
        }
    }

    #[test]
    fn a_layout_failure_stops_the_check() {
        assert_fails_at("layout");
    }

    #[test]
    fn a_fetch_failure_stops_the_check() {
        assert_fails_at("fetch");
    }

    #[test]
    fn a_deps_failure_stops_the_check() {
        assert_fails_at("deps");
    }

    #[test]
    fn a_format_failure_stops_the_check() {
        assert_fails_at("fmt");
    }

    #[test]
    fn a_clippy_failure_stops_the_check() {
        assert_fails_at("clippy");
    }

    #[test]
    fn a_test_failure_stops_the_check() {
        assert_fails_at("test");
    }

    #[test]
    fn a_docs_failure_stops_the_check() {
        assert_fails_at("docs");
    }

    #[test]
    fn a_commits_failure_stops_the_check() {
        assert_fails_at("commits");
    }

    #[test]
    fn a_workflows_failure_fails_the_check_after_every_other_step() {
        assert_fails_at("workflows");
    }

    #[test]
    fn a_failing_step_keeps_the_facts_it_added() {
        let ran = Rc::new(RefCell::new(Vec::new()));
        let outcomes = run_steps(Path::new("."), &with_failure("docs", &ran));
        assert_eq!(outcomes[6].step, "docs");
        assert_eq!(
            outcomes[6].facts,
            [("seen".to_string(), "before failing".to_string())]
        );
        assert_eq!(
            outcomes[6].status,
            Status::Failed("forced failure".to_string())
        );
    }

    #[test]
    fn records_how_long_each_command_ran_even_when_it_fails() {
        let step = |script: &str| Step {
            name: "fmt",
            action: Action::Commands(
                [("first", "exit 0"), ("second", script)]
                    .map(|(name, script)| Cmd {
                        program: "sh".into(),
                        args: vec!["-c".into(), script.into()],
                        env: Vec::new(),
                        timing: format!("{name}_ms"),
                    })
                    .to_vec(),
            ),
        };
        for (script, passed) in [("exit 0", true), ("exit 3", false)] {
            let outcomes = run_steps(Path::new("."), &[step(script)]);
            assert_eq!(outcomes[0].status == Status::Passed, passed);
            let keys: Vec<&str> = outcomes[0]
                .facts
                .iter()
                .map(|(key, value)| {
                    assert!(value.parse::<u128>().is_ok(), "{value}");
                    key.as_str()
                })
                .collect();
            assert_eq!(keys, ["first_ms", "second_ms"]);
        }
    }

    /// The Cargo commands of `steps` after the fetch step that do not set
    /// the offline variable, the deps step standing for its metadata
    /// command.
    fn online_after_fetch(steps: &[Step]) -> Vec<String> {
        let fetch = steps
            .iter()
            .position(|step| step.name == "fetch")
            .expect("there is a fetch step");
        let mut online = Vec::new();
        for step in &steps[fetch + 1..] {
            let commands = match &step.action {
                Action::Commands(commands) => commands.clone(),
                Action::Internal(_) if step.name == "deps" => {
                    vec![deps::metadata_command("xtask", "aarch64-apple-darwin")]
                }
                Action::Internal(_) => continue,
            };
            online.extend(
                commands
                    .iter()
                    .filter(|cmd| {
                        !cmd.env
                            .iter()
                            .any(|(name, value)| (name.as_str(), value.as_str()) == OFFLINE)
                    })
                    .map(Cmd::shown),
            );
        }
        online
    }

    #[test]
    fn cargo_commands_after_the_fetch_step_run_offline() {
        assert_eq!(online_after_fetch(&steps()), Vec::<String>::new());
        let mut steps = steps();
        for step in &mut steps {
            if let (Action::Commands(commands), "clippy") = (&mut step.action, step.name) {
                commands.iter_mut().for_each(|cmd| cmd.env.clear());
            }
        }
        let online = online_after_fetch(&steps);
        assert_eq!(online.len(), WORKSPACES.len(), "{online:?}");
        assert!(
            online.iter().all(|cmd| cmd.contains(" clippy ")),
            "{online:?}"
        );
    }

    #[test]
    fn a_command_step_passes_the_variables_of_its_commands() {
        // A variable that this process does not have reaches the command
        // only through the variables of the command, as the offline
        // variable reaches the Cargo commands after the fetch step.
        let step = |env: Vec<(String, String)>| Step {
            name: "fmt",
            action: Action::Commands(vec![Cmd {
                program: "sh".into(),
                args: vec!["-c".into(), "test \"$XTASK_CHECK_PROBE\" = set".into()],
                env,
                timing: "sh_ms".to_string(),
            }]),
        };
        let set = vec![("XTASK_CHECK_PROBE".to_string(), "set".to_string())];
        let outcomes = run_steps(Path::new("."), &[step(set)]);
        assert_eq!(outcomes[0].status, Status::Passed, "{outcomes:?}");
        let outcomes = run_steps(Path::new("."), &[step(Vec::new())]);
        assert!(
            matches!(outcomes[0].status, Status::Failed(_)),
            "{outcomes:?}"
        );
    }

    #[test]
    fn run_each_runs_every_command_and_records_each_time() {
        let cmd = |name: &str, script: &str| Cmd {
            program: "sh".into(),
            args: vec!["-c".into(), script.into()],
            env: vec![("NAME".to_string(), name.to_string())],
            timing: format!("{name}_ms"),
        };
        let commands = [
            cmd("first", "exit 101"),
            cmd("second", "test \"$NAME\" = second"),
            cmd("third", "echo \"$NAME failed\" >&2; exit 2"),
        ];
        let mut facts = Facts::new();
        let error = run_each(Path::new("."), &commands, &mut facts).unwrap_err();
        let failures: Vec<&str> = error.lines().collect();
        assert_eq!(
            failures,
            [
                "`NAME=first sh -c exit 101` exited with exit status: 101",
                "`NAME=third sh -c echo \"$NAME failed\" >&2; exit 2` exited with exit status: 2",
                "  third failed",
            ]
        );
        let keys: Vec<&str> = facts.iter().map(|(key, _)| key.as_str()).collect();
        assert_eq!(keys, ["first_ms", "second_ms", "third_ms"]);
    }

    #[test]
    fn every_step_records_each_fact_once() {
        // The receipt writes the facts of a step as the members of one JSON
        // object, which keeps one member per key, so a key recorded twice
        // would lose a value. The commits step finds its base in one of two
        // ways, chosen by the environment; its own tests check the keys of
        // both in a scratch repository.
        let root = crate::repo::root();
        for step in steps() {
            let action = match step.action {
                // A command that exits at once stands in for each Cargo
                // command and records its time under the same key.
                Action::Commands(commands) => Action::Commands(
                    commands
                        .into_iter()
                        .map(|cmd| Cmd {
                            program: "sh".into(),
                            args: vec!["-c".into(), "exit 0".into()],
                            env: Vec::new(),
                            timing: cmd.timing,
                        })
                        .collect(),
                ),
                // The checks of this crate run on this working copy and keep
                // their facts whether they pass or fail.
                Action::Internal(check) => Action::Internal(check),
            };
            let outcomes = run_steps(
                &root,
                &[Step {
                    name: step.name,
                    action,
                }],
            );
            let keys: Vec<&str> = outcomes[0]
                .facts
                .iter()
                .map(|(key, _)| key.as_str())
                .collect();
            let distinct: BTreeSet<&str> = keys.iter().copied().collect();
            assert_eq!(distinct.len(), keys.len(), "{}: {keys:?}", step.name);
        }
    }
}
