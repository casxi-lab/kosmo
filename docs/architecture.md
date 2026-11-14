# Architecture

This page states the design rules of kosmo's first release for the editing
structure of formulas, versions, the host job layer, the keyboard event record,
the source layout, the platform boundary, the framework, and rendering, text
and determinism. It also names the two frontends of the first release and
lists every component with its verification level.

The rules are fixed before the code that follows them. So far the only product
code is the `kosmo` program, which only reports its version: every component
here is planned, and nothing described here works today. Crate names
(`kosmo-jobs`, `kosmo-math-model` and the others) and the names of types,
traits and constants (`NodeId`, `JobRunner`, `MAX_LIVE_JOBS` and the others)
are provisional. Changing a rule is a design change, not an implementation
choice.

kalyx here means kalyx 0.1.0 from crates.io, which the first release builds
on. The job-layer thread is the thread that holds the job layer and calls
its API. The layout table is `layout.toml` at the repository root, which lists
the workspaces, their layers and every crate that exists (see
[Source layout](#source-layout)); its platform boundary column is planned.
[Scope](scope.md) describes what kosmo will provide.

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
| Matrix | $r \times c$ cells, row-major | Row and column counts are part of the kind. Adding and removing rows and columns are the only edits that change the slot count; other cells keep their identities |

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
- Coordinates use the fixed-point type that the
  [numeric representation](#numeric-representation) defines, and the
  accessibility hook is the AccessKit tree that
  [accessibility](#accessibility) describes.
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
  `sync_channel(2 * MAX_LIVE_JOBS)`. A live job sends at most two reports and
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
  Pending line bytes are at most $8 \times 4 \times m$, where $m$ is the
  largest of 65,536 and every request's `max_output_bytes`; under the default
  limits this is 2 MiB.
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
backend once krepi runs interactively, on the device by the backend for
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

## Source layout

The source is divided into Cargo workspaces, each with its own root manifest
and `Cargo.lock`; there is no `Cargo.toml` at the repository root, and no
workspace root contains another. The product code has three workspaces, each
with layers listed from the lowest:

- `display/`, the display protocol and the display server: layer D0, exports
  (the protocol and shared types); layer D1, the pure core (the state
  machines of the trusted core); layer D2, server libraries (compositing, the
  input pipeline and the window policy modules); layer D3, server programs
  and backends (the server and its headless and nested backends); layer D4,
  conformance tests, never published.
- `framework/`, the GUI framework: layer F0, foundations and models
  (rendering, text, the platform interface, the job layer, the math model,
  test support and the exported client library); layer F1, core, adapters
  and backends (the framework core, the kalyx adapter, math typesetting and
  the headless and display server backends); layer F2, controls, general and
  mathematical; layer F3, composition (the Scratchpad) and the exported SDK
  facade `kosmo-sdk`; layer F4, integration tests, never published.
- `desktop/`, the shell, the system interface and the first-party
  applications: layer S0, the shell and system interface; layer S1,
  first-party applications; layer S2, the program `kosmo`.

The repository tasks are the separate `xtask/` workspace, outside the product
workspaces and without path dependencies. Layer identifiers, directory names
and crate names are provisional.

Crates of the repository depend on each other through `path` dependencies,
only in these directions:

- `display/` depends on no other product workspace.
- `framework/` depends on `display/` only through its export crates.
- `desktop/` depends on `framework/` only through its export crates, the
  facade `kosmo-sdk` and the client library, and on no crate of `display/`.
  First-party applications depend on other workspaces only through the
  facade, as third-party applications do.
- Within a workspace, a crate depends only on crates of lower layers and on
  crates of its own layer that the layout table lists for it. The internal
  dependency graph, dev-dependencies included, has no cycle.
- A pure-core crate depends only on pure-core crates and never on a crate
  marked provisional.
- Export crates and pure-core crates have no external dependency. The other
  server crates and the host backends will use only the few packages that the
  layout table allows for each of them, and a host window library only in the
  nested backend crate.

The layout table, `layout.toml`, states these rules for each workspace: its
layers, the workspaces it uses, and its target platforms, for which
dependencies and licenses are evaluated (`aarch64-unknown-linux-gnu` and
`aarch64-apple-darwin` for the product workspaces, later also krepi's
user-space target). It lists every crate that exists with its directory,
layer and marks: export, facade (an export crate that only re-exports),
pure core (`no_std` with `alloc`, no unsafe code, no build script, no
external dependency and no interior mutability), provisional (with the
condition that ends it), publishable, and whether it may have a build script
or be a procedural-macro library. The allowed direct external dependencies of
each crate will be listed in its entry, and the allowed packages of each
workspace's `Cargo.lock` in the file that the workspace's `lock-allowlist`
names; both are planned, with the dependency check.

Crates and workspaces appear with their first code. So far only `desktop/`
exists, with the program `kosmo`; the table states the rules of `display/` and
`framework/` and lists no crate for them. A change that adds a crate registers
it in the table, and the first crate of a workspace adds the workspace root.
The layout step of `cargo xtask check` enforces this layout on the tracked
files; [Contributing](../CONTRIBUTING.md#the-layout-step) describes its checks
and their limits.

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
- The layout step of `cargo xtask check` will gain a static check that
  parses core libraries' `src/`, expands `use` trees and reports every
  forbidden path, macro and type with file and line, and checks that
  registered crates use only their category's types. Any hit fails the check;
  negative fixtures are kept in the repository. Its limits, macro-generated
  paths and uses inside dependencies, will be stated in
  [Contributing](../CONTRIBUTING.md). The check and the column arrive with the
  framework runtime; earlier crates rely on review. The column does not change
  the workspace's layering rules.

## Framework

kosmo's framework is its own: a tree of controls with fixed passes, a
single-threaded event loop with an injected clock, versioned result slots and a
platform interface with two backends. Its core is `kosmo-ui` in layer F1. The
platform interface `kosmo-platform`, test support `kosmo-testkit`, rendering
`kosmo-render` and text `kosmo-text` are in layer F0, and the headless backend
`kosmo-backend-headless` and the display server backend `kosmo-backend-display`
in layer F1. [Rendering, text and determinism](#rendering-text-and-determinism)
states the rules of drawing.

### Application model and passes

- The core is a retained tree of controls. A keyed declarative view sits on top
  of it, and application logic uses messages and effects, as in the Elm
  architecture. There is no immediate mode and, in the first release, no
  fine-grained signals.
- An application is a model and two functions. `update` handles messages one at
  a time in order; it is the only place that changes the model, and it outputs
  effects and timer requests. `view` reads the model and returns a view tree.
  Neither function can reach the platform interface, the execution port or the
  control tree.
- The framework holds the control tree. It compares each new view tree with the
  previous one by key and applies only additions, removals and changes; a
  control under the same parent with the same key and type keeps its identity.
- Controls hold only presentation state, such as a scroll offset or the phase of
  a blinking caret. Application state, the editing structure and results are in
  the model. The formula editing control turns the commands of key presses into
  messages of editing operations and keeps no copy of the editing structure,
  whose instance is the Scratchpad's current line.

A round runs eight passes in a fixed order:

1. Events: the round's input becomes messages in this order: receipts of the
   previous round's effects, input events, other platform events (focus and
   surface changes), job reports and due timers. Key events reach controls
   through [dispatch](#dispatch-and-hit-testing).
2. Update: `update` is called once for each message, in order. For a job report,
   `update` hands the report to the result slot together with the region's
   state.
3. Effects: the effects go to the execution port in the order they were
   produced.
4. View: if the round handled a message, `view` is called once and its tree is
   compared with the previous one; a round in which only presentation state
   changed skips this pass.
5. Layout: only dirty subtrees are laid out again.
6. Drawing: display lists are built for the update regions.
7. Accessibility: the tree updates of each surface are produced.
8. Presentation: if there are update regions, the frame is rasterized and
   presented.

Each pass reads only what earlier passes produced. Receipts, reports and due
timers that arise after the round's effect pass wait for the next round.
Receipts come first so that a report is judged after the receipt of its own
submission has updated the region's latest submission; otherwise a current
result would be judged superseded. Input comes before reports so that an edit
changes the source version first, and a result of the same round is judged
against the new version instead of being shown as current and at once as stale.

### Layout pass

- A parent passes a minimum and a maximum size to each child; the child reports
  its size and an optional baseline, and the parent places it. Layout reads only
  the view model, display parameters and text metrics and is a pure function.
- Only dirty subtrees are laid out again; a node whose size depends only on its
  parent's constraints bounds the work. Scrolling changes only the offset.
  Overflow is marked in the layout result, never clipped silently.
- Coordinates use the [numeric representation](#numeric-representation); a value
  out of its range makes the whole layout return a distinct error.
- The first release has rows, columns and scrolling, written in kosmo without a
  general layout library.

### Control tree

- Controls live in a slot table of `kosmo-ui`. A control's identity,
  provisionally `WidgetId`, is a `u32` slot and a `u32` generation and fits in a
  `u64` without loss.
- Removing a control increases its slot's generation and puts the slot on a free
  list that is used last in, first out, so the same sequence of operations gives
  the same identities. A lookup compares the generation, and a mismatch means
  that the control does not exist. Events, timers and accessibility actions
  aimed at a control that does not exist are dropped and counted. A slot whose
  generation would pass its maximum is retired; generations never wrap.
- Control identities are used only inside the framework. They never enter the
  application model, job requests or region state.
- The tree has one root. Each control records its parent and an ordered list of
  its children, in view tree order. Layout, drawing and accessibility traverse
  the tree in preorder, never in slot order, and the tree uses no hash
  containers.
- The number of controls and the depth of the tree are bounded, with values
  fixed with the implementation. A view rebuild first counts the new slots it
  needs; if a bound would be exceeded, the round leaves the tree unchanged and
  reports a distinct error, without panicking.
- Through its context, a control reaches only event values, its own identity and
  presentation state, the messages and requests that it outputs as values,
  layout constraints and results, the construction of display lists and its
  accessibility nodes. It reaches neither the platform interface, nor the
  execution port, nor a mutable reference to the model.
- The platform interface and its backends see only surfaces, pixel buffers and
  events returned by value, and know no controls. Rendering reads only display
  lists. A replacement of the control layer, made of the control tree, the
  control interface and the basic controls, is thus confined to that layer and
  leaves the view layer, the result slot, the event loop, the platform interface
  and display lists unchanged. Whether an outside control library fits this
  boundary is part of judging such a replacement.

### Effects and the result slot

- Effects are values that `update` and the event handlers of controls write into
  their output. After the update pass, the event loop hands them to the
  execution port in the order they were produced. `update`, controls and the
  drawing entry never reach the execution port.
- The effects of the first release submit a job, with a request that the
  application builds, and cancel a job, named by the submission identity of its
  acceptance receipt. The effect enum is `#[non_exhaustive]`; another kind of
  effect, such as a semantic edit or an exit that the application requests, is a
  design change.
- The execution port answers a submission at once with an acceptance, which
  carries the submission identity, or with one of the job layer's rejections.
  The event loop queues this receipt as a message at the start of the next
  round, and the region updates its latest submission only on acceptance. A
  cancellation gives the application no receipt: the job layer's acceptance
  report goes only into the event handling record, and the terminal state
  arrives as a job report.
- Effects and receipts are written one by one into the event handling record.
- The execution port, provisionally `JobPort`, is defined in `kosmo-ui`. It
  submits, cancels and takes reports, with the request, the submission identity
  and the report as associated types. Taking reports also returns the number of
  jobs accepted through the port whose terminal state has not been taken yet,
  which sets the [polling deadline](#job-reports).
- `kosmo-ui` depends on neither `kosmo-jobs` nor kalyx, so the control crates
  built on it, `kosmo-math-controls` included, do not depend on them even
  indirectly. In the first release the application's assembly, expected in
  `kosmo-calc` and never in `kosmo-ui` or a control crate, implements the
  execution port on the job layer with `submit`, `cancel` and `poll`; it never
  calls `wait`. The execution port is not part of the platform interface.
- The result slot, provisionally `ResultSlot`, is in `kosmo-ui` and generic over
  the result value and the submission identity. It holds at most one result with
  its three versions and its submission identity. Each region has one slot; in
  the first release that is the calculator's input region. The slot is part of
  the application model, beside the region's holder.
- The region's holder, expected to be the Scratchpad, keeps the region's current
  versions and latest accepted submission and passes them to the slot at
  delivery and at display.
- Results are delivered only in the update pass: `update` calls the slot's
  delivery with the holder's state when it handles a job report, so the
  acceptance receipts of the round have always updated the latest submission
  first. The slot judges the result with the function of [current, superseded
  and stale results](#current-superseded-and-stale-results). A current result is
  written into the slot and returned to the caller to be written into its
  record; any other result is returned unchanged, to go into its own
  submission's record or count toward the region's discard count. The returned
  value is `#[must_use]`.
- The result view takes its display value only from the slot: empty, current or
  stale. The slot's operations take no reference to the editing structure or the
  cursor. The slot and the Scratchpad do not depend on each other; the
  application's assembly connects them.
- No effect of the first release changes shared values, and `kosmo-ui` has no
  interface that writes them. The only way to change them will be an effect that
  submits a semantic edit to the document service, added by a design change.

### Event loop and clock

- The framework runs only on the thread that calls `run`, and `kosmo-ui` creates
  no threads. The platform interface and its backends need no threads and never
  call into application code.
- The event loop has two parts in `kosmo-ui`: the round function and `run`. The
  round function neither reads a clock, nor waits, nor calls a port. Its first
  step takes the round's time as a `MonoTime` and the round's events, reports
  and receipts, runs the event and update passes and outputs the effects. Its
  second step runs the passes from the view to accessibility and outputs the
  update regions, the display lists, the accessibility updates of each surface,
  the next deadline and the event handling record.
- Before the first round, `run` obtains the [fonts](#fonts). In each round it
  waits for platform events with the round's deadline, reads the clock once,
  takes job reports through the execution port, calls the first step, hands the
  effects to the execution port, calls the second step, pushes the accessibility
  updates of each surface and, if there are update regions, rasterizes and
  presents them. A single-round entry, provisionally `run_round`, lets a driver
  advance the framework one round at a time.
- The wait does not block while receipts or messages are pending. Otherwise its
  deadline is the earlier of the next timer and the [polling
  deadline](#job-reports); with neither, it blocks without limit.
- The job-layer thread is the thread of `run`. Through the execution port the
  event loop calls only operations that do not block, so input, editing and the
  stop control stay usable while jobs run.
- The framework clock is the platform interface's `Clock`: read-only, with
  `MonoTime` readings and read only on the event loop's thread; it need not be
  `Send` or forkable. The job layer's clock port stays as [cancellation and
  deadlines](#cancellation-and-deadlines) describes; the two are separate
  traits.
- The application's assembly connects both clocks to one source: for the
  headless backend and tests, a virtual time source that the driver advances;
  for the display server backend, one real monotonic clock built in a boundary
  crate. On the device both will read krepi's monotonic time.
- `run` reads the clock once per round, and every judgement of the round uses
  that reading. Only the driver advances the headless backend's virtual clock,
  and waiting does not advance it. A real backend's clock reads the platform's
  monotonic clock, and no code advances it.
- Timing in the framework uses only the clock's readings. The `time` of event
  records is only recorded and passed on and may belong to another clock domain.

Timers:

- Timer requests are output values like effects and go into the event handling
  record. The round function registers each in a timer table with an absolute
  deadline as a `MonoTime`, the requester (a control's identity or the
  application) and a label. Timers fire once; the requester of a periodic timer
  registers it again at the previous deadline plus the period.
- Every timer whose deadline is not later than the round's time is due. Due
  timers become messages ordered by deadline and then by registration order; a
  timer whose requester no longer exists is dropped and counted. The next
  deadline is the earliest deadline in the table.
- The timer table is bounded, with its value fixed with the implementation;
  registering into a full table is rejected with a distinct error.
- Key repeat and long press, which the display server produces, job deadlines,
  which the job layer handles, and frame requests, which come later, are not
  timers of this table.

### Job reports

- The platform interface has no wake-up port; job reports reach the event loop
  by polling. In each round, `run` takes reports through the execution port
  once, after reading the clock and before the first step, and the job layer's
  `poll` never blocks. Submissions happen in the effect pass, so the reports
  that have arrived are always taken before the round's submissions. The
  acceptance receipt of a submission keeps the next wait from blocking, so the
  next round reads the new number of jobs whose terminal state has not been
  taken.
- While that number is above 0, the deadline of the next wait is at most the
  round's clock reading plus `JOB_POLL_INTERVAL`; when it is 0, only timers set
  the deadline. `JOB_POLL_INTERVAL` is 16 ms, one frame at 60 Hz rounded down,
  and is a constant of `kosmo-ui`.
- A report that arrives after the round's take is taken in the next round. The
  next wait starts only after the round's layout, drawing, rasterization and
  presentation, and it returns at once if its deadline has passed. The delay
  from a report's arrival to the round that takes it is thus at most the larger
  of `JOB_POLL_INTERVAL` and the time of one round, from reading the clock to
  the start of the next wait, plus the host's wake-up latency. While that number
  is above 0, the loop wakes at least every 16 ms. A frame can take longer than
  16 ms, so 16 ms alone is not a bound.
- A job cancelled while queued ends `cancelled` as soon as the job layer accepts
  the cancellation, and it is counted until its terminal state is taken.
- The headless backend's wait returns according to the virtual clock, and the
  order in which jobs complete comes from the release plan of a [script
  test](#headless-backend-and-scripts).
- Polling is used while job reports cannot arrive through the platform's wait.
  These rules are reconsidered when krepi's job service becomes the job layer's
  backend on the device; if reports then arrive through the platform's wait,
  the event loop stops polling.

### Dispatch and hit testing

- The first release has only keyboard input, and only key events go through
  dispatch. A key event that passes the [focus check](#focus) is dispatched as
  described below. Other input is delivered without dispatch: focus in and focus
  out go to the region's root, surface changes to the surface's holder, due
  timers to their requester, and job reports to the application as messages, for
  `update` to hand to the result slot.
- The command tables resolve a key event to a command. Dispatch starts at the
  focused control, the target; if it does not handle the command, its ancestors,
  the region's root and the application are tried in turn, bubbling, and the
  first that handles it ends dispatch.
- Every key event has exactly one outcome, written into the event handling
  record: dropped with its reason, handled with its handler, or unhandled.
- Controls of the first release receive no raw key events. The enum of dispatch
  phases is `#[non_exhaustive]`; a new phase is a design change, and an existing
  control returns unhandled for a phase it does not know.
- Hit testing is a geometric query over layout results: given a surface and a
  point, it returns the path from the root to the control hit, or nothing. Boxes
  are moved by their ancestors' scroll offsets and clipped by all ancestors; a
  box includes its left and top edges but not its right and bottom edges. Where
  boxes overlap, the last in drawing order is hit, and a control that draws
  nothing is never hit. Hit testing uses the [numeric
  representation](#numeric-representation), is a pure function and in the first
  release is called only by tests. Pointer dispatch is defined with the pointer
  bodies of the event record.

### Focus

- In the first release each surface has one region, and its application chooses
  the focused control: in the calculator, the input line. There is no focus
  movement inside a region; Tab and spatial navigation come with two-level
  focus.
- Focus in and focus out are platform events that carry the epoch and the focus
  generation. The framework updates its current values from them and hands them
  to the region's root. The framework keeps no key state, so leaving focus
  clears nothing; a later control that keeps key state treats every key as
  released when focus leaves.
- The framework checks focus stamps in one place, after it takes platform events
  and before dispatch; controls and regions do not check them. For each surface
  it records the epoch and the focus generation of the latest focus in and
  whether the surface holds focus. A key event whose epoch or focus generation
  differs from them, or that arrives for a surface without focus, is dropped,
  counted by that reason and written into the event handling record.
- The focus generation and the epoch are each a `u64` newtype in the shared
  types crate, `Copy`, which the framework compares only for equality. How their
  values are produced and change, and what happens when they run out, is defined
  with the display server's trusted core; the framework carries them in the
  outer layer of platform events.
- A region level of focus generation is added by the first code that moves focus
  within the framework. Its rules are fixed now: a key event is dispatched only
  when its epoch, its surface's focus generation and its region's generation all
  equal the current values; a key event records the region's generation when the
  framework takes it; and a focus change within the framework and the
  destruction of a region each increase the region's generation by 1. A region's
  generation only increases and never wraps, and a focus change that would pass
  its maximum is rejected. Whether the later key events of a round take the new
  generation after a focus change caused by input is decided with that code.

### Commands and key bindings

- A command is a value: an identifier and its parameters. Identifiers are
  constants in namespaces: the framework's shared commands have one, and an
  application's own commands carry the application's namespace.
- Binding tables are constant data, compiled in. Each context can have one
  table, and the framework's minimal key map is the outermost. A table that maps
  one key meaning and set of modifiers to two commands is an error, which a unit
  test checks. Keys that the shell reserves are taken by the display server, so
  the framework has no "reserved" outcome.
- Each command states whether it repeats: moves and deletions repeat;
  submitting, templates and cancelling do not, and they ignore repeat events.
- The display server's key maps turn HID usages and layer state into key
  meanings and produce repeat and long press. The framework's key maps use only
  key meanings, never physical usages, and ignore the high 8 bits of `ModSet`.
  Framework scripts write key meanings directly, so framework tests do not
  depend on the server's key maps.
- A binding names a named key, a character, or any character, which then becomes
  the command's parameter. It lists the Shift, Ctrl, Alt and GUI modifiers it
  needs, read from the low 8 bits of `ModSet` without distinguishing left and
  right, and the event's modifiers must equal them. A character binding ignores
  Shift, because the display server has already applied it to the character.
- Press triggers a binding; Repeat triggers only repeating commands; Release,
  synthetic or not, triggers nothing. The first release has no LongPress
  bindings.
- Lookup goes along the focus chain from the innermost table outwards, ending
  with the minimal key map, and the first table with a match gives the command.

The minimal key map of the first release adds five `NamedKey` entries:

| Key meaning | Entry | Command | Repeats |
| --- | --- | --- | --- |
| Move the cursor left | `Left` | `MoveLeft` | Yes |
| Move the cursor right | `Right` | `MoveRight` | Yes |
| Delete backwards | `Delete` | `DeleteBackward` | Yes |
| Submit the current line | `Execute` | `EvaluateSelection` | No |
| Cancel the current job | `Cancel` | `CancelJob` | No |
| A character `c` | `Char(c)`, a `KeyMeaning` rather than an entry | `InsertText(c)` | Yes |

- Entries get their numbers when they are written into `kosmo-types`, and other
  entries are added by the same rules.
- `Cancel` is the application's cancel key, apart from the display server's part
  of the stop key. The host keys that produce these meanings are defined with
  the display server's key maps.

### Platform interface

The platform interface is in `kosmo-platform` (layer F0) and has exactly nine
traits. The root trait `Platform` has `EventSource`, `Clock`, `SurfacePort`,
`Presenter`, `Capabilities`, `ResourceProvider` and `AccessibilitySink` as
supertraits and adds only `shutdown`; `PlatformTestHooks` has `Platform` as its
supertrait. A backend is one value. `run` is generic over `P: Platform`, and the
conformance suite and the script runner over `P: PlatformTestHooks`. Backends
implement the interface and never extend it; their constructors and inherent
methods are only for whoever builds them.

| Trait | Methods (sketch) | Used by |
| --- | --- | --- |
| `Platform` | `shutdown(&mut self) -> Result<(), PlatformError>` | `run` and drivers |
| `EventSource` | `wait(&mut self, deadline: Option<MonoTime>, out: &mut Vec<PlatformEvent>) -> Result<WaitEnd, PlatformError>` | `run` and the single-round entry, once per round |
| `Clock` | `now(&self) -> MonoTime` | `run` and the single-round entry, once per round |
| `SurfacePort` | `create_surface(&mut self, req: SurfaceRequest) -> Result<SurfaceId, PlatformError>`; `destroy_surface(&mut self, s: SurfaceId) -> Result<(), PlatformError>` | The application skeleton, which creates the main surface and destroys it before `shutdown` |
| `Presenter` | `buffer(&mut self, s: SurfaceId) -> Result<PixelBufferMut<'_>, PlatformError>`; `present(&mut self, s: SurfaceId, damage: &[DamageRect]) -> Result<(), PlatformError>` | `run`, for presentation |
| `Capabilities` | `caps(&self) -> PlatformCaps`, which reports only the accepted pixel formats and the output size | The application skeleton, which chooses the buffer format and the initial size |
| `ResourceProvider` | `resource(&self, id: ResourceId) -> Result<&[u8], PlatformError>` | `run`, which obtains the two fonts before the first round |
| `AccessibilitySink` | `update(&mut self, surface: SurfaceId, update: TreeUpdate) -> Result<(), PlatformError>` | `run`, after the accessibility pass |
| `PlatformTestHooks` | Test builds only: `inject(&mut self, e: PlatformEvent) -> Result<(), PlatformError>`, `advance_to`, `read_back`, `inject_fault`, `a11y_tree` | The conformance suite, the script runner and accessibility queries |

- In the first release `EventSource` returns `Key { surface, stamp, event }`, a
  delivered key event with its focus stamp `FocusStamp` (the epoch and the focus
  generation); `FocusIn` and `FocusOut` with their stamps;
  `Configure { surface, size }`; and `Released { surface }` when a presented
  buffer is given back. The event enum is `#[non_exhaustive]`; frame events and
  presentation feedback will be new kinds of it. Lifecycle, clipboard, memory
  pressure and pointer events are not returned, and events carry no trace
  number. A wait ends with `Events`, `Deadline` or `Idle`; only a backend that
  never blocks returns `Idle`.
- A buffer is `PixelBufferMut { desc: BufferDesc, bytes: &mut [u8] }`, which
  borrows `&mut self`, so the type ensures that nothing writes to it after
  presentation. A presented buffer that has not been released is not handed out
  again; with no free buffer, `buffer` returns `Busy`. [Pixel formats and
  buffers](#pixel-formats-and-buffers) defines `BufferDesc` and the formats.
- `PlatformError` is kosmo's own `#[non_exhaustive]` enum with the kinds
  `Unsupported`, `StaleHandle`, `InvalidArgument`, `Busy`, `Exhausted`,
  `Disconnected`, `NotFound` (a missing resource) and `Backend`. Applications of
  the first release do not request exit: when `wait` returns `Disconnected`,
  `run` calls `shutdown` and returns.
- The interface allows several surfaces. The enum of surface roles is
  `#[non_exhaustive]`, and events, `Presenter` and `AccessibilitySink` name a
  surface by its `SurfaceId`.
- The number of events returned by one wait, of rectangles in one presentation,
  of surfaces, of queued injected events and of accessibility nodes per surface
  is bounded, with values fixed with the implementation. Events beyond the bound
  wait for the next wait; any other operation beyond a bound returns `Exhausted`
  and changes nothing.
- Signatures hold only kosmo values such as `MonoTime`, `ResourceId` and
  `PlatformError`. Events, requests and capabilities are plain values that can
  be compared and recorded, without host or krepi types and without callbacks.
  `run` returns an exit status; exit codes exist only at the program's entry.
- `kosmo-platform` depends only on the shared types crate and on AccessKit,
  which it re-exports. It depends neither on other crates of `framework/` nor on
  `kosmo-proto` or `kosmo-client`, and no type of the raster or text libraries
  appears in it.
- Only `run` and drivers hold handles of the platform interface; `update`,
  `view` and the drawing entry cannot reach any of its traits.
- `PlatformTestHooks` and the backends' implementations of it compile only under
  a feature that is off by default and that `kosmo-ui`, the control crates and
  the program never enable, so applications, the framework core and controls
  cannot inject events, advance the clock or read buffers back. A driver that
  builds the headless backend may use its inherent methods. `read_back` returns
  the last presented buffer with its description, and `kosmo-testkit` computes
  its digest. `inject_fault` injects platform failures such as a missing
  resource, and `a11y_tree` returns the tree that the backend kept.
- `kosmo-platform` and both backends read no files and are core libraries; they
  have `#![forbid(unsafe_code)]` and are not pure core. `kosmo-testkit` reads
  script files and is expected to be a boundary crate.
- Clipboard, text input, lifecycle and memory pressure are not part of the first
  release's interface. Adding a trait, a method or an event kind is a design
  change.

### Headless backend and scripts

- The framework has exactly two backends, the headless backend
  `kosmo-backend-headless` and the display server backend
  `kosmo-backend-display`. They implement the same traits, `ResourceProvider`
  included, implement `PlatformTestHooks` in test builds and pass the same
  conformance suite. The framework has no host window backend.
- The headless backend has no threads and never blocks. `wait` returns `Events`
  when events are due, otherwise `Deadline` when the deadline has been reached,
  otherwise `Idle`.
- Its clock and input come only from the driver: in tests through `advance_to`
  and `inject`, otherwise through inherent methods with the same meaning. The
  backend never advances its clock.
- `present` records the buffer and the update regions and gives the buffer back
  at once, so the next wait returns `Released`. The backend keeps the tree that
  `AccessibilitySink` receives, and `ResourceProvider` returns the resource
  table given at construction. Once the main surface exists, the backend queues
  `Configure` and `FocusIn`, so a script of key presses needs no focus line.
- The headless frontend is the script runner of `kosmo-testkit` (layer F0) with
  the headless backend. The runner reads scripts of delivered key events and
  applies them line by line, advancing the framework round by round through the
  single-round entry; the backend receives only the events injected into it. At
  each checkpoint the runner runs the framework until it is idle: the backend
  returns `Idle`, and every job report that the test's release plan has let
  through has been delivered. It then writes three records, the view model
  record, the display lists and the pixel digests, in a fixed order with ordered
  containers, so two runs of one script write byte-identical records.
- The runner is generic over `PlatformTestHooks` and a driver trait.
  `kosmo-testkit` also provides call probes, so it obtains records and display
  lists only through the driver trait and does not depend on `kosmo-render`, and
  `kosmo-platform` has no probes. The same-layer dependency of `kosmo-testkit`
  on `kosmo-platform` is listed in the layout table when the crate is created.
  Tests never sleep or read the wall clock.

Besides key lines, which follow the [event record](#time-decoding-and-scripts),
a script has these lines; the text format is fixed with the runner:

- a header with the format version and the size and pixel format of the main
  surface;
- time lines, which advance the virtual clock and only increase it, as the time
  of a key line also does;
- focus lines, only in scripts run through the framework alone;
- checkpoint lines, which run the framework until it is idle and write the three
  records.

Scripts hold input only:

- Platform failures are injected through `inject_fault`, never written in
  scripts.
- The order in which jobs complete is not written in scripts. A test gives the
  controllable job-layer backend a release plan as part of the same test code,
  keyed only on what the client sees: the submission order of jobs, the
  positions of gates and their release conditions, which are another job
  reaching its terminal state or the n-th key event of the event handling record
  being handled, but never virtual time. A script run through both the headless
  framework and the headless display server therefore uses the same plan in both
  runs.
- The key lines of a script run through both are presses and releases only,
  without repeats or long presses; such a script may also hold time and
  checkpoint lines, which the runner on the display server side must interpret
  too.
  A script run through the framework alone may write repeat lines.
- A key line of a script run through the framework alone may state the outer
  focus stamp, a part of the framework's format that does not change `KeyEvent`,
  and `inject` accepts events with any stamp. A test that input of an earlier
  focus generation never reaches a new view thus leaves focus, enters again with
  a new generation and then delivers a key with the old one.

### Display server backend

- `kosmo-backend-display` implements the platform interface through the client
  library `kosmo-client` (layer F0) and the client transport port, and is
  generic over the transport. It is single-threaded: `wait` calls the
  transport's wait with the same deadline, so the client transport port must
  provide a wait with a deadline.
- In the first release it covers one session with its main surface; submission
  with update regions, writing a buffer again only after its release; key events
  with their focus stamps; events of an earlier epoch, which `kosmo-client`
  drops; and disconnection, after which it returns `Disconnected` without
  panicking. Its `Clock` returns the injected clock, and `ResourceProvider` the
  resource table given at construction.
- In production builds `AccessibilitySink` counts updates and drops them; only
  test builds keep the tree and implement `PlatformTestHooks` through an
  in-process channel.
- It may depend on the provisional protocol crate and on `kosmo-client`, never
  on the server crates of `display/`.
- Reconnection, frame events, clipboard, pop-ups, lifecycle and a transport on
  krepi are not part of the first release. The path of the shell extension's
  events is defined with the display protocol and the shell.

### Accessibility

- Controls produce AccessKit nodes in the accessibility pass, and `kosmo-ui`
  builds one tree per surface, without a schema of its own. Node identifiers are
  derived deterministically from control identities, one to one, and are never
  reused. Bounding boxes are converted exactly from the [numeric
  representation](#numeric-representation) to floating point.
- `kosmo-platform` is the only crate that depends on AccessKit directly, and it
  re-exports AccessKit; other crates use the same version through the re-export.
- After the accessibility pass and before presentation, `run` pushes one
  AccessKit `TreeUpdate` through `AccessibilitySink` for each surface that
  changed. The first update holds the whole tree, and focus changes come with
  the updates. The framework pushes only updates; only the platform side keeps
  the whole tree.
- The number of nodes per surface is bounded; an update beyond it returns
  `Exhausted` and leaves the tree unchanged.
- In the first release every control has a role and a name. The tree is built
  but connected to no assistive technology of the system, and there are no
  action requests.
- Test queries are in `kosmo-testkit`. A test reads the tree that the backend
  kept through `a11y_tree(surface)` and queries it by role and name; no match
  and more than one match are distinct failures. Tests thus query the tree that
  the platform received.

## Rendering, text and determinism

These rules make the same input give byte-identical display lists and pixels on
every host. Drawing produces display lists; `kosmo-render` rasterizes them on
the CPU, and `kosmo-text` shapes text and lays it out. Both crates are in layer
F0 and are core libraries.

### Renderer interface

- The renderer interface is a trait of `kosmo-render`, provisionally
  `Rasterizer`. The raster library is one implementation of it, and no type of
  the library appears in the shared types crate, the platform interface or
  display lists.
- One call draws one frame from a display list, the update region, read-only
  glyph coverage and images into a target that holds the previous frame, and
  writes only inside the update region. A second entry fills an outline into an
  8-bit coverage mask. `render` returns a deterministic work count, and both
  entries return a distinct error: a limit exceeded, an unsupported command or
  a missing glyph.

```rust
pub trait Rasterizer {
    fn render(&mut self, list: &DisplayList, damage: &Damage,
              glyphs: &dyn GlyphCoverage, images: &ImageStore,
              target: &mut RenderTarget) -> Result<WorkCount, RenderError>;
    fn fill_coverage(&mut self, outline: &Path, out: &mut CoverageMask)
        -> Result<(), RenderError>;
}
```

- The interface's documentation states in one place the pixel semantics of each
  command, the order of clipping and transformation, and how drawing is clipped
  to the update region. The raster library and a per-pixel reference
  implementation implement the trait, and tests are generic over it: with the
  raster library, redrawing only the update region must give the same bytes as
  redrawing the whole frame, and fixed scenes must give the same pixels with
  both implementations. The reference implementation supports only the commands
  that those scenes use and reports the others as unsupported.
- Glyph commands carry only the font, the size, the glyph identifier and a
  position in the [numeric representation](#numeric-representation); display
  lists hold no bitmaps. The glyph cache of `kosmo-text` holds coverage masks,
  filled through `fill_coverage` by the same library that draws shapes and
  prepared before drawing; a glyph missing at drawing time is an error. The
  trait through which the renderer reads coverage, provisionally
  `GlyphCoverage`, is defined in `kosmo-render` and implemented by `kosmo-text`;
  this same-layer dependency is listed in the layout table when `kosmo-text` is
  created.
- Rasterization starts no threads and reads no clock and no file.
- These rules assume rasterization on the CPU. If the rendering path on the
  device does not rasterize on the CPU, the renderer interface, the raster
  library and the pixel formats are reconsidered, together with whether display
  lists can be serialized to the display server.

### Raster library

- `kosmo-render` rasterizes with vello_cpu 0.4.0, on one thread, as the
  implementation of the renderer interface. Its default features are off;
  `u8_pipeline` and `libm` are on; `std`, `multithreading`, `png`, `text` and
  `f32_pipeline` stay off.
- Without `text`, vello_cpu reads no fonts. Glyph outlines come from skrifa, and
  vello_cpu fills their coverage through `fill_coverage`, as it draws shapes.
- Partial redraws use the library's default drawing, with which redrawing only
  the update region gives the same bytes as redrawing the whole frame.
- The SIMD level is given explicitly through vello_cpu's `RenderSettings` as
  Neon, the baseline common to the target platforms, instead of the default,
  which detects the level at run time. Conversion between pixel formats does not
  use library functions that detect the level themselves; the shared [conversion
  function](#pixel-formats-and-buffers) converts.
- vello_cpu's sizes are `u16`, so a buffer is at most 65,535 pixels wide and
  high, and the buffer limit of `kosmo-render` lies within that.
- guillotiere 0.7.0, a dependency of vello_cpu that cannot be turned off, states
  its license as `MIT/Apache-2.0`, which is not a valid SPDX expression. The
  planned dependency step reads it as `MIT OR Apache-2.0` or lists it as an
  exception with its reason.

### Glyphs

- Outlines are read only with skrifa. Coverage is 8-bit grayscale by area, with
  the nonzero winding rule. There is no LCD subpixel rendering, because product
  screens may rotate, their subpixel order is unknown and it would break
  determinism; there is no gamma correction and no emboldening.
- Glyphs are not hinted, and the hinting instructions of fonts never run.
  Advances do not depend on outlines, and layout never reads outlines.
- Vertically a glyph has one position: the baseline is rounded to a whole pixel.
  Horizontally it has four offset steps, 0, $1/4$, $1/2$ and $3/4$ of a pixel.
  The origin `x` of a glyph, an integer in the numeric representation, is
  rounded to the nearest quarter pixel as `q = (x + 8) >> 4` with an arithmetic
  shift; the whole pixel is `q >> 2` and the step `q & 3`.
- The key of the coverage cache is the digest of the font asset, the glyph
  identifier, the size in the numeric representation and the horizontal step,
  and the cache's capacity counts each step separately.
- The size reaches skrifa in the numeric representation, so the pixels per em,
  the size divided by 64, is an exact binary fraction. TrueType outlines are
  already in units of $1/64$ pixel after scaling and enter without loss; CFF
  outlines would be rounded once by the [rounding
  rule](#numeric-representation). Both bundled fonts are TrueType.

### Text stack

- `kosmo-text` uses parley 0.13.0 for shaping, through harfrust, for line
  breaking and paragraph layout, through parley_engine, and for hit testing and
  caret geometry. fontique registers only the bytes of the two fonts obtained
  through `ResourceProvider`.
- parley's default features are off, so its system font back end `system` is not
  built; `libm` is on; `std` and `complex-scripts` stay off. With `std`,
  fontique would bring in memmap2, which uses `libc` on Unix, and code that
  opens font files by path. Without it, the library's floating-point functions
  come from core_maths and the same `libm` as kosmo's [wrapper
  module](#transcendental-functions).
- `kosmo-text` reads outlines through skrifa 0.49.0, the version that
  parley_engine uses, as a direct dependency.
- parley lays text out in `f32`. At the exit of `kosmo-text` positions are
  rounded once into the numeric representation, which is the only one used
  afterwards, and parley's `quantize` option is off.
- The first release has no font fallback and no bidirectional text; the
  paragraph direction is left to right.

### Fonts

- kosmo bundles two unmodified OFL-1.1 fonts, without subsetting, in the asset
  directory of `kosmo-text`: Fira Sans, the proportional interface font for
  labels, results and the status area, and Fira Mono, the monospaced font for
  the optional style of the one-line input and for aligned columns of numbers.
  There is no CJK font, and a math font comes with two-dimensional typesetting.
- The files are the static Regular styles `FiraSans-Regular.ttf`, version 4.203,
  and `FiraMono-Regular.ttf`, version 3.206, from the google/fonts repository at
  commit `29b4ec6`. The planned asset manifest of `kosmo-text` records their
  SHA-256 digests. Variable fonts are not used, because taking an instance
  rewrites the file.
- Without fallback, each font covers every character of its role; the character
  set is fixed with `kosmo-text`. Fira Sans has no `□` (U+25A1), so the
  formula editing control draws a hole as a rectangle and no interface string
  contains `□`. Fira Mono has no `∆` (U+2206) and no `⋅` (U+22C5), and
  monospaced interface strings do not use them.

Fonts are loaded through the platform interface:

- `ResourceProvider::resource` returns the bytes of a resource. `ResourceId` is
  a kosmo value type; in the first release it names one of the two fonts by its
  file name in the asset manifest.
- Before the first round, `run` and the single-round entry obtain both fonts and
  hand them to the font set constructor of `kosmo-text`. `kosmo-text` accepts
  only bytes, does not depend on `kosmo-platform`, holds no platform handle and
  stays a core library.
- Each backend gets its resource table, from identifiers to bytes, from whoever
  builds it, and reads no files. The builder, a driver or the application's
  assembly, takes the bytes from the asset directory, with `include_bytes!`
  inside its workspace or by reading the files in a boundary crate; the expected
  digests are those of the asset manifest.
- A resource missing from the table, or injected as missing through
  `inject_fault`, gives `NotFound` and changes nothing. If `run` meets one at
  start-up, it calls `shutdown` and returns a distinct exit status. The
  conformance suite checks that `ResourceProvider` returns the bytes given at
  construction.
- Mapping fonts from a read-only system image on the device later does not
  change this interface.

### Pixel formats and buffers

`PixelFormat` and `BufferDesc` are in the shared types crate `kosmo-types`
(layer D0), which the display server's compositing and the framework's rendering
share. Both are fixed-size `Copy` types that never allocate. `PixelFormat` has
exactly four variants and is `#[non_exhaustive]`; adding or removing a variant
is a design change.

| Format | Layout |
| --- | --- |
| `Rgba8Premul` | Bytes R, G, B, A in memory order, with color premultiplied by alpha; the format that the raster library draws |
| `Xrgb8888` | A little-endian 32-bit word with X, R, G and B from the high bits to the low bits, so bytes B, G, R, X in memory; X is always `0xFF` |
| `Argb8888Premul` | A little-endian 32-bit word with A, R, G and B from the high bits to the low bits, so bytes B, G, R, A in memory, with color premultiplied by alpha; reserved for ARGB hardware planes, and used only by conversion in the first release |
| `Rgb565` | A little-endian 16-bit word with R in bits 15 to 11, G in bits 10 to 5 and B in bits 4 to 0 |

- Packed formats are named as DRM fourcc codes are, by their components from the
  high bits to the low bits of a little-endian word; formats whose names end in
  `8` are named in memory byte order. The layout does not depend on the host's
  byte order. Colors are sRGB-encoded values.
- `BufferDesc { width: u32, height: u32, stride: u32, format: PixelFormat }`
  gives `stride` in bytes. The first byte of pixel $(x, y)$ is at
  `y * stride + x * bpp`, and row 0 is at the top. Construction checks that the
  width and the height are at least 1, that `stride` is at least `width * bpp`,
  and that `stride * height` neither overflows `u32` nor exceeds the buffer
  limit of `kosmo-render`. Rotation is not part of the description.
- `kosmo-render` keeps the previous frame as a tightly packed `Rgba8Premul`
  buffer and, before presentation, converts each damage rectangle into the
  target buffer. Repainting by buffer age for double buffering is done on the
  presenting side, which can convert any rectangle again.

`kosmo-types` provides one conversion function, from a source description, its
bytes and a source rectangle into a target, that never allocates. The display
server and `kosmo-render` share it and its test vectors. It covers the 12
ordered pairs of different formats, copies rows within one format, never dithers
and uses integer arithmetic only:

- between the two premultiplied formats it only reorders bytes;
- from a premultiplied to an opaque format it takes the premultiplied components
  as they are;
- from an opaque to a premultiplied format it sets alpha to 255;
- an 8-bit component `c` becomes 5 bits as `(c * 31 + 127) / 255` and 6 bits as
  `(c * 63 + 127) / 255`;
- a 5-bit or 6-bit component `v` becomes 8 bits as `(v * 255 + m / 2) / m`,
  where `m` is 31 or 63.

### Numeric representation

- Layout coordinates, layout, text positions and sizes and the geometry of
  display lists use one fixed-point type, provisionally `Fx`: an `i32` in units
  of $1/64$ pixel, the 26.6 format. It is in the shared types crate, written in
  kosmo without dependencies, and the layout queries of the editing structure
  use it too.
- Multiplication and division go through `i64` intermediate values. Rounding
  goes only through one function of the shared types crate: to the nearest
  value, with ties toward positive infinity, that is $\lfloor v + 1/2 \rfloor$;
  to whole pixels it is `(x + 32) >> 6`. From an `f32`, the function first takes
  the floor and then compares the exact fraction with $1/2$, using only
  operations that the [transcendental functions](#transcendental-functions)
  rules allow.
- Layout inputs (constraints, content sizes and scroll offsets) have absolute
  values of at most $2^{18}$ pixels, that is $2^{24}$ units, and larger ones are
  rejected at entry. Fixed-point arithmetic checks overflow; a value out of
  range makes the whole layout or drawing return a distinct error and never
  panics.
- Fixed-point values meet external libraries only at these points. Into the
  raster library, `v as f32 / 64.0` is exact up to $2^{24}$ units, and
  conversion to `f64`, the type of vello_cpu's coordinates, is always exact; the
  buffer limit of `kosmo-render` stays within that range. Glyph outlines enter
  as [glyphs](#glyphs) describes. parley's `f32` positions are multiplied by 64,
  which is exact, and rounded once by the rule above; widths passed to parley
  are converted to `f32` exactly. Accessibility bounding boxes are converted
  exactly.
- Floating point inside dependencies is examined when a library is chosen,
  because a check of kosmo's code cannot reach it.
- Layout trees and display lists are written out as integers of this
  representation, so golden display lists record exactly the numbers handed to
  the rasterizer.

### Transcendental functions

- The deterministic path is kosmo code that writes layout coordinates, layout
  trees, display lists, glyph positions, coverage or pixels. In the first
  release it is every target (`src/`, tests and examples) of `kosmo-types`,
  `kosmo-ui`, `kosmo-render`, `kosmo-text`, `kosmo-controls` and
  `kosmo-math-controls`; a later crate is judged by the same definition. Whether
  `display/` belongs to it is decided with the display server.
- On the deterministic path, transcendental functions are called only through
  one wrapper module, provisionally `det_math` in `kosmo-render`, which calls
  `libm` 0.2.16, pinned exactly. Its functions have only `f32` and `f64`
  signatures, each forwards to `libm` without an approximation of its own, and
  each has a unit test. A table may instead be generated offline as constants,
  with the command that generates it written beside it.
- `libm`'s `arch` feature, which dependencies enable through its default
  features, replaces only `fma`, `fmaf`, `rint`, `rintf`, `sqrt` and `sqrtf`
  with hardware instructions on AArch64. All six are correctly rounded, so the
  results are the same with and without it.
- Other code on the deterministic path uses only the basic floating-point
  operations `+`, `-`, `*`, `/` and `%`, `sqrt`, `mul_add`, and operations whose
  result is fully determined: `floor`, `ceil`, `round`, `round_ties_even`,
  `trunc`, `abs`, `signum`, `copysign`, comparisons, `div_euclid`, `rem_euclid`
  and `as` conversions. Minimum, maximum and clamping are written with explicit
  comparisons or on integers.

A planned check enforces these rules on the deterministic path. It rejects:

- a direct call to `libm`, `core_maths` or a similar crate outside the wrapper
  module; only `kosmo-render` depends on `libm` directly;
- an `f32` or `f64` method whose documentation has an "Unspecified precision"
  section. In toolchain 1.99.0 these are the 29 stable methods `powi`, `powf`,
  `exp`, `exp2`, `ln`, `log`, `log2`, `log10`, `abs_sub`, `cbrt`, `hypot`,
  `sin`, `cos`, `tan`, `asin`, `acos`, `atan`, `atan2`, `sin_cos`, `exp_m1`,
  `ln_1p`, `sinh`, `cosh`, `tanh`, `asinh`, `acosh` and `atanh` of `std` and
  `to_degrees` and `to_radians` of `core`, and the unstable `gamma`, `ln_gamma`,
  `erf` and `erfc`. `sqrt` and `mul_add` are correctly rounded and not listed;
- `algebraic_add`, `algebraic_sub`, `algebraic_mul`, `algebraic_div` and
  `algebraic_rem`, whose results are not deterministic;
- floating-point `min`, `max` and `clamp`, which may return either of two inputs
  that compare equal, such as `+0.0` and `-0.0`, while the sign of zero reaches
  the byte-exact output.

The check reports method calls, path calls and function pointers by the resolved
receiver type, so a method of the same name on a kosmo type is not reported.
Expected values in tests are written as constants. The check cannot be turned
off at a single place in the source, except in the wrapper module. Its list of
methods is generated from the toolchain's documentation and generated again when
the toolchain is upgraded. `no_std` does not replace the check: `core` already
has `to_degrees`, the `algebraic_*` methods, `min` and `max`, and its
floating-point math will become inherent methods.

The raster library and the text stack are built without `std` and with `libm`,
so the transcendental functions inside them come from the same `libm` 0.2.16 as
kosmo's wrapper module, in one version. With `std` they would call the host's
math library, which differs between macOS, glibc and newlib, the C library on
krepi, and reaches the pixels: kurbo, for example, computes round stroke joins
with `atan2`. When both `std` and `libm` are enabled, `std` takes precedence, so
one package of the workspace that enables `std` for these libraries switches
them back. The feature table of the allowed dependencies and a check of the
resolved features reject that. A library that cannot be built without `std` is a
design change and is listed with the external dependencies.

### Determinism rules

Besides the rules above:

- **SIMD and code generation.** The SIMD level of the pixel and text paths
  depends only on the compilation target, never on detection at run time; the
  raster library gets the level explicitly. Paths in dependencies that branch on
  target features other than `neon` are listed for each library when it is
  chosen. Builds add neither `-C target-cpu` nor `-C target-feature`. rustc by
  default neither contracts a multiplication and an addition into a fused
  multiply-add nor reorders additions.
- **Hash containers.** kosmo code on the deterministic path uses ordered
  containers, never `std::collections::HashMap`, `HashSet` or
  `std::hash::RandomState`, also through re-exports. Hash containers inside
  dependencies, whose seeds change from run to run, are covered by comparing
  golden digests across separate processes.
- **Threads.** The deterministic path starts no threads, and the libraries'
  threading features stay off; no package of the rayon or crossbeam families is
  allowed. Multithreaded rasterization is added only after showing that merging
  tiles does not depend on the number of threads.
- **Color.** Colors are 8-bit sRGB-encoded constants without color management.
  Compositing works on sRGB-encoded premultiplied values with the raster
  library's `u8_pipeline`. RGB565 is produced only when a buffer is submitted,
  by the conversion function, without dithering.
- **Font sources.** Fonts come only from the asset directory of `kosmo-text`
  through `ResourceProvider`, and system font discovery is off; the feature
  table rejects it.
- **Environment.** Output depends only on input. Nothing reads the locale or
  environment variables, line-breaking data is compiled in, options are
  constants, and records contain no wall-clock time.
- **Caches.** Each cache counts its own entries and bytes, each entry knowing
  its size, without the global allocator's statistics, and is bounded. Dropping
  a cache that can be rebuilt and rebuilding it gives byte-identical pixels.
- **Finite values.** Layout trees, display lists and their serialization contain
  no NaN and no infinity, which are rejected at entry; the sign of a NaN is not
  deterministic.
- **Digests.** Pixel goldens come only from the CPU rasterizer. Pixels are
  compared by the SHA-256 of the buffer description and the raw bytes, without
  image encoding.
- **Pinning.** The toolchain and the chosen libraries are pinned exactly. Each
  upgrade is a pull request of its own, which updates the goldens by category,
  and the list of forbidden methods is generated again with a toolchain upgrade.
- **Authority.** Pixel goldens have zero tolerance, and the check on
  GitHub-hosted runners is authoritative. The digests of a local run are
  compared with those of CI one by one, and a difference between hosts is a
  failure.

### External dependencies

The libraries chosen above enter as these direct external dependencies, which
the layout table will list for their crates:

| Package | Version | Default features | Enabled features | Kept off | Crate |
| --- | --- | --- | --- | --- | --- |
| `vello_cpu` | `=0.4.0` | Off | `u8_pipeline`, `libm` | `std`, `multithreading`, `png`, `text`, `f32_pipeline` | `kosmo-render` |
| `parley` | `=0.13.0` | Off | `libm` | `std`, `system`, `complex-scripts` | `kosmo-text` |
| `skrifa` | `=0.49.0` | Off | `libm` | `std`, `autohint_shaping` | `kosmo-text` |
| `accesskit` | `=0.25.2` | Off; it has none | None | `serde`, `schemars`, `pyo3`, `enumn` | `kosmo-platform` |
| `libm` | `=0.2.16` | Off | None; `arch` and `default` after resolution, because other packages use `libm` with its default features | None | `kosmo-render` |

- Each row is one direct dependency of one crate; a package that two crates
  depend on takes two rows.
- Changing the version or the features of a listed package is a pull request of
  its own, checked by the dependency step and with the goldens updated by
  category. Adding or replacing a package is a design change.

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
