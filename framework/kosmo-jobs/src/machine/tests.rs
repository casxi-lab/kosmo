use kosmo_math_model::version::{ContextVersion, SettingsVersion};

use super::*;
use crate::{ErrorClass, Operation, OutcomeKind, Request, RequestReport, SerializeFailure};

fn versions() -> Versions {
    Versions {
        context: ContextVersion::new(2),
        settings: SettingsVersion::new(3),
        ..Versions::default()
    }
}

fn other_versions() -> Versions {
    Versions {
        settings: SettingsVersion::new(4),
        ..versions()
    }
}

fn line(text: &str) -> Option<Vec<u8>> {
    Some(text.as_bytes().to_vec())
}

fn report(kind: OutcomeKind, error: ErrorClass, text: Option<&str>) -> RequestReport {
    RequestReport::new(kind, error, text.and_then(line), versions())
}

fn value() -> RequestReport {
    report(OutcomeKind::Value, ErrorClass::NoError, Some("value"))
}

fn fault() -> RequestReport {
    report(OutcomeKind::Fault, ErrorClass::Outcome, Some("fault"))
}

fn end(requests: Vec<RequestReport>, stop: Option<StopReason>) -> Finished {
    Finished::new(JobId::new(1), requests, stop)
}

fn lines(record: &TerminalRecord) -> Vec<&str> {
    record
        .lines()
        .iter()
        .map(|l| std::str::from_utf8(l).unwrap())
        .collect()
}

/// The terminal state, stop and lines of a job outside `cancel_requested`.
type Outside = (TerminalState, Option<StopReason>, Vec<String>);

/// The terminal state and lines of a job inside `cancel_requested`.
type Inside = (TerminalState, Vec<String>);

/// The terminal records of a job of `requests` requests outside and inside
/// `cancel_requested`.
fn both(requests: usize, finished: &Finished) -> (Outside, Inside) {
    let outside = terminal(false, versions(), requests, finished.clone());
    let inside = terminal(true, versions(), requests, finished.clone());
    let owned = |r: &TerminalRecord| lines(r).into_iter().map(String::from).collect();
    (
        (outside.state(), outside.stop(), owned(&outside)),
        (inside.state(), owned(&inside)),
    )
}

#[test]
fn every_row_of_the_outcome_table_maps_to_its_terminal_state() {
    let rows = [
        (value(), TerminalState::Completed, None, vec!["value"]),
        (
            report(OutcomeKind::State, ErrorClass::NoError, Some("state")),
            TerminalState::Completed,
            None,
            vec!["state"],
        ),
        (fault(), TerminalState::Failed, None, vec!["fault"]),
        (
            report(
                OutcomeKind::ResourceLimit,
                ErrorClass::Serialize(SerializeFailure::ResourceLimit),
                Some("fallback"),
            ),
            TerminalState::Failed,
            None,
            vec!["fallback"],
        ),
        (
            report(
                OutcomeKind::DeadlineExceeded,
                ErrorClass::Serialize(SerializeFailure::DeadlineExceeded),
                Some("fallback"),
            ),
            TerminalState::ResourceExhausted,
            Some(StopReason::Deadline),
            vec!["fallback"],
        ),
        (
            report(
                OutcomeKind::Value,
                ErrorClass::Serialize(SerializeFailure::ResourceLimit),
                None,
            ),
            TerminalState::Failed,
            None,
            vec![],
        ),
        (
            report(
                OutcomeKind::Value,
                ErrorClass::Serialize(SerializeFailure::DeadlineExceeded),
                None,
            ),
            TerminalState::ResourceExhausted,
            Some(StopReason::Deadline),
            vec![],
        ),
    ];
    for (row, state, stop, expected) in rows {
        let ((outside, outside_stop, outside_lines), (inside, inside_lines)) =
            both(1, &end(vec![row], None));
        assert_eq!((outside, outside_stop), (state, stop));
        assert_eq!(outside_lines, expected);
        assert_eq!(inside, TerminalState::Cancelled);
        assert!(inside_lines.is_empty());
    }
}

#[test]
fn a_cancelled_request_ends_the_job_cancelled_without_lines() {
    let cancelled = report(
        OutcomeKind::Cancelled,
        ErrorClass::Serialize(SerializeFailure::Cancelled),
        Some("fallback"),
    );
    let ((outside, stop, outside_lines), (inside, _)) =
        both(2, &end(vec![value(), cancelled], None));
    assert_eq!(
        (outside, stop),
        (TerminalState::Cancelled, Some(StopReason::Cancellation))
    );
    assert!(outside_lines.is_empty());
    assert_eq!(inside, TerminalState::Cancelled);
}

#[test]
fn stops_between_requests_and_forced_stops_end_the_job() {
    let cases = [
        (StopReason::Cancellation, TerminalState::Cancelled, false),
        (
            StopReason::Deadline,
            TerminalState::ResourceExhausted,
            false,
        ),
        (
            StopReason::ForcedStop,
            TerminalState::ResourceExhausted,
            true,
        ),
        (
            StopReason::CpuBudget,
            TerminalState::ResourceExhausted,
            true,
        ),
        (
            StopReason::BackendInternalError,
            TerminalState::Failed,
            false,
        ),
    ];
    for (stop, state, forced) in cases {
        let outside = terminal(false, versions(), 2, end(vec![fault()], Some(stop)));
        assert_eq!(
            (outside.state(), outside.stop(), outside.forced()),
            (state, Some(stop), forced)
        );
        let delivered = match state {
            TerminalState::ResourceExhausted => vec!["fault"],
            _ => vec![],
        };
        assert_eq!(lines(&outside), delivered);
        assert_eq!(
            outside.internal_error(),
            stop == StopReason::BackendInternalError
        );
        let inside = terminal(true, versions(), 2, end(vec![fault()], Some(stop)));
        assert_eq!(
            (inside.state(), inside.stop(), inside.forced()),
            (TerminalState::Cancelled, Some(stop), forced)
        );
        assert!(inside.lines().is_empty());
        assert!(!inside.internal_error());
    }
}

#[test]
fn a_stop_takes_precedence_over_earlier_faults() {
    let deadline = report(
        OutcomeKind::DeadlineExceeded,
        ErrorClass::Serialize(SerializeFailure::DeadlineExceeded),
        Some("deadline"),
    );
    let record = terminal(false, versions(), 3, end(vec![fault(), deadline], None));
    assert_eq!(record.state(), TerminalState::ResourceExhausted);
    assert_eq!(lines(&record), ["fault", "deadline"]);
    let record = terminal(
        false,
        versions(),
        3,
        end(vec![fault(), value()], Some(StopReason::Cancellation)),
    );
    assert_eq!(record.state(), TerminalState::Cancelled);
    assert!(record.lines().is_empty());
}

#[test]
fn faults_and_limits_do_not_stop_a_job_of_several_requests() {
    let limit = report(
        OutcomeKind::ResourceLimit,
        ErrorClass::Outcome,
        Some("limit"),
    );
    let record = terminal(
        false,
        versions(),
        3,
        end(vec![fault(), limit, value()], None),
    );
    assert_eq!(record.state(), TerminalState::Failed);
    assert_eq!(lines(&record), ["fault", "limit", "value"]);
    assert!(!record.internal_error());
}

#[test]
fn an_echo_of_other_versions_fails_the_job_outside_cancel_requested() {
    let echo = RequestReport::new(
        OutcomeKind::Value,
        ErrorClass::NoError,
        line("value"),
        other_versions(),
    );
    let outside = terminal(false, versions(), 1, end(vec![echo.clone()], None));
    assert_eq!(outside.state(), TerminalState::Failed);
    assert!(outside.internal_error());
    assert!(outside.lines().is_empty());
    assert_eq!(outside.echoed(), Some(other_versions()));
    let inside = terminal(true, versions(), 1, end(vec![echo], None));
    assert_eq!(inside.state(), TerminalState::Cancelled);
    assert!(!inside.internal_error());
}

#[test]
fn reports_that_do_not_fit_the_job_fail_it() {
    let cancelled = report(OutcomeKind::Cancelled, ErrorClass::NoError, None);
    let misfits = [
        (1, end(vec![value(), value()], None)),
        (2, end(vec![value()], None)),
        (3, end(vec![cancelled, value()], None)),
    ];
    for (requests, finished) in misfits {
        let record = terminal(false, versions(), requests, finished);
        assert_eq!(record.state(), TerminalState::Failed);
        assert!(record.internal_error());
        assert!(record.lines().is_empty());
    }
    let fewer_with_stop = terminal(
        false,
        versions(),
        2,
        end(vec![value()], Some(StopReason::ForcedStop)),
    );
    assert_eq!(fewer_with_stop.state(), TerminalState::ResourceExhausted);
    assert_eq!(lines(&fewer_with_stop), ["value"]);
}

#[test]
fn the_record_keeps_the_echo_of_the_requests_that_ran() {
    let record = terminal(false, versions(), 1, end(vec![value()], None));
    assert_eq!(record.echoed(), Some(versions()));
    let none = terminal(
        false,
        versions(),
        1,
        end(vec![], Some(StopReason::Deadline)),
    );
    assert_eq!(none.echoed(), None);
}

fn submission(session: SessionId) -> Submission {
    Submission::new(session, versions(), Request::new(Operation::Eval, "", "1"))
}

fn accept(core: &mut Machine, session: SessionId) -> JobId {
    let (accepted, commands) = core.submit(submission(session), Some(9));
    let id = accepted.unwrap();
    match commands.as_slice() {
        [Command::Hand(request)] => {
            assert_eq!((request.id(), request.deadline()), (id, Some(9)));
        }
        other => panic!("unexpected commands {other:?}"),
    }
    id
}

#[test]
fn identities_count_up_and_a_refused_submission_uses_none() {
    let mut core = Machine::new();
    let session = core.open_session().unwrap();
    assert_eq!(session.get(), 1);
    let ids: Vec<u64> = (0..MAX_LIVE_JOBS)
        .map(|_| accept(&mut core, session).get())
        .collect();
    assert_eq!(ids, (1..=8).collect::<Vec<u64>>());
    let (refused, commands) = core.submit(submission(session), None);
    assert_eq!(
        refused,
        Err(Rejected::Full {
            live: MAX_LIVE_JOBS
        })
    );
    assert!(commands.is_empty());
    assert_eq!(core.cancel(JobId::new(1)).0, CancelAnswer::Cancelled);
    core.report(Report::Finished(Finished::new(JobId::new(1), vec![], None)));
    assert!(core.take(JobId::new(1)).is_some());
    assert_eq!(accept(&mut core, session).get(), 9);
}

#[test]
fn identities_never_wrap() {
    let mut core = Machine::new();
    let session = core.open_session().unwrap();
    core.next_job = Some(u64::MAX);
    core.next_session = Some(u64::MAX);
    assert_eq!(accept(&mut core, session).get(), u64::MAX);
    assert_eq!(core.admits(), Err(Rejected::Exhausted));
    assert_eq!(
        core.submit(submission(session), None).0,
        Err(Rejected::Exhausted)
    );
    assert_eq!(core.open_session().map(SessionId::get), Ok(u64::MAX));
    assert_eq!(core.open_session(), Err(Rejected::Exhausted));
    assert_eq!(core.live(), 1);
}

#[test]
fn cancel_answers_follow_the_state_of_the_job() {
    let mut core = Machine::new();
    let session = core.open_session().unwrap();
    let queued = accept(&mut core, session);
    let running = accept(&mut core, session);
    core.report(Report::Started(running));
    assert_eq!(
        core.cancel(queued),
        (CancelAnswer::Cancelled, vec![Command::SetCancel(queued)])
    );
    assert_eq!(
        core.cancel(running),
        (CancelAnswer::Requested, vec![Command::SetCancel(running)])
    );
    assert_eq!(
        core.cancel(running),
        (CancelAnswer::AlreadyRequested, vec![])
    );
    assert_eq!(core.cancel(queued), (CancelAnswer::AlreadyTerminal, vec![]));
    assert_eq!(core.cancel(JobId::new(0)), (CancelAnswer::Unknown, vec![]));
    assert_eq!(core.cancel(JobId::new(3)), (CancelAnswer::Unknown, vec![]));

    // A job that ended and was taken is gone, and still answers as
    // terminal.
    core.report(Report::Finished(Finished::new(queued, vec![], None)));
    assert_eq!(
        core.take(queued).map(|t| t.record().state()),
        Some(TerminalState::Cancelled)
    );
    assert_eq!(core.state(queued), None);
    assert_eq!(core.cancel(queued), (CancelAnswer::AlreadyTerminal, vec![]));
}

#[test]
fn reports_of_a_job_cancelled_while_queued_are_discarded() {
    let mut core = Machine::new();
    let session = core.open_session().unwrap();
    let id = accept(&mut core, session);
    core.cancel(id);
    assert_eq!(core.ready(), vec![id]);
    assert!(core.take(id).is_some());
    assert_eq!(core.live(), 1, "live until the backend finishes it");
    assert_eq!(core.report(Report::Started(id)), ReportEffect::Discarded);
    assert_eq!(
        core.report(Report::Finished(Finished::new(id, vec![value()], None))),
        ReportEffect::Discarded
    );
    assert_eq!(core.live(), 0);
    assert_eq!(core.take(id), None);
}

#[test]
fn a_terminal_state_is_absorbing_and_taken_once() {
    let mut core = Machine::new();
    let session = core.open_session().unwrap();
    let id = accept(&mut core, session);
    assert_eq!(
        core.report(Report::Finished(Finished::new(id, vec![value()], None))),
        ReportEffect::Terminal
    );
    assert_eq!(
        core.report(Report::Finished(Finished::new(id, vec![], None))),
        ReportEffect::Discarded
    );
    assert_eq!(core.report(Report::Started(id)), ReportEffect::Discarded);
    assert_eq!(core.cancel(id).0, CancelAnswer::AlreadyTerminal);
    let taken = core.take(id).unwrap();
    assert_eq!(taken.record().state(), TerminalState::Completed);
    assert_eq!(taken.versions(), versions());
    assert_eq!(core.take(id), None);
    assert_eq!(core.live(), 0);
}

#[test]
fn the_step_function_gives_the_same_answers_as_the_methods() {
    let mut core = Machine::new();
    let Answer::Session(Ok(session)) = core.step(Input::OpenSession).answer else {
        panic!("no session");
    };
    let output = core.step(Input::Submit {
        submission: submission(session),
        deadline: None,
    });
    let Answer::Submitted(Ok(id)) = output.answer else {
        panic!("not accepted");
    };
    assert_eq!(output.commands.len(), 1);
    assert_eq!(
        core.step(Input::Report(Report::Started(id))).answer,
        Answer::Report(ReportEffect::Started)
    );
    let cancelled = core.step(Input::CancelSession(session));
    assert_eq!(
        cancelled.answer,
        Answer::SessionCancelled(CancelCount {
            queued: 0,
            requested: 1
        })
    );
    assert_eq!(cancelled.commands, vec![Command::SetCancel(id)]);
    assert_eq!(
        core.step(Input::Cancel(id)).answer,
        Answer::Cancel(CancelAnswer::AlreadyRequested)
    );
    core.step(Input::Report(Report::Finished(Finished::new(
        id,
        vec![value()],
        None,
    ))));
    let Answer::Taken(Some(taken)) = core.step(Input::Take(id)).answer else {
        panic!("not taken");
    };
    assert_eq!(taken.record().state(), TerminalState::Cancelled);
}
