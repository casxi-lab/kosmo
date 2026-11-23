//! The terminal loop on the host stub with the kalyx runner: each line of
//! the syntax, lines that are entered, incomplete, invalid, outside the
//! character set or too long, results and errors with their versions, and
//! how the terminal ends. The clock always reads 0, so no test sleeps or
//! reads the wall clock.

use std::io::{self, Write};

use kosmo::{Ended, MAX_LINE_BYTES, enter, job_layer, run};
use kosmo_sdk::kosmo_jobs::controllable::{ControllableBackend, Gate, Plan, Release};
use kosmo_sdk::kosmo_jobs::{JobClock, JobLayer, Operation, StopReason, TerminalState};
use kosmo_sdk::kosmo_math_kalyx::convert;
use kosmo_sdk::kosmo_math_model::{Completeness, EditBuffer, Item, Position};
use kosmo_sdk::kosmo_scratchpad::{Entry, Scratchpad};

type Checked<T = ()> = Result<T, Box<dyn std::error::Error>>;

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

/// A run of the terminal on a new Scratchpad and a new job layer.
struct Ran {
    output: Vec<u8>,
    ended: Ended,
    pad: Scratchpad,
    live_threads: usize,
    live_jobs: usize,
    /// The input that the terminal did not read.
    unread: Vec<u8>,
}

impl Ran {
    fn output(&self) -> String {
        String::from_utf8_lossy(&self.output).into_owned()
    }
}

/// Runs the terminal on `input` with `time_limit` for every submission,
/// writing to `output`.
fn run_into(input: &[u8], output: &mut dyn Write, time_limit: Option<u64>) -> Checked<Ran> {
    let mut layer = job_layer(Box::new(Still))?;
    let session = layer.open_session()?;
    let mut pad = Scratchpad::new();
    let mut unread = input;
    let ended = run(
        &mut unread,
        output,
        &mut pad,
        &mut layer,
        session,
        time_limit,
    );
    Ok(Ran {
        output: Vec::new(),
        ended,
        pad,
        live_threads: layer.backend().live_threads(),
        live_jobs: layer.live_jobs(),
        unread: unread.to_vec(),
    })
}

/// Runs the terminal on `input` without a time limit.
fn run_on(input: &[u8]) -> Checked<Ran> {
    run_limited(input, None)
}

/// Runs the terminal on `input` with `time_limit` for every submission.
fn run_limited(input: &[u8], time_limit: Option<u64>) -> Checked<Ran> {
    let mut output = Vec::new();
    let mut ran = run_into(input, &mut output, time_limit)?;
    ran.output = output;
    Ok(ran)
}

/// Asserts that the terminal ended at the end of its input or at `:quit`
/// with its job layer shut down.
fn assert_done(ran: &Ran) {
    assert_eq!(ran.ended, Ended::Done);
    assert_eq!(ran.ended.exit_code(), 0);
    assert_eq!(ran.live_threads, 0);
    assert_eq!(ran.live_jobs, 0);
}

/// The items of `text`, one character each.
fn chars(text: &str) -> Vec<Item> {
    text.chars().map(Item::Char).collect()
}

/// Asserts that `entry` holds `operation`, `variable` and the expression
/// `expression`.
fn assert_entry(entry: &Entry, operation: Operation, variable: &str, expression: &str) {
    assert_eq!(entry.operation(), operation);
    assert_eq!(entry.variable(), variable);
    assert_eq!(entry.input().items(), chars(expression).as_slice());
}

#[test]
fn the_terminal_ends_at_the_end_of_input_with_its_worker_thread_ended() {
    // The last line has no line feed, and is computed all the same.
    let ran = run_on(b"1/3+1/6").unwrap();
    assert_eq!(ran.output(), "value (source 7, settings 0): 1/2\n");
    assert_done(&ran);
    assert_eq!(ran.pad.history().len(), 1);
}

#[test]
fn an_expression_line_is_submitted_as_eval() {
    let ran = run_on(b"1/3+1/6\n").unwrap();
    assert_eq!(ran.output(), "value (source 7, settings 0): 1/2\n");
    assert_done(&ran);
    let [entry] = ran.pad.history() else {
        panic!("not one entry: {:?}", ran.pad.history());
    };
    assert_entry(entry, Operation::Eval, "", "1/3+1/6");
}

#[test]
fn expand_submits_its_expression_in_its_variable() {
    let ran = run_on(b":expand x (x+1)^2\n").unwrap();
    assert_eq!(
        ran.output(),
        "value (source 7, settings 0): 1 + 2*x + x^2\n"
    );
    assert_done(&ran);
    let [entry] = ran.pad.history() else {
        panic!("not one entry: {:?}", ran.pad.history());
    };
    // The command word and the variable are not entered into the line.
    assert_entry(entry, Operation::Expand, "x", "(x+1)^2");
}

#[test]
fn diff_differentiates_in_the_variable_it_names() {
    let ran = run_on(b":diff y 2*y^3\n").unwrap();
    assert_eq!(ran.output(), "value (source 5, settings 0): 6*y^2\n");
    assert_done(&ran);
    let [entry] = ran.pad.history() else {
        panic!("not one entry: {:?}", ran.pad.history());
    };
    assert_entry(entry, Operation::Diff, "y", "2*y^3");
}

#[test]
fn history_lists_each_entry_with_its_line_and_result() {
    let ran = run_on(b"1/3+1/6\n:diff x x^2\n:history\n").unwrap();
    assert_eq!(
        ran.output(),
        "value (source 7, settings 0): 1/2\n\
         value (source 11, settings 0): 2*x\n\
         job 1: 1/3+1/6 => value (source 7, settings 0): 1/2\n\
         job 2: :diff x x^2 => value (source 11, settings 0): 2*x\n"
    );
    assert_done(&ran);
    assert_eq!(ran.pad.history().len(), 2);
    // 7 characters, a clear, 3 characters and a clear: listing the history
    // changes neither the line nor its source version.
    assert!(ran.pad.line().items().is_empty());
    assert_eq!(ran.pad.line().version().get(), 12);
}

#[test]
fn history_without_entries_writes_nothing() {
    let ran = run_on(b":history\n").unwrap();
    assert_eq!(ran.output(), "");
    assert_done(&ran);
}

#[test]
fn quit_ends_the_terminal_without_reading_further() {
    let ran = run_on(b":quit\n1/3+1/6\n").unwrap();
    assert_eq!(ran.output(), "");
    assert_done(&ran);
    assert!(ran.pad.history().is_empty());
    assert_eq!(ran.unread, b"1/3+1/6\n");
}

#[test]
fn an_unknown_command_is_written_as_an_error_and_not_submitted() {
    let ran = run_on(b":dif x x^2\n1/3+1/6\n").unwrap();
    // The second line starts from source version 0: the first one entered
    // nothing into the line.
    assert_eq!(
        ran.output(),
        "input error: UnknownCommand\nvalue (source 7, settings 0): 1/2\n"
    );
    assert_done(&ran);
    assert_eq!(ran.pad.history().len(), 1);
}

#[test]
fn commands_with_fields_that_do_not_fit_are_malformed() {
    let ran = run_on(":diff x\n:history 1\n:diff  x x^2\n:quit now\n:diff α x^2\n:\n".as_bytes())
        .unwrap();
    assert_eq!(
        ran.output(),
        "input error: MalformedCommand\n\
         input error: MalformedCommand\n\
         input error: MalformedCommand\n\
         input error: MalformedCommand\n\
         input error: MalformedCommand\n\
         input error: UnknownCommand\n"
    );
    assert_done(&ran);
    assert!(ran.pad.history().is_empty());
    assert_eq!(ran.pad.line().version().get(), 0);
}

#[test]
fn kalyx_judges_the_variable() {
    let ran = run_on(b":diff 2 x^2\n:expand pi x\n").unwrap();
    assert_eq!(
        ran.output(),
        "error (source 3, settings 0): Syntax on the whole line\n\
         error (source 5, settings 0): Unsupported on the whole line\n"
    );
    assert_done(&ran);
    assert_eq!(ran.pad.history().len(), 2);
}

/// A line of 89 characters with all 72 characters of the character set, a
/// space first, two spaces in a row, a unary minus and two levels of
/// brackets.
const ENTERED: &str =
    " -(x^2 - .5)*(y/3) +  0.123456789 + abcdefghijklmnopqrstuvwxyz_ABCDEFGHIJKLMNOPQRSTUVWXYZ";

#[test]
fn an_entered_line_has_the_structure_written_out_directly() {
    assert_eq!(ENTERED.len(), 89);
    let mut set: Vec<char> = ENTERED.chars().collect();
    set.sort_unstable();
    set.dedup();
    assert_eq!(set.len(), 72);

    // The function that the terminal enters each expression with.
    let mut pad = Scratchpad::new();
    assert_eq!(pad.line().version().get(), 0);
    enter(&mut pad, ENTERED.as_bytes()).unwrap();
    let line = pad.line();
    let root = EditBuffer::new().root();
    assert_eq!(root.get(), 0);
    assert_eq!(line.items(), chars(ENTERED).as_slice());
    assert_eq!(
        line.cursor(),
        Position {
            block: root,
            index: 89
        }
    );
    assert_eq!(line.selection(), None);
    assert_eq!(line.completeness(), Completeness::Complete);
    // One edit for each character, where one insertion of the text would
    // be one edit.
    assert_eq!(line.version().get(), 89);
    assert_eq!(
        convert(&line.snapshot()).unwrap(),
        " -(x^2 - 0.5)*(y/3) +  0.123456789 + abcdefghijklmnopqrstuvwxyz_ABCDEFGHIJKLMNOPQRSTUVWXYZ"
    );

    // The same line through the terminal: the entry keeps the snapshot of
    // the line before it was cleared.
    let ran = run_on(format!("{ENTERED}\n").as_bytes()).unwrap();
    assert_eq!(
        ran.output(),
        "error (source 89, settings 0): UnknownSymbol on the whole line\n"
    );
    assert_done(&ran);
    let [entry] = ran.pad.history() else {
        panic!("not one entry: {:?}", ran.pad.history());
    };
    assert_entry(entry, Operation::Eval, "", ENTERED);
    assert_eq!(entry.input().completeness(), Completeness::Complete);
    assert_eq!(entry.input().version().get(), 89);
    assert_eq!(entry.versions().source.get(), 89);
}

#[test]
fn an_incomplete_line_writes_its_holes_and_is_not_submitted() {
    let ran = run_on(b"(1/\n").unwrap();
    assert_eq!(
        ran.output(),
        "input error: MissingOperand at position 3, MissingRightBracket at position 3\n"
    );
    assert_done(&ran);
    assert!(ran.pad.history().is_empty());
}

#[test]
fn empty_lines_and_lines_of_spaces_give_the_empty_line_hole() {
    let ran = run_on(b"\n   \n1/\n").unwrap();
    assert_eq!(
        ran.output(),
        "input error: EmptyLine at position 0\n\
         input error: EmptyLine at position 3\n\
         input error: MissingOperand at position 2\n"
    );
    assert_done(&ran);
    assert!(ran.pad.history().is_empty());
}

#[test]
fn an_invalid_line_writes_its_first_invalid_part() {
    let ran = run_on(b"1.2.3\n1)\n2x\n").unwrap();
    assert_eq!(
        ran.output(),
        "input error: MalformedNumber at items 0..5\n\
         input error: SurplusRightBracket at items 1..2\n\
         input error: MissingOperator at position 1\n"
    );
    assert_done(&ran);
    assert!(ran.pad.history().is_empty());
}

#[test]
fn a_character_outside_the_character_set_is_an_error() {
    let ran = run_on("x²+1\n".as_bytes()).unwrap();
    // The `x` before it was entered; the character itself is not echoed.
    assert_eq!(
        ran.output(),
        "input error: OutsideCharacterSet at position 1\n"
    );
    assert_done(&ran);
    assert!(ran.pad.history().is_empty());
    // The line was cleared after the error.
    assert!(ran.pad.line().items().is_empty());

    let ran = run_on(b"\xff1\n=\n").unwrap();
    assert_eq!(
        ran.output(),
        "input error: OutsideCharacterSet at position 0\n\
         input error: OutsideCharacterSet at position 0\n"
    );
    assert_done(&ran);
}

#[test]
fn nesting_deeper_than_the_limit_is_an_error() {
    let ran = run_on(format!("{}\n", "(".repeat(70)).as_bytes()).unwrap();
    assert_eq!(ran.output(), "input error: TooDeep at position 64\n");
    assert_done(&ran);
    assert!(ran.pad.history().is_empty());
    assert!(ran.pad.line().items().is_empty());
}

#[test]
fn an_over_long_line_is_an_error_and_the_next_line_runs() {
    let input = format!("{}\n1/3+1/6\n", "1".repeat(MAX_LINE_BYTES + 1));
    let ran = run_on(input.as_bytes()).unwrap();
    assert_eq!(
        ran.output(),
        "input error: LineTooLong\nvalue (source 7, settings 0): 1/2\n"
    );
    assert_done(&ran);
    assert_eq!(ran.pad.history().len(), 1);
}

#[test]
fn a_line_of_exactly_the_limit_is_submitted() {
    let input = format!("{}1\n", " ".repeat(MAX_LINE_BYTES - 1));
    let ran = run_on(input.as_bytes()).unwrap();
    assert_eq!(ran.output(), "value (source 4096, settings 0): 1\n");
    assert_done(&ran);
    let [entry] = ran.pad.history() else {
        panic!("not one entry");
    };
    // The spaces are kept.
    assert_eq!(entry.input().items().len(), MAX_LINE_BYTES);
}

#[test]
fn one_carriage_return_before_the_line_feed_is_dropped() {
    let ran = run_on(b"1/3+1/6\r\n").unwrap();
    assert_eq!(ran.output(), "value (source 7, settings 0): 1/2\n");
    assert_done(&ran);
}

#[test]
fn an_error_with_a_span_is_written_with_its_items() {
    let ran = run_on(b"1/0\n").unwrap();
    assert_eq!(
        ran.output(),
        "error (source 3, settings 0): DivisionByZero at items 0..3\n"
    );
    assert_done(&ran);
}

#[test]
fn a_job_past_its_deadline_writes_deadline_exceeded() {
    // The clock reads 0 and the time limit is 0, so the deadline has
    // passed before the request runs.
    let ran = run_limited(b"1/3+1/6\n", Some(0)).unwrap();
    assert_eq!(
        ran.output(),
        "error (source 7, settings 0): DeadlineExceeded on the whole line\n"
    );
    assert_done(&ran);
    let [entry] = ran.pad.history() else {
        panic!("not one entry");
    };
    let outcome = entry.outcome().unwrap();
    assert_eq!(outcome.state(), TerminalState::ResourceExhausted);
    assert_eq!(outcome.stop(), Some(StopReason::Deadline));
    assert_eq!(outcome.item(), None);
}

#[test]
fn jobs_that_end_without_a_line_write_an_internal_error_with_their_versions() {
    // The controllable backend ends each job with a stop and no request
    // report, as the host stub with the kalyx runner never does.
    let mut backend = ControllableBackend::new();
    backend.plan(0, Plan::new().stop(StopReason::Cancellation));
    backend.plan(1, Plan::new().stop(StopReason::BackendInternalError));
    backend.plan(2, Plan::new().stop(StopReason::ForcedStop));
    let mut layer = JobLayer::new(backend, Box::new(Still));
    let session = layer.open_session().unwrap();
    let mut pad = Scratchpad::new();
    let mut output = Vec::new();
    let input = b"1/3+1/6\n2\n3\n";
    let ended = run(
        &mut &input[..],
        &mut output,
        &mut pad,
        &mut layer,
        session,
        None,
    );
    assert_eq!(ended, Ended::Done);
    assert_eq!(
        String::from_utf8(output).unwrap(),
        "internal error (source 7, settings 0): the job was cancelled\n\
         internal error (source 9, settings 0): the job failed with an internal error\n\
         internal error (source 11, settings 0): the job was stopped by force\n"
    );
    assert!(layer.backend().is_shut_down());
    assert_eq!(pad.history().len(), 3);
}

#[test]
fn a_job_without_a_terminal_state_writes_the_same_internal_error_in_the_history() {
    // The controllable backend holds the job at its start and is never
    // released, so the job layer has no report to come for it, which the
    // host stub with the kalyx runner never does.
    let mut backend = ControllableBackend::new();
    backend.plan(0, Plan::new().gate(Gate::Started, Release::Manual));
    let mut layer = JobLayer::new(backend, Box::new(Still));
    let session = layer.open_session().unwrap();
    let mut pad = Scratchpad::new();
    let mut output = Vec::new();
    let input = b"1/3+1/6\n:history\n";
    let ended = run(
        &mut &input[..],
        &mut output,
        &mut pad,
        &mut layer,
        session,
        None,
    );
    assert_eq!(ended, Ended::Done);
    assert_eq!(
        String::from_utf8(output).unwrap(),
        "internal error: the job layer gave no terminal state for job 1\n\
         job 1: 1/3+1/6 => internal error: the job layer gave no terminal state for job 1\n"
    );
    assert!(layer.backend().is_shut_down());
}

/// An output that fails every write and flush.
struct Broken;

impl Write for Broken {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::Error::other("the output failed"))
    }

    fn flush(&mut self) -> io::Result<()> {
        Err(io::Error::other("the output failed"))
    }
}

#[test]
fn a_failed_write_stops_the_terminal_with_exit_code_one() {
    let ran = run_into(b"1/3+1/6\n2\n", &mut Broken, None).unwrap();
    assert_eq!(ran.ended, Ended::Failed);
    assert_eq!(ran.ended.exit_code(), 1);
    assert_eq!(ran.live_threads, 0);
    // It read no line after the one whose answer it could not write.
    assert_eq!(ran.unread, b"2\n");
    assert_eq!(ran.pad.history().len(), 1);
}

/// An output that keeps what is written to it and fails every flush.
struct Unflushable(Vec<u8>);

impl Write for Unflushable {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Err(io::Error::other("the output failed"))
    }
}

#[test]
fn a_failed_flush_stops_the_terminal_with_exit_code_one() {
    let mut output = Unflushable(Vec::new());
    let ran = run_into(b"1/3+1/6\n2\n", &mut output, None).unwrap();
    assert_eq!(ran.ended, Ended::Failed);
    assert_eq!(ran.live_threads, 0);
    // The line was written, but the terminal read no line after it.
    assert_eq!(output.0, b"value (source 7, settings 0): 1/2\n");
    assert_eq!(ran.unread, b"2\n");
    assert_eq!(ran.pad.history().len(), 1);
}

/// An input that fails after its first line.
struct FailingInput {
    first: Option<&'static [u8]>,
}

impl io::Read for FailingInput {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let Some(first) = self.first.take() else {
            return Err(io::Error::other("the input failed"));
        };
        buf[..first.len()].copy_from_slice(first);
        Ok(first.len())
    }
}

#[test]
fn a_failed_read_stops_the_terminal_with_exit_code_one() {
    let mut layer = job_layer(Box::new(Still)).unwrap();
    let session = layer.open_session().unwrap();
    let mut pad = Scratchpad::new();
    let mut input = io::BufReader::new(FailingInput {
        first: Some(b"1/3+1/6\n"),
    });
    let mut output = Vec::new();
    let ended = run(&mut input, &mut output, &mut pad, &mut layer, session, None);
    assert_eq!(ended, Ended::Failed);
    assert_eq!(output, b"value (source 7, settings 0): 1/2\n");
    assert_eq!(layer.backend().live_threads(), 0);
}
