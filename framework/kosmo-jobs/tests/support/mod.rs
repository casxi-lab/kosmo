//! Helpers shared by the integration tests: versions, requests and reports,
//! a clock that stands still, and a model of a region that receives
//! results.

#![allow(dead_code, reason = "each test file uses some of the helpers")]

use std::collections::BTreeMap;
use std::error::Error;

use kosmo_jobs::controllable::ControllableBackend;
use kosmo_jobs::{
    ErrorClass, JobClock, JobId, JobLayer, Operation, OutcomeKind, Request, RequestReport,
    SessionId, Submission, Terminal, TerminalState,
};
use kosmo_math_model::EditBuffer;
use kosmo_math_model::version::{
    ContextVersion, Judgement, SettingsVersion, SourceVersion, Versions, judge,
};

/// The result of a helper or a test.
pub type Checked<T = ()> = Result<T, Box<dyn Error>>;

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

/// The source version `n`, which `n` edits of a line give.
pub fn source(n: u64) -> Checked<SourceVersion> {
    let mut buffer = EditBuffer::new();
    for _ in 0..n {
        buffer.insert_char('1')?;
    }
    Ok(buffer.version())
}

/// The versions with the source version `source_version`, the context
/// version `context` and the settings version `settings`.
pub fn versions(source_version: u64, context: u64, settings: u64) -> Checked<Versions> {
    Ok(Versions {
        source: source(source_version)?,
        context: ContextVersion::new(context),
        settings: SettingsVersion::new(settings),
    })
}

/// A job of one `eval` request of `text`.
pub fn eval(session: SessionId, versions: Versions, text: &str) -> Submission {
    Submission::new(session, versions, Request::new(Operation::Eval, "", text))
}

/// An output line of the tag `tag` with the echoed `versions` and the
/// fields `rest`, ended by a line feed.
pub fn line(tag: &str, versions: Versions, rest: &str) -> Vec<u8> {
    format!(
        "{tag}\t{}\t{}\t{}\t{rest}\n",
        versions.source.get(),
        versions.context.get(),
        versions.settings.get()
    )
    .into_bytes()
}

/// The report of a request whose value has the display text `text`.
pub fn value(versions: Versions, text: &str) -> RequestReport {
    RequestReport::new(
        OutcomeKind::Value,
        ErrorClass::NoError,
        Some(line("value", versions, text)),
        versions,
    )
}

/// A job layer on a controllable backend that has received no job.
pub fn controllable(backend: ControllableBackend) -> JobLayer<ControllableBackend> {
    JobLayer::new(backend, Box::new(Still))
}

/// The lines of a terminal state as text.
pub fn texts(terminal: &Terminal) -> Vec<String> {
    terminal
        .record()
        .lines()
        .iter()
        .map(|line| String::from_utf8_lossy(line).into_owned())
        .collect()
}

/// A result as a region holds it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Held {
    /// The submission whose result it is.
    pub id: JobId,
    /// The versions that the result echoed.
    pub versions: Versions,
}

/// A region that submits jobs and receives their results: its current
/// versions, its latest accepted submission, its current result slot, a
/// record for each submission and its discard count. It judges each result
/// with the job layer's judgement and counts what it did.
#[derive(Clone, Debug)]
pub struct Region {
    buffer: EditBuffer,
    context: ContextVersion,
    settings: SettingsVersion,
    latest: Option<JobId>,
    slot: Option<Held>,
    records: BTreeMap<JobId, Option<Held>>,
    /// Results delivered to the region.
    pub delivered: usize,
    /// Results written into the slot and their record.
    pub into_slot: usize,
    /// Results written only into their record.
    pub record_only: usize,
    /// Results counted toward the discard count.
    pub discarded: usize,
    /// Deliveries that broke a rule of the slot: an older result replaced
    /// a newer one, a result that is not current reached the slot, or a
    /// record received a second result.
    pub violations: usize,
    /// The judgements given, as current, superseded and stale.
    pub judgements: [usize; 3],
}

impl Region {
    /// A region whose line holds one character, so its source version is
    /// 1, with context and settings version 0.
    pub fn new() -> Checked<Region> {
        let mut buffer = EditBuffer::new();
        buffer.insert_char('1')?;
        Ok(Region {
            buffer,
            context: ContextVersion::new(0),
            settings: SettingsVersion::new(0),
            latest: None,
            slot: None,
            records: BTreeMap::new(),
            delivered: 0,
            into_slot: 0,
            record_only: 0,
            discarded: 0,
            violations: 0,
            judgements: [0; 3],
        })
    }

    /// The region's current versions.
    pub fn versions(&self) -> Versions {
        Versions {
            source: self.buffer.version(),
            context: self.context,
            settings: self.settings,
        }
    }

    /// An edit that changes the source.
    pub fn edit_source(&mut self) -> Checked {
        self.buffer.insert_char('2')?;
        Ok(())
    }

    /// A change of the context.
    pub fn change_context(&mut self) {
        self.context = ContextVersion::new(self.context.get() + 1);
    }

    /// A change of a semantic setting.
    pub fn change_settings(&mut self) {
        self.settings = SettingsVersion::new(self.settings.get() + 1);
    }

    /// The job layer accepted submission `id` for this region.
    pub fn accepted(&mut self, id: JobId) {
        self.latest = Some(id);
        self.records.insert(id, None);
    }

    /// Receives a terminal state. A cancelled job has no result; any other
    /// is judged and goes into the slot and its record, only its record, or
    /// the discard count. Returns the judgement of a result.
    pub fn deliver(&mut self, terminal: &Terminal) -> Option<Judgement> {
        if terminal.record().state() == TerminalState::Cancelled {
            return None;
        }
        let held = Held {
            id: terminal.id(),
            versions: terminal.record().echoed().unwrap_or(terminal.versions()),
        };
        self.delivered += 1;
        let judgement = judge(
            held.versions,
            &held.id,
            self.versions(),
            self.latest.as_ref(),
        );
        let index = match judgement {
            Judgement::Current => 0,
            Judgement::Superseded => 1,
            Judgement::Stale => 2,
        };
        self.judgements[index] += 1;
        match judgement {
            Judgement::Current => {
                let older = self.slot.is_some_and(|before| before.id > held.id);
                let not_current = held.versions != self.versions() || self.latest != Some(held.id);
                if older || not_current {
                    self.violations += 1;
                }
                self.slot = Some(held);
                if self.fill(held) {
                    self.into_slot += 1;
                } else {
                    self.discarded += 1;
                }
            }
            Judgement::Superseded | Judgement::Stale => {
                if self.fill(held) {
                    self.record_only += 1;
                } else {
                    self.discarded += 1;
                }
            }
        }
        Some(judgement)
    }

    /// Writes `held` into its record; false when the region keeps no
    /// record for it.
    fn fill(&mut self, held: Held) -> bool {
        match self.records.get_mut(&held.id) {
            Some(record) => {
                if record.is_some() {
                    self.violations += 1;
                }
                *record = Some(held);
                true
            }
            None => false,
        }
    }

    /// The result in the current result slot.
    pub fn slot(&self) -> Option<Held> {
        self.slot
    }

    /// The result in the record of submission `id`.
    pub fn record(&self, id: JobId) -> Option<Held> {
        self.records.get(&id).copied().flatten()
    }

    /// Whether every delivered result went exactly one way: into the slot
    /// and its record, only into its record, or into the discard count; and
    /// whether the slot holds a result that its record holds too.
    pub fn conserved(&self) -> bool {
        let filled = self.records.values().filter(|r| r.is_some()).count();
        let slot_recorded = self
            .slot
            .is_none_or(|held| self.record(held.id) == Some(held));
        self.delivered == self.into_slot + self.record_only + self.discarded
            && filled == self.into_slot + self.record_only
            && slot_recorded
    }
}
