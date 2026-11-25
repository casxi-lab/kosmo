use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use kalyx::command::Settings;
use kalyx::context::CancelFlag;
use kalyx::{ErrorKind, FaultKind};
use kosmo_jobs::controllable::ControllableBackend;
use kosmo_jobs::{
    EchoedVersions, ErrorClass, JobClock, JobLayer, JobPreparer, Limits, Operation, OutcomeKind,
    Request, SerializeFailure, Submission,
};
use kosmo_math_model::EditBuffer;
use kosmo_math_model::version::{ContextVersion, SettingsVersion, Versions};

use super::{
    ClockFork, KalyxPreparer, Ran, kalyx_limits, kalyx_request, run_request, serialize_failure,
};

/// A clock that always reads `at` and counts its readings, those of its
/// forks included.
struct Counting {
    at: u64,
    readings: Arc<AtomicUsize>,
}

impl JobClock for Counting {
    fn now(&mut self) -> u64 {
        self.readings.fetch_add(1, Ordering::SeqCst);
        self.at
    }

    fn fork(&self) -> Box<dyn JobClock> {
        Box::new(Counting {
            at: self.at,
            readings: Arc::clone(&self.readings),
        })
    }
}

/// A runner's clock that always reads `at`, and the count of its readings.
fn clock(at: u64) -> (ClockFork, Arc<AtomicUsize>) {
    let readings = Arc::new(AtomicUsize::new(0));
    let counting = Counting {
        at,
        readings: Arc::clone(&readings),
    };
    (ClockFork(Box::new(counting)), readings)
}

/// Runs `eval` of `text` with the versions 0, 0 and 0 under `limits`,
/// with the deadline `deadline` and the flag `flag`, on a clock that reads
/// 0; gives what it ran and the number of clock readings.
fn eval(
    text: &str,
    limits: kalyx::Limits,
    deadline: Option<u64>,
    flag: &CancelFlag,
) -> (Ran, usize) {
    let (clock, readings) = clock(0);
    let request = Request::new(Operation::Eval, "", text);
    let ran = run_request(
        &request,
        Versions::default(),
        limits,
        deadline,
        &clock,
        flag,
    );
    (ran, readings.load(Ordering::SeqCst))
}

fn line(ran: &Ran) -> String {
    String::from_utf8(ran.report.line().unwrap().to_vec()).unwrap()
}

#[test]
fn the_default_limits_of_a_job_are_those_of_kalyx_field_by_field() {
    let ours = Limits::new();
    let theirs = kalyx::Limits::new();
    assert_eq!(ours.max_work, theirs.max_work);
    assert_eq!(ours.max_input_bytes, theirs.max_input_bytes);
    assert_eq!(ours.max_output_bytes, theirs.max_output_bytes);
    assert_eq!(ours.max_depth, theirs.max_depth);
    assert_eq!(ours.max_nodes, theirs.max_nodes);
    assert_eq!(ours.max_storage_bytes, theirs.max_storage_bytes);
    assert_eq!(ours.max_integer_bits, theirs.max_integer_bits);
    assert_eq!(ours.max_exponent, theirs.max_exponent);
    assert_eq!(ours.max_frames, theirs.max_frames);
    assert_eq!(kalyx_limits(ours), theirs);
}

#[test]
fn each_limit_of_a_job_goes_into_its_own_kalyx_field() {
    let mut limits = Limits::new();
    limits.max_work = 1;
    limits.max_input_bytes = 2;
    limits.max_output_bytes = 3;
    limits.max_depth = 4;
    limits.max_nodes = 5;
    limits.max_storage_bytes = 6;
    limits.max_integer_bits = 7;
    limits.max_exponent = 8;
    limits.max_frames = 9;
    let theirs = kalyx_limits(limits);
    assert_eq!(
        [
            theirs.max_work,
            theirs.max_input_bytes,
            theirs.max_output_bytes,
            theirs.max_depth,
            theirs.max_nodes,
            theirs.max_storage_bytes,
            theirs.max_integer_bits,
            theirs.max_exponent,
            theirs.max_frames,
        ],
        [1, 2, 3, 4, 5, 6, 7, 8, 9]
    );
}

#[test]
fn a_kalyx_request_carries_the_operation_text_and_versions_of_the_job() {
    let mut buffer = EditBuffer::new();
    buffer.insert_char('1').unwrap();
    buffer.insert_char('2').unwrap();
    let versions = Versions {
        source: buffer.version(),
        context: ContextVersion::new(3),
        settings: SettingsVersion::new(4),
    };
    for (operation, name) in [
        (Operation::Eval, "eval"),
        (Operation::Expand, "expand"),
        (Operation::Diff, "diff"),
    ] {
        let request = kalyx_request(&Request::new(operation, "t", "t^3 + 1"), versions);
        assert_eq!(request.operation, name);
        assert_eq!(
            (request.variable.as_str(), request.input.as_str()),
            ("t", "t^3 + 1")
        );
        assert_eq!(
            (
                request.input_version,
                request.context_version,
                request.settings_version
            ),
            (2, 3, 4)
        );
        assert_eq!(request.settings, Settings::new());
    }
}

#[test]
fn the_report_echoes_each_version_of_the_outcome_in_its_own_field() {
    let mut buffer = EditBuffer::new();
    buffer.insert_char('1').unwrap();
    buffer.insert_char('2').unwrap();
    let versions = Versions {
        source: buffer.version(),
        context: ContextVersion::new(3),
        settings: SettingsVersion::new(4),
    };
    let (clock, _) = clock(0);
    let request = Request::new(Operation::Eval, "", "1/3+1/6");
    let ran = run_request(
        &request,
        versions,
        kalyx::Limits::new(),
        None,
        &clock,
        &CancelFlag::new(),
    );
    assert_eq!(ran.report.echoed(), EchoedVersions::new(2, 3, 4));
    assert_eq!(line(&ran), "value\t2\t3\t4\t1/2\n");
}

#[test]
fn the_handle_of_a_job_sets_the_flag_that_only_its_own_token_reads() {
    let (fork, _) = clock(0);
    let mut layer = JobLayer::new(ControllableBackend::new(), fork.0);
    let session = layer.open_session().unwrap();
    let request = Request::new(Operation::Eval, "", "1");
    let first = layer
        .submit(Submission::new(
            session,
            Versions::default(),
            request.clone(),
        ))
        .unwrap();
    let second = layer
        .submit(Submission::new(session, Versions::default(), request))
        .unwrap();
    let mut preparer = KalyxPreparer::new();
    let (handle, token) = preparer.prepare(first);
    let (_, other) = preparer.prepare(second);
    assert!(!token.0.is_cancelled());
    handle.clone().set();
    assert!(token.0.is_cancelled());
    assert!(!other.0.is_cancelled());
    handle.set();
    assert!(token.0.is_cancelled());
}

#[test]
fn a_value_line_is_written_under_the_context_of_its_request() {
    let (ran, readings) = eval("1/3+1/6", kalyx::Limits::new(), None, &CancelFlag::new());
    assert_eq!(ran.report.kind(), OutcomeKind::Value);
    assert_eq!(ran.report.error(), ErrorClass::NoError);
    assert_eq!(line(&ran), "value\t0\t0\t0\t1/2\n");
    assert_eq!(ran.report.echoed(), EchoedVersions::new(0, 0, 0));
    assert!(!ran.stops);
    assert_eq!(readings, 0, "a context without a deadline reads no clock");
}

#[test]
fn a_value_whose_line_does_not_fit_is_not_written_again() {
    let limits = kalyx::Limits::new().with_max_output_bytes(12);
    let (ran, _) = eval("1/3+1/6", limits, None, &CancelFlag::new());
    assert_eq!(ran.report.kind(), OutcomeKind::Value);
    assert_eq!(
        ran.report.error(),
        ErrorClass::Serialize(SerializeFailure::ResourceLimit)
    );
    assert_eq!(ran.report.line(), None);
    assert!(!ran.stops);
}

#[test]
fn an_error_line_that_does_not_fit_is_written_again_as_the_fallback_line() {
    let limits = kalyx::Limits::new().with_max_output_bytes(4);
    let (ran, _) = eval("1/0", limits, None, &CancelFlag::new());
    assert_eq!(ran.report.kind(), OutcomeKind::Fault);
    assert_eq!(
        ran.report.error(),
        ErrorClass::Serialize(SerializeFailure::ResourceLimit)
    );
    assert!(line(&ran).starts_with("error\t0\t0\t0\tDivisionByZero\t"));
    assert!(!ran.stops);
}

#[test]
fn an_error_line_that_fits_is_written_under_the_context_of_its_request() {
    let (ran, _) = eval("1/0", kalyx::Limits::new(), None, &CancelFlag::new());
    assert_eq!(ran.report.kind(), OutcomeKind::Fault);
    assert_eq!(ran.report.error(), ErrorClass::Outcome);
    assert!(line(&ran).starts_with("error\t0\t0\t0\tDivisionByZero\t"));
    assert!(!ran.stops);
}

#[test]
fn a_set_flag_stops_the_request_and_its_line_and_the_fallback_line_is_written() {
    let flag = CancelFlag::new();
    flag.cancel();
    let (ran, _) = eval("1/3+1/6", kalyx::Limits::new(), None, &flag);
    assert_eq!(ran.report.kind(), OutcomeKind::Cancelled);
    assert_eq!(
        ran.report.error(),
        ErrorClass::Serialize(SerializeFailure::Cancelled)
    );
    assert!(line(&ran).starts_with("error\t0\t0\t0\tCancelled\t"));
    assert!(ran.stops);
}

#[test]
fn a_passed_deadline_stops_the_request_and_the_fallback_line_reads_no_clock() {
    let (ran, readings) = eval("1/3+1/6", kalyx::Limits::new(), Some(0), &CancelFlag::new());
    assert_eq!(ran.report.kind(), OutcomeKind::DeadlineExceeded);
    assert_eq!(
        ran.report.error(),
        ErrorClass::Serialize(SerializeFailure::DeadlineExceeded)
    );
    assert!(line(&ran).starts_with("error\t0\t0\t0\tDeadlineExceeded\t"));
    assert!(ran.stops);
    // The first charge read the clock and saw the deadline pass, which
    // stays observed; the fallback line has no deadline.
    assert_eq!(readings, 1);
}

#[test]
fn a_deadline_that_has_not_passed_lets_the_request_write_its_value_line() {
    let (ran, readings) = eval("1/3+1/6", kalyx::Limits::new(), Some(1), &CancelFlag::new());
    assert_eq!(ran.report.kind(), OutcomeKind::Value);
    assert_eq!(ran.report.error(), ErrorClass::NoError);
    assert_eq!(line(&ran), "value\t0\t0\t0\t1/2\n");
    assert!(!ran.stops);
    // The first charge read the clock, which reads 0, before the deadline 1.
    assert!(readings >= 1);
}

#[test]
fn a_line_that_fails_with_a_fault_is_a_failed_request() {
    assert_eq!(
        serialize_failure(ErrorKind::Fault(FaultKind::Internal)),
        SerializeFailure::Fault
    );
    assert_eq!(
        serialize_failure(ErrorKind::ResourceLimit),
        SerializeFailure::ResourceLimit
    );
    assert_eq!(
        serialize_failure(ErrorKind::Cancelled),
        SerializeFailure::Cancelled
    );
    assert_eq!(
        serialize_failure(ErrorKind::DeadlineExceeded),
        SerializeFailure::DeadlineExceeded
    );
}
