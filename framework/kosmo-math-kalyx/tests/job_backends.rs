//! One contract test of the job layer, generic over its backend, run on the
//! host stub with the kalyx runner and on the controllable backend: the job
//! layer's code is the same, and only the backend's type differs.

mod support;

use kosmo_jobs::controllable::{ControllableBackend, Plan};
use kosmo_jobs::{
    CancelAnswer, EchoedVersions, ErrorClass, JobBackend, JobLayer, Operation, OutcomeKind,
    Request, RequestReport, StopReason, Submission, TerminalState,
};
use kosmo_math_kalyx::{convert, read_output_line};
use kosmo_math_model::display::Content;
use kosmo_math_model::version::Versions;
use support::Checked;
use support::jobs::{FakeClock, KalyxJobs, Typed, typed};

/// The manifest of the job layer's crate.
const JOBS_MANIFEST: &str = include_str!("../../kosmo-jobs/Cargo.toml");

/// What the contract saw of a job that ran to its end and of a job that was
/// cancelled while queued.
#[derive(Debug, PartialEq, Eq)]
struct Seen {
    state: TerminalState,
    versions: Versions,
    echoed: Option<EchoedVersions>,
    content: Content,
    answer: CancelAnswer,
    cancelled: TerminalState,
    cancelled_stop: Option<StopReason>,
    cancelled_lines: usize,
}

/// The contract: submits an `eval` job of the linear text of `line`, with
/// the snapshot's versions, waits for its terminal state and reads its line
/// back; then submits the same job again and cancels it while the job layer
/// still has it queued. It ends with every job taken and finished.
fn contract<B: JobBackend>(layer: &mut JobLayer<B>, line: &Typed) -> Checked<Seen> {
    let session = layer.open_session()?;
    let request = Request::new(Operation::Eval, "", convert(&line.snapshot)?);
    let submission = Submission::new(session, line.versions, request);

    let id = layer.submit(submission.clone())?;
    let done = layer.wait(id).ok_or("no terminal state")?;
    let output = done.record().lines().first().ok_or("no line")?;
    let item = read_output_line(output, line.versions, &line.snapshot);
    assert_eq!(item.versions, line.versions);

    let queued = layer.submit(submission)?;
    let answer = layer.cancel(queued);
    let cancelled = layer.take(queued).ok_or("not terminal")?;
    layer.shutdown();
    assert_eq!(layer.live_jobs(), 0);
    Ok(Seen {
        state: done.record().state(),
        versions: done.versions(),
        echoed: done.record().echoed(),
        content: item.content,
        answer,
        cancelled: cancelled.record().state(),
        cancelled_stop: cancelled.record().stop(),
        cancelled_lines: cancelled.record().lines().len(),
    })
}

/// The crates that the manifest `manifest` names in any table of
/// dependencies: the keys of a table such as `[dependencies]`, and the name
/// in the header of a table of one dependency, such as
/// `[dependencies.<name>]` or `[dev-dependencies.<name>]`. It skips a
/// comment, from a `#` to the end of its line, also after a header; it
/// takes no key or header to hold a `#`.
fn dependencies(manifest: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut in_dependencies = false;
    for line in manifest.lines() {
        let line = line.split_once('#').map_or(line, |(code, _)| code).trim();
        if let Some(table) = line.strip_prefix('[') {
            let table = table.trim_end_matches(']').trim();
            in_dependencies = table.ends_with("dependencies");
            if let Some((parent, name)) = table.rsplit_once('.')
                && parent.trim_end().ends_with("dependencies")
            {
                names.push(name.trim().trim_matches(['"', '\'']).to_string());
            }
        } else if in_dependencies && let Some((key, _)) = line.split_once('=') {
            let name = key.trim().split('.').next().unwrap_or_default();
            names.push(name.to_string());
        }
    }
    names
}

#[test]
fn the_job_layer_runs_alike_on_the_kalyx_runner_and_the_controllable_backend() -> Checked {
    let line = typed("1/3+1/6")?;

    let mut kalyx = KalyxJobs::new()?;
    let on_kalyx = contract(&mut kalyx.layer, &line)?;
    assert_eq!(kalyx.layer.backend().inner().live_threads(), 0);

    // The controllable backend gives the first job the line that kalyx
    // writes for it and leaves the second waiting before it starts.
    let value = format!("value\t{}\t0\t0\t1/2\n", line.versions.source.get());
    let report = RequestReport::new(
        OutcomeKind::Value,
        ErrorClass::NoError,
        Some(value.into_bytes()),
        EchoedVersions::from(line.versions),
    );
    let mut backend = ControllableBackend::new();
    backend.plan(0, Plan::new().request(report));
    let (clock, _) = FakeClock::new();
    let mut controllable = JobLayer::new(backend, Box::new(clock));
    let on_controllable = contract(&mut controllable, &line)?;

    assert_eq!(on_kalyx, on_controllable);
    assert_eq!(
        on_kalyx,
        Seen {
            state: TerminalState::Completed,
            versions: line.versions,
            echoed: Some(EchoedVersions::new(1, 0, 0)),
            content: Content::Value("1/2".to_string()),
            answer: CancelAnswer::Cancelled,
            cancelled: TerminalState::Cancelled,
            cancelled_stop: Some(StopReason::Cancellation),
            cancelled_lines: 0,
        }
    );

    // The job layer depends on neither kalyx nor this crate.
    assert_eq!(dependencies(JOBS_MANIFEST), ["kosmo-math-model"]);
    Ok(())
}

#[test]
fn the_manifest_reader_finds_dependencies_in_every_form_of_table() {
    let manifest = "\
[package]
name = \"example\"

[dependencies]
kosmo-math-model.workspace = true

[dependencies.kalyx]
workspace = true

[dev-dependencies.\"kosmo-math-kalyx\"]
workspace = true

[target.'cfg(unix)'.build-dependencies]
cc = \"=1.0.0\"

[dev-dependencies] # tests only
proptest = \"=1.0.0\" # pinned
# serde = \"=1.0.0\"

[target.'cfg(all())'.dependencies] # every target
libc.workspace = true
";
    assert_eq!(
        dependencies(manifest),
        [
            "kosmo-math-model",
            "kalyx",
            "kosmo-math-kalyx",
            "cc",
            "proptest",
            "libc"
        ]
    );
}
