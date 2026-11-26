//! Bounded exhaustive enumeration of event orders on the pure core and a
//! region: in no order does an older result replace a newer one in the
//! region's current result slot, every delivered result goes exactly one
//! way, and every job has exactly one terminal state.
//!
//! Two parts, each bounded by the named constants below:
//!
//! - Version changes: 1 to `VERSION_JOBS` jobs, each submitted at the
//!   region's versions of that moment, and at most `SOURCE_CHANGES`,
//!   `CONTEXT_CHANGES` and `SETTINGS_CHANGES` changes of the three
//!   versions anywhere. Each job's report, `Started` and a completed
//!   `Finished` together, comes anywhere after its submission, in any
//!   order across jobs.
//! - Cancellation races: 1 to `RACE_JOBS` jobs with separate `Started` and
//!   `Finished` reports, at most `CANCELS_PER_JOB` cancel requests per job
//!   anywhere after its submission, and at most `RACE_SOURCE_CHANGES`
//!   source edits. A `Finished` that comes after a cancel request of its
//!   job is either a completion or a stop for the cancellation, as when the
//!   backend saw the cancellation in time or did not.
//!
//! After every event the region takes every terminal state and judges its
//! result. The test prints the numbers of orders and cases, of each
//! judgement and of each terminal state with `--nocapture`, and checks each
//! of them against a constant. The inputs of the version-change part end
//! every job completed, and those of the race part end every job completed
//! or cancelled, so the other terminal states count 0.

mod support;

use kosmo_jobs::machine::Machine;
use kosmo_jobs::{
    CancelAnswer, Finished, JobId, JobState, Report, SessionId, StopReason, Terminal, TerminalState,
};
use kosmo_math_model::version::Versions;
use support::{Checked, Region, eval, value};

/// The most jobs of the version-change part.
const VERSION_JOBS: usize = 3;
/// The most source edits of the version-change part.
const SOURCE_CHANGES: usize = 1;
/// The most context changes of the version-change part.
const CONTEXT_CHANGES: usize = 1;
/// The most settings changes of the version-change part.
const SETTINGS_CHANGES: usize = 1;
/// The most jobs of the race part.
const RACE_JOBS: usize = 2;
/// The most cancel requests of each job of the race part.
const CANCELS_PER_JOB: usize = 1;
/// The most source edits of the race part.
const RACE_SOURCE_CHANGES: usize = 1;

/// The orders of the version-change part for 1, 2 and 3 jobs.
const VERSION_ORDERS: [usize; VERSION_JOBS] = [106, 948, 10_410];
/// The orders of the race part for 1 and 2 jobs.
const RACE_ORDERS: [usize; RACE_JOBS] = [23, 4_175];
/// The cases of the race part, orders times the two kinds of a `Finished`
/// after a cancel request, for 1 and 2 jobs.
const RACE_CASES: [usize; RACE_JOBS] = [35, 10_405];

/// The judgements of the version-change part: current, superseded, stale.
const VERSION_JUDGEMENTS: [usize; 3] = [10_231, 4_185, 18_816];
/// The terminal states of the version-change part: completed, failed,
/// cancelled, resource exhausted.
const VERSION_TERMINALS: [usize; 4] = [33_232, 0, 0, 0];
/// The judgements of the race part: current, superseded, stale.
const RACE_JUDGEMENTS: [usize; 3] = [2_159, 1_256, 2_146];
/// The terminal states of the race part: completed, failed, cancelled,
/// resource exhausted.
const RACE_TERMINALS: [usize; 4] = [5_561, 0, 15_284, 0];

/// What the enumeration counted.
#[derive(Debug, Default)]
struct Counts {
    /// Orders, and cases with the kinds of `Finished`, by number of jobs.
    orders: Vec<usize>,
    cases: Vec<usize>,
    /// Orders in which an older result replaced a newer one or a result
    /// that was not current reached the slot.
    replaced: usize,
    /// Cases that broke another rule, with a description of the first.
    broken: usize,
    first_broken: Option<String>,
    /// Judgements: current, superseded, stale.
    judgements: [usize; 3],
    /// Terminal states: completed, failed, cancelled, resource exhausted.
    terminals: [usize; 4],
}

impl Counts {
    fn new(jobs: usize) -> Counts {
        Counts {
            orders: vec![0; jobs],
            cases: vec![0; jobs],
            ..Counts::default()
        }
    }

    fn broke(&mut self, what: String) {
        self.broken += 1;
        if self.first_broken.is_none() {
            self.first_broken = Some(what);
        }
    }
}

fn terminal_index(state: TerminalState) -> usize {
    match state {
        TerminalState::Completed => 0,
        TerminalState::Failed => 1,
        TerminalState::Cancelled => 2,
        _ => 3,
    }
}

/// One job of an enumeration.
#[derive(Clone, Debug, Default)]
struct Job {
    id: Option<JobId>,
    versions: Option<Versions>,
    started: bool,
    finished: bool,
    cancels_left: usize,
    /// The core accepted a cancel request of the job.
    cancel_accepted: bool,
    /// A cancel request came after the job's `Finished` was handled.
    cancel_after_finish: bool,
    /// A cancel request was answered other than as expected.
    wrong_answer: Option<CancelAnswer>,
    /// A cancel request came before the job's `Finished`.
    cancel_before_finish: bool,
    terminals: Vec<Terminal>,
}

/// The state of one enumeration: the core, the region and the jobs.
#[derive(Clone, Debug)]
struct World {
    core: Machine,
    region: Region,
    session: SessionId,
    jobs: Vec<Job>,
    submitted: usize,
    /// Changes left of the source, the context and the settings.
    changes_left: [usize; 3],
    /// Whether some `Finished` was a stop for the cancellation.
    stopped_kind: bool,
    /// What went wrong in this order, if anything.
    broken: Option<String>,
}

impl World {
    fn new(jobs: usize, changes: [usize; 3], cancels: &[usize]) -> Checked<World> {
        let mut core = Machine::new();
        let session = core.open_session()?;
        let jobs = (0..jobs)
            .map(|j| Job {
                cancels_left: cancels.get(j).copied().unwrap_or(0),
                ..Job::default()
            })
            .collect();
        Ok(World {
            core,
            region: Region::new()?,
            session,
            jobs,
            submitted: 0,
            changes_left: changes,
            stopped_kind: false,
            broken: None,
        })
    }

    fn broke(&mut self, what: String) {
        if self.broken.is_none() {
            self.broken = Some(what);
        }
    }

    fn submit(&mut self) {
        let j = self.submitted;
        self.submitted += 1;
        let versions = self.region.versions();
        let (accepted, _) = self.core.submit(eval(self.session, versions, "1"), None);
        match (accepted, self.jobs.get_mut(j)) {
            (Ok(id), Some(job)) => {
                job.id = Some(id);
                job.versions = Some(versions);
                self.region.accepted(id);
            }
            (other, _) => self.broke(format!("submission {j} gave {other:?}")),
        }
    }

    fn change(&mut self, kind: usize) -> Checked {
        if let Some(left) = self.changes_left.get_mut(kind) {
            *left -= 1;
        }
        match kind {
            0 => self.region.edit_source()?,
            1 => self.region.change_context(),
            _ => self.region.change_settings(),
        }
        Ok(())
    }

    fn started(&mut self, j: usize) {
        if let Some(job) = self.jobs.get_mut(j) {
            job.started = true;
            if let Some(id) = job.id {
                self.core.report(Report::Started(id));
            }
        }
    }

    /// The job's `Finished`: a completion with a value line, or, when
    /// `stopped`, a stop for the cancellation before the first request.
    fn finished(&mut self, j: usize, stopped: bool) {
        let Some(job) = self.jobs.get_mut(j) else {
            return;
        };
        job.finished = true;
        let (Some(id), Some(versions)) = (job.id, job.versions) else {
            return;
        };
        let finished = if stopped {
            self.stopped_kind = true;
            Finished::new(id, Vec::new(), Some(StopReason::Cancellation))
        } else {
            Finished::new(id, vec![value(versions, "1")], None)
        };
        self.core.report(Report::Finished(finished));
    }

    fn cancel(&mut self, j: usize) {
        let Some(job) = self.jobs.get_mut(j) else {
            return;
        };
        job.cancels_left -= 1;
        let Some(id) = job.id else {
            return;
        };
        let terminal_before = !job.terminals.is_empty()
            || matches!(self.core.state(id), None | Some(JobState::Terminal(_)));
        let expected = if job.cancel_accepted {
            if terminal_before {
                CancelAnswer::AlreadyTerminal
            } else {
                CancelAnswer::AlreadyRequested
            }
        } else if job.finished {
            CancelAnswer::AlreadyTerminal
        } else if job.started {
            CancelAnswer::Requested
        } else {
            CancelAnswer::Cancelled
        };
        let (answer, _) = self.core.cancel(id);
        if answer != expected {
            job.wrong_answer = Some(answer);
        }
        if matches!(answer, CancelAnswer::Cancelled | CancelAnswer::Requested) {
            job.cancel_accepted = true;
        }
        if job.finished {
            job.cancel_after_finish = true;
        } else {
            job.cancel_before_finish = true;
        }
    }

    /// Takes every terminal state and delivers it to the region.
    fn take(&mut self) {
        for id in self.core.ready() {
            let Some(terminal) = self.core.take(id) else {
                continue;
            };
            self.region.deliver(&terminal);
            match self.jobs.iter_mut().find(|job| job.id == Some(id)) {
                Some(job) => job.terminals.push(terminal),
                None => self.broke(format!("{id} is no job of this order")),
            }
        }
    }

    /// Checks a finished order and adds it to `counts`.
    fn check(&self, counts: &mut Counts, jobs: usize) {
        let index = jobs - 1;
        if let Some(cases) = counts.cases.get_mut(index) {
            *cases += 1;
        }
        if !self.stopped_kind
            && let Some(orders) = counts.orders.get_mut(index)
        {
            *orders += 1;
        }
        if self.region.violations > 0 {
            counts.replaced += 1;
        }
        for (place, count) in self.region.judgements.iter().enumerate() {
            if let Some(total) = counts.judgements.get_mut(place) {
                *total += count;
            }
        }
        if let Some(what) = &self.broken {
            counts.broke(what.clone());
        }
        if !self.region.conserved() {
            counts.broke(format!("results not conserved: {:?}", self.region));
        }
        if self.core.live() != 0 {
            counts.broke(format!("{} jobs still live", self.core.live()));
        }
        for (j, job) in self.jobs.iter().enumerate() {
            let [terminal] = job.terminals.as_slice() else {
                counts.broke(format!(
                    "job {j} has {} terminal states",
                    job.terminals.len()
                ));
                continue;
            };
            let record = terminal.record();
            if let Some(total) = counts.terminals.get_mut(terminal_index(record.state())) {
                *total += 1;
            }
            let expected = if job.cancel_accepted {
                TerminalState::Cancelled
            } else {
                TerminalState::Completed
            };
            if record.state() != expected {
                counts.broke(format!("job {j} ended {:?}", record.state()));
            }
            if job.cancel_accepted && !record.lines().is_empty() {
                counts.broke(format!("cancelled job {j} delivered lines"));
            }
            if let Some(answer) = job.wrong_answer {
                counts.broke(format!("job {j} got the cancel answer {answer:?}"));
            }
            if job.cancel_after_finish && job.cancel_accepted {
                counts.broke(format!("job {j} was cancelled after its end"));
            }
            let delivered = job.id.and_then(|id| self.region.record(id));
            if job.cancel_accepted && delivered.is_some() {
                counts.broke(format!("cancelled job {j} delivered a result"));
            }
        }
    }
}

/// Visits every order of the version-change part from `world`.
fn versions_part(world: World, jobs: usize, counts: &mut Counts) -> Checked {
    let mut leaf = true;
    if world.submitted < jobs {
        leaf = false;
        let mut next = world.clone();
        next.submit();
        next.take();
        versions_part(next, jobs, counts)?;
    }
    for j in 0..world.submitted {
        if world.jobs.get(j).is_some_and(|job| !job.finished) {
            leaf = false;
            let mut next = world.clone();
            next.started(j);
            next.finished(j, false);
            next.take();
            versions_part(next, jobs, counts)?;
        }
    }
    for kind in 0..3 {
        if world.changes_left.get(kind).is_some_and(|left| *left > 0) {
            leaf = false;
            let mut next = world.clone();
            next.change(kind)?;
            next.take();
            versions_part(next, jobs, counts)?;
        }
    }
    if leaf {
        world.check(counts, jobs);
    }
    Ok(())
}

/// Visits every order and case of the race part from `world`.
fn race_part(world: World, jobs: usize, counts: &mut Counts) -> Checked {
    let mut leaf = true;
    if world.submitted < jobs {
        leaf = false;
        let mut next = world.clone();
        next.submit();
        next.take();
        race_part(next, jobs, counts)?;
    }
    for j in 0..world.submitted {
        let Some(job) = world.jobs.get(j) else {
            continue;
        };
        if !job.started {
            leaf = false;
            let mut next = world.clone();
            next.started(j);
            next.take();
            race_part(next, jobs, counts)?;
        } else if !job.finished {
            leaf = false;
            let kinds: &[bool] = if job.cancel_before_finish {
                &[false, true]
            } else {
                &[false]
            };
            for &stopped in kinds {
                let mut next = world.clone();
                next.finished(j, stopped);
                next.take();
                race_part(next, jobs, counts)?;
            }
        }
        if job.cancels_left > 0 {
            leaf = false;
            let mut next = world.clone();
            next.cancel(j);
            next.take();
            race_part(next, jobs, counts)?;
        }
    }
    if world.changes_left.first().is_some_and(|left| *left > 0) {
        leaf = false;
        let mut next = world.clone();
        next.change(0)?;
        next.take();
        race_part(next, jobs, counts)?;
    }
    if leaf {
        world.check(counts, jobs);
    }
    Ok(())
}

/// Every way to choose a number from `0..=most` for each of `places`.
fn choices(places: usize, most: usize) -> Vec<Vec<usize>> {
    let mut all = vec![Vec::new()];
    for _ in 0..places {
        all = all
            .into_iter()
            .flat_map(|chosen| {
                (0..=most).map(move |n| {
                    let mut more = chosen.clone();
                    more.push(n);
                    more
                })
            })
            .collect();
    }
    all
}

#[test]
fn no_bounded_order_lets_an_older_result_replace_a_newer_one() -> Checked {
    let mut versions = Counts::new(VERSION_JOBS);
    for jobs in 1..=VERSION_JOBS {
        for source in 0..=SOURCE_CHANGES {
            for context in 0..=CONTEXT_CHANGES {
                for settings in 0..=SETTINGS_CHANGES {
                    let world = World::new(jobs, [source, context, settings], &[])?;
                    versions_part(world, jobs, &mut versions)?;
                }
            }
        }
    }

    let mut race = Counts::new(RACE_JOBS);
    for jobs in 1..=RACE_JOBS {
        for cancels in choices(jobs, CANCELS_PER_JOB) {
            for source in 0..=RACE_SOURCE_CHANGES {
                let world = World::new(jobs, [source, 0, 0], &cancels)?;
                race_part(world, jobs, &mut race)?;
            }
        }
    }

    println!(
        "version changes: orders {:?} (total {}), judgements current/superseded/stale {:?}, \
         terminal states completed/failed/cancelled/resource_exhausted {:?}",
        versions.orders,
        versions.orders.iter().sum::<usize>(),
        versions.judgements,
        versions.terminals
    );
    println!(
        "cancellation races: orders {:?} (total {}), cases {:?} (total {}), judgements \
         current/superseded/stale {:?}, terminal states \
         completed/failed/cancelled/resource_exhausted {:?}",
        race.orders,
        race.orders.iter().sum::<usize>(),
        race.cases,
        race.cases.iter().sum::<usize>(),
        race.judgements,
        race.terminals
    );

    for counts in [&versions, &race] {
        assert_eq!(counts.replaced, 0, "an older result replaced a newer one");
        assert_eq!(counts.broken, 0, "first: {:?}", counts.first_broken);
        assert!(counts.judgements.iter().all(|n| *n > 0), "{counts:?}");
    }
    assert_eq!(versions.orders, VERSION_ORDERS);
    assert_eq!(versions.cases, VERSION_ORDERS);
    assert_eq!(race.orders, RACE_ORDERS);
    assert_eq!(race.cases, RACE_CASES);
    assert_eq!(versions.judgements, VERSION_JUDGEMENTS);
    assert_eq!(race.judgements, RACE_JUDGEMENTS);
    // Only completions reach the version-change part; the race part ends
    // jobs completed or cancelled.
    assert_eq!(versions.terminals, VERSION_TERMINALS);
    assert_eq!(race.terminals, RACE_TERMINALS);
    Ok(())
}
