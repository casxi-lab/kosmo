# Job layer

The library `kosmo-jobs` in the `framework/` workspace runs computations as
versioned jobs. A submitter hands it the requests of a job with the versions
of the content they were converted from; the job layer hands the job to a
backend, keeps every job in exactly one terminal state, checks the versions
that the computation echoes, and lets the submitter take each terminal state
once, with the output lines of the job. Whether a result is then current is
judged against the region's versions and its latest submission, as
[current, superseded and stale results](../../architecture.md#current-superseded-and-stale-results)
describes.

The backend of the first release is a protocol stub on the host: the host
stub of `kosmo-jobs`, which runs one job at a time on one worker thread, and
the kalyx runner of `kosmo-math-kalyx`, which executes the requests of each
job with kalyx 0.1.0 on that thread. `kosmo-jobs` depends on no computation
library: it defines the ports, and `kosmo-math-kalyx` implements the runner
port. A controllable backend in `kosmo-jobs` implements the same backend port
for tests, which arrange its reports and the order of its events. On the
device, a backend for krepi's job service is planned in place of the host
stub; the job layer itself stays as it is. The design rules are in
[completion and cancellation](../../architecture.md#completion-and-cancellation)
and the [host job layer](../../architecture.md#host-job-layer) of the
architecture.

## Using it

A submitter opens a session with `JobLayer::open_session` and submits a
`Submission`: the session, one request or up to four requests in order, each
an operation (`Eval`, `Expand` or `Diff`) with its variable and its linear
text, the three versions that the text was converted from, one set of limits,
of which each request gets its own copy, and an optional time limit in
nanoseconds. `submit` accepts the job
and returns its identity, or refuses it. `poll` applies the reports that have
arrived and returns the jobs whose terminal states wait to be taken; `take`
takes one, and `wait` waits for one job and takes its terminal state. `cancel`
and `cancel_session` answer at once whether they were accepted. `shutdown`,
which dropping the job layer also does, cancels every job that has not ended
and stops the backend.

On the host, the job layer runs on the host stub with the kalyx runner, whose
running half reads a fork of the job layer's clock:

```rust
use kosmo_jobs::stub::{HostStub, WorkerNotStarted};
use kosmo_jobs::{JobClock, JobLayer};
use kosmo_math_kalyx::{KalyxPreparer, KalyxRunner};

fn host_layer(
    clock: Box<dyn JobClock>,
) -> Result<JobLayer<HostStub<KalyxPreparer>>, WorkerNotStarted> {
    let stub = HostStub::new(KalyxPreparer::new(), KalyxRunner::new(clock.fork()))?;
    Ok(JobLayer::new(stub, clock))
}
```

The crate documentation of both libraries has complete examples. `cargo test`
runs one job on the controllable backend and a job on the host stub with a
runner of its own. The example of a line converted, run with kalyx and read
back into the display model is only compiled; the tests in
`framework/math-kalyx/tests/` run the same steps. Run the tests from
the repository root with:

```sh
cargo test --locked --manifest-path framework/Cargo.toml -p kosmo-jobs
cargo test --locked --manifest-path framework/Cargo.toml -p kosmo-math-kalyx
```

## Jobs and bounds

A job is `queued` from its acceptance until its backend reports that it
started, then `running`, or `cancel_requested` once a cancel request was
accepted while it ran. It ends `completed`, `failed`, `cancelled` or
`resource_exhausted`. A job is live from its acceptance until its terminal
state has been taken and its backend has finished it.

- One job layer has at most `MAX_LIVE_JOBS`, 8, live jobs. A submission
  while 8 jobs are live is refused as `Rejected::Full` with the number of
  live jobs: no job is created, no identity is used, nothing blocks and no
  job is evicted. Every live job keeps room for its terminal state, so a
  terminal state is never lost and its delivery never waits.
- A job has at most `MAX_REQUESTS_PER_JOB`, 4, requests. Each request gets
  its own copy of the job's limits; nothing scales them by the number of
  requests.
- Job and session identities count up from 1, are never reused and never
  wrap. After the last job identity, `submit` refuses with
  `Rejected::Exhausted`; after the last session identity, `open_session`
  does.
- The nine limits default to `Limits::new()` of kalyx 0.1.0.

## Ports

The job layer reaches its backend only through the port `JobBackend`:
`submit` hands it an accepted job as a `JobRequest`, `cancel(id)` sets the
cancellation of a job after the job layer accepted a cancel request,
`try_report` and `wait_report` give its reports, and `shutdown` stops it
once the job layer has cancelled every job that has not ended. A backend
reports `Started(id)` for every job that starts and exactly one `Finished`
for every job it receives, in any order across jobs.

The host stub is generic over a runner of two halves:

- On the job-layer thread, `JobPreparer::prepare(id)` returns the job's
  cancel handle and a token, the runner's own value for the job. The kalyx
  runner makes a kalyx `CancelFlag` for each job: the handle sets it, and the
  token, a `JobFlag`, carries a clone to the running half. The preparing half
  keeps nothing.
- A `CancelHandle` is `Send` and can be cloned, and setting it is its only
  operation: it has `new` and `set`, and nothing can read or clear a
  cancellation through it. The host stub keeps each handle until the job's
  `Finished` arrives.
- On the worker thread, `JobRunner::run(item, token)` runs the job. The work
  item is the `JobRequest` with its cancel handle; the token comes beside it,
  untouched. The runner runs one job at a time.

`JobClock` is the clock port: readings in nanoseconds that never decrease,
and forks that read the same clock. `SessionCancel` is the port that cancels
every job of a session; the job layer implements it, and it is object safe.
Port types are `#[non_exhaustive]` and only grow.

## Deadlines

A submission without a time limit has no deadline, and its acceptance reads
no clock. A submission with a time limit is accepted with one reading of the
job layer's clock: its deadline is that reading plus the limit, saturating,
and it is written into the `JobRequest`. A deadline has passed when a
reading is at least the deadline, so it keeps passing while a job waits for
the worker thread.

The kalyx runner reads a fork of the clock that the job layer was built with,
and gives every context it builds a further fork, through an adapter that
reads the job layer's clock as kalyx's `Clock`. That the runner's clock is a
fork of the job layer's clock is up to the code that builds both; nothing
checks it. Tests inject a fake clock that they set themselves.

## Cancellation

- A cancel request for a job that has not started, as far as the job layer
  knows, is answered `CancelAnswer::Cancelled`: the job is `cancelled` at
  once and delivers nothing. A backend that has not started it stops it
  before its first request; whatever it reports for the job is discarded.
- A cancel request for a running job is answered `CancelAnswer::Requested`:
  the job moves to `cancel_requested`, only then is its cancellation set, and
  it ends `cancelled` once its backend reports its end, whatever that report
  holds.
- Before each request, the first included, the kalyx runner checks the job's
  flag and then its deadline, and stops the job there without running the
  request. Within a request, kalyx checks the flag at each charge point,
  before it observes the deadline on its context's schedule, so a running
  request stops at the next charge point after the cancellation becomes
  visible. A passed deadline stops it within the bound that
  [Limits](#limits) states.
- `cancel_session` cancels every job of the session that has not reached its
  terminal state, queued ones included, and returns how many queued jobs it
  ended and how many running jobs it moved to `cancel_requested`. A second
  call accepts nothing, and other sessions' jobs do not change.
- Acceptance comes first: a cancel request accepted before a job's
  `Finished` is handled ends it `cancelled` and discards its result, and a
  cancel request after its terminal state is answered
  `CancelAnswer::AlreadyTerminal`.

## Requests with kalyx

The kalyx runner builds a kalyx `Request` for each request of a job: the
operation by its name, `eval`, `expand` or `diff`, the variable and the
linear text unchanged, the source, context and settings versions of the job,
and `Settings::new()`. It runs the request under
`Context::with_observers(limits, clock, flag)` with the job's limits, a fork
of its clock and a clone of the job's flag, and with the job's deadline when
the job has one. It then writes the outcome's output line with `serialize`
under the same context.

- A line that fails under the request's context is written again for an
  error outcome, as the fallback line, under the default limits, a fork of
  the clock, a new flag and no deadline. A value or a state is never written
  again, so the request has no line.
- A request whose outcome or line stopped at the cancellation or the
  deadline is the job's last request.
- The report of a request holds the kind of its outcome, the class of the
  error of writing its line (`ErrorClass`, the job layer's own type, unlike
  kalyx's `ErrorClass` of an error), the line's bytes as `serialize` wrote
  them, final line feed included, and the versions that the outcome echoed.
- The job layer neither parses nor changes a line. The submitter reads the
  value, state or error from it with `read_output_line` of
  `kosmo-math-kalyx`, against the snapshot whose text it submitted.

## Versions and echoes

The submitter gives the versions of the snapshot it converted: the source
version of the snapshot, which is at least 1, and the context and settings
versions. The backend builds the echo of each request from the numbers that
its computation gave back, with `EchoedVersions::new`; the kalyx runner takes
the three version numbers of kalyx's `Outcome`. The job layer compares each
echo, version by version, with the job's versions. Outside
`cancel_requested`, an echo that differs fails the job, marked as an internal
error, and no line is delivered.

## Terminal states

One pure function of the job layer's core computes the terminal record of
each job whose `Finished` it handles, from that `Finished`, the job's versions
and number of requests, and whether a cancel request was accepted while the
job ran; a job cancelled while queued gets its record when the cancellation
is accepted. The record holds the
terminal state, the stop, the delivered lines and the echoed versions. A stop
is one of five reasons: cancellation, deadline, forced stop, CPU budget and
backend internal error. A record is marked as forced exactly when its stop is
a forced stop or a CPU budget; the mark follows from the stop and is not a
separate field.

| Request outcome | Delivered line | Without `cancel_requested` | In `cancel_requested` | Stops the job |
| --- | --- | --- | --- | --- |
| A value | `value` | `completed` | `cancelled`, no line | No |
| A state | `state` | `completed` | `cancelled`, no line | No |
| A fault | `error` or fallback | `failed` | `cancelled`, no line | No |
| A limit of the request | `error` or fallback | `failed` | `cancelled`, no line | No |
| `Cancelled` | None | Cannot occur: the flag is set only after acceptance | `cancelled` | Yes |
| `DeadlineExceeded` | Fallback `error` | `resource_exhausted` | `cancelled`, no line | Yes |
| Its line fails | Fallback for an error, none for a value or state | As the error of the line: a cancellation cannot occur, a passed deadline gives `resource_exhausted`, anything else fails the request and the job goes on | `cancelled`, no line | As its error |

- A stop seen between requests ends the job: a cancellation `cancelled`, a
  passed deadline `resource_exhausted`. A forced stop or an exhausted CPU
  budget, which only a backend reports, gives `cancelled` in
  `cancel_requested` and otherwise `resource_exhausted`; a backend internal
  error gives `cancelled` in `cancel_requested` and otherwise `failed`, marked
  as an internal error, without lines.
- A stop takes precedence over earlier faults and limits. Without a stop, a
  fault or a limit fails the job, and otherwise it is `completed`.
- A `cancelled` job delivers no line, not even those of requests that ran
  before the cancellation. A `resource_exhausted` job delivers the lines of
  the requests that ran; a request interrupted by a forced stop or a CPU
  budget has no report and no line.
- A request's own stop comes first. When a request report already stopped the
  job and the backend also reports a forced stop, a CPU budget or an internal
  error, the record keeps the request's stop, without a forced or internal
  error mark.
- Reports that do not fit the job, such as more reports than requests, a
  report after one that stopped the job, or fewer reports without a stop,
  fail it like an echo that differs. A cancellation reported outside
  `cancel_requested` gives `cancelled`. Only a `failed` record is marked as
  an internal error.
- A record keeps the echo of the requests that ran even when the job is
  `cancelled` and delivers no line: the job's own versions when every echo
  matched, otherwise the first echo that differs, and none when no request
  ran.

## Protocol stub fields

The fields of the protocol stub against the items of casξ's interface for
computation jobs and the fields of kalyx's `Request`:

| Field of the job layer | Item of the job interface | Field of kalyx's `Request` | Notes |
| --- | --- | --- | --- |
| Job identity of a `JobRequest` (`JobId`, `u64`) | Request or job identity | None | Assigned on acceptance, never reused or wrapping, refused when exhausted; kalyx's `Outcome` carries no identity either |
| Session of a `JobRequest` (`SessionId`) | None | None | Issued by `open_session`, opaque, never reused; only the job layer has it |
| Operation of a request (`Operation`: `Eval`, `Expand`, `Diff`, `#[non_exhaustive]`) | None | `operation` | The kalyx runner writes `eval`, `expand` or `diff`; no other name can be submitted |
| Variable of a request | None | `variable` | Passed on unchanged; kalyx judges it |
| Linear text of a request | None | `input` | What formula conversion produced, passed on unchanged; mapping ranges back stays with the submitter |
| `versions.source` | Input or context version | `input_version` | The version of the converted snapshot, at least 1; passed on unchanged and checked against the echo |
| `versions.context` | Input or context version; document or context version | `context_version` | 0 in the first release; passed on unchanged and checked against the echo |
| `versions.settings` | Input or context version | `settings_version` | Passed on unchanged and checked against the echo |
| No field | None | `settings` | Always `Settings::new()`, the only value that kalyx 0.1.0 can build |
| Nine limits of a `JobRequest`, defaulting to kalyx's `Limits::new()` | Algorithm budget and system resource scope | None | Each request gets its own `kalyx::Limits` in its own context; there is no resource scope field yet |
| Job deadline, an absolute reading of the injected clock | Monotonic deadline | None | Computed from the time limit on acceptance; each request's context gets `with_deadline(job deadline)`, and the runner checks it between requests |
| Cancel handle of the work item (`CancelHandle`, not in `JobRequest`) | Cancellation checkpoint | None | The runner makes and keeps a kalyx `CancelFlag` for each job, and the handle only sets it; within a request kalyx checks the flag at its charge points, between requests the runner checks it first |
| `JobBackend::cancel(id)` and `SessionCancel::cancel_session` | Cancellation checkpoint | None | Set only after the job layer accepted the cancellation; a queued job is `cancelled` at once |
| Reports `Started(id)` and `Finished(id, request reports, stop)` | Structured result or error | None | Exactly one `Finished` for each job a backend receives; the stop covers stops between requests and forced stops and CPU budgets within a request |
| Request report: outcome kind, error class, line bytes | Structured result or error | None | Kinds `Value`, `State`, `Fault`, `ResourceLimit`, `Cancelled`, `DeadlineExceeded`; the classes include a failed `serialize`; the submitter reads the value, state or error from the line with `read_output_line` |
| Echoed versions of a request report | Input or context version | None: the echo of `Outcome`, fields 2 to 4 of the line | The runner takes them from `Outcome`; the job layer checks them, and an echo that differs fails the job outside `cancel_requested`, marked as an internal error |
| Terminal record: state, stop, delivered lines, echoed versions, forced mark | Structured result or error | None | Five stop reasons; the forced mark follows from the stop |
| Line bytes, a `Vec<u8>` that moves with its report | Temporary and result memory ownership; result ownership | None | Nothing is shared; kalyx terms, `Outcome` and `Context` stay on the worker thread, and delivered lines belong to the submitter; pending line bytes are at most 8 × 4 × the largest of 65,536 and every request's `max_output_bytes` |
| No field | Representative workload | None | kalyx's resource profile is a measurement, not a bound; the worker thread's stack is set to 2 MiB |
| No field; a later one will be a new field with a default | Parent resource scope | None | No resource scope in the first release; on the host nothing bounds real memory or CPU time, see [Limits](#limits) |
| No field | Service epoch | None | The host stub has no service restart; after its worker thread ends, the job layer has to be rebuilt to run jobs again |
| No field | Parallelism limit | None | The host stub has one worker thread; the core sets no bound on running jobs and assumes no report order, and `MAX_LIVE_JOBS` bounds live jobs, not parallelism |

## Host stub

The host stub creates its worker thread when it is built, named
`kosmo-jobs-worker` and with a stack of 2 MiB; if the thread cannot be
created, `HostStub::new` returns `WorkerNotStarted`. Jobs reach the worker
through a channel of 8 places and reports return through one of 16, so
neither ever blocks. The worker runs one job at a time in the order it
received them and takes its next job by itself; reports reach the job layer
when it polls or waits.

If the worker thread ends before it reports a job's `Finished`, by a panic
of the runner for example, that job and every job handed to the worker or
submitted afterwards end with the stop "backend internal error", and the job
layer has to be rebuilt. Shutting down, also on drop, cancels every job that
has not ended, lets the worker stop each remaining job before its first
request, and joins the worker; `HostStub::live_threads` is 0 afterwards.

## Limits

- On the host, nothing bounds the real memory or the CPU time of a job. kalyx
  bounds each request by its nine logical limits. A cancellation stops the
  request at its next charge point, and a passed deadline stops it within
  the overrun bound that kalyx's context contract states; a primitive
  operation that runs long between two charge points is not interrupted.
  Bounds of real memory and CPU time are planned with the backend for
  krepi's job service.
- Exhaustions of a job's resource scope other than its CPU budget have no
  input in the first release: there is no resource scope, the terminal
  function has no stop for them, and a forced stop does not stand in for
  them. krepi's job contract will define them.
- The worker thread's stack is 2 MiB. kalyx 0.1.0's resource profile measured
  a stack of at most 9,728 bytes for its cases, which is a measurement on one
  host, not a bound.
- The host stub produces neither forced stops nor exhausted CPU budgets; only
  the controllable backend produces them, in tests. kalyx 0.1.0 has no
  operation that ends in a state, so state lines too come only from the
  controllable backend.
- kalyx reads its flag with relaxed ordering, so a cancellation set on the
  job-layer thread is seen at the first charge point after it becomes
  visible on the worker thread. The tests make that order definite with a
  gate at a reading of the worker thread's clock.
- In kalyx 0.1.0 the fallback line always fits. If it failed, the request
  would have no line and its class would record the failed line; the job
  would end by the last row of the table, not as a backend internal error.
- A terminal record lists the lines of the requests that have one, in order,
  but not which request each belongs to, nor the kinds and classes of the
  requests. The submitters of the first release submit jobs of one request.
- The runner's clock has to be a fork of the job layer's clock; see
  [Deadlines](#deadlines).

## Requirements

The tests are in `framework/jobs` and `framework/math-kalyx`: the
unit tests in the `tests` modules under each crate's `src/` (named here with
their crate and module, such as `kosmo-jobs` `machine::tests::…`) and the
integration tests in each crate's `tests/` (named with their crate and file).
The tests of whether a result is current are the unit tests
`kosmo-math-model` `version::tests::…` in `framework/math-model`, and
the tests of the layout step are in `xtask/src/layout/tests.rs`.
The integration tests of `kosmo-math-kalyx` run jobs on the host stub with
the kalyx runner; those of `kosmo-jobs` use the controllable backend or the
host stub with a runner of their own. No test sleeps or reads the wall clock:
events are ordered by gates of the controllable backend, by handshakes with a
test runner, or by a gate at a reading of a fake clock.

1. A job has one to four requests, and the job layer refuses a submission
   while eight jobs are live, as full with their number, until one is taken
   and finished; identities count up, a refused submission uses none, and
   they never wrap. Tests: `kosmo-jobs` `request::tests::a_job_holds_one_to_four_requests`,
   `machine::tests::identities_count_up_and_a_refused_submission_uses_none`,
   `machine::tests::identities_never_wrap`,
   `machine::tests::taken_jobs_that_the_backend_has_not_finished_still_count_as_live`;
   `kosmo-jobs` `tests/bounds.rs`:
   `a_submission_beyond_the_live_jobs_is_refused_as_full_until_one_is_taken`.
2. An accepted job carries its submission and limits. A time limit becomes a
   deadline from one reading of the clock on acceptance; without a time
   limit there is no deadline and no reading. Tests: `kosmo-jobs`
   `request::tests::an_accepted_job_keeps_the_submission_and_its_deadline`;
   `kosmo-jobs` `tests/shell.rs`:
   `an_accepted_job_carries_its_limits_and_a_deadline_read_once_from_the_clock`.
3. The default limits are those of kalyx 0.1.0, field by field; each limit
   of a job reaches its own field of kalyx's limits, and each request of a
   job gets its own copy of them. Tests:
   `kosmo-jobs` `request::tests::the_default_limits_are_those_of_kalyx`;
   `kosmo-math-kalyx`
   `runner::tests::the_default_limits_of_a_job_are_those_of_kalyx_field_by_field`,
   `runner::tests::each_limit_of_a_job_goes_into_its_own_kalyx_field`;
   `kosmo-math-kalyx` `tests/job_requests.rs`:
   `each_request_of_a_job_gets_its_own_copy_of_the_limits`,
   `the_second_request_runs_under_the_limits_of_the_job`.
4. The kalyx runner gives kalyx the operation by its name, the variable, the
   linear text, the job's three versions and the default settings. Tests:
   `kosmo-jobs` `request::tests::the_operations_have_the_names_of_the_command_entry`;
   `kosmo-math-kalyx`
   `runner::tests::a_kalyx_request_carries_the_operation_text_and_versions_of_the_job`.
5. Each row of the table of terminal states gives its state: a value
   completes the job with its value line, a state with its state line, a
   fault and a limit of the request fail it with their error lines and do
   not stop it, a request that saw the cancellation ends it `cancelled`
   without lines, a deadline that kalyx sees exhausts it with the fallback
   line, and a value line that does not fit fails it without a line. Tests:
   `kosmo-math-kalyx` `tests/job_outcomes.rs`:
   `a_value_completes_the_job_and_delivers_its_value_line`,
   `a_fault_fails_the_job_with_its_error_line_without_stopping_it`,
   `a_limit_of_the_request_fails_the_job_with_its_error_line_without_stopping_it`,
   `a_deadline_that_kalyx_sees_exhausts_the_job_with_the_fallback_line`,
   `a_value_line_that_does_not_fit_fails_the_job_without_a_line`;
   `kosmo-jobs` `tests/terminal_states.rs`:
   `a_state_line_completes_the_job_and_is_delivered`,
   `a_request_cancelled_after_acceptance_ends_the_job_cancelled_without_lines`;
   `kosmo-jobs` `machine::tests::every_row_of_the_outcome_table_maps_to_its_terminal_state`,
   `machine::tests::a_cancelled_request_ends_the_job_cancelled_without_lines`.
6. A job in `cancel_requested` ends `cancelled` whatever its report holds,
   a `DeadlineExceeded` included, delivers no line, and keeps the stop that
   the terminal function found. Tests: `kosmo-math-kalyx`
   `tests/job_outcomes.rs`:
   `a_deadline_that_kalyx_sees_after_a_cancel_request_ends_the_job_cancelled`;
   `kosmo-jobs`
   `machine::tests::a_cancelled_record_keeps_the_stop_that_the_terminal_function_found`.
7. A cancel request accepted before a job's `Finished` is handled ends it
   `cancelled` and discards its result; one after its terminal state is
   answered as already terminal, and a terminal state is absorbing and taken
   once. Tests: `kosmo-jobs` `tests/terminal_states.rs`:
   `a_cancel_accepted_before_the_finished_report_is_handled_discards_the_result`;
   `kosmo-jobs` `machine::tests::cancel_answers_follow_the_state_of_the_job`,
   `machine::tests::a_terminal_state_is_absorbing_and_taken_once`,
   `machine::tests::a_started_report_does_not_withdraw_a_cancellation`;
   `kosmo-jobs` `tests/bounded_orders.rs`:
   `no_bounded_order_lets_an_older_result_replace_a_newer_one`.
8. A job of several requests runs them in order, stops at a cancellation or
   a deadline and goes on after a fault or a limit; a stop takes precedence
   over earlier faults, and a deadline seen before the second request or a
   forced stop in it still delivers the line of the first. Tests:
   `kosmo-math-kalyx` `tests/job_requests.rs`:
   `a_fault_in_the_first_request_does_not_stop_the_second`,
   `a_deadline_seen_between_two_requests_delivers_the_line_of_the_first`;
   `kosmo-jobs`
   `machine::tests::stops_between_requests_and_forced_stops_end_the_job`,
   `machine::tests::a_stop_takes_precedence_over_earlier_faults`,
   `machine::tests::faults_and_limits_do_not_stop_a_job_of_several_requests`;
   `kosmo-jobs` `tests/terminal_states.rs`:
   `a_forced_stop_in_the_second_request_delivers_the_line_of_the_first`.
9. A forced stop and an exhausted CPU budget end a job `cancelled` in
   `cancel_requested` and otherwise `resource_exhausted`, both marked as
   forced, and only those two stops are forced. Tests: `kosmo-jobs`
   `tests/terminal_states.rs`:
   `an_exhausted_cpu_budget_without_a_cancel_request_ends_resource_exhausted_and_forced`,
   `an_exhausted_cpu_budget_after_a_cancel_request_ends_cancelled_and_forced`,
   `a_forced_stop_after_a_cancel_request_ends_cancelled_and_forced`;
   `kosmo-jobs` `report::tests::only_forced_stops_and_cpu_budgets_are_forced`.
10. An echo that differs from the job's versions in any of them, and reports
    that do not fit the job, fail it outside `cancel_requested`, marked as an
    internal error and without lines; the record keeps the echo of the
    requests that ran, or the first echo that differs. Tests: `kosmo-jobs`
    `tests/terminal_states.rs`:
    `an_echo_of_other_versions_fails_the_job_unless_it_was_cancelled`;
    `kosmo-jobs`
    `machine::tests::an_echo_of_other_versions_fails_the_job_outside_cancel_requested`,
    `machine::tests::an_echo_that_differs_in_any_one_version_fails_the_job`,
    `machine::tests::an_echo_of_an_older_source_version_fails_the_job`,
    `machine::tests::the_record_keeps_the_echo_of_the_requests_that_ran`,
    `machine::tests::the_record_keeps_the_first_echo_that_differs`,
    `machine::tests::reports_that_do_not_fit_the_job_fail_it`,
    `machine::tests::the_core_checks_reports_against_the_requests_of_the_job`,
    `report::tests::an_echo_keeps_its_three_numbers`.
11. A line is delivered as kalyx wrote it. A value line that fails under the
    request's context is not written again; an error line that fails is
    written again as the fallback line under the default limits, a new flag
    and no deadline; a line that fails with a fault or a limit fails the
    request without stopping the job, and one that saw the cancellation or
    the deadline stops it. Tests: `kosmo-jobs`
    `report::tests::a_report_gives_back_its_line_bytes_unchanged`,
    `report::tests::values_and_states_have_no_error_unless_their_line_failed`,
    `report::tests::a_fault_in_writing_a_line_fails_the_request_and_does_not_stop_the_job`,
    `report::tests::faults_and_limits_fail_and_cancellation_comes_before_the_deadline`,
    `report::tests::the_kind_of_an_outcome_stops_the_job_whose_line_was_written`;
    `kosmo-math-kalyx`
    `runner::tests::a_value_line_is_written_under_the_context_of_its_request`,
    `runner::tests::a_value_whose_line_does_not_fit_is_not_written_again`,
    `runner::tests::an_error_line_that_fits_is_written_under_the_context_of_its_request`,
    `runner::tests::an_error_line_that_does_not_fit_is_written_again_as_the_fallback_line`,
    `runner::tests::a_set_flag_stops_the_request_and_its_line_and_the_fallback_line_is_written`,
    `runner::tests::a_passed_deadline_stops_the_request_and_the_fallback_line_reads_no_clock`,
    `runner::tests::a_line_that_fails_with_a_fault_is_a_failed_request`.
12. A job cancelled before it starts runs no request: the job layer ends it
    `cancelled` on acceptance and discards what its backend reports for it.
    Tests: `kosmo-math-kalyx` `tests/job_cancellation.rs`:
    `a_job_cancelled_before_it_starts_runs_no_request`; `kosmo-jobs`
    `machine::tests::reports_of_a_job_cancelled_while_queued_are_discarded`.
13. A running job stops at the next charge point after its cancellation and
    ends `cancelled` without lines. Test: `kosmo-math-kalyx`
    `tests/job_cancellation.rs`:
    `a_running_job_stops_at_the_next_charge_point_after_its_cancellation`.
14. A cancellation seen between two requests runs no further request and
    ends the job `cancelled`. Tests: `kosmo-math-kalyx`
    `tests/job_cancellation.rs`:
    `a_cancellation_seen_between_two_requests_runs_no_second_request`;
    `kosmo-jobs` `tests/terminal_states.rs`:
    `a_cancellation_seen_between_two_requests_runs_no_second_request`.
15. Cancelling a session cancels each of its jobs that has not ended once,
    queued ones included, returns the counts of that call, accepts nothing
    the second time and leaves other sessions' jobs alone; the port is
    object safe. Tests: `kosmo-jobs` `tests/session_cancel.rs`:
    `cancelling_a_session_cancels_its_queued_and_running_jobs_once`,
    `the_session_cancel_port_can_be_used_as_a_trait_object`.
16. A cancel handle only sets its own job's cancellation, which the runner
    reads through its token, and the host stub lets go of each handle once
    the job has finished. Tests: `kosmo-jobs`
    `port::tests::every_clone_of_a_handle_sets_the_same_target`;
    `kosmo-jobs` `tests/host_stub.rs`:
    `the_runner_sees_each_cancellation_and_the_stub_lets_go_of_each_handle`,
    `cancelling_one_job_sets_only_its_own_cancellation`; `kosmo-jobs`
    `tests/shell.rs`:
    `a_cancel_request_marks_only_its_job_in_the_controllable_backend`;
    `kosmo-math-kalyx`
    `runner::tests::the_handle_of_a_job_sets_the_flag_that_only_its_own_token_reads`.
17. A deadline goes through the injected clock: a job whose deadline passed
    while it was queued runs no request and ends `resource_exhausted` with
    the stop "deadline" and no line. Test: `kosmo-math-kalyx`
    `tests/job_deadlines.rs`:
    `a_deadline_that_passed_while_the_job_was_queued_runs_no_request`.
18. A result is current only while all three of its versions equal the
    region's and it belongs to the latest submission; an older result that
    arrives late or early goes only to its own record, and no bounded order
    of jobs, version changes and reports lets an older result replace a
    newer one. Tests: `kosmo-math-model`
    `version::tests::a_result_of_the_latest_submission_at_the_current_versions_is_current`,
    `version::tests::a_result_of_another_submission_at_the_current_versions_is_superseded`,
    `version::tests::a_result_with_any_other_version_is_stale_whatever_its_submission`,
    `version::tests::the_judgement_compares_submissions_only_for_equality`;
    `kosmo-jobs` `tests/late_and_early_results.rs`:
    `a_late_result_of_an_older_version_goes_only_to_its_record`,
    `an_early_result_of_an_older_version_goes_only_to_its_record`;
    `kosmo-jobs` `tests/bounded_orders.rs`:
    `no_bounded_order_lets_an_older_result_replace_a_newer_one`.
19. The host stub runs jobs on one named worker thread; a full stub refuses
    nothing and blocks nothing; a runner panic fails its job and every later
    one, each with exactly one terminal state; shutting down or dropping the
    job layer, also after a panic, leaves no live worker. Tests: `kosmo-jobs`
    `tests/host_stub.rs`:
    `shutdown_leaves_no_live_worker_and_one_terminal_state_per_job`,
    `a_runner_panic_fails_its_job_and_every_later_job`,
    `dropping_the_layer_or_shutting_down_after_a_panic_leaves_no_live_worker`,
    `a_full_stub_refuses_nothing_blocks_nothing_and_finishes_each_job_once`,
    `a_panic_fails_the_held_job_and_the_jobs_in_the_dispatch_channel_once_each`,
    `a_shutdown_after_a_worker_panic_finishes_the_jobs_left_in_the_dispatch_channel`,
    `a_worker_panic_during_a_shutdown_finishes_the_job_in_the_dispatch_channel`;
    `kosmo-jobs` `tests/shell.rs`:
    `a_shutdown_ends_a_running_job_cancelled_before_it_returns`,
    `a_shutdown_finishes_the_jobs_that_have_no_plan`.
20. The job layer's code is the same on the host stub with the kalyx runner
    and on the controllable backend, with the same terminal states and
    echoed versions, and `kosmo-jobs` depends neither on kalyx nor on
    `kosmo-math-kalyx`. Tests: `kosmo-math-kalyx` `tests/job_backends.rs`:
    `the_job_layer_runs_alike_on_the_kalyx_runner_and_the_controllable_backend`;
    `xtask/src/layout/tests.rs`: `no_other_crate_declares_kalyx_in_any_table`,
    `crates_may_reach_kalyx_through_the_adapter`.
21. The core's step function gives the same answers as its typed methods.
    Test: `kosmo-jobs`
    `machine::tests::the_step_function_gives_the_same_answers_as_the_methods`.

The bounded enumeration in `kosmo-jobs` `tests/bounded_orders.rs` takes its
range from named constants: up to `VERSION_JOBS`, 3, jobs with at most one
change of each version (`SOURCE_CHANGES`, `CONTEXT_CHANGES`,
`SETTINGS_CHANGES`), 11,464 orders; and up to `RACE_JOBS`, 2, jobs with at
most `CANCELS_PER_JOB`, 1, cancel request each and `RACE_SOURCE_CHANGES`, 1,
source change, 4,198 orders and 10,440 cases. The counts of orders,
judgements and terminal states are constants of the test too.
