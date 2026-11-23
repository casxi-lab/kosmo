//! Helpers of the unit tests: a job layer on the controllable backend, with
//! a clock that stands still, and the reports of the jobs it runs.

use std::error::Error;

use kosmo_jobs::controllable::{ControllableBackend, Plan};
use kosmo_jobs::{
    EchoedVersions, ErrorClass, JobClock, JobId, JobLayer, Limits, Operation, OutcomeKind,
    RequestReport, SessionId, Terminal,
};
use kosmo_math_model::version::Versions;

use crate::Scratchpad;

/// The result of a helper or a test.
pub(crate) type Checked<T = ()> = Result<T, Box<dyn Error>>;

/// A clock that always reads 0.
struct Still;

impl JobClock for Still {
    fn now(&mut self) -> u64 {
        0
    }

    fn fork(&self) -> Box<dyn JobClock> {
        Box::new(Still)
    }
}

/// A job layer on the controllable backend with one open session.
pub(crate) struct Jobs {
    pub(crate) layer: JobLayer<ControllableBackend>,
    pub(crate) session: SessionId,
}

impl Jobs {
    pub(crate) fn new() -> Checked<Jobs> {
        let mut layer = JobLayer::new(ControllableBackend::new(), Box::new(Still));
        let session = layer.open_session()?;
        Ok(Jobs { layer, session })
    }

    /// Prepares the current line of `pad` as an `eval`, gives its job the
    /// plan `plan`, submits it and accepts the answer.
    pub(crate) fn submit(&mut self, pad: &mut Scratchpad, plan: Plan) -> Checked<JobId> {
        let (submission, pending) =
            pad.prepare(self.session, Operation::Eval, "", Limits::new(), None)?;
        let order = self.layer.backend().received();
        self.layer.controllable().plan(order, plan);
        Ok(pad.accept(pending, self.layer.submit(submission))?)
    }

    /// Waits for job `id` and takes its terminal state.
    pub(crate) fn wait(&mut self, id: JobId) -> Checked<Terminal> {
        Ok(self.layer.wait(id).ok_or("no terminal state")?)
    }
}

/// The report of a request of the kind `kind` whose output line is the
/// fields `fields` after the echo of `versions`, ended by a line feed.
pub(crate) fn report(kind: OutcomeKind, versions: Versions, fields: &str) -> RequestReport {
    let tag = match kind {
        OutcomeKind::Value => "value",
        OutcomeKind::State => "state",
        _ => "error",
    };
    let error = match kind {
        OutcomeKind::Value | OutcomeKind::State => ErrorClass::NoError,
        _ => ErrorClass::Outcome,
    };
    let line = format!(
        "{tag}\t{}\t{}\t{}\t{fields}\n",
        versions.source.get(),
        versions.context.get(),
        versions.settings.get()
    );
    RequestReport::new(
        kind,
        error,
        Some(line.into_bytes()),
        EchoedVersions::from(versions),
    )
}

/// A plan of one request whose value has the display text `text`.
pub(crate) fn value(versions: Versions, text: &str) -> Plan {
    Plan::new().request(report(OutcomeKind::Value, versions, text))
}
