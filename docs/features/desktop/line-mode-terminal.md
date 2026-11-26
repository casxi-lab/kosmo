# Line-mode terminal

The `kosmo` program in the `desktop/` workspace, run without arguments, is
the line-mode terminal of kosmo on a host computer. It reads whole lines
from standard input, computes each line through the
[Scratchpad](../framework/scratchpad.md) and the
[job layer](../framework/job-layer.md) with kalyx 0.1.0, and writes the
answer to standard output, one line at a time. It uses only the Rust
standard library and neither reads nor changes terminal attributes: it
reads what the host's terminal hands it when you press Enter, and it writes
no prompt and no echo of its own.

The program reaches the framework only through the SDK facade `kosmo-sdk`,
which [the SDK facade](#the-sdk-facade) describes. The design rules that the
terminal follows are in
[frontends of the first release](../../architecture.md#frontends-of-the-first-release)
and the [platform boundary](../../architecture.md#platform-boundary) in the
architecture.

## Running it

Start the terminal from the repository root with:

```sh
cargo run --quiet --locked --manifest-path desktop/Cargo.toml -p kosmo
```

Type a line and press Enter, and the answer follows on the next line. End
the input, with Ctrl-D on most hosts, or type `:quit` to leave. Lines that
come through a pipe run the same way:

```sh
printf ':diff x x^2\n:diff x x^3\n:history\n' | cargo run --quiet --locked --manifest-path desktop/Cargo.toml -p kosmo
```

writes

```text
value (source 3, settings 0): 2*x
value (source 7, settings 0): 3*x^2
job 1: :diff x x^2 => value (source 3, settings 0): 2*x
job 2: :diff x x^3 => value (source 7, settings 0): 3*x^2
```

`kosmo --version` prints `kosmo` and its version, and any other arguments
print `Usage: kosmo [--version]` to standard error. Run the program's tests
from the repository root with:

```sh
cargo test --locked --manifest-path desktop/Cargo.toml -p kosmo
```

## Line syntax

A line that starts with `:` is a command; any other line, an empty one
included, is an expression. `:` is outside the character set of the line
and the grammar of kalyx, so a command and an expression never mix; inside
an expression, words such as `diff` and `expand` are ordinary names.

| Line | What it does | What it writes |
| --- | --- | --- |
| An expression | Evaluates it to an exact rational value | One line: the result, or an input error |
| `:expand <variable> <expression>` | Expands the expression as a polynomial in the variable | One line: the result, or an input error |
| `:diff <variable> <expression>` | Differentiates that polynomial with respect to the variable | One line: the result, or an input error |
| `:history` | Lists the history, oldest first | One line for each entry, and nothing when the history is empty |
| `:quit` | Ends the terminal | Nothing |

- The command word runs to the first space. `:expand` and `:diff` take
  exactly one space, the variable, exactly one space and the expression.
  The variable is a run of bytes from `!` to `~`, 0x21 to 0x7E, which the
  terminal passes to kalyx as it is, and kalyx judges it: `:diff 2 x^2`
  gives the error `Syntax` and `:expand pi x` the error `Unsupported`, both
  on the whole line, because the variable is not part of the expression. The
  expression is the rest of the line, spaces included, and may be empty.
- `:history` and `:quit` take nothing after them, not even a space.
- A command word that is not in the table writes
  `input error: UnknownCommand`, and a command whose fields do not fit it,
  such as `:diff x` without an expression, `:diff  x x^2` with two spaces,
  a variable with a byte outside 0x21 to 0x7E or `:quit now`, writes
  `input error: MalformedCommand`. Neither enters anything into the current
  line or submits anything, and the terminal reads the next line.
- The syntax is the terminal's own for now, and it can change: it has no
  aliases, no `:eval` and no comment lines.

## Lines and the current line

- A line is the bytes before its line feed; the last line of the input may
  lack one. One carriage return at the end of a line is dropped, so a line
  that ends with a carriage return and a line feed reads as one that ends
  with a line feed.
- A line holds at most `MAX_LINE_BYTES`, 4,096 bytes, a carriage return at
  its end included. It is read into a buffer that never grows beyond that. A
  longer line is read on to its line feed and discarded: it writes
  `input error: LineTooLong`, enters nothing and submits nothing. 4,096 is
  also the most items of the current line, so an expression that fits a
  line fits the current line.
- The expression is entered into the current line of the Scratchpad one
  character at a time through the insert operation, the function `enter`.
  Each character is an edit that increases the source version by 1.
  Entering stops at the first character that the line rejects and writes
  `input error: <kind> at position <i>`, where `<i>` is the position of its
  byte in the expression, counted from 0. The kind is
  `OutsideCharacterSet` for a character outside the
  [character set](../math/one-line-editing.md#the-line) and for bytes that
  are not UTF-8, and otherwise the name of the rejection, such as `TooDeep`
  for a 65th level of brackets and unary minus signs. The rejected bytes are
  not written back.
- An entered line that is not complete is not submitted. An incomplete line
  writes all its holes in reading order, as
  `input error: MissingOperand at position 3, MissingRightBracket at position 3`
  for `(1/`, with the kinds `EmptyLine`, `MissingOperand`,
  `MissingOperator` and `MissingRightBracket`. An invalid line writes its
  first invalid part, as `input error: MalformedNumber at items 0..5` for
  `1.2.3`, with the kinds `MalformedNumber` and `SurplusRightBracket`. An
  empty line and a line of spaces are expressions with the hole `EmptyLine`,
  at position 0 and at the end of the spaces.
- A complete line is submitted on the job layer with the default limits of
  kalyx 0.1.0 and no time limit, and the terminal waits until the job has
  reached its terminal state before it reads the next line. So at most one
  job is live, and the output of an input does not depend on when results
  arrive.
- After the line for an expression is written, the current line is cleared
  when it is not empty, which is an edit of its own. So source versions
  count up over a session: above, `x^2` is entered at source version 3,
  clearing it makes 4, and `x^3` is entered at 7.

## Output

- Everything that the terminal writes goes to standard output, as lines
  each ended by a line feed, and is flushed before the next line is read.
  Standard error gets only the usage and the reason why the terminal could
  not start.
- A result is written as the display model renders it, with its source and
  settings versions: `value (source 7, settings 0): 1/2` for `1/3+1/6`, or
  `error (source 3, settings 0): DivisionByZero at items 0..3` for `1/0`. An
  error is written by the name of its kind and its place in the submitted
  line: ` at items a..b`, ` at position i` or ` on the whole line`. A result
  is current when the terminal writes it, because the line is cleared only
  afterwards; a result that is not would be marked `, stale`.
- A job whose deadline passed before its request ran writes
  `error (source S, settings T): DeadlineExceeded on the whole line`. The
  program sets no time limit, so only a caller of the library function that
  gives one sees it. Any other job that ends without an output line writes
  `internal error (source S, settings T): ` and the reason. A submission
  that the job layer does not accept, and a job that the job layer gives no
  terminal state for, write `internal error: ` and the reason, the latter in
  the history too; with one live job, neither is expected.
- `:history` writes `job <n>: <line> => <result>` for each entry: `<n>`
  counts the jobs of the session from 1, `<line>` is the expression for an
  evaluation and `:<operation> <variable> <expression>` otherwise, and
  `<result>` is the line that was written for it. The history is the
  Scratchpad's own: the last 32 submissions, in memory.
- Every character outside the bytes 0x20 to 0x7E is written as `\u{h}`, with
  `h` its code point in lower-case hexadecimal, so standard output holds only
  those bytes and line feeds. kalyx writes values, states and kinds in
  printable ASCII, but the name of a parameter of a resource limit may hold
  any character, such as a C1 control character or one that changes the
  direction of text; escaped, it cannot act on the host's terminal. A name
  with letters outside ASCII is written escaped too. A backslash is written
  as it is, so a name that holds the text `\u{9b}` is written the same as one
  that holds the character U+009B.

## Exit codes

| Exit code | When |
| --- | --- |
| 0 | The input ended or `:quit` was read, or `--version` printed the version |
| 1 | Reading the input or writing the output failed, or the job layer could not start its worker thread or open a session |
| 2 | Arguments other than none or the single `--version` |

The job layer is shut down before the program exits, so its worker thread
has ended. The error of a line never changes the exit code.

## The library function

The terminal is `kosmo::run(input, output, pad, layer, session, time_limit)`
in the library of the `kosmo` package. It reads a `BufRead` and writes a
`Write`, with a `Scratchpad`, a job layer and a session of it that the
caller holds, and an optional time limit in nanoseconds for every
submission. It returns `Ended::Done` or `Ended::Failed`, whose `exit_code()`
is 0 or 1, and shuts the job layer down before it returns, so the caller
can read the history of the Scratchpad and the worker threads of the host
stub afterwards. It expects an empty current line and a history whose
entries all come from that job layer, and after it returns, the Scratchpad
and the job layer are for reading. `kosmo::job_layer(clock)` builds the job layer of the
terminal, the host stub with the kalyx runner, on the clock it is given.
`kosmo::program(args, input, output, errors)` is what `main` calls with the
standard streams and the arguments: it builds the monotonic clock of the
host, the job layer and a Scratchpad and runs the terminal, so `main` binds
only the standard streams, the arguments and the exit code. This is the
example of the crate documentation, which `cargo test` runs:

```rust
use kosmo::{Ended, job_layer, run};
use kosmo_sdk::kosmo_jobs::JobClock;
use kosmo_sdk::kosmo_scratchpad::Scratchpad;

struct Still;
impl JobClock for Still {
    fn now(&mut self) -> u64 { 0 }
    fn fork(&self) -> Box<dyn JobClock> { Box::new(Still) }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut layer = job_layer(Box::new(Still))?;
    let session = layer.open_session()?;
    let mut pad = Scratchpad::new();
    let mut output = Vec::new();
    let input = b":diff x x^2\n:history\n";
    let ended = run(&mut &input[..], &mut output, &mut pad, &mut layer, session, None);
    assert_eq!(ended, Ended::Done);
    assert_eq!(
        output,
        b"value (source 3, settings 0): 2*x\n\
          job 1: :diff x x^2 => value (source 3, settings 0): 2*x\n"
    );
    assert_eq!(pad.history().len(), 1);
    assert_eq!(layer.backend().live_threads(), 0);
    Ok(())
}
```

## The SDK facade

The library `kosmo-sdk` in the `framework/` workspace is the SDK facade of
kosmo. It only re-exports the framework libraries other than the framework
core `kosmo-ui` and the rendering library `kosmo-render`, each as a module
under its own name: `kosmo_math_model`, `kosmo_jobs`, `kosmo_math_kalyx` and
`kosmo_scratchpad`, so `kosmo_sdk::kosmo_scratchpad::Scratchpad` is the
Scratchpad. `kosmo-ui` and `kosmo-render` are not re-exported; the facade
re-exports `kosmo-ui` with the first application that uses it. The facade has
no code, no features and no direct
external dependency of its own; kalyx and the packages that kalyx depends on
come through `kosmo-math-kalyx`, which is why the allowlist of `desktop/`
lists them. It is unstable, so its modules can change in any version, and
it is not published.

It is the framework's only export crate so far, so the crates of
`desktop/` reach the framework only through it. The layout table marks it
as the framework's export crate and facade, and the
layout and deps steps of `cargo xtask check` fail when a crate of
`desktop/` depends on a crate of `framework/` that is not an export crate.
An SDK interface of its own, with its contracts and documentation for the
developers of applications, is planned; until then the facade is only
these re-exports. Its crate documentation has an example, which this
command runs:

```sh
cargo test --locked --manifest-path framework/Cargo.toml -p kosmo-sdk
```

## Limits

- The terminal runs only on a host computer.
- It takes whole lines. It has no editing within a line beyond what the
  host's terminal offers before Enter, no recall of earlier lines and no key
  that cancels a running job: a computation runs until its job ends, within
  the default limits of kalyx 0.1.0. A raw-mode terminal, if one is added,
  is an adapter crate of its own.
- It waits for each job, so it never cancels one and never gets a late
  result of an older version; the [Scratchpad](../framework/scratchpad.md)
  and the [job layer](../framework/job-layer.md) describe how those are
  handled.
- It shows results only as one line of linear text; two-dimensional display
  is planned.
- A line holds at most 4,096 bytes. The history holds the last 32
  submissions, only in memory, and is lost when the program exits. The
  syntax has no command that reuses an input or a result of the history.
- That the `kosmo` package uses no interface of the operating system beyond
  its standard streams, arguments, exit code and monotonic clock, starts no
  thread of its own and neither reads nor changes terminal attributes is
  checked by review until the layout step checks the platform boundary. The
  deps step checks that it has no direct external dependency.
- That `kosmo-sdk` holds only re-exports and has no features is checked by
  review; the layout step checks only that it is an export crate.

## Requirements

The tests are in `desktop/kosmo`: the unit tests in the `tests` modules
under `src/` (named here with their module, such as `line::tests::…`) and
the integration tests in `tests/` (named with their file). The integration
tests run the terminal on the host stub with the kalyx runner and a clock
that always reads 0, so no test sleeps or reads the wall clock.

1. The terminal is a library function that reads a `BufRead` and writes a
   `Write`. It ends at the end of the input, after a last line without a
   line feed too, and at `:quit`, which leaves the rest of the input unread,
   with exit code 0 and the worker thread of the job layer ended; a failed
   read or write ends it with exit code 1, also with the worker thread
   ended, and it reads no further line. Tests: `tests/terminal.rs`:
   `the_terminal_ends_at_the_end_of_input_with_its_worker_thread_ended`,
   `quit_ends_the_terminal_without_reading_further`,
   `a_failed_write_stops_the_terminal_with_exit_code_one`,
   `a_failed_flush_stops_the_terminal_with_exit_code_one`,
   `a_failed_read_stops_the_terminal_with_exit_code_one`;
   `tests::the_program_runs_the_terminal_without_arguments`.
2. Without arguments the program runs the terminal, `kosmo --version` prints
   the program name and version, and any other arguments print the usage to
   standard error with exit code 2. Tests: `tests/version.rs`:
   `prints_the_name_and_version`; `tests/arguments.rs`:
   `other_arguments_get_the_usage_and_exit_with_two`;
   `tests::no_arguments_run_the_terminal_and_only_the_single_version_flag_prints_the_version`,
   `tests::the_version_goes_to_the_output_and_the_usage_to_the_errors`.
3. Each form of the syntax does what its row of the table says: an
   expression is submitted as an evaluation with an empty variable,
   `:expand` and `:diff` with their operation and variable and only the
   expression entered, `:history` lists the entries with their lines and
   results and changes neither the current line nor its source version, and
   `:quit` ends the terminal. Tests: `tests/terminal.rs`:
   `an_expression_line_is_submitted_as_eval`,
   `expand_submits_its_expression_in_its_variable`,
   `diff_differentiates_in_the_variable_it_names`,
   `history_lists_each_entry_with_its_line_and_result`,
   `history_without_entries_writes_nothing`,
   `quit_ends_the_terminal_without_reading_further`;
   `command::tests::each_form_of_the_syntax_gives_its_command`,
   `command::tests::every_line_without_a_colon_first_is_an_expression`,
   `command::tests::the_expression_keeps_its_spaces_and_may_be_empty`.
4. A line outside the syntax writes `UnknownCommand` or `MalformedCommand`,
   enters nothing and submits nothing, and the terminal goes on; the
   variable goes to kalyx as it is written. Tests: `tests/terminal.rs`:
   `an_unknown_command_is_written_as_an_error_and_not_submitted`,
   `commands_with_fields_that_do_not_fit_are_malformed`,
   `kalyx_judges_the_variable`;
   `command::tests::command_words_outside_the_syntax_are_unknown`,
   `command::tests::fields_that_do_not_fit_are_malformed`.
5. The expression is entered one character at a time by the same function
   that the terminal uses, and the current line then equals, field by field,
   the items, cursor, selection and completeness written out in the test,
   with the source version increased by the number of characters; through
   the terminal, the entry keeps that line as its input. Test:
   `tests/terminal.rs`:
   `an_entered_line_has_the_structure_written_out_directly`.
6. A line that is not entered whole or not complete writes the position of
   the rejected byte, every hole or the first invalid part, and submits
   nothing. Tests: `tests/terminal.rs`:
   `an_incomplete_line_writes_its_holes_and_is_not_submitted`,
   `empty_lines_and_lines_of_spaces_give_the_empty_line_hole`,
   `an_invalid_line_writes_its_first_invalid_part`,
   `a_character_outside_the_character_set_is_an_error`,
   `nesting_deeper_than_the_limit_is_an_error`;
   `enter::tests::entering_stops_at_the_first_rejected_character_and_keeps_those_before`,
   `enter::tests::bytes_that_are_not_utf8_are_outside_the_character_set`,
   `enter::tests::other_rejections_are_named_by_their_kind`,
   `output::tests::holes_and_invalid_parts_are_written_by_their_kind`.
7. A line of more than 4,096 bytes, a carriage return at its end included,
   writes `LineTooLong`, is not submitted and never grows the line buffer
   beyond 4,096 bytes, and the next line runs; a line of exactly 4,096 bytes
   is submitted, and one carriage return before the line feed is dropped.
   Tests: `tests/terminal.rs`:
   `an_over_long_line_is_an_error_and_the_next_line_runs`,
   `a_line_of_exactly_the_limit_is_submitted`,
   `one_carriage_return_before_the_line_feed_is_dropped`;
   `line::tests::lines_end_at_line_feeds_and_the_last_may_lack_one`,
   `line::tests::one_carriage_return_at_the_end_of_a_line_is_dropped`,
   `line::tests::a_line_of_the_limit_is_read_and_one_byte_more_is_too_long`,
   `line::tests::a_line_without_a_line_feed_beyond_the_limit_never_grows_the_buffer_past_it`,
   `line::tests::an_interrupted_read_is_retried_and_another_error_is_returned`.
8. Results and errors are written as the display model renders them, with
   their source and settings versions and the place of an error, and a job
   whose deadline passed before its request ran writes `DeadlineExceeded`
   on the whole line; any other job without an output line, which only
   another backend gives, writes an internal error with its versions and
   the reason; a job without a terminal state writes an internal error
   without versions, and the same in the history. Tests: `tests/terminal.rs`:
   `an_error_with_a_span_is_written_with_its_items`,
   `a_job_past_its_deadline_writes_deadline_exceeded`,
   `jobs_that_end_without_a_line_write_an_internal_error_with_their_versions`,
   `a_job_without_a_terminal_state_writes_the_same_internal_error_in_the_history`.
9. Every line is written with each character outside 0x20 to 0x7E escaped,
   the name of a parameter with control characters included, and the
   rendering itself is unchanged. Tests:
   `output::tests::printable_ascii_is_written_as_it_is`,
   `output::tests::every_other_character_is_escaped`,
   `output::tests::a_parameter_name_with_control_characters_is_written_escaped`.
10. The session script `tests/data/session-input.txt`, with two derivatives,
    an incomplete line, a line outside the syntax and the history, goes
    through a pipe into the built `kosmo`, and its standard output equals
    `tests/data/session-output.txt` byte for byte, with nothing on standard
    error and exit code 0, in two runs. Test: `tests/session.rs`:
    `the_session_writes_the_expected_output_in_two_runs`.
11. The clock of the program never goes back, and its forks read the same
    clock. Test:
    `clock::tests::readings_never_decrease_and_a_fork_reads_the_same_clock`.
12. The crates of `desktop/` depend on the framework only through
    `kosmo-sdk`, and the packages that `desktop/` resolves are exactly those
    of its allowlist: the layout and deps steps of `cargo xtask check`. That
    `kosmo-sdk` only re-exports is checked by review (see Limits). The
    example of the facade's crate documentation uses the Scratchpad and
    formula conversion through it.
