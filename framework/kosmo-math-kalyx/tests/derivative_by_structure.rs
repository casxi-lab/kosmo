//! The derivative of `x^3` with respect to `x` on the job layer's host stub
//! with the kalyx runner, for a line typed one character at a time as `x^2`
//! and changed to `x^3`: its output line is the one that the Scratchpad's
//! test of a resubmitted input checks byte for byte, and kalyx 0.1.0 reads
//! its value as a term equal by structure to `3*x^2`. The Scratchpad does
//! not depend on kalyx, so the comparison by structure is made here.

mod support;

use kosmo_jobs::{Operation, Request, Submission, TerminalState};
use kosmo_math_kalyx::{convert, read_output_line};
use kosmo_math_model::EditBuffer;
use kosmo_math_model::display::Content;
use kosmo_math_model::version::Versions;
use support::jobs::KalyxJobs;
use support::{Checked, equal_terms};

#[test]
fn the_derivative_of_x_cubed_equals_three_x_squared_by_structure() -> Checked {
    let mut buffer = EditBuffer::new();
    for c in "x^2".chars() {
        buffer.insert_char(c)?;
    }
    buffer.delete_backward()?;
    buffer.insert_char('3')?;
    let snapshot = buffer.snapshot();
    let versions = Versions {
        source: snapshot.version(),
        ..Versions::default()
    };
    assert_eq!(versions.source.get(), 5);

    let mut jobs = KalyxJobs::new()?;
    let session = jobs.layer.open_session()?;
    let request = Request::new(Operation::Diff, "x", convert(&snapshot)?);
    let id = jobs
        .layer
        .submit(Submission::new(session, versions, request))?;
    let terminal = jobs.layer.wait(id).ok_or("no terminal state")?;
    assert_eq!(terminal.record().state(), TerminalState::Completed);
    let [line] = terminal.record().lines() else {
        return Err("not one line".into());
    };
    assert_eq!(line.as_slice(), b"value\t5\t0\t0\t3*x^2\n");

    let item = read_output_line(line, versions, &snapshot);
    let Content::Value(text) = &item.content else {
        return Err(format!("not a value: {:?}", item.content).into());
    };
    assert!(equal_terms(text, "3*x^2", kalyx::Limits::new())?);
    jobs.layer.shutdown();
    assert_eq!(jobs.layer.backend().inner().live_threads(), 0);
    Ok(())
}
