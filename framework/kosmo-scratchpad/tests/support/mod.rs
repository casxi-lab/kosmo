//! Helpers shared by the integration tests: a job layer on the controllable
//! backend, the output lines of kalyx 0.1.0 that its jobs answer with, and
//! the cases of history and reuse that several test files run.

#![allow(dead_code, reason = "each test file uses some of the helpers")]

pub mod cases;

use std::error::Error;

use kosmo_jobs::controllable::{ControllableBackend, Gate, Plan};
use kosmo_jobs::{
    EchoedVersions, ErrorClass, JobClock, JobId, JobLayer, Limits, Operation, OutcomeKind,
    RequestReport, SessionId, Terminal,
};
use kosmo_math_kalyx::convert;
use kosmo_math_model::Item;
use kosmo_math_model::display::Content;
use kosmo_math_model::version::Versions;
use kosmo_scratchpad::{Entry, Outcome, Scratchpad};

/// The result of a helper or a test.
pub type Checked<T = ()> = Result<T, Box<dyn Error>>;

/// The output line, without its final line feed, that kalyx 0.1.0 writes
/// for `eval` of `1/3+1/6` at the versions 1, 0 and 0.
pub const SUM: &str = "value\t1\t0\t0\t1/2";

/// The line of `expand` of `(x+1)^2` with respect to `x` at the versions
/// 1, 0 and 0.
pub const EXPANDED: &str = "value\t1\t0\t0\t1 + 2*x + x^2";

/// The line of `diff` of `x^2` with respect to `x` at the versions 1, 0
/// and 0.
pub const DERIVATIVE: &str = "value\t1\t0\t0\t2*x";

/// The line of `eval` of `1/0` at the versions 1, 0 and 0.
pub const DIVISION_BY_ZERO: &str =
    "error\t1\t0\t0\tDivisionByZero\t0..3\t-\t-\t-\t-\tdivision by zero";

/// The line of `eval` of `1/3+1/6` at the versions 1, 0 and 1.
pub const OLDER: &str = "value\t1\t0\t1\t1/2";

/// The line of `eval` of `1/3+1/60` at the versions 2, 0 and 1.
pub const NEWER: &str = "value\t2\t0\t1\t7/20";

/// The line of `eval` of the line of `k` ones at the source version `k`
/// and context and settings version 0: the number itself.
pub fn ones(k: usize) -> String {
    format!("value\t{k}\t0\t0\t{}", "1".repeat(k))
}

/// A clock that always reads 0.
pub struct Still;

impl JobClock for Still {
    fn now(&mut self) -> u64 {
        0
    }

    fn fork(&self) -> Box<dyn JobClock> {
        Box::new(Still)
    }
}

/// A job layer on the controllable backend with one open session.
pub struct Jobs {
    /// The job layer.
    pub layer: JobLayer<ControllableBackend>,
    /// The session of the Scratchpad's jobs.
    pub session: SessionId,
}

impl Jobs {
    /// A job layer that has received no job.
    pub fn new() -> Checked<Jobs> {
        let mut layer = JobLayer::new(ControllableBackend::new(), Box::new(Still));
        let session = layer.open_session()?;
        Ok(Jobs { layer, session })
    }

    /// Prepares the current line of `pad` with `operation` and `variable`,
    /// gives its job the plan `plan`, submits it and accepts the answer.
    pub fn submit(
        &mut self,
        pad: &mut Scratchpad,
        operation: Operation,
        variable: &str,
        plan: Plan,
    ) -> Checked<JobId> {
        let (submission, pending) =
            pad.prepare(self.session, operation, variable, Limits::new(), None)?;
        let order = self.layer.backend().received();
        self.layer.controllable().plan(order, plan);
        Ok(pad.accept(pending, self.layer.submit(submission))?)
    }

    /// Releases the `Finished` gate of the job of submission order `order`.
    pub fn release(&mut self, order: usize) {
        self.layer.controllable().release(order, Gate::Finished);
    }

    /// Applies the reports that have arrived and takes the terminal state
    /// of job `id`, which must have reached it.
    pub fn take(&mut self, id: JobId) -> Checked<Terminal> {
        self.layer.poll();
        Ok(self.layer.take(id).ok_or("not terminal")?)
    }
}

/// The report of a request whose output line is `line` with its final line
/// feed, and whose computation echoed `versions`: an error line reports a
/// fault, any other line a value.
pub fn report(line: &str, versions: Versions) -> RequestReport {
    let (kind, error) = if line.starts_with("error\t") {
        (OutcomeKind::Fault, ErrorClass::Outcome)
    } else {
        (OutcomeKind::Value, ErrorClass::NoError)
    };
    RequestReport::new(
        kind,
        error,
        Some(format!("{line}\n").into_bytes()),
        EchoedVersions::from(versions),
    )
}

/// A plan of one request whose output line is `line`.
pub fn answer(line: &str, versions: Versions) -> Plan {
    Plan::new().request(report(line, versions))
}

/// The three version numbers of `versions`: source, context and settings.
pub fn numbers(versions: Versions) -> (u64, u64, u64) {
    (
        versions.source.get(),
        versions.context.get(),
        versions.settings.get(),
    )
}

/// The characters of the current line of `pad`.
pub fn text(pad: &Scratchpad) -> String {
    pad.line()
        .items()
        .iter()
        .map(|item| match item {
            Item::Char(c) => *c,
            _ => '?',
        })
        .collect()
}

/// The linear text of the current line of `pad`.
pub fn linear(pad: &Scratchpad) -> Checked<String> {
    Ok(convert(&pad.line().snapshot())?)
}

/// The display text of the value of the entry of job `id` in `pad`.
pub fn value_of(pad: &Scratchpad, id: JobId) -> Checked<String> {
    let item = pad
        .entry(id)
        .and_then(Entry::outcome)
        .and_then(Outcome::item)
        .ok_or("no result")?;
    match &item.content {
        Content::Value(text) => Ok(text.clone()),
        other => Err(format!("not a value: {other:?}").into()),
    }
}
