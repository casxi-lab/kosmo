# Resubmitted input

An integration test of `kosmo-scratchpad` in the `framework/` workspace runs,
on a host, the example of the
[first integration target](../../scope.md#first-integration-target) in which
the derivative of `x^2` with respect to `x` is requested, the input is
changed to `x^3` and submitted again. The input goes through the
[Scratchpad](scratchpad.md) and the [job layer](job-layer.md), and the jobs
run with kalyx 0.1.0 on the host stub's worker thread. The test
runs the example in two orders: the old job is cancelled before kalyx runs
it, or its result arrives after the new one. In both, only the new result,
`3*x^2`, is shown, and whatever the old job delivers goes only into the
history entry of `x^2`. The deliveries of both orders are written as a
record, which equals a checked-in record byte for byte in every run.

The test holds the Scratchpad and the job layer itself, as a frontend does.
It does not go through the `kosmo` program: the
[line-mode terminal](../desktop/line-mode-terminal.md) waits for each job,
so it never has two jobs at once. The design rules that the example checks
are in [results that are not current](../../architecture.md#results-that-are-not-current)
and [completion and cancellation](../../architecture.md#completion-and-cancellation)
in the architecture.

## Running it

The example runs only on a host. Run it from the repository root with:

```sh
cargo test --locked --manifest-path framework/Cargo.toml -p kosmo-scratchpad --test resubmitted_input
cargo test --locked --manifest-path framework/Cargo.toml -p kosmo-math-kalyx --test derivative_by_structure
```

The first command runs both orders, compares their record with the
checked-in one and repeats each order 100 times. The second runs the new
job's derivative on its own and compares its value by structure, with kalyx
0.1.0, since `kosmo-scratchpad` does not depend on kalyx. `cargo xtask check`
runs both with the other tests.

## The two orders

Each run uses a new Scratchpad, with context and settings versions 0, and a
new job layer on the host stub with the kalyx runner and a clock that always
reads 0. The test types `x`, `^` and `2` one character at a time, so the
line is `x^2` at source version 3, and submits its derivative with respect
to `x`, with the default limits and no time limit: the old job. It then
deletes the `2` and types `3`, so the line is `x^3` at source version 5, and
submits the new job the same way. kalyx 0.1.0 answers the new job with the
output line of the value `3*x^2` at the versions 5, 0 and 0, and the old job,
when it runs, with the value `2*x` at the versions 3, 0 and 0.

On its own, the host stub runs one job at a time in submission order, so the
old job always finishes first. Two test gates in
`framework/scratchpad/tests/support/gates.rs` arrange the other
orders. They implement only the public ports of the job layer, so neither the
job layer nor the host stub changes for the test:

- a runner on the worker thread that wraps the kalyx runner and makes the job
  of a chosen submission order wait at a gate, before the kalyx runner gets
  it, until the test opens the gate;
- a backend that wraps the host stub and holds the `Finished` report of the
  job of a chosen submission order until the test releases it.

The test waits for the worker at the gate through a mutex and a condition
variable; nothing sleeps or reads the wall clock. Waiting at the gate fails
instead of hanging when the host stub has not received the job at the gate,
once that job has ended without waiting there, or once the worker thread has
ended. Dropping the job layer of a run opens the
gate and releases the hold first, so a test that fails still ends.

**The old job is cancelled before kalyx runs it.** The old job waits at the
gate. Once it waits there, the test lets the job layer handle its `Started`
report, so the job is running, submits the new job, so two jobs are live, and
cancels the old one, which the job layer answers with `Requested`. Then it
opens the gate. The kalyx runner finds the cancellation before the first
request, so kalyx never runs for the old job: its `Finished` has no request
report and the stop "cancellation", and the job ends `cancelled` without a line.
The new job runs after it. The test delivers the old job's terminal state,
which is stale and leaves the old entry without a display item, and then the
new one's, which is current and shown. When the test cancels the old job
before the job layer has handled its `Started`, the job layer answers
`Cancelled` and ends the job at once; the kalyx runner still stops it before
its first request, and the record is the same.

**The old result arrives late.** Nothing is cancelled and no job waits at a
gate. The backend holds the old job's `Finished`. The worker still runs the
old job first, but the job layer sees the new job finish first: the test
waits for the new job and delivers its result, which is current and shown.
Then it releases the hold, polls the job layer and takes and delivers the old
job's terminal state. The old job ends `completed` with `2*x`, is judged
stale and goes into the entry of `x^2` at source version 3. The new entry,
the latest submission, the discard count and the shown result stay as they
were.

## The record

The test keeps its own shown result in place of a result slot: the result of
the last delivery that the Scratchpad judged current. Each run of an order
writes a record of its deliveries, only in memory, and the record of the
example is that of the cancelled old job followed by that of the late old
result. It is UTF-8 text with a line feed at the end of every line and no
header or empty line; the lines of the first order start with `cancel: `,
and those of the second with `late: `. Each delivery writes one line, in the
order of the deliveries:

- A terminal state that went into its entry writes
  `job <n> <operation> <variable> <input>: <judgement>: <result>`.
  `<input>` is the linear text of the entry's input and `<judgement>` is
  `current`, `superseded` or `stale`. `<result>` is the display item as the
  display model renders it, as current for a delivery judged current and as
  stale otherwise; an entry without a display item writes its terminal state
  with the versions of its submission instead, such as
  `cancelled (source 3, settings 0)`.
- A terminal state that no entry took writes `job <n> discarded: <judgement>`.

The last line of each order writes `shown` and the shown result, or `shown -`
when no delivery was judged current. The context version is always 0 and is
not written. The checked-in record,
`framework/scratchpad/tests/data/resubmitted-input.txt`, is:

```text
cancel: job 1 diff x x^2: stale: cancelled (source 3, settings 0)
cancel: job 2 diff x x^3: current: value (source 5, settings 0): 3*x^2
cancel: shown value (source 5, settings 0): 3*x^2
late: job 2 diff x x^3: current: value (source 5, settings 0): 3*x^2
late: job 1 diff x x^2: stale: value (source 3, settings 0, stale): 2*x
late: shown value (source 5, settings 0): 3*x^2
```

The test never writes the file and has no switch that regenerates it: a
change of the expected record is a change of the file under review.

## Limits

- The example runs only on a host, on the host stub's single worker thread
  and with kalyx 0.1.0 as `framework/Cargo.lock` pins it. On the device it
  will run with the display server holding krepi's display and input device
  services, as the
  [first integration target](../../scope.md#first-integration-target)
  describes, and its results there will come from krepi's tests.
- It goes neither through the planned framework runtime, its result slots
  and its event record, nor through the `kosmo` program or a display
  server: the test holds the Scratchpad and the job layer and keeps its own
  shown result.
- The display bytes of the checked-in record hold for kalyx 0.1.0 only. A
  change that moves kosmo to another kalyx version checks them again.
- The late order holds a report: on the host stub the old job still runs
  first on the worker thread, and only the job layer sees it finish after
  the new one.
- The old job is cancelled before kalyx runs its request. How a running
  kalyx request stops at its next charge point is in the
  [cancellation](job-layer.md#cancellation) of the job layer.

## Requirements

The tests are integration tests: `tests/resubmitted_input.rs` in
`framework/scratchpad` and `tests/derivative_by_structure.rs` in
`framework/math-kalyx`, named here with their file. Both run jobs on
the host stub with the kalyx runner and a clock that always reads 0, and no
test sleeps or reads the wall clock.

1. The derivative of `x^3` with respect to `x`, typed as `x^2` one character
   at a time and changed to `x^3`, has the output line of the value `3*x^2`
   at the versions 5, 0 and 0, and kalyx 0.1.0 reads that value as a term
   equal by structure to `3*x^2`; the new job of both orders has the same
   line byte for byte. Tests: `tests/derivative_by_structure.rs`:
   `the_derivative_of_x_cubed_equals_three_x_squared_by_structure`;
   `tests/resubmitted_input.rs`:
   `the_old_job_cancelled_at_the_gate_ends_cancelled_and_shows_no_result`,
   `the_late_old_result_goes_only_into_the_entry_of_its_own_version`.
2. The old job, cancelled while it waits at the gate before the kalyx runner,
   ends `cancelled` without a line, and kalyx never runs for it: its
   `Finished` has no request report and the stop "cancellation". Its delivery
   is stale and leaves its entry without a display item, and only the new
   result is shown. The cancellation is answered `Requested` once the job
   layer has handled the job's `Started`, and `Cancelled` before, with the
   same record. Tests: `tests/resubmitted_input.rs`:
   `the_old_job_cancelled_at_the_gate_ends_cancelled_and_shows_no_result`,
   `a_cancel_before_the_start_is_handled_also_stops_the_old_job_before_kalyx`.
3. Without a cancellation, the new job reaches its terminal state first and
   its result is current and shown. The old result arrives afterwards: it is
   stale and goes only into the entry of `x^2` at source version 3, and the
   new entry, the latest submission, the discard count and the shown result
   stay as they were. Test: `tests/resubmitted_input.rs`:
   `the_late_old_result_goes_only_into_the_entry_of_its_own_version`.
4. The record of both orders equals the checked-in record byte for byte, in
   each of two runs. Test: `tests/resubmitted_input.rs`:
   `the_record_equals_the_checked_in_record_in_two_runs`.
5. Each order, repeated `REPETITIONS` times, 100, gives its part of the
   checked-in record every time, and after each run the host stub has no live
   worker thread once the job layer is shut down. Test:
   `tests/resubmitted_input.rs`:
   `each_order_gives_the_same_record_in_every_repetition`.
