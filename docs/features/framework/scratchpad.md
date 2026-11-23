# Scratchpad

The library `kosmo-scratchpad` in the `framework/` workspace is the
Scratchpad of the first release: one line of formula input that you edit,
the submission of that line as a versioned job, a bounded history of what
you submitted with the results, and the reuse of inputs and results of the
history in the line. Its state is only in memory: it reads and writes no
file and refers to no document.

It builds on the [one-line editing](../math/one-line-editing.md) structure
of `kosmo-math-model`, on [formula conversion](../math/formula-conversion.md)
and the reading of output lines in `kosmo-math-kalyx`, and on the
[job layer](job-layer.md) `kosmo-jobs`. It holds no job layer itself:
whoever holds the job layer submits each job, takes its terminal state and
hands it to the Scratchpad. No frontend uses it yet; the line-mode terminal
of the `kosmo` program and the calculator application `kosmo-calc` are
planned on top of it. The design rules that it follows are in
[versions](../../architecture.md#versions),
[results that are not current](../../architecture.md#results-that-are-not-current)
and [effects and the result slot](../../architecture.md#effects-and-the-result-slot)
in the architecture.

## Using it

A `Scratchpad` holds the current line. You edit it through the Scratchpad,
prepare a submission from it, submit that on your job layer, hand the
answer back, and deliver the job's terminal state when you take it. This is
the example of the crate documentation, which `cargo test` runs: one job on
the controllable backend, which answers with the line that kalyx 0.1.0
writes for differentiating `x^2` with respect to `x`, and the result reused
in the hole of `1/`.

```rust
use kosmo_jobs::controllable::{ControllableBackend, Plan};
use kosmo_jobs::{
    EchoedVersions, ErrorClass, JobClock, JobLayer, Limits, Operation, OutcomeKind,
    RequestReport,
};
use kosmo_math_kalyx::convert;
use kosmo_math_model::version::Judgement;
use kosmo_scratchpad::{Destination, Part, Scratchpad};

struct Still;
impl JobClock for Still {
    fn now(&mut self) -> u64 { 0 }
    fn fork(&self) -> Box<dyn JobClock> { Box::new(Still) }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let line = b"value\t1\t0\t0\t2*x\n".to_vec();
    let echo = EchoedVersions::new(1, 0, 0);
    let report = RequestReport::new(OutcomeKind::Value, ErrorClass::NoError, Some(line), echo);
    let mut backend = ControllableBackend::new();
    backend.plan(0, Plan::new().request(report));
    let mut layer = JobLayer::new(backend, Box::new(Still));
    let session = layer.open_session()?;

    let mut pad = Scratchpad::new();
    pad.insert_text("x^2")?;
    let (submission, pending) = pad.prepare(session, Operation::Diff, "x", Limits::new(), None)?;
    let id = pad.accept(pending, layer.submit(submission))?;
    let terminal = layer.wait(id).ok_or("no terminal state")?;
    let delivery = pad.deliver(&terminal);
    assert_eq!(delivery.judgement, Judgement::Current);
    assert_eq!(delivery.destination, Destination::Entry);

    // Clearing the line is an edit of its own; then the result `2*x` fills
    // the hole of `1/` in brackets.
    pad.clear()?;
    pad.insert_text("1/")?;
    pad.reuse(id, Part::Result, 0..3)?;
    assert_eq!(convert(&pad.line().snapshot())?, "1/(2*x)");
    assert_eq!(pad.history().len(), 1);
    Ok(())
}
```

Run the library's tests from the repository root with:

```sh
cargo test --locked --manifest-path framework/Cargo.toml -p kosmo-scratchpad
```

## The current line

- The current line is an `EditBuffer`. `line()` shows it, and the
  Scratchpad's operations `insert_char`, `insert_text`, `delete_backward`,
  `delete_forward`, `move_left`, `move_right`, `move_to_start`,
  `move_to_end`, `extend_left`, `extend_right`, `select_all` and `clear`
  pass each edit on to it unchanged, with the results that
  [one-line editing](../math/one-line-editing.md#editing-operations)
  describes. Fragments come into the line only through reuse.
- The Scratchpad never hands out the buffer to be changed or replaced, so
  the source version of the line starts at 0 and only increases.
- `versions()` gives the region's current versions: the source version of
  the line, and the context and settings versions. Both are 0 for
  `Scratchpad::new()`, and no operation changes them; kalyx 0.1.0 has no
  settings. `Scratchpad::with_versions` builds a Scratchpad with other
  context and settings versions, for tests.

## Submitting

A submission takes two steps, because whoever holds the job layer gets its
answer apart from building it: a terminal gets the answer at once, and in
the planned framework the event loop hands submissions to the job layer
after the update pass and brings each answer back in the next round.

1. `prepare(session, operation, variable, limits, time_limit)` takes a
   snapshot of the current line and converts it. It gives the `Submission`
   of a job of one request, with the operation, the variable and the linear
   text of the line, the session that you opened on your job layer, the
   limits and the optional time limit, and the versions of the snapshot: its
   source version and the Scratchpad's context and settings versions. With
   it comes a `Pending` value that keeps the snapshot, the versions, the
   operation and the variable. Preparing changes nothing, neither the line
   nor its source version. The line is checked first: an empty, incomplete
   or invalid line is not converted, whatever the variable, and gives its
   holes or its first invalid part instead, as `NotPrepared::NotConverted`,
   so nothing is computed from it. Then a variable longer than
   `MAX_VARIABLE_BYTES`, 4,096 bytes, is not submitted and gives
   `NotPrepared::VariableTooLong`.
2. `accept(pending, answer)` takes the pending value with the job layer's
   answer, `Result<JobId, kosmo_jobs::Rejected>`, as `submit` returns it. An
   accepted submission becomes the latest one and appends an entry to the
   history, with the snapshot and the versions of the pending value, and
   `accept` returns its job identity. A refused submission changes nothing:
   it appends no entry and does not become the latest, and the refusal comes
   back as `NotAccepted::Refused`. An identity that does not come after the
   latest accepted one, which a second answer or another job layer would
   give, comes back as `NotAccepted::NotAfterLatest` and changes nothing
   either. The job layer did accept that job, and it runs on: unless it is
   the job of an entry already, as for a second answer, cancel it or take
   its terminal state without delivering it to the Scratchpad.

You keep the pending value until the answer comes; the Scratchpad keeps no
pending submissions. An edit of the line between the two steps does not
change the entry, which keeps the line as it was prepared, and the job's
result is then stale.

Submitting does not clear the line, so the result of a submission can be
current. Clearing the line, as the planned terminal will before it reads the
next line, is an edit that increases the source version: a result that
arrives after it is stale and goes only into its entry.

## History

`history()` lists the entries, oldest first, and `entry(id)` finds the entry
of a job. An entry holds the job identity, the snapshot of the submitted
line, the three versions of the submission, the operation, the variable
and, once the job's terminal state has been delivered, its outcome. An
entry is a snapshot of its own: editing the current line, reusing the entry
and later results change none of its input, versions, operation and
variable, and its outcome is written once.

- The history holds at most `MAX_HISTORY_ENTRIES`, 32, entries. Accepting a
  submission when it holds 32 evicts the oldest entry first. There is no
  separate bound on its bytes: an entry's input holds at most 4,096 items,
  its variable at most 4,096 bytes, and the output line of its result at
  most the larger of 65,536 bytes and the `max_output_bytes` of its job's
  limits, which is 65,536 by default.
- The job of an evicted entry is not cancelled. It runs to its end and
  counts toward the job layer's live jobs until its terminal state is
  taken; delivering that terminal state counts toward the discard count.
- Job identities count up within one job layer, so the entries of one
  Scratchpad belong to the jobs of one job layer. A frontend that builds a
  new job layer, for example after the host stub's worker thread has ended,
  builds a new Scratchpad with it.

## Delivering results

`deliver(&terminal)` takes the terminal state of a job, as `wait` or `take`
of the job layer gives it, and returns a `Delivery`. Only terminal states of
the Scratchpad's own job layer, the one that accepted its submissions, may
be delivered: a terminal state of another job layer can carry the identity
of a different job that has an entry here.

- `judgement`: the result judged against the Scratchpad's current versions
  and latest submission, from the job's identity and versions, as
  [current, superseded and stale results](../../architecture.md#current-superseded-and-stale-results)
  describes.
- `destination`: `Entry` when the terminal state went into the entry of its
  job, whatever the judgement, so that a late result of an older version
  goes only into its own entry; `Discarded` when no entry has the job's
  identity, because the entry was evicted or the job was never accepted
  here, which increases `discarded()` by 1; `AlreadyFinished` when the entry
  has an outcome already, which stays.

The outcome of an entry keeps the job's terminal state, its stop, whether it
failed with an internal error, and the display item of its output line,
which `read_output_line` reads against the entry's snapshot, so an error
lies on the items of the submitted line, never on a later state of the
current line. A job of the Scratchpad has one request, so it delivers at
most one line. A job that ended without a line, such as a cancelled job, one
that failed with an internal error or one whose deadline passed before its
request ran, keeps only its state and stop.

The Scratchpad keeps no current result. Whoever shows one writes the result
of a delivery judged current into its result slot.

## Reuse

`reuse(id, part, items)` inserts a copy of the half-open item range `items`
of a line of the entry of job `id` at the cursor of the current line,
replacing the selection if there is one. `Part::Input` takes the range from
the entry's input; `Part::Result` takes it from the entry's value, whose
display text is read back into a line of its own when it is reused, as
[reading back](../math/formula-conversion.md#reading-back) describes. Any
range of either line can be reused, not only one that forms an operand. The
copy gets brackets when it is not a single operand and an operand is missing
where it goes, as fragment insertion does.

| Entry | Reused | Line before | Line after | Linear text |
| --- | --- | --- | --- | --- |
| `eval` of `1/3+1/6` | Input, items 0..7, `1/3+1/6` | Empty | `1/3+1/6` | `1/3+1/6` |
| `expand` of `(x+1)^2` in `x`, value `1 + 2*x + x^2` | Result, items 10..13, `x^2` | Empty | `x^2` | `x^2` |
| `diff` of `x^2` in `x`, value `2*x` | Result, items 0..3, `2*x` | `1/` | `1/(2*x)` | `1/(2*x)` |
| `expand` of `(x+1)^2` in `x` | Input, items 1..4, `x+1` | `2*` | `2*(x+1)` | `2*(x+1)` |
| `expand` of `(x+1)^2` in `x`, value `1 + 2*x + x^2` | Result, items 0..3, `1 +` | `1/` | `1/(1 +)` | None: the line misses an operand |

A copy that changes the line increases its source version by exactly 1, as
every edit does. An empty range changes nothing when nothing is selected;
with a selection it deletes the selection, as any insertion replaces it. The
entry stays as it was. The result of an entry of an older version can be
reused too. When nothing can be reused, `NotReused` says why, and the line,
its cursor, its selection and its source version stay as they were:

| Reason | When |
| --- | --- |
| `NotInHistory` | No entry has the job identity: it was evicted, or the job was never accepted here |
| `NoResultYet` | The entry's job has not been delivered |
| `NoLine` | The job ended without an output line, with its terminal state, such as a cancelled job |
| `Error` | The result is an error |
| `State` | The result is a state |
| `Unreadable` | The result's output line could not be read, or holds a kind of content that this version cannot reuse |
| `NotReadBack` | The value's display text does not read back into a line, such as one longer than the 4,096 bytes of one insertion |
| `Rejected` | The current line rejected the copy, such as past its 4,096 items, or the range runs backwards or past the reused line |

## Memory only

The Scratchpad and its entries hold no path, file or document: a history
entry is a snapshot of a submitted line with its result, and nothing of the
Scratchpad reaches a document. `kosmo-scratchpad` is a core library, so its
`src/`, unit tests included, uses none of the interfaces that the
[platform boundary](../../architecture.md#platform-boundary) forbids, among
them files, paths, the environment, threads, clocks and input and output.
Until the layout step checks that, review does. An integration test lists
the files of the working directory before and after running the cases of the
history and of reuse, and finds them unchanged.

## Limits

- The Scratchpad has no undo, no formula templates, no function catalog,
  no switching between calculation and graph, no context menus and no
  settings, and its work cannot be saved as a document. These belong to
  the Scratchpad and discovery capability of [Scope](../../scope.md), and
  are planned.
- No frontend uses it yet: the terminal of the `kosmo` program, which will
  list the history, and the calculator application `kosmo-calc`, which will
  connect it to a result slot, are planned.
- It holds one-line input only; two-dimensional templates come with
  two-dimensional editing.
- The variable of a submission holds at most 4,096 bytes.
- Only a value can be reused as a result, and only when its display text
  fits one insertion of 4,096 bytes. A value of kalyx 0.1.0 reads back as
  the same term; a later kalyx version is checked again when kosmo moves to
  it, as [formula conversion](../math/formula-conversion.md#limits)
  describes.
- The history keeps the last 32 submissions and evicts older ones without
  asking. It is lost when the Scratchpad is dropped.

## Requirements

The tests are in `framework/kosmo-scratchpad`: the unit tests in the `tests`
modules under `src/` (named here with their module, such as
`scratchpad::tests::…`) and the integration tests in `tests/` (named with
their file). The integration tests run jobs on the controllable backend of
the job layer, which answers with the output lines that kalyx 0.1.0 writes
for the same requests; `tests/with_kalyx.rs` runs those requests through the
Scratchpad on the host stub with the kalyx runner and compares the lines.
No test sleeps or reads the wall clock.

1. Every editing operation reaches the current line as the editing buffer
   does it, and nothing else changes the line, so its source version starts
   at 0 and only increases; the context and settings versions are those the
   Scratchpad was built with. Tests:
   `scratchpad::tests::a_new_scratchpad_is_empty_at_version_zero`,
   `scratchpad::tests::editing_operations_reach_the_current_line_as_they_are`.
2. Preparing converts a complete line into a job of one request with its
   operation, variable, session, limits, time limit and versions, and
   changes nothing; an empty, incomplete or invalid line gives its holes or
   its first invalid part whatever the variable, and a complete line with a
   variable longer than 4,096 bytes gives the variable's own reason. Tests:
   `scratchpad::tests::preparing_changes_neither_the_line_nor_its_version`,
   `scratchpad::tests::a_prepared_job_carries_the_line_its_versions_limits_and_time_limit`,
   `scratchpad::tests::lines_that_are_not_complete_give_their_holes_or_invalid_part`,
   `scratchpad::tests::a_variable_longer_than_its_bound_is_not_prepared`.
3. An accepted submission becomes the latest and appends one entry with
   the snapshot and versions it was prepared from, also when the line
   changed before the answer; a refused submission, or an identity that
   does not come after the latest, changes nothing. Tests:
   `tests/history.rs`: `an_entry_keeps_the_line_as_it_was_prepared`,
   `a_refused_submission_appends_no_entry_and_is_not_the_latest`;
   `scratchpad::tests::an_accepted_submission_becomes_the_latest_and_appends_its_entry`,
   `scratchpad::tests::a_refused_submission_changes_nothing`,
   `scratchpad::tests::an_identity_that_does_not_come_after_the_latest_is_not_accepted`,
   `scratchpad::tests::reasons_name_what_was_not_accepted`.
4. A delivered terminal state is judged against the current versions and
   the latest submission and goes into the entry of its own job, read
   against the entry's snapshot, whatever the judgement: a late result of
   an older version goes only into its own entry and changes neither the
   newer entry, nor the latest submission, nor the discard count. A job
   without a line keeps its state and stop, and an entry keeps its first
   outcome. Tests: `tests/history.rs`:
   `a_late_result_goes_only_into_the_entry_of_its_own_version`;
   `scratchpad::tests::a_result_goes_into_its_own_entry_whatever_its_judgement`,
   `scratchpad::tests::an_error_lies_in_the_submitted_snapshot_after_the_line_changed`,
   `scratchpad::tests::jobs_that_end_without_a_line_keep_their_state_and_stop`,
   `scratchpad::tests::a_second_delivery_of_a_terminal_state_leaves_the_entry_as_it_was`.
5. The history holds at most 32 entries: the 33rd acceptance evicts the
   oldest entry and leaves the inputs, versions and results of the others
   as they were. Test: `tests/history.rs`:
   `the_oldest_entry_is_evicted_past_the_bound`.
6. The job of an evicted entry is not cancelled, and its terminal state,
   like any other without an entry, counts toward the discard count and
   changes no entry. Tests: `tests/history.rs`:
   `the_job_of_an_evicted_entry_runs_on_and_its_result_is_discarded`;
   `scratchpad::tests::a_terminal_state_without_an_entry_counts_toward_the_discard_count`.
7. Reuse inserts a copy of any range of an entry's input or of its value
   read back, with brackets where an operand is missing and the copy is not
   one, and the line converts to the linear text of the table in
   [Reuse](#reuse). A copy replaces the selection; an empty range changes
   nothing without a selection and deletes the selection with one. Tests:
   `tests/reuse.rs`: `a_whole_input_is_reused_in_an_empty_line`,
   `part_of_a_result_is_reused`, `a_result_fills_the_hole_of_one_over`,
   `part_of_an_input_fills_the_hole_of_two_times`,
   `a_fragment_need_not_be_an_operand`;
   `reuse::tests::a_copy_replaces_the_selection_and_an_empty_one_changes_nothing`,
   `reuse::tests::an_empty_copy_deletes_the_selection`,
   `reuse::tests::the_result_of_a_stale_entry_can_be_reused`.
8. A reused copy is a copy: editing the line afterwards leaves the entry's
   input, versions and result as they were, and reuse increases the source
   version by exactly 1. Tests: `tests/reuse.rs`:
   `editing_after_reuse_changes_no_entry`,
   `reuse_increases_the_source_version_by_one`.
9. Each result that cannot be reused gives its own reason and changes
   neither the line nor its cursor, selection and source version. Tests:
   `tests/reuse.rs`: `results_that_cannot_be_reused_say_why_and_change_nothing`;
   `tests/with_kalyx.rs`: `a_value_longer_than_one_insertion_is_not_reused`;
   `reuse::tests::a_result_without_a_value_is_not_reused_and_says_why`,
   `reuse::tests::a_cancelled_job_has_no_line_to_reuse`,
   `reuse::tests::a_range_that_runs_backwards_or_past_the_line_is_rejected`,
   `reuse::tests::a_copy_that_the_line_cannot_hold_is_rejected`,
   `reuse::tests::a_refusal_keeps_the_selection`,
   `reuse::tests::reasons_name_what_could_not_be_reused`.
10. Submitting, delivering, evicting and reusing write no file: the files of
    the working directory are the same before and after. Test:
    `tests/memory_only.rs`: `the_scratchpad_writes_no_file`.
11. The output lines that the tests give the controllable backend are those
    that kalyx 0.1.0 writes for the same requests through the Scratchpad on
    the host stub, where the terminal's way of submitting, answering,
    waiting and delivering works. Test: `tests/with_kalyx.rs`:
    `the_output_lines_of_the_tests_are_those_of_kalyx`.
