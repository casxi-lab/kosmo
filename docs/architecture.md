# Architecture

This page states the design rules of kosmo's first release for the editing
structure of formulas, versions, the host job layer, the keyboard event record
and the platform boundary. It also names the two frontends of the first
release and lists every component with its verification level.

The rules are fixed before the code that follows them. kosmo has no product
code yet: every component here is planned, and nothing described here works
today. Crate names (`kosmo-jobs`, `kosmo-math-model` and the others) and the
names of types, traits and constants (`NodeId`, `JobRunner`, `MAX_LIVE_JOBS`
and the others) are provisional. Changing a rule is a design change, not an
implementation choice.

kalyx here means kalyx 0.1.0 from crates.io, which the first release builds
on. The job-layer thread is the thread that holds the job layer and calls
its API. The layout table is the planned table of the workspace's crates with
their layers, dependencies and platform boundary. [Scope](scope.md) describes
what kosmo will provide.

## Editing structure

The editing structure is the form of a formula while you edit it. It is kept
apart from kalyx's expressions: it holds incomplete input such as `1/□` and
contains no kalyx type.

### Blocks, items and templates

- The structure is a tree in which blocks and templates alternate. A block is
  a sequence of items; an item is a character or a template. A template has a
  fixed number of slots for its kind, and each slot is a block. The root is a
  block.
- One-line input is a root block of characters only. Two-dimensional editing
  only adds templates as items; the types of blocks, items, identities and
  positions do not change.
- Items are stored per character. The character set is fixed when the
  structure is implemented; it is a subset of ASCII that includes the space.
- Whitespace is stored as characters and is only a separator when tokenizing,
  as in kalyx's lexer. Frontends never remove it.
- Token boundaries are not stored; kosmo's tokenizer derives them. A name
  starts with a letter or `_`, followed by letters, digits and `_`. A number is
  a run of digits and dots; a digit followed directly by a letter ends the
  number at the letter, so `2x` is a juxtaposition. kosmo never sends `2x` to
  kalyx, which does not read it as `2*x`. If name splitting later differs
  between frontends, an explicit name-separator item is added; the item enum is
  `#[non_exhaustive]` and only grows.
- Brackets are characters and pair only within one block. Precedence comes
  from one parser written to the grammar of the locked kalyx version; hole
  derivation, completeness, the brackets of reused fragments, the base of a
  superscript and the presentation all use it.
- When an inserted fragment is not a single operand and the insertion point
  needs one, one-line input gets visible brackets: `x+1` inserted into `2*□`
  gives `2*(x+1)`. In two dimensions the template boundary groups. A temporary
  right bracket shown by an interactive frontend is presentation only.

### Node identity

- Blocks and templates carry a `NodeId(u64)`. An editing buffer allocates them
  in increasing order and never reuses one; the root takes the first, and
  clearing does not reset the counter. Characters have no identity.
- Reuse, paste and fragment insertion produce copies with new identities. A
  history entry is an immutable snapshot that keeps the original identities.
- The structure is an ordered map keyed by identity, without pointers,
  serializable with a format version. The same edits run twice give the same
  identities.
- In one-line input only the root has an identity. Diagnostics and ranges are
  (root, half-open item range) and are valid only within one source version.
  A node of the input means a position: a template identity, a block with an
  item range, or the whole line; a node count is an item count.
- The model contains no platform or input and output types, reads no clock or
  random source, and uses no interior mutability and no `unsafe`.
- A rejected edit changes none of the structure, the cursor and the versions.
- Limits are checked at entry: item count, block nesting depth, parse depth
  (bracket nesting and chains of unary minus) and input bytes. Their values
  are fixed when the structure is implemented and are meant to stay within
  kalyx's defaults (`max_nodes` 8,192, `max_depth` 128, `max_input_bytes`
  65,536).

### Positions, fragments and selection

- A position is `Position { block, index }`, with the index between items, from
  0 to the item count. The cursor is buffer state, not source content.
- Left and right follow reading order: right steps over a character, enters a
  template's first slot, moves from slot to slot and leaves after the
  template; left is symmetric. Line start and end are the ends of the root
  block. Up, down and pointing use the layout queries and come with
  two-dimensional editing.
- Edits elsewhere adjust the cursor's index. When the structure that holds
  the cursor is deleted, the cursor goes to the position the deleted structure
  occupied.
- A fragment is (block, half-open item range), used for reusing
  subexpressions and for error ranges. A selection is an anchor and an active
  end in one block and normalizes to a fragment. Interactive selection
  commands are defined with the editing operations.

### Holes and completeness

- Holes inside a block are not stored; parsing derives four kinds: empty
  line, missing operand, missing operator (juxtaposition, including `2x`,
  `2(x+1)` and adjacent templates) and missing right bracket.
- An empty template slot is an explicit hole, identified by its slot block's
  identity. Optional slot kinds are only reserved in the types.
- Holes are ordered in reading order; matrix cells row by row.
- Every state is exactly one of complete, incomplete and invalid:
  - juxtaposition and unclosed brackets are incomplete;
  - `.5` and `1.` are complete; conversion normalizes them to `0.5` and `1`;
  - writings that cannot form a number (`1.2.3`, a lone `.`) and surplus right
    brackets are invalid;
  - a call (a name directly followed by `(`, whitespace allowed between) and
    reserved names are operands and complete; kalyx reports them as
    Unsupported, and the range maps back to the name and the left bracket.
- Whether input is supported is decided by conversion against the grammar of
  the locked kalyx version.
- `□` appears only on the screen and is never sent to kalyx.

| Input | Root items | Holes, as (block, index) |
| --- | --- | --- |
| `1/□` | `1`, `/` | Missing operand at (root, 2) |
| `□+□` | `+` | Missing operands at (root, 0) and (root, 1) |
| `x^□` | `x`, `^` | Missing operand at (root, 2) |
| `(□` | `(` | Missing operand and missing right bracket at (root, 1) |
| Empty line | None | Empty line at (root, 0) |
| `1)` | `1`, `)` | None; invalid at (root, 1) |

### Results never change the editing structure

A job receives only linear text converted from an immutable snapshot. Its
result goes into the result slot and the history entry, never back into the
editing buffer; result types hold no mutable reference to the structure. A
result changes none of the structure, the cursor and the source version.

### Two-dimensional templates

A template holds only the slots it draws.

| Template | Slots | Rule |
| --- | --- | --- |
| Fraction | 2 | One operand of its block |
| Superscript | 1, the exponent | The base is the preceding operand in the same block, found by parsing; without one it is a missing operand |
| Matrix | r × c cells, row-major | Row and column counts are part of the kind. Adding and removing rows and columns are the only edits that change the slot count; other cells keep their identities |

- The template-kind enum is `#[non_exhaustive]`, and each variant with fields
  is marked `#[non_exhaustive]` when it is added. In the first release the
  enum is empty. Other crates cannot write an empty match on an external
  `#[non_exhaustive]` empty enum, so conversion and the controls have a
  fallback arm that reports "no linear form", unreachable in the first
  release. Two-dimensional editing adds only variants, commands and parse
  rules.
- Linear form: template boundaries keep their grouping with added brackets;
  a superscript's base comes from parsing and is bracketed as a whole; a slot
  with unpaired brackets has no linear text; a template with no writing in the
  locked grammar, such as the matrix in kalyx 0.1.0, reports no linear form.
  The exact writing comes with the conversion of two-dimensional input.

### Presentation tree and layout queries

- Node kinds are the elements of MathML Core plus extension nodes: hole,
  ellipsis, condition list, badge and error mark. The enum is
  `#[non_exhaustive]`.
- Every node has exactly one origin. On the input side it is a template
  identity or a block with an item range. On the result side, until kalyx
  provides structured output, it is a block with an item range in the editing
  structure read back from the display text of the output line. Frames such as
  version annotations and badges have no origin. A node that merges several
  sources takes their smallest common enclosing origin. Each presentation
  transform produces origins; they are never recovered from text.
- The presentation tree is read-only and rebuilt deterministically. Layout
  reads only the tree and metrics.
- Layout queries: exactly one caret rectangle per cursor position; a hit test
  defined for every point; selection rectangles; geometry for moving up and
  down; and origin to boxes and back.
- The numeric representation of coordinates and the accessibility hook are
  not fixed here; the planned rendering and determinism rules fix the first,
  and the planned framework architecture the second.
- The mapping of functions to two-dimensional presentation follows the
  function catalog that kosmo keeps for the locked kalyx version.

## Versions

### Three version types

| Version | kalyx `Request` field | Held by |
| --- | --- | --- |
| Source | `input_version` | The region whose input it numbers |
| Context | `context_version` | The region |
| Settings | `settings_version` | The settings holder, one per Scratchpad in the first release |

- They are three distinct `u64` newtypes, compared only within their region or
  holder. Each starts at 0, only increases and never wraps; a change that would
  increase one past its maximum is rejected, leaving structure, cursor and
  versions unchanged.
- The context version is what [Scope](scope.md) calls the environment
  version.
- Source version: every accepted edit command that changes the source content
  increases it by exactly 1, including clearing, fragment insertion and reuse
  insertion, and later undo and redo. Cursor and selection moves, rejected
  commands, commands that change no content, submitting, result arrival and
  cancellation do not change it. Snapshots and history entries keep the
  version they were taken at. An empty line is not submitted, so a submitted
  source version is at least 1.
- Context version: no operation of the first release changes the context, so
  it is constant at 0, the kalyx default. It is still a field of requests,
  result tags and region state, and it takes part in every comparison.
- Settings version: semantic settings count, options that only change the
  presentation do not. The holder increases it by 1 for every accepted
  operation that changes a semantic setting. kalyx 0.1.0 has no settings, so
  it is always 0 outside tests.
- Tests change the context and settings versions by constructing region
  state.

### Current, superseded and stale results

A region records the latest submission that the job layer accepted,
identified by job identity in the first release, and every result carries its
job identity with its three versions. One pure function, shared by the job
layer and the result slot and generic over the submission identity, judges a
result:

| Result versions | Submission | Judgement |
| --- | --- | --- |
| All three equal the region's current versions | The latest | Current |
| All three equal the region's current versions | An earlier one | Superseded |
| Any one differs | Any | Stale |

- Superseded and stale results are handled the same way in the first release.
- A rejected submission does not become the latest. A cancelled or failed
  latest submission does not roll back to an earlier one.
- The version newtypes, the triple `Versions` and this function are in a
  version module of `kosmo-math-model`, the first crate that needs them, where
  the display model's result types also carry versions. `kosmo-jobs`, in the
  same layer, depends on it through a same-layer edge listed in the layout
  table when `kosmo-jobs` is created, and uses only the version module. The
  result slot in `kosmo-ui`, a higher layer, calls the same function.
  `kosmo-math-controls` depends on `kosmo-math-model` without `kosmo-jobs`.

### Results that are not current

- A result that is not current is not written into the current result slot
  and does not change its display. It goes into the record of its own
  submission, in the first release the Scratchpad history entry found by job
  identity. If that record was evicted, or the region keeps no record per
  submission, it counts toward the region's discard count.
- A current result goes into the slot and into its record.
- A result already in the slot is shown as stale once the region's versions or
  latest submission change, until the next current result arrives.
- A result that is not current never marks ranges or nodes of the current
  input.
- A cancelled job has no result.
- Handling a still-running job whose history entry was evicted is defined with
  the Scratchpad.

## Completion and cancellation

kalyx's command contract leaves the order of a cancel request and a stop
report to the job service. kosmo's job layer decides it by seven rules:

1. Only one state machine, in the pure core of the job layer, changes a job's
   state. Cancel requests, completion reports, `DeadlineExceeded` and stop
   reports are events applied one at a time in order of arrival; the first
   applied is accepted first. A one-request job completes when its request
   ends; a job of several requests when all have run or it stopped by kalyx's
   rule for a job of several requests. One request's result is not the job's
   completion.
2. On an accepted cancel request, a queued job goes directly to `cancelled`
   and never runs; a running job goes to `cancel_requested`, and only then is
   its `CancelFlag` set, so kalyx's `Cancelled` always follows
   `cancel_requested`. Acceptance and terminal state are reported separately.
   A cancellation cannot be withdrawn.
3. A job in `cancel_requested` that receives any completion or stop report
   becomes `cancelled`; the report's lines and results are discarded and the
   record holds only `cancelled`.
4. A job whose completion is accepted first ends as Table 1 of kalyx's
   command contract and its rule for a job of several requests give; a later
   cancel request is answered "already terminal".
5. A passed deadline ends a job without an accepted cancel request in
   `resource_exhausted`, and one in `cancel_requested` in `cancelled`.
6. Every job has exactly one terminal state, which is absorbing, and its
   report is delivered exactly once. Cancelling a session applies one cancel
   request to each job of the session that is not terminal.
7. Tests order these events with a controllable backend and injected probes,
   never with sleeps or the wall clock.

## Host job layer

In the first release the job layer's backend is a protocol stub on the host:
a host stub in `kosmo-jobs` that runs jobs on one worker thread, and a kalyx
runner in `kosmo-math-kalyx` that executes a job's requests with kalyx.

### Pure core and shell

- The pure core is a module of `kosmo-jobs`, not a separate crate. It has no
  threads, channels, clock reads, input or output, interior mutability or
  kalyx types.
- It holds the bounded job table, the state machine, the terminal function,
  the race rule, the bounds and session cancellation. It is a step function:
  inputs are submit, cancel, cancel session, a report and take; outputs are the
  new state, commands to the shell (hand a job to the backend, set a cancel
  handle) and events for the submitter. Order depends only on input order, so
  bounded exhaustive enumeration needs no threads.
- The shell is the API on the job-layer thread (`submit`, `cancel`,
  `cancel_session`, `poll`, `wait`, `shutdown`) and the host implementations
  of the ports. The host stub's worker thread and its two channels belong to
  the shell.
- The boundary check works per crate, not per module, so review checks that
  the core module stays pure.

### Host stub

The core hands a job to the backend in the same step in which it accepts it,
sets no bound on running jobs and assumes no report order. The host stub is
generic over the runner port and has a single worker thread, which runs one
job at a time in the order it receives them.

- **Worker thread.** The stub creates the worker when it is constructed, with
  `std::thread::Builder`, a fixed name and an explicit stack size whose value
  is fixed with the implementation. If the worker cannot be created,
  constructing the job layer returns an error; nothing panics.
- **Channels.** Jobs reach the worker through a dispatch channel,
  `sync_channel(MAX_LIVE_JOBS)`. Every job in it is live, and live jobs never
  exceed `MAX_LIVE_JOBS`, so dispatch never blocks. Reports return through
  `sync_channel(2 × MAX_LIVE_JOBS)`. A live job sends at most two reports and
  keeps its slot until `Finished`, so no report blocks either.
- **Reports.** The backend reports `Started` for every job that starts and
  exactly one `Finished` for every job it receives, carrying each request's
  report and the stop seen between requests. A job that never starts (see
  worker death below) gets only `Finished`, and the core handles it the same
  way.
- **Runner calls.** The runner's `prepare` is called on the job-layer thread
  and its run call only on the worker thread; the port's exact shape is fixed
  with the implementation.
- **Order.** Jobs complete one at a time, in submission order. A test with
  several jobs that only submits them and then waits for each to reach its
  terminal state in turn has a determined result without gates. Events
  interleaved with a running job, namely cancellation, session cancellation
  and advances of the fake clock, are still ordered through the controllable
  backend's gates and probes. So are cases that need another completion
  order: both completion orders of an old and a new job of one region, and
  late and early results. No test sleeps or reads the wall clock, and the
  core and the stub do not change for tests.
- **Cancellation before the first request.** When the core accepts a cancel
  request, the shell sets the job's cancel handle. The worker takes a job
  that is still in the dispatch channel as usual, reports `Started` and hands
  it to the runner; the runner finds the flag before the first request and
  reports a `Finished` with no request reports and the stop reason
  "cancelled", without running a request. A job that the worker has already
  taken, but whose `Started` the core has not handled, stops before its first
  request in the same way. For the core these jobs were still queued, so it
  ended them `cancelled` when it accepted the cancellation, and it discards
  these reports.
- **Deadline while queued.** A job's absolute deadline is fixed when `submit`
  accepts it; time keeps passing while the job waits in the dispatch channel.
  A job already past its deadline when the worker takes it is found by the
  runner before its first request and ends `resource_exhausted`.
- **Worker death.** The worker holds a report guard for its current job. If
  the thread ends before reporting that job's `Finished`, unwinding from a
  panic included, the guard's `Drop` sends a `Finished` for the job with no
  request reports and the stop reason "backend internal error". On the
  job-layer thread the stub keeps the set of jobs handed to the worker whose
  `Finished` has not arrived; once the report channel is disconnected, it
  reports the same `Finished` for each of them. Every job submitted
  afterwards, including one whose dispatch fails, joins that set and gets the
  same `Finished` at once. No rejection kind is added; the job layer has to
  be rebuilt to run jobs again. Every received job thus gets exactly one
  `Finished`, and no live job stays taken.
- **Shutdown**, also on `Drop`: cancel every job that is not terminal and set
  its cancel handle, then drop the dispatch sender. The worker hands each
  remaining job to the runner, which stops it before its first request, and
  then exits; the stub joins it and does not re-raise a panic returned by
  `join`. Tests can read the number of live threads, which is 0 after
  shutdown. The public API has no thread or channel types.
- **Progress.** The worker takes its next job by itself, without waiting for
  the job-layer thread. Results are still delivered only when the job-layer
  thread calls `poll` or `wait`, so the framework's event loop calls the job
  layer's `poll` when reports arrive.
- **Memory.** Line bytes move with the reports as `Vec<u8>`; nothing is
  shared. kalyx terms, `Outcome` and `Context` stay on the worker thread and
  are freed when a request ends; delivered lines belong to the submitter. On
  the host the stub bounds neither real memory nor CPU time, a limit that the
  job layer's planned feature document will state. The representative
  workload is the derivative example of the
  [first integration target](scope.md#first-integration-target). kalyx
  0.1.0's resource profile measured a stack of at most 9,728 bytes and a
  largest heap peak of 901,176 bytes among its cases, as measurements, not
  bounds.

### Cancellation and deadlines

- Cancellation goes through kalyx's `CancelFlag`, deadlines through an
  injected clock. The core reads no clock; tests inject a shared fake clock
  and order events with injected gates and probe handshakes.
- `kosmo-jobs` depends neither on kalyx nor on `kosmo-math-kalyx`, a higher
  layer. It defines the runner port, provisionally `JobRunner`, which the kalyx
  runner in `kosmo-math-kalyx` implements, so the runner holds each job's
  `CancelFlag`:
  - Before handing a job to the stub, the shell calls the runner's
    `prepare(id)` on the job-layer thread and gets an opaque handle owned by
    kosmo, provisionally `CancelHandle`: `Send`, cloneable, with setting as its
    only operation and no kalyx type. The runner creates and keeps a kalyx
    `CancelFlag` for the job; setting the handle sets it.
  - A work item is a `JobRequest` plus its `CancelHandle`, with no kalyx type;
    `JobRequest` has no cancellation field. The shell sets the handle when it
    executes the core's command, which the core issues only after accepting a
    cancel request.
  - The runner builds each request's `Context` with that flag on the worker
    thread. Within a request kalyx checks cancellation, then the deadline, at
    its charge points; between requests, and before the first, the runner
    checks the flag and then the deadline.
  - How handles map to flags, and the shapes of `prepare` on the job-layer
    thread and of the run call on the worker thread (for example whether the
    runner is `Sync`, or is split per job), are fixed with the
    implementation. The runner need not support two runs at once. This
    division stays.
- The job layer is built with an injected monotonic clock port: `u64`
  nanoseconds, the unit of kalyx's `Clock`, `Send` and forkable. The stub and
  the runner on the worker thread each hold a fork. `submit` reads it once on
  acceptance and writes the time limit into the request as an absolute
  deadline reading. Each request's context is
  `Context::with_observers(limits, job clock fork, job flag clone).with_deadline(job deadline)`,
  the first of the two ways kalyx's command contract gives.

### Bounds

| Bound | Value | Rule |
| --- | --- | --- |
| `MAX_LIVE_JOBS` | 8 per job-layer instance | Live: queued, running, `cancel_requested`, terminal but not taken, or `Finished` not yet returned by the backend |
| `MAX_REQUESTS_PER_JOB` | 4 | The most requests in one job |

- Jobs in the host stub's dispatch channel are live.
- Terminal states awaiting delivery never outnumber live jobs: a delivery slot
  is reserved on acceptance, so delivery never blocks and no terminal state is
  lost.
- A submission while `MAX_LIVE_JOBS` jobs are live gets a distinct rejection,
  "full", with the current number of live jobs; no job is created, no
  identity used, nothing blocks or is evicted.
- Job and session identities are `u64` counters, never reused and never
  wrapping; exhaustion is rejected.
- kalyx charges every line's bytes and the job layer does not truncate again.
  Pending line bytes are at most 8 × 4 × max(each request's
  `max_output_bytes`, 65,536), 2 MiB under the default limits.
- The values 8 and 4 are part of this design.
- Fallback line: when the error line of an `Err` outcome fails under the
  request's context, kalyx's contract has the host write it again under
  `Context::new(Limits::new(), CancelFlag::new())`, with a fresh flag and no
  deadline, and `Context::new` builds a real clock. The kalyx runner writes
  the fallback line under
  `Context::with_observers(Limits::new(), job clock fork, new CancelFlag)`
  without a deadline: limits, flag and deadline as in the contract, but no
  real clock, so `kosmo-math-kalyx` stays a core library.

### Protocol stub types

- The backend port, provisionally `JobBackend` (`submit`, `cancel(id)`, taking
  reports), the runner port and their types are in `kosmo-jobs`, with no kalyx,
  krepi or thread types.
- A request, provisionally `JobRequest`, is `#[non_exhaustive]` and built with a
  constructor. It holds the job identity, the session, the operation (an
  `#[non_exhaustive]` enum of Eval, Expand and Diff), the variable, the linear
  text, `Versions { source, context, settings }` from `kosmo-math-model`,
  nine-field limits defaulting to kalyx 0.1.0's `Limits::new()`, and the job
  deadline. The cancel flag is not in it; the work item carries the handle.
- Reports are `Started(id)` and `Finished(id, request reports, stop seen
  between requests)`. A request report holds the outcome kind (Value, State,
  Fault, ResourceLimit, Cancelled, DeadlineExceeded), an error class from a
  kosmo enum that includes a failed `serialize`, and the output line's bytes.
- Results return only as output lines, which the submitter parses with the
  display model.
- There is no settings field; the kalyx runner always passes `Settings::new()`.
- All conversion is in the kalyx runner, which builds `Request` and `Context`
  on the worker thread; kalyx terms never leave it.
- A resource scope has no field yet; it will be a new field with a default.

| kalyx `Request` field | Filled from |
| --- | --- |
| `operation` | The operation as `eval`, `expand` or `diff`; no other name can be submitted |
| `variable` | `variable`, unchanged; kalyx judges it |
| `input` | The linear text, unchanged; mapping ranges back stays with the submitter |
| `input_version` | `versions.source`, unchanged, checked against the echo |
| `context_version` | `versions.context`, unchanged; 0 in the first release |
| `settings` | Always `Settings::new()`, the only value kalyx 0.1.0 can build |
| `settings_version` | `versions.settings`, unchanged, checked against the echo |

The job identity and the session stay in the job layer; the limits go into
the context's `Limits`, the deadline into `with_deadline`, and the flag is the
context's probe, held by the runner.

### Terminal states

A job is `queued` (handed to the backend, not yet started, including waiting
in the stub's dispatch channel), `running` or `cancel_requested`, and ends
`completed`, `failed`, `cancelled` or `resource_exhausted`. Its terminal
record holds the state, the stop reason, the delivered lines and the echoed
versions. A stop reason is one of:

- cancellation;
- deadline;
- forced stop or CPU budget, reported only by a backend;
- backend internal error: in the host stub, the worker thread ended before
  reporting the job's `Finished`, or the job was submitted after the worker
  thread ended.

One pure core function with a finite, exhaustively enumerable domain
computes the terminal state: the seven rows of kalyx's Table 1 below, the
cases after it, and the rules for completion and cancellation above. A job of
several requests stops at cancellation or a deadline, not at a fault or
ResourceLimit, and a stop takes precedence over earlier faults and
ResourceLimits. A `cancelled` job carries no lines, not even those of requests
that ran before the cancellation. This is stricter than kalyx's contract,
which publishes the line of every request that ran; the host stub still
produces those lines, and the core discards them. Outside `cancel_requested`,
echoed versions that differ from the request's give `failed`, marked as an
internal error, and no line is delivered as a result. Forced stops and CPU
budgets do not occur in the host stub; only the controllable backend produces
them, in unit tests.

| Request outcome | Delivered line | Without `cancel_requested` | After `cancel_requested` | Stops the job |
| --- | --- | --- | --- | --- |
| `Ok(Settled::Value)` | `value` | `completed` | `cancelled`, line discarded | No |
| `Ok(Settled::State)` | `state` | `completed` | `cancelled`, line discarded | No |
| `Err` Fault | `error` or fallback | `failed` | `cancelled`, line discarded | No |
| `Err` ResourceLimit | `error` or fallback | `failed` | `cancelled`, line discarded | No |
| `Err` Cancelled | None | Cannot occur: the flag is set only after acceptance | `cancelled` | Yes |
| `Err` DeadlineExceeded | Fallback `error` | `resource_exhausted` | `cancelled`, line discarded | Yes |
| `serialize` fails | Fallback for an `Err`; none for a value or state | With Cancelled, cannot occur; with DeadlineExceeded, `resource_exhausted`; otherwise the request fails and the job goes on | `cancelled`, line discarded | As its error |

Outside the table:

- cancellation seen between requests: `cancelled`;
- a passed deadline seen between requests: `resource_exhausted`; that request
  does not run, and the terminal frontend writes `DeadlineExceeded` from the
  stop reason;
- cancelling a queued job: `cancelled` at once; the later report is
  discarded;
- a cancel request accepted after all requests ran but before their report is
  handled: `cancelled`, results discarded;
- a forced stop reported by a backend: `cancelled` in `cancel_requested`,
  otherwise `resource_exhausted`, both marked as forced;
- a backend internal error: `cancelled` in `cancel_requested`, otherwise
  `failed` marked as an internal error, both without lines;
- no stop: `failed` if any request has a fault or ResourceLimit, otherwise
  `completed`.

### Cancelling a session

- An object-safe trait, provisionally `SessionCancel`, has
  `cancel_session(&mut self, session: SessionId) -> CancelCount`. The job layer
  issues each `SessionId` at `open_session`: opaque, `u64`, never reused.
- In one core step, the session's queued jobs end `cancelled` and its running
  jobs go to `cancel_requested`, each with its handle set, which sets its
  kalyx `CancelFlag`. Jobs already in `cancel_requested` or terminal, and other
  sessions' jobs, are unchanged. All jobs of the session that are not
  terminal, queued ones included, are cancelled.
- It returns the two counts accepted in this call, does not wait for terminal
  states and is idempotent. It uses the same core entry as `cancel(id)`, and
  its order against completions is the order of acceptance.
- It is called only on the job-layer thread: on the host by a stand-in for the
  system's stop-request port, on the device by krepi's job service.

### Replacing the host stub

The protocol stub, the host implementation of `JobBackend` made of the
single-worker host stub in `kosmo-jobs` and the kalyx runner, will be
replaced by krepi backends: on the host by krepi's host operating-system
backend from its interactive-device stage, on the device by the backend for
krepi's job service that krepi's integration provides. The pure core, the API
on the job-layer thread, the session-cancel port and the rules for terminal
states and version echoes stay; the host stub is known rework. This holds
while:

- the job layer reaches backends only through the ports, and `kosmo-jobs`
  depends on none of kalyx, krepi and `kosmo-math-kalyx`;
- port types are `#[non_exhaustive]` and only grow;
- the terminal function's domain includes krepi's job states, forced stops
  and CPU budgets;
- the job layer checks echoed versions itself;
- cancellation goes through `cancel(id)` and opaque handles, and the clock
  through a port;
- threads exist only in the host stub, which has one worker thread, and kosmo
  uses no threads on the device;
- the core assumes neither a backend's parallelism nor its report order; the
  first release's host stub completes jobs one at a time in submission order,
  and the controllable backend's gates cover other orders;
- the controllable backend implements the same port, with contract tests
  generic over it.

## Event record

Keyboard input has two record layers: the device record from an input device
and the key event that kosmo's display server delivers. Both are in the
shared types crate, provisionally `kosmo-types`: `no_std`, no dependencies,
fixed-size `Copy` types that never allocate. kosmo's keyboard input uses only
these two types, and no component defines its own.

### Device record

- `InputRecord { time: MonoTime, body: RecordBody }`. `RecordBody` is
  `Key { usage: HidUsage { page: u16, id: u16 }, state }` with `state` Press or
  Release, `PacketEnd` or `Dropped`.
- It holds only the HID usage, press or release, the IRQ timestamp and the
  packet-end and loss marks; no device identity or sequence number. Modifiers
  are ordinary usages. Repeat and long press cannot be represented.
- After a loss the server resynchronizes through the device state query; the
  device description and state query belong to the device ports.

### Delivered key event

- `KeyEvent { usage, meaning: KeyMeaning, mods: ModSet, action: KeyAction, time: MonoTime, synthetic: bool }`.
  The delivered key event carries both the physical usage and the key
  meaning.
- `KeyAction` is Press, Release, Repeat or LongPress; `synthetic` marks a
  release that the server synthesized.
- Focus generation, epoch and trace number are added in outer layers by the
  framework, the trusted core and the display protocol.
- `ModSet(u16)`: the low 8 bits follow the HID modifier byte; the high 8 bits
  are reserved for layer state, defined with the key maps. More bits are a
  change of the event record.
- `KeyMeaning` is `Char(char)` or `Named(NamedKey)`. `NamedKey` is a `u16`
  numbered table in `kosmo-types` that only grows and never renumbers; the
  framework and the display stack propose entries.
- Only kosmo's display server produces repeat and long press, timed by an
  injected clock; which keys and parameters is defined with the key maps. The
  reserved stop key's long press belongs to krepi's driver: it is not in device
  records, and the server produces no LongPress for it. Host normalization
  drops the host's repeats and pointer events. The framework produces neither;
  framework scripts write repeat lines directly.

### Time, decoding and scripts

- `MonoTime` is `u64` nanoseconds, like kalyx's `Clock::now()`, with one
  monotonic clock domain per input path: the converted IRQ timestamp on the
  device, the injected clock's reading at normalization in the nested host
  backend, and the written time in scripts. Server-produced events take their
  planned time. `time` never decreases on one input path. The shared types
  crate's logical time is this type; the framework clock's semantics (who
  advances it, timers, timeouts) belong to the framework.
- Decoding from bytes is bounded and returns errors, never panics. The wire
  format belongs to the display protocol, device service messages to krepi's
  input device interface. `RecordBody` and `KeyAction` are
  `#[non_exhaustive]`; pointer, touch and input methods will be new bodies,
  added by a design change.
- An event script holds one record per line, in one layer only: framework
  scripts use delivered events, server tests may use device records. The
  projection from delivered events to device records takes usage, press or
  release, and time; lines with one time form a packet ending with
  `PacketEnd`. Scripts run through both the headless framework and the
  headless server contain only presses and releases, and a test fails when the
  server's key meaning or modifiers differ from the script. Non-key lines and
  the text format are defined with the framework and the platform interface.

## Platform boundary

- Every library crate is a core library unless the layout table's "platform
  boundary" column registers a platform category for it; a registered crate
  uses only the types of its category.
- Forbidden in a core library's `src/`, unit tests included:

  | Area | Forbidden |
  | --- | --- |
  | Files and paths | `std::fs`, `std::path`, `std::ffi::OsStr`, `OsString` |
  | Network and OS | `std::net`, `std::os` |
  | Processes and environment | `std::process`, `std::env`; compile-time `env!` is allowed |
  | Threads | `std::thread`, `thread_local!` |
  | Time | `std::time::Instant`, `SystemTime`, `UNIX_EPOCH` |
  | Input and output | All of `std::io`; `print!`, `println!`, `eprint!`, `eprintln!`, `dbg!` |
  | kalyx | `MonotonicClock`, and `kalyx::Context::new`, which builds one; the fallback line uses `Context::with_observers` |

- External window, terminal, async-runtime, `libc` and system font discovery
  packages never enter a core library. `HashMap` and `HashSet` with the
  default hasher are not listed; determinism belongs to the rendering and
  determinism rules. The list forbids; it is not an allow-list of `std`
  interfaces. Adding frontends adds no external dependencies to core
  libraries.
- Capabilities are injected. Time comes from an injected clock (job deadlines
  in the job layer, the framework clock in the framework); real clocks are
  built only in boundary crates. Computation goes through the job layer. Fonts
  and resources come only from the owning crate's asset directory, loaded as
  the framework and rendering define. Surfaces, events and devices come
  through the platform interface and the ports.
- First-release registration: `kosmo` is a boundary crate for standard
  streams, arguments, exit code and the real clock, and `kosmo-jobs` for
  threads; `kosmo-types`, `kosmo-math-model`, `kosmo-math-kalyx`,
  `kosmo-scratchpad`, `kosmo-sdk` and `kosmo-ui` are core libraries. Other
  crates are registered by the change that creates them; `kosmo-server-host`,
  `kosmo-testkit` and the crates of the host transport and process entry are
  expected to be boundary crates.
- A planned layout step of `cargo xtask check` will run a static check that
  parses core libraries' `src/`, expands `use` trees and reports every
  forbidden path, macro and type with file and line, and checks that
  registered crates use only their category's types. Any hit fails the check;
  negative fixtures are kept in the repository. Its limits, macro-generated
  paths and uses inside dependencies, will be stated in
  [Contributing](../CONTRIBUTING.md). The check and the column arrive with the
  framework runtime; earlier crates rely on review. The column does not change
  the workspace's layering rules.

## Frontends of the first release

The first release has exactly two frontends: the line-mode terminal of
`kosmo` without arguments, and the framework's headless backend. A host window
appears only through the nested display server. Frontends hold only input and
output buffers; session state is in the core libraries.

- The terminal is a library function that reads a `BufRead` and writes a
  `Write`; `main` binds only the standard streams, arguments and exit code.
- It reads whole lines with a byte limit per line. An over-long line gives an
  error, is not submitted and never gets a buffer beyond the limit.
- It enters each accepted line into the current line one character at a time
  through the insert operation, then submits it through the job layer.
- After each submission it waits for the job's terminal state, takes and
  writes the result, then reads the next line. It has at most one live job, so
  its output does not depend on when results arrive, and two runs on the same
  input write byte-identical output. The host stub fixes only the order in
  which jobs complete, not when a result arrives relative to the next input
  line; the wait fixes that.
- It produces no event records and uses neither the shared types crate nor the
  framework runtime. The `kosmo` package has no direct external dependency,
  uses neither `std::os` nor `libc`, and neither reads nor changes terminal
  attributes.
- A raw-mode terminal, if added later, is a separate adapter crate that
  produces delivered key events; these rules do not define it.
- The headless backend reads scripts of delivered key events, and its tests
  never sleep or read the wall clock. Its clock, threads and runner are
  defined with the framework.

## Components

A component is a unit with its own role, contract and level.

| Level | Meaning |
| --- | --- |
| A0 | Tested: tests, with each run recorded with its environment; stated as tested, per environment |
| A1 | Contract: numbered requirements, a contract, contract tests with negative cases, and fuzz tests for untrusted input; pure state machines use bounded exhaustive enumeration instead of fuzz tests; stated as contract tested |
| A2 | Specification checked: an executable specification of the component and its properties is checked; stated as specification checked, never as verified. No component of the first release is at A2 |

- Every planned crate and `xtask/` appear in at least one row; crates with
  components of different roles (`kosmo-math-model`, `kosmo-math-kalyx`,
  `kosmo-server`, and possibly the crate of the privileged shell extension,
  chosen with the display stack design) appear in several.
- Evidence is `planned` until the component is delivered; the delivering
  change replaces it with a link to the feature document and the location of
  the tests. Whether a component exists is read from this column, not from
  the existence of its crate.
- Release tag notes copy each component's level from this table; a component
  not yet delivered at that tag is listed as not complete, without a level.
- Crate names are provisional.

| Component | Crates | Role | Level | Evidence |
| --- | --- | --- | --- | --- |
| Workspace base, layout and supply-chain checks | `xtask/` | Tool | A0 | `planned` |
| Line-mode terminal frontend | `kosmo` | Frontend | A0 | `planned` |
| One-line editing structure | `kosmo-math-model` | Data-structure core | A1 | `planned` |
| Exact display | `kosmo-math-model`, `kosmo-math-kalyx` | Presentation | A0 | `planned` |
| Formula conversion (linear text, range mapping, read-back) | `kosmo-math-kalyx` | Cross-project format | A1 | `planned` |
| Job layer and protocol stub (kalyx and controllable backends) | `kosmo-jobs`, `kosmo-math-kalyx` | Cross-project protocol core | A1 | `planned` |
| Scratchpad | `kosmo-scratchpad` | Composition | A0 | `planned` |
| SDK facade (re-exports only) | `kosmo-sdk` | Facade | A0 | `planned` |
| Shared types (geometry, logical time, focus generation and epoch, event records, pixel formats, buffer descriptions, damage regions) | `kosmo-types` | Shared types | A0 | `planned` |
| Framework runtime | `kosmo-ui` | Framework core | A0 | `planned` |
| Layout | `kosmo-ui` | Framework core | A0 | `planned` |
| Accessibility tree and test queries | `kosmo-ui`, `kosmo-testkit` | Framework core | A0 | `planned` |
| Rendering | `kosmo-render` | Presentation | A0 | `planned` |
| Text | `kosmo-text` | Presentation | A0 | `planned` |
| Platform interface and conformance suite | `kosmo-platform` (the suite's crate is chosen with the suite) | Platform contract | A1 | `planned` |
| Headless backend | `kosmo-backend-headless` | Backend | A0, passing the platform conformance suite | `planned` |
| Test support (script runner, clock control, call probes) | `kosmo-testkit` | Tool | A0 | `planned` |
| Basic controls | `kosmo-controls` | Controls | A0 | `planned` |
| Math controls (one-line formula editing, result view) | `kosmo-math-controls` | Controls | A0 | `planned` |
| One-line calculator | `kosmo-calc` | Application | A0 | `planned` |
| Display protocol v0 (provisional) codec and conformance-suite skeleton | `kosmo-proto`, `kosmo-proto-conformance` | Parsing of untrusted input | A1, with fuzz tests | `planned` |
| View tree and stacking order | `kosmo-scene` | Trusted-core pure core | A1 | `planned` |
| Focus routing and focus generation (with the server part of the stop key's short press) | `kosmo-focus` | Trusted-core pure core | A1 | `planned` |
| Surface and buffer lifecycle | `kosmo-surface` | Trusted-core pure core | A1 | `planned` |
| Software compositing | `kosmo-compositor` | Presentation | A0 | `planned` |
| Session and device ports | `kosmo-server` | Port | A1, with fuzz tests of inbound messages | `planned` |
| Input pipeline and key maps | `kosmo-server` (module) | Server module | A0 | `planned` |
| Headless server backend | `kosmo-server-headless` | Backend | A0, passing the session and device port suite | `planned` |
| Nested backend | `kosmo-server-host` | Backend | A0 | `planned` |
| Client library and client transport | `kosmo-client` | Port | A1 | `planned` |
| Display server backend | `kosmo-backend-display` | Backend | A1 | `planned` |
| Window policy module | `kosmo-wm` | Policy | A0 | `planned` |
| Shell (skeleton, switch and stop entries, lifecycle presentation, status area, notifications) | `kosmo-shell` | Shell | A0 | `planned` |
| Privileged shell extension | Chosen with the display stack design | Protocol extension | A1, with fuzz tests of extension message decoding | `planned` |
