//! An older job of a region whose result arrives after or before the
//! result of a newer job: the older result never replaces the newer one.

mod support;

use kosmo_jobs::controllable::{ControllableBackend, Gate, Plan, Release};
use kosmo_jobs::{JobId, JobLayer};
use kosmo_math_model::version::Judgement;
use support::{Checked, Held, Region, controllable, eval, value};

/// A job layer, its region and the region's two jobs.
type TwoJobs = (JobLayer<ControllableBackend>, Region, [JobId; 2]);

/// A region with two jobs on a controllable backend: the first submitted
/// at source version 1, the second after an edit at source version 2. Both
/// wait before their `Finished` reports.
fn two_jobs() -> Checked<TwoJobs> {
    let mut region = Region::new()?;
    let old = region.versions();
    let mut new = old;
    new.source = support::source(2)?;
    let mut backend = ControllableBackend::new();
    for (order, tagged) in [(0, old), (1, new)] {
        backend.plan(
            order,
            Plan::new()
                .request(value(tagged, "1/2"))
                .gate(Gate::Finished, Release::Manual),
        );
    }
    let mut layer = controllable(backend);
    let session = layer.open_session()?;
    let first = layer.submit(eval(session, region.versions(), "1/3+1/6"))?;
    region.accepted(first);
    region.edit_source()?;
    assert_eq!(region.versions(), new);
    let second = layer.submit(eval(session, region.versions(), "1/3+1/6"))?;
    region.accepted(second);
    Ok((layer, region, [first, second]))
}

/// Lets the job of submission order `order` finish and delivers its
/// terminal state to the region.
fn finish(
    layer: &mut JobLayer<ControllableBackend>,
    region: &mut Region,
    order: usize,
    id: JobId,
) -> Checked<Option<Judgement>> {
    layer.backend_mut().release(order, Gate::Finished);
    assert_eq!(layer.poll(), vec![id]);
    let terminal = layer.take(id).ok_or("no terminal state")?;
    Ok(region.deliver(&terminal))
}

#[test]
fn a_late_result_of_an_older_version_goes_only_to_its_record() -> Checked {
    let (mut layer, mut region, [first, second]) = two_jobs()?;
    let newer = Held {
        id: second,
        versions: region.versions(),
    };

    assert_eq!(
        finish(&mut layer, &mut region, 1, second)?,
        Some(Judgement::Current)
    );
    assert_eq!(region.slot(), Some(newer));
    assert_eq!(region.record(second), Some(newer));

    assert_eq!(
        finish(&mut layer, &mut region, 0, first)?,
        Some(Judgement::Stale)
    );
    assert_eq!(region.slot(), Some(newer));
    assert_eq!(region.record(first).map(|held| held.id), Some(first));
    assert_eq!((region.into_slot, region.record_only), (1, 1));
    assert_eq!(region.violations, 0);
    assert!(region.conserved());
    Ok(())
}

#[test]
fn an_early_result_of_an_older_version_goes_only_to_its_record() -> Checked {
    let (mut layer, mut region, [first, second]) = two_jobs()?;

    // When the first result arrives, the region is already at the newer
    // version and the second job is its latest submission.
    assert_eq!(
        finish(&mut layer, &mut region, 0, first)?,
        Some(Judgement::Stale)
    );
    assert_eq!(region.slot(), None);
    assert_eq!(region.record(first).map(|held| held.id), Some(first));

    assert_eq!(
        finish(&mut layer, &mut region, 1, second)?,
        Some(Judgement::Current)
    );
    assert_eq!(region.slot().map(|held| held.id), Some(second));
    assert_eq!(region.record(second).map(|held| held.id), Some(second));
    assert_eq!((region.into_slot, region.record_only), (1, 1));
    assert_eq!(region.violations, 0);
    assert!(region.conserved());
    Ok(())
}
