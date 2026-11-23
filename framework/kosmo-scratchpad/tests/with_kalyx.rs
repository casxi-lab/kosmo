//! The Scratchpad on the job layer's host stub with the kalyx runner, as a
//! terminal uses it: it prepares a submission, the terminal submits it,
//! hands the answer to the Scratchpad, waits for the job and delivers its
//! terminal state. The output lines that the other tests give the
//! controllable backend are those that kalyx 0.1.0 writes here, and a value
//! longer than one insertion is not reused.
//!
//! Each test shuts its job layer down and checks that the host stub's
//! worker thread has ended.

mod support;

use kosmo_jobs::stub::HostStub;
use kosmo_jobs::{JobClock, JobId, JobLayer, Limits, Operation, SessionId};
use kosmo_math_kalyx::{KalyxPreparer, KalyxRunner, NotReusable};
use kosmo_math_model::Rejected;
use kosmo_math_model::version::{ContextVersion, SettingsVersion};
use kosmo_scratchpad::{Destination, NotReused, Part, Scratchpad};
use support::{
    Checked, DERIVATIVE, DIVISION_BY_ZERO, EXPANDED, NEWER, OLDER, SUM, Still, ones, text, value_of,
};

type Layer = JobLayer<HostStub<KalyxPreparer>>;

/// A job layer on the host stub with the kalyx runner, whose clock reads
/// 0, and a session of it.
fn kalyx() -> Checked<(Layer, SessionId)> {
    let clock = Still;
    let stub = HostStub::new(KalyxPreparer::new(), KalyxRunner::new(clock.fork()))?;
    let mut layer = JobLayer::new(stub, Box::new(clock));
    let session = layer.open_session()?;
    Ok((layer, session))
}

/// Submits the current line of `pad` with `operation` and `variable` as a
/// terminal does, waits for its job and delivers it. Gives the job and its
/// output line without the final line feed.
fn run(
    layer: &mut Layer,
    session: SessionId,
    pad: &mut Scratchpad,
    operation: Operation,
    variable: &str,
) -> Checked<(JobId, String)> {
    let (submission, pending) = pad.prepare(session, operation, variable, Limits::new(), None)?;
    let id = pad.accept(pending, layer.submit(submission))?;
    let terminal = layer.wait(id).ok_or("no terminal state")?;
    let [line] = terminal.record().lines() else {
        return Err("not one line".into());
    };
    let line = std::str::from_utf8(line)?
        .strip_suffix('\n')
        .ok_or("no final line feed")?
        .to_string();
    assert_eq!(pad.deliver(&terminal).destination, Destination::Entry);
    Ok((id, line))
}

/// Shuts `layer` down and checks that its worker thread ended.
fn shut_down(mut layer: Layer) {
    layer.shutdown();
    assert_eq!(layer.backend().live_threads(), 0);
}

#[test]
fn the_output_lines_of_the_tests_are_those_of_kalyx() -> Checked {
    let (mut layer, session) = kalyx()?;
    let typed = [
        (Operation::Eval, "", "1/3+1/6", SUM),
        (Operation::Expand, "x", "(x+1)^2", EXPANDED),
        (Operation::Diff, "x", "x^2", DERIVATIVE),
        (Operation::Eval, "", "1/0", DIVISION_BY_ZERO),
    ];
    for (operation, variable, input, expected) in typed {
        let mut pad = Scratchpad::new();
        pad.insert_text(input)?;
        let (_, line) = run(&mut layer, session, &mut pad, operation, variable)?;
        assert_eq!(line, expected, "{input}");
    }

    let mut pad = Scratchpad::with_versions(ContextVersion::new(0), SettingsVersion::new(1));
    pad.insert_text("1/3+1/6")?;
    assert_eq!(
        run(&mut layer, session, &mut pad, Operation::Eval, "")?.1,
        OLDER
    );
    pad.insert_char('0')?;
    assert_eq!(text(&pad), "1/3+1/60");
    assert_eq!(
        run(&mut layer, session, &mut pad, Operation::Eval, "")?.1,
        NEWER
    );

    for k in [1, 2, 33] {
        let mut pad = Scratchpad::new();
        for _ in 0..k {
            pad.insert_char('1')?;
        }
        assert_eq!(
            run(&mut layer, session, &mut pad, Operation::Eval, "")?.1,
            ones(k)
        );
    }
    shut_down(layer);
    Ok(())
}

#[test]
fn a_value_longer_than_one_insertion_is_not_reused() -> Checked {
    let (mut layer, session) = kalyx()?;
    let mut pad = Scratchpad::new();
    pad.insert_text("(x+1)^128")?;
    let (id, line) = run(&mut layer, session, &mut pad, Operation::Diff, "x")?;
    let value = value_of(&pad, id)?;
    assert_eq!(value.len(), 4_749);
    assert_eq!(line, format!("value\t1\t0\t0\t{value}"));

    pad.clear()?;
    pad.insert_text("1/")?;
    let before = pad.clone();
    assert_eq!(
        pad.reuse(id, Part::Result, 0..1),
        Err(NotReused::NotReadBack(NotReusable::Rejected(
            Rejected::InsertTooLong
        )))
    );
    assert_eq!(pad, before);
    shut_down(layer);
    Ok(())
}
