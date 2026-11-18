# Architecture

This page states the design rules of kosmo's first release for the editing
structure of formulas, versions, formula conversion with kalyx, the host job
layer, the keyboard event record, the source layout, the platform boundary,
the framework, rendering, text and determinism, and the display stack. It
also names the two frontends of the first release and lists every component
with its verification level.

The rules are fixed before the code that follows them. So far the product code
is the `kosmo` program, which only reports its version, and the one-line
editing structure in `kosmo-math-model`; [Components](#components) shows which
components are delivered, and everything else described here is planned.
Crate names (`kosmo-jobs`, `kosmo-math-model` and the others) and the names of
types, traits and constants (`NodeId`, `JobRunner`, `MAX_LIVE_JOBS` and the
others) are provisional. Changing a rule is a design change, not an
implementation choice.

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
  structure is implemented; it is a subset of ASCII that includes the space,
  listed in [one-line editing](features/math/one-line-editing.md#the-line).
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
  65,536); the values are in
  [one-line editing](features/math/one-line-editing.md#limits).

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
  commands are defined with the editing operations, listed in
  [one-line editing](features/math/one-line-editing.md#editing-operations).

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

## Formula conversion

kosmo gives kalyx a formula as linear text and takes every result from kalyx's
output line. Conversion runs before submission, so a job carries only the
linear text. These rules hold for kalyx 0.1.0, the version that the first
release locks. When kosmo moves to a later kalyx version, they are checked
again, and revised where needed, against that version's grammar and output
lines.

- **Only complete input.** Conversion produces no text for an incomplete or
  invalid structure, so no job is submitted; it returns the holes or the
  position that makes the structure invalid instead, as
  [holes and completeness](#holes-and-completeness) defines them.
- **Only the locked grammar.** Conversion writes only forms that kalyx 0.1.0's
  grammar accepts, with one exception that the last item names:
  - no juxtaposition, and no digit directly followed by a name: kosmo treats
    `2x` as a juxtaposition, and kalyx reads it as one invalid number literal,
    never as a product;
  - no unary plus and no character outside ASCII;
  - `.5` and `1.` are written as `0.5` and `1`;
  - the exception is function-call syntax and reserved names: conversion
    passes them on unchanged, and kalyx reports them as `Unsupported`.
- **The grammar only grows.** Text that kalyx 0.1.0 accepts reads the same in
  every later kalyx version. Writings that it rejects, such as calls and
  reserved names, may later get a value or a different error, so they are
  judged again when kosmo moves to a later version.
- **Results only through output lines.** The submitter receives a result only
  as the output line that `kalyx::command::serialize` writes, and parses it
  with the display model. kalyx terms and `Outcome` stay with the kalyx runner
  on the worker thread (see [host job layer](#host-job-layer)); the runner
  takes the outcome kind and the error class from the `Outcome`, or from
  `serialize` when writing the line fails, as
  [protocol stub types](#protocol-stub-types) states. kosmo never uses the
  `Debug` formatting of kalyx's types. The wording of an error's message is
  not part of kalyx's interface, so kosmo reads the other fields of the line.
- **Spans.** The span of an error is a half-open range of bytes of the linear
  text, and it may be empty. The submitter maps it back into the snapshot that
  the text was converted from: a non-empty span to a fragment, an empty span
  to a position between items, and an error without a span, written `-`, to
  the whole line.
- **Version echo.** The output line echoes the three versions of the request.
  The job layer first checks them against the request: outside
  `cancel_requested`, a mismatch ends the job as `failed`, as
  [terminal states](#terminal-states) describes. Whether the result is current
  is then judged from the three versions and the submission, as
  [current, superseded and stale results](#current-superseded-and-stale-results)
  describes.
- **Reading back.** The display text of every value of kalyx 0.1.0 is text
  that its grammar accepts and reads back as the same term, so a value can be
  read back into a separate editing structure, from which reuse inserts copies
  into the buffer. That structure is checked against the editing structure's
  own limits, as [node identity](#node-identity) states. When the text is
  submitted again, kalyx reads it under the limits of that request like any
  other input, so the text of a term near those limits is read only under
  larger limits.

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
  thread calls `poll` or `wait`. The framework's event loop calls `poll` once
  in every round, and while a job that it submitted has not delivered its
  terminal state, each wait ends at most `JOB_POLL_INTERVAL`, 16 ms, after the
  round's clock reading. A report is thus taken in the round after it arrives,
  as [job reports](#job-reports) describes.
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
- All conversion into kalyx types is in the kalyx runner, which builds
  `Request` and `Context` on the worker thread; kalyx terms never leave it.
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
  device description and state query belong to the
  [device ports](#device-ports).

### Delivered key event

- `KeyEvent { usage, meaning: KeyMeaning, mods: ModSet, action: KeyAction, time: MonoTime, synthetic: bool }`.
  The delivered key event carries both the physical usage and the key
  meaning.
- `KeyAction` is Press, Release, Repeat or LongPress; `synthetic` marks a
  release that the server synthesized.
- Focus generation, epoch and trace number are added in outer layers by the
  framework, the trusted core and the display protocol.
- `ModSet(u16)`: the low 8 bits follow the HID modifier byte; the high 8 bits
  hold the layer state that
  [layer keys and layer state](#layer-keys-and-layer-state) defines. More bits
  are a change of the event record.
- `KeyMeaning` is `Char(char)` or `Named(NamedKey)`. `NamedKey` is a `u16`
  numbered table in `kosmo-types` that only grows and never renumbers; the
  framework and the display stack propose entries.
- Only kosmo's display server produces repeat and long press, timed by an
  injected clock; which keys and parameters is defined with the key maps, as
  [repeat and long press](#repeat-and-long-press) describes. The
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
  advances it, timers, timeouts) belong to the framework's [event loop and
  clock](#event-loop-and-clock).
- Decoding from bytes is bounded and returns errors, never panics. The wire
  format belongs to the display protocol's [wire format](#wire-format), device
  service messages to krepi's input device interface. `RecordBody` and
  `KeyAction` are `#[non_exhaustive]`; pointer, touch and input methods will
  be new bodies, added by a design change.
- An event script holds one record per line, in one layer only: framework
  scripts use delivered events, server tests may use device records. The
  projection from delivered events to device records takes usage, press or
  release, and time; lines with one time form a packet ending with
  `PacketEnd`. The key lines of scripts run through both the headless
  framework and the headless server are only presses and releases, and a test
  fails when the server's key meaning or modifiers differ from the script.
  [Headless backend and scripts](#headless-backend-and-scripts) defines the
  other lines of a script and where its text format is fixed.

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
- Only the kalyx adapter `kosmo-math-kalyx` depends on kalyx directly: no
  other crate, and not `xtask`, declares the package `kalyx`. Other crates
  reach kalyx only through the adapter.

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
each crate are listed in its entry, and the allowed packages of each
workspace's `Cargo.lock`, with the license exceptions of the workspace, in the
allowlist that the workspace's `lock-allowlist` names; the deps step of
`cargo xtask check` compares both with what Cargo resolves and checks the
licenses and build scripts of the packages.

Crates and workspaces appear with their first code. So far `framework/` exists,
with the library `kosmo-math-model`, and `desktop/`, with the program `kosmo`;
the table states the rules of `display/` and lists no crate for it. A change
that adds a crate registers it in the table, and the first crate of a
workspace adds the workspace root. The layout step of `cargo xtask check`
enforces this layout on the tracked files, and the deps step on what Cargo
resolves; Contributing describes the checks of
[the layout step](../CONTRIBUTING.md#the-layout-step) and of
[the fetch and deps steps](../CONTRIBUTING.md#the-fetch-and-deps-steps) and
their limits.

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
  default hasher are not listed; determinism belongs to the [rendering and
  determinism rules](#rendering-text-and-determinism). The list forbids; it is
  not an allow-list of `std` interfaces. Adding frontends adds no external
  dependencies to core libraries.
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
  crates are registered by the change that creates them; `kosmo-testkit` is
  expected to be a boundary crate, and [host programs](#host-programs) lists the
  boundary crates of the display stack with their categories.
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
  by the display server's trusted core, as
  [epoch and focus generation](#epoch-and-focus-generation) describes; the
  framework carries them in the outer layer of platform events.
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
  of the stop key. The host keys that produce these meanings are in the display
  server's [host key map](#host-key-map).

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
  transport's wait with the same deadline, which the
  [client transport port](#client-transport-port) provides.
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
  krepi are not part of the first release. The shell extension's events do not
  pass through this backend: `kosmo-client` hands them to a receiver that the
  shell registers, as [shell extension](#shell-extension) describes.

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
  its license as `MIT/Apache-2.0`, which is not a valid SPDX expression, so the
  deps step requires a license exception for it: the allowlist of the framework
  will list one that reads it as `MIT OR Apache-2.0`, with its reason.

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
  `kosmo-math-controls`; a later crate is judged by the same definition. In
  `display/`, `kosmo-compositor` and later server modules that write pixels
  belong to it. `display/` needs no transcendental functions in the first
  release; if it needs them later, their source is a design change.
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
  its own, checked by the deps step and with the goldens updated by
  category. Adding or replacing a package is a design change.

## Display stack

The display stack is kosmo's display protocol, its display server and the ports
through which the server reaches devices, clients and krepi's supervisor. Its
crates are in `display/`: the protocol `kosmo-proto` and the shared types
`kosmo-types` in layer D0; the pure cores `kosmo-scene`, `kosmo-focus` and
`kosmo-surface` in layer D1; the compositor `kosmo-compositor` and the window
policy module `kosmo-wm` in layer D2; the server `kosmo-server` with its
headless backend `kosmo-server-headless` and its nested backend
`kosmo-server-host` in layer D3; and the protocol's conformance tests
`kosmo-proto-conformance` in layer D4. In the first release the input pipeline
and the key maps are a module of `kosmo-server`, not a server library of their
own in layer D2. Clients reach the server through the client library
`kosmo-client` in layer F0 of `framework/`, which the framework's [display
server backend](#display-server-backend) uses.

What is provisional:

- Protocol v0 is provisional: its object model, wire format, version
  negotiation, errors and transport rules, the rules of the shell extension
  included, and the client transport port that carries its frames. v0 is
  provisional until it is frozen at the first pre-release of the kosmo version
  paired with krepi 0.2. Until then, only a revision that a review of this
  design makes necessary, approved as a design change, may be incompatible;
  every other revision only adds compatibly, as [version
  negotiation](#version-negotiation) states. A review is due when krepi's side
  of the display path changes, that is its development display path, the form of
  its drivers, the hardware side of rendering, or the formats of events or
  scan-out, and at the freeze, when the constants and queue capacities are
  checked against krepi's transport. `kosmo-proto` is the only crate of the
  display stack that the layout table marks provisional. `kosmo-server` and
  `kosmo-client` are not marked; their provisional parts are the ports listed
  here.
- The device port details that krepi still decides are provisional: the
  operations, events, errors and description fields of the display and input
  device ports, the profile of the minimal frame-buffer subset, the holder
  handover, and that the ports have no mode list, mode switching, brightness or
  backlight. They are compared with krepi's device service interface when krepi
  fixes it for krepi 0.2: what the ports can map stays, and krepi's integration
  adapts to it.
- These port details are settled, because krepi has chosen the form of its
  display and input drivers: there is one display device instance for each
  display engine, which may drive several outputs; there is one input device
  instance for each input device service, so a stop key on a button driver of
  its own is an instance of its own; output enumeration states the transforms
  that the scan-out hardware can do; the display server performs rotations that
  the hardware cannot do while it composes; each output scans out one plane; and
  vblank events come from the display device service, which generates them with
  a timer on QEMU.
- The route of the protocol is not provisional: a session is a capability, its
  identity comes from the authenticated endpoint, roles are granted one by one,
  and every protocol object has a row in the [Wayland table](#wayland-table).
  Neither are the stop key's path and the boundary between krepi's drivers and
  the display server.
- Everything else in this section is not provisional: the structure of the ports
  and the stop-request port, the trusted core and the execution model, the key
  map engine and the stop key, and the host form, except that the counts and
  framing of the [host transport](#host-transport) follow the transport rules of
  protocol v0 and change with them.

### Protocol v0

Applications and the shell talk to the display server through sessions of
protocol v0. A session is a capability: its identity comes from the
authenticated endpoint that the transport reports, and the server grants its
roles; no message names an application or asks for a role. `kosmo-proto` holds
the protocol's types, constants and codec, the shell extension's included.

#### Objects and identifiers

v0 has exactly six objects. Each kind of object is an interface with a `u16`
number:

| Number | Object | What it is |
| --- | --- | --- |
| 1 | Session | The session itself, which exists with the connection |
| 2 | Buffer | A client buffer: pixels in a memory object of the client |
| 3 | Surface | Content that the client commits |
| 4 | AppView | The application role of a surface |
| 5 | Seat | The logical seat |
| 6 | Keyboard | The key events of the seat |

- Numbers 7 to 11 are reserved for EmbeddedView, Pointer, Touch, TextInput and
  ImeSession. `kosmo-proto` defines no type or message for them, no `Welcome`
  lists them, and binding one gives `Unsupported`. They are added later with
  their messages and their rows of the [Wayland table](#wayland-table); Seat
  gains capability events with Pointer and Touch.
- Popup, DataOffer, DataSource, Output, FrameCredit, PresentationFeedback and
  Capture are not in the first release and reserve no number; objects added
  later take numbers from 12 in the order they are added.
- Interface numbers: 0 is unused; `0x0001` to `0x00FF` are the core objects;
  `0x0100` to `0x01FF` the privileged [shell extension](#shell-extension);
  `0x0200` to `0xFEFF` later extensions, 256 numbers each, assigned by the
  revision that adds the extension; `0xFF00` to `0xFFFF` are reserved. A range
  states which part of the protocol an interface belongs to, not who may use it:
  permission always comes from the session's roles.
- Opcodes are `u16`, numbered from 0 within each interface, separately for
  requests and for events. They only grow and are never reused.
- Object identifiers are `u32`. 0 means none and is never a target or a
  reference; 1 is the session itself. A client creates objects with identifiers
  in $[2, 2^{31})$, each greater than every identifier it has used in the
  session, so identifiers are never reused and destroying an object needs no
  acknowledgement. $[2^{31}, 2^{32} - 1)$ is for objects that the server
  creates, of which v0 has none, and $2^{32} - 1$ is reserved. A client that
  runs out of identifiers can only end the session. A stale handle is thus
  always an unknown object on the wire. Extension objects use the same space and
  the same rule.

#### Messages

A call is a synchronous request with a reply; every other message goes through
the session's bounded queues, as [transport rules](#transport-rules) describe.
The body of every call starts with `queued_before: u64`, and every call that
creates an object carries the new object's `version`. An event marked reserved
may use the reserved part of the event queue.

| Object | Message | Direction | Path | Arguments |
| --- | --- | --- | --- | --- |
| Session | `Hello` | Client to server | Call | `major: u16`; `session_version: u32`, the highest Session version that the client supports |
| Session | `Welcome` | Server to client | Reply to `Hello` | `epoch: u64`; `formats: u32`, the set of accepted pixel format codes as bits; `output_width: u32`, `output_height: u32`, the logical size of the only output; `session_version: u32`; the interface table, a segment of at most `MAX_INTERFACES` entries of `interface: u16` and `version: u32` |
| Session | `Reply` | Server to client | Reply to the other calls | `status: u16`, 0 for success or a code of class A; `detail: u32` |
| Session | `Bind` | Client to server | Call | `interface: u16`, `version: u32`, `new_id: u32` |
| Session | `CreateBuffer` | Client to server | Call | `new_id: u32`, `version: u32`; `width`, `height`, `stride: u32`; `format: u16`; one memory object attached to the call |
| Session | `CreateSurface` | Client to server | Call | `new_id: u32`, `version: u32` |
| Session | `CreateAppView` | Client to server | Call | `new_id: u32`, `version: u32`, `surface: u32` |
| Session | `Ping` | Server to client | Event | `serial: u32` |
| Session | `Pong` | Client to server | Queued | `serial: u32` |
| Session | `Violation` | Server to client | Event, reserved | `reason: u16`, a code of class B; `opcode: u16`; `object: u32` |
| Buffer | `Released` | Server to client | Event, reserved | `commit_seq: u64` |
| Buffer | `Destroy` | Client to server | Queued | None |
| Surface | `Commit` | Client to server | Queued | `seq: u64`; `buffer: u32`; `damage`, a segment of at most `MAX_DAMAGE` rectangles of `x`, `y`, `width` and `height`, each `u32`, in buffer coordinates |
| Surface | `Destroy` | Client to server | Queued | None |
| AppView | `Configure` | Server to client | Event | `width: u32`, `height: u32` |
| AppView | `Destroy` | Client to server | Queued | None |
| Seat | `GetKeyboard` | Client to server | Call | `new_id: u32`, `version: u32` |
| Seat | `Destroy` | Client to server | Queued | None |
| Keyboard | `Enter` | Server to client | Event | `surface: u32`, `epoch: u64`, `gen: u64` |
| Keyboard | `Leave` | Server to client | Event | `surface`, `epoch` and `gen` as they were before leaving |
| Keyboard | `Key` | Server to client | Event | `surface`, `epoch`, `gen`; `usage_page: u16`, `usage_id: u16`; `meaning_kind: u16`, `meaning_value: u32`; `mods: u16`; `action: u8`; `synthetic: u8`; `time: u64` |
| Keyboard | `Overflow` | Server to client | Event, reserved | None |
| Keyboard | `Destroy` | Client to server | Queued | None |

Session:

- A connection starts without a session. A `Hello` that the server accepts,
  answered by `Welcome`, establishes the session, and the session closes when
  the client disconnects, when it violates the protocol or when the server ends.
  A `Hello` with another major version gets only the rejection, and the
  connection ends without a session. Any message other than `Hello` before the
  session is established, and a second `Hello`, are state violations. Closing
  releases every child object, and the duty to return committed buffers lapses.
- `Hello` is the first message, and its `queued_before` is 0. It carries no
  application identity, nothing that could request a role and no interface
  table.
- `Welcome` lists the interfaces that the server offers the session, those it
  supports and the session's roles allow, each with its highest version; Session
  is always among them. `formats` and the output size are what the framework's
  `Capabilities` of the [platform interface](#platform-interface) reports, and
  `epoch` is the server's epoch. It carries no session number, roles or quotas.
- `Reply.detail` is the server's major version for `VersionMismatch`, the
  interface number for `Unsupported` and `PermissionDenied`, the kind of quota
  for `QuotaExceeded` (1 surfaces, 2 views, 3 buffers, 4 buffer bytes), and 0
  otherwise.
- `Bind` creates the root object of an interface. In v0 only Seat can be bound;
  the root objects of extensions are bound the same way. Binding an offered
  interface that has no root object, such as Buffer, gives `InvalidArgument`.
- `Ping` serials strictly increase within a session, and a `Pong` must answer a
  serial that was sent and not yet answered, or it is a state violation. The
  protocol gives only this mechanism: when the server probes, how long it waits
  and how the shell learns that an application does not respond are defined with
  the shell extension.
- `Violation` is the last message of a session; the server then ends the
  connection.

Buffer:

- `CreateBuffer` carries the four fields of a
  [`BufferDesc`](#pixel-formats-and-buffers) and exactly one memory object,
  which the transport attaches to the call; frames never contain memory objects.
  The server checks the description by the rules of `BufferDesc`, checks that
  the format is in `Welcome.formats` and that the memory object holds at least
  `stride × height` bytes, and otherwise replies `InvalidArgument` and changes
  nothing. A buffer counts toward the session's quotas of buffers and of buffer
  bytes. v0 has no pool offsets and no usage flags.
- Pixel format codes are `u16`: 0 is unused, 1 is `Rgba8Premul`, 2 `Xrgb8888`, 3
  `Rgb565` and 4 `Argb8888Premul`, the four [pixel
  formats](#pixel-formats-and-buffers); other values are out of range.
- A buffer is writable, becomes committed when a `Commit` references it, and
  becomes writable again with `Released`, which comes once the server has read
  it or a newer commit of the same surface has superseded it. The server treats
  it as writable only after the client has taken that `Released` from its event
  queue, so each buffer has at most one `Released` that the client has not
  taken. Until then, a commit that references the buffer, or destroying it, is a
  state violation.
- `Released` comes exactly once for each commit, or lapses when the session
  closes; afterwards the server never accesses the buffer. A buffer is
  referenced by at most one commit that has not been released, and `Destroy` is
  allowed only while it is writable.
- Writing a buffer before its `Released` affects only the client's own picture,
  as [buffer sealing](#buffer-sealing-and-private-copies) explains.

Surface:

- `Commit { seq, buffer, damage }` is the whole pending state, so a commit is
  atomic by construction. Pending properties added later, such as an input
  region or a transform, will be separate requests that take effect with the
  next `Commit`.
- `seq` strictly increases on each surface; a `Commit` whose `seq` does not is a
  state violation. `buffer` is a writable Buffer of the session; 0 is out of
  range, since v0 cannot detach content. Rectangles decode into the damage
  rectangle type of `kosmo-types`; those with a width or height of 0 are dropped
  and the parts beyond the buffer are clipped.
- A new commit supersedes the previous one when the server has not read that
  yet: the superseded buffer is released unread, and its damage is merged into
  the new commit.
- Only a surface with a role can commit; a `Commit` on a surface without a role,
  or whose role has expired, is a state violation. A surface starts without a
  role and gets the application role with `CreateAppView`; once its AppView is
  destroyed its role has expired, and it can only be destroyed. A surface has at
  most one role in its lifetime.
- `Destroy` is allowed in any state and destroys the surface's AppView too; each
  committed buffer that has not been released gets one `Released` once the
  server has stopped accessing it. Surface has no events in v0.

AppView:

- `CreateAppView` gives `PermissionDenied` when the session has no application
  role, and is a state violation when the surface already has a role or its role
  has expired. Views count toward the session's quota of views.
- v0 has no requests for placement, layers or stacking order: the window policy
  decides placement, and the trusted core enforces the placement constraints.
- `Configure` gives the view's logical size, with the server's rotation already
  applied. It is queued before the reply to `CreateAppView` and again whenever
  the size changes. The size is a hint that the client commits to; a buffer of
  another size is composed as [private
  copies](#buffer-sealing-and-private-copies) describe and is no protocol error.
  There is no acknowledgement in v0: a configuration in two steps comes with a
  later window model as a new AppView version. In the first release, whose
  policy shows applications full screen, the size changes only with the output.
- When a view is destroyed, focus falls back as the window policy decides.

Seat and Keyboard:

- A session has at most one Seat, bound through `Bind` with interface 5; a
  second is a state violation. The Seat is a logical seat that merges every
  keyboard-like input device service that the server holds; the protocol shows
  no devices and no device identities. A Seat has at most one Keyboard, and a
  second `GetKeyboard` is a state violation; destroying the Seat destroys its
  Keyboard. v0 has no capability events.
- A Keyboard sends no events after its `Destroy` or the destruction of its Seat.
  If focus is on one of the session's views when the Keyboard is created, the
  server sends `Enter` at once.
- `Enter`, `Leave` and `Key` carry an outer stamp of the surface, the epoch and
  the focus generation `gen`. The epoch and the generation are `u64` newtypes of
  `kosmo-types`, whose values come from the trusted core, as [epoch and focus
  generation](#epoch-and-focus-generation) state.
- `Key` encodes the fields of the [delivered key event](#delivered-key-event)
  one by one, adding and dropping none:
  - `usage` as two `u16`;
  - `meaning_kind` 1 for `Char`, with a Unicode scalar value in `meaning_value`,
    or 2 for `Named`, with the `NamedKey` number in the low 16 bits of
    `meaning_value` and 0 in its high 16 bits;
  - `mods` unchanged: the low 8 bits as the event record defines them, and in
    the high 8 bits 0 for every bit that the Keyboard's version does not define;
  - `action` 0 for Press, 1 for Release, 2 for Repeat and 3 for LongPress;
  - `synthetic` 0 or 1, and 1 only with Release;
  - `time`, the `MonoTime` in nanoseconds, passed on unchanged.
- Every argument takes whole words, so `Key` is 64 bytes, and `Enter` and
  `Leave` are 28 bytes.
- The server sends a Keyboard only the `NamedKey` numbers and actions of its
  version; new `NamedKey` entries, one or several, come with a new Keyboard
  version.
- Version 1 of Keyboard has the nine `NamedKey` entries of the first release:
  the five of [commands and key bindings](#commands-and-key-bindings) and the
  four PC modifier entries of the [key maps](#key-maps).
- When focus moves, the leaving side first gets a synthetic Release with the old
  stamp for each key it has pressed, then `Leave`, and then the new focus gets
  `Enter`. `Enter` lists no pressed keys and no modifiers, since the framework
  keeps no key state.
- The stop key's short press and the other reserved keys are never delivered as
  `Key`. The core protocol has no messages for global keys; the shell learns of
  them through the shell extension. No object reads another client's input or
  forges input for another client.
- The server resolves key meanings, so the protocol has no key map object and no
  text fields. Key maps for Linux programs are a task of the compatibility
  service.
- v0 has no trace number in `Key` or `Commit`. When the server gains trace
  points, `trace: u64` is appended to both, with new Keyboard and Surface
  versions, as a compatible revision.
- A client drops events whose epoch differs from `Welcome.epoch` and events of
  objects or surfaces that it has destroyed. After `Overflow` it treats every
  key as released and focus as left until the next `Enter`.

#### Protocol errors

Errors have two classes:

- **Class A, results of calls: the session stays.** They come only in the
  `Reply.status` of a call, and the state does not change. The one exception is
  `VersionMismatch`, which comes before there is a session: the server ends the
  connection after replying.
- **Class B, protocol violations: the session closes.** The server sends
  `Violation { reason, opcode, object }` through the reserved part of the event
  queue and ends the connection.

Queued requests, `Commit`, `Pong` and every `Destroy`, never give a class A
result: they either take effect or are a violation. Class A results thus come
only from calls, and a client always learns them synchronously.

| Code | Name | Class | When |
| --- | --- | --- | --- |
| 1 | `VersionMismatch` | A | `Hello` has another major version; `detail` is the server's major version, and the connection ends |
| 2 | `Unsupported` | A | `Bind` names an interface that the server does not support, reserved and unassigned numbers included; a requested `version` is above the one that `Welcome` lists; a creating call needs an interface that `Welcome` does not list |
| 3 | `PermissionDenied` | A | The server supports the interface but the session's roles do not allow it, as for the shell extension, which is then not listed in its `Welcome`; `CreateAppView` without the application role; a request of the shell extension that would break the core's placement constraints |
| 4 | `QuotaExceeded` | A | A session quota would be exceeded; `detail` is the kind of quota |
| 5 | `InvalidArgument` | A | Well formed but invalid: a buffer description that fails its checks, a format not in `formats`, a memory object that is too small or not exactly one, or a memory object that does not meet the transport's requirements |
| 6 | `OutOfMemory` | A | The server could not allocate its own state for the call |
| 64 | `Truncated` | B | The frame is shorter than its header or than its stated length; the arguments of the target's version run past the end of the frame, a segment's content included |
| 65 | `TooLong` | B | The frame is longer than `MAX_FRAME`; bytes remain after the arguments; a segment's count is above its bound |
| 66 | `OutOfRange` | B | An unknown enum code; a reserved or undefined bit, `mods` included, that is set; nonzero high bits of a narrow value or nonzero padding of a segment; a frame length that is not a multiple of 4; an invalid scalar value or string; an identifier or a version of 0 where it must not be 0 |
| 67 | `UnknownOperation` | B | The opcode is not defined in the target's version |
| 68 | `UnknownObject` | B | The target or a referenced identifier is not a live object of the session, destroyed objects included |
| 69 | `WrongKind` | B | The identifier is a live object of another kind than the one required |
| 70 | `BadNewId` | B | A new identifier is outside the client's range or not greater than every identifier used |
| 71 | `StateViolation` | B | The object's current state does not allow the request, as the rules of each object state; `queued_before` does not match the queue; more unprocessed request frames than `C_R` |
| 72 | `BufferFault` | B | The server could not read a client buffer while copying it |

- Codes 1 to 63 are class A, 64 to 127 class B, and the rest are reserved.
- The calls give these class A results: `Hello` 1; `Bind` 2, 3 and 5;
  `CreateBuffer` 2, 4, 5 and 6; `CreateSurface` 2, 4 and 6; `CreateAppView` 2,
  3, 4 and 6; `GetKeyboard` 2 and 6.
- `BufferFault` follows no message: it comes when copying at a latch fails, with
  the buffer as `object` and the opcode of the `Commit` that referenced it.
- A call that causes a violation gets no `Reply`: the server sends `Violation`
  and closes the session, and the call ends with `Disconnected`.
- The client transport port adds three results that are not on the wire:
  `QueueFull` when the request queue is full, which is backpressure and keeps
  the session; `Timeout` when a call has no reply by its deadline; and
  `Disconnected` when the session has ended or the server is gone, with the
  reason: a `Violation` from the server, a server message that the client could
  not decode, or a lost connection.
- An unknown operation, a missing permission, a stale handle, an invalid
  argument, missing memory, a full queue and a timeout are thus all distinct.
  Cancelling belongs to the job layer and a failed application ends its session,
  so v0 has no codes for them.
- A client that cannot decode a server message ends the session and reports
  `Disconnected` with the reason, without panicking. After a call times out its
  result is unknown, so the client ends the session and does not retry.

#### Wire format

1. **Frame header.** 8 bytes: `object: u32`, the target of a request or the
   source of an event; `opcode: u16`; and `length: u16`, the length of the whole
   frame in bytes, header included. Everything is little endian. `length` is a
   multiple of 4, at least 8 and at most `MAX_FRAME`.
2. **Arguments and words.** The body holds the message's arguments in the order
   of its definition, which gives the kinds and the order of the arguments and
   the bound of each segment and comes from the target's interface and version.
   Each argument takes whole 4-byte words: a `u32` one word; a `u64` two words,
   the low word first; a `u16`, a `u8` or an enum code one word with its high
   bits 0. There is no other alignment and no padding between arguments.
3. **Segments.** A variable-length argument is a segment: a word with the count,
   then the content, padded with zeros to a multiple of 4 bytes. A segment is a
   byte string, counted in bytes; a string, counted in bytes, in UTF-8 without
   U+0000 and without a terminating NUL; or an array of fixed-size elements made
   of words, counted in elements. A message may have several segments anywhere
   among its arguments, but no segment contains another, so nesting is one
   level. Each count has a bound in the message definition, and the decoder
   compares the count with it before it computes the length, so the length
   cannot overflow. v0 has two segments, the damage of `Commit` and the
   interface table of `Welcome`, and no byte strings or strings.
4. **One encoding.** Reading every argument of the definition from the start
   must end exactly at the end of the frame: ending early is `Truncated`, bytes
   left over are `TooLong`, and so is a count above its bound. Padding and the
   high bits of narrow values are 0, strings are valid UTF-8, and enum codes,
   reserved bits and scalar values are checked (`OutOfRange`). Decoding an
   accepted frame and encoding it again therefore gives the same bytes, and
   encoding a message and decoding it gives the same values.
5. **A private copy, no allocation.** The transport copies a frame, at most
   `MAX_FRAME` bytes, into private memory before the decoder reads it. Decoding
   never allocates on the heap: a segment decodes into a view that borrows the
   copy, a byte slice, a `&str` once checked or a read-only view of checked
   elements, or is held inline up to its bound. The definition, not the frame,
   gives the number of segments, so the work is proportional to the frame's
   length.
6. **Capabilities outside frames.** Memory objects never enter frames; the
   transport attaches them to calls.
7. **Constants.** `kosmo-proto` has `MAX_FRAME = 512`, `MAX_DAMAGE = 16` and
   `MAX_INTERFACES = 32`; changing one is a revision of the protocol. The
   longest v0 messages are a full `Welcome`, 292 bytes, and a full `Commit`, 280
   bytes. `MAX_FRAME` grows only with a new Session version, and a session uses
   the value of its Session object's version.

- The decoder's input is one frame and the session's object table, which maps
  identifiers to an interface and a version. It looks up the header's `object`,
  which gives `UnknownObject` if it is not a live object of the session; then
  the opcode in that interface and version, `UnknownOperation` if it is
  undefined; and then it reads and checks the arguments by rules 1 to 4. `Hello`
  arrives before the Session object has a version; its arguments are the same in
  every version and every major version. Identifiers that the body references
  are checked for `UnknownObject` and `WrongKind`, and new identifiers for
  `BadNewId`, while decoding or dispatching, with the same result.
- The server closes the session on any decoding failure, and a client ends the
  session on any decoding failure. Extension messages decode by the same rules.
- `kosmo-proto` depends only on `kosmo-types`, has no external dependency, is
  `no_std` with `alloc` and `#![forbid(unsafe_code)]`, and is fuzz tested. The
  bounds of its contract are the length `MAX_FRAME`, the bound of each segment's
  count and one level of nesting.

#### Version negotiation

1. The protocol has one major version, 0 for v0. Every interface, those of
   extensions included, has a `u32` version from 1; 0 is unused. A version
   counts compatible revisions of its interface: each one increases it by 1.
2. `Hello` carries the client's major version and the highest Session version
   that it supports, but no interface table. Another major version gets
   `VersionMismatch`, and the connection ends. Otherwise the Session object's
   version is the lower of the client's and the server's highest, written in
   `Welcome.session_version`, and `Welcome` lists every interface offered to the
   session with its highest version.
3. An object gets its version when it is created. A root object from `Bind`, and
   an object from `CreateBuffer`, `CreateSurface`, `CreateAppView`,
   `Seat.GetKeyboard` or a later creating request, has the version that the
   request names, for life. Objects of one interface may have different
   versions, so the session's object table keeps a version for each object. The
   client names the lower of its own highest version and the one that `Welcome`
   lists. The server only checks it: a version of 0 is `OutOfRange`; an
   interface missing from `Welcome` gives `PermissionDenied` when the server
   supports it but the session's roles do not allow it, and `Unsupported`
   otherwise; a version above the one listed gives `Unsupported`. A new object's
   version does not depend on the version of the object that created it.
4. A new version only adds compatibly: opcodes of new requests and events, enum
   values, `NamedKey` entries and `mods` bits, or arguments, segments included,
   appended to an existing message. Both sides take the arguments of each
   message from the object's version, and neither sends an object a message, a
   value or an entry that its version lacks.
5. The frame header, `Hello` and `Reply` keep their layout in every major
   version, and `Welcome` within one major version; new Session versions only
   add messages.
6. Until v0 is frozen, a revision that a review of this design makes necessary,
   approved as a design change, may be incompatible. It keeps major version 0
   and the interface versions, and is identified by its commit: before the
   freeze krepi builds the device from one pinned kosmo commit, so no
   negotiation spans such a revision. Other revisions only add compatibly.
   Version 1 of the protocol comes with kosmo 1.0.

So the version that both sides support is fixed for each object when it is
created: no higher than the client supports, because the client names it, and no
higher than `Welcome` lists, because the server checks it.

#### Transport rules

1. **Two paths.** Each session has a call path, for a synchronous call and its
   reply, and two bounded queues of frames: the request queue from the client to
   the server and the event queue from the server to the client.
2. **Which path.** Operations that need a result are calls: `Hello`, `Bind`,
   `CreateBuffer`, `CreateSurface`, `CreateAppView`, `Seat.GetKeyboard`, and
   later the calls of extensions. Operations without a class A result go through
   the request queue: `Commit`, `Pong` and every `Destroy`, so a destruction
   stays in order with earlier commits. Every event goes through the event
   queue.
3. **Order.** A session has at most one call without a reply. `queued_before` is
   the number of frames that the client has put into the request queue before
   the call; the server first processes the request queue up to that number,
   frames that are already in it, and then the call, and fewer frames is a state
   violation. All requests of a session are thus processed in the order the
   client sent them.
4. **Replies.** Each session has one reply slot. Events that a call causes, such
   as `Configure`, go into the event queue before the reply. The server handles
   the call as soon as it has processed the queue up to `queued_before`, without
   waiting for the client.
5. **The server never waits for clients.** Writing a reply or an event never
   blocks the server. Synchronous waits go only from client to server and from
   server to device service.
6. **Event queue.** The event queue is a first-in, first-out ring of `C_E + R`
   frames. Ordinary events (`Enter`, `Leave`, `Key`, `Configure`, `Ping`) are
   put only while fewer than `C_E` are queued; events that must arrive
   (`Released`, `Overflow`, `Violation`) may use all `C_E + R`. `R` is
   `B_max + 2`: `B_max` is the session's quota of buffers, each with at most one
   `Released` that the client has not taken, and 2 counts one `Overflow` and one
   `Violation`. This needs the server to see the event queue's read position,
   and a request that the client sends after taking an event must show that
   taking when the server reads the request.
7. **Full request queue.** The request queue holds `C_R` frames. When it is
   full, the client's put returns `QueueFull` and sends nothing; the session
   stays, and the client tries again later. The framework's `Presenter` may then
   return `Busy`.
8. **Deadlines.** A client's calls carry a deadline, and its waits may; a wait
   has none only when the framework waits without one. A call that times out
   ends the session. The server sets no timeout for clients and learns that an
   application does not respond through `Ping`.
9. **Private copies.** Frames are copied out of shared memory into private
   memory before decoding.
10. **Fairness.** How many request frames of a session the server processes in a
    round, and the rotation between sessions, belong to the [execution
    model](#execution-model); processing never blocks across sessions.
11. **Constants.** `C_E` and `C_R` are 64 frames in the first release, each
    frame at most `MAX_FRAME` bytes. The in-process channel, the [host
    transport](#host-transport) and krepi's endpoints and shared queues follow
    the same rules.

When an ordinary event finds no ordinary slot:

1. The session enters overflow: an `Overflow` goes into the reserved part, and
   that event and every later ordinary event are dropped, neither kept nor sent
   again.
2. During overflow, events that must arrive still go into the reserved part.
3. Overflow ends when the server sees that the client has taken the `Overflow`.
   The server then sends the current `Configure` to every view that lost one,
   and the current `Enter` if focus is on one of the session's views; state
   events are sent again with their current values, while `Key`, `Leave` and
   `Ping` are not. When the `Overflow` is taken every earlier event has been
   taken, so there is room for these events. A new overflow starts only after
   the previous `Overflow` has been taken, so at most one is untaken at any
   time.
4. The client handles `Overflow` as the Keyboard rules state, so the `Leave` and
   the synthetic releases dropped during overflow need not arrive.
5. For this the server keeps three things per session: whether it is in
   overflow, the position of its `Overflow` in the queue, and which views lost a
   `Configure`.

A full event queue is backpressure, not an error. Only a protocol violation
closes a session, so a client that stops reading for a while keeps its session;
disconnecting a client that stops reading is left to a later release.

#### Shell extension

The privileged shell extension is the part of the protocol that only the shell's
session gets.

1. **Only for the shell role.** Its interfaces appear in `Welcome` only for a
   session with the shell role. Another session that binds one gets
   `PermissionDenied` and keeps its session. The trusted core grants the role,
   as [sessions, roles and placement](#sessions-roles-and-placement) describe,
   and no client can request it.
2. **Numbers.** Its interface numbers are `0x0100` to `0x01FF`, assigned from
   `0x0100` in order; each interface has its own opcodes. Its objects share the
   session's identifier space and rule, and its root objects are bound through
   `Session.Bind`.
3. **Versions.** Each of its interfaces has its own version under the protocol's
   major version, from 1 in the first release, with the rules of the core:
   `Welcome` lists them with the server's highest versions only for the shell's
   session, and its objects get their versions when they are bound or created.
   It is revised by the rules of the core, and frozen with v0.
4. **Wayland table.** Each of its objects has a row, with the status "shell
   extension v0 (provisional)".
5. **The same rules.** Its messages use the same frame header, encoding, bounded
   decoding, errors and transport rules as the core, and are split into calls
   and queued requests the same way, so a placement request that may be refused
   is a call. A request that would break the core's placement constraints gets
   `PermissionDenied`, and the session stays.
6. **Its events.** They go through the shell session's own event queue; the
   events of the extension that must arrive are listed with the extension and
   added to that session's reserved part `R`. `kosmo-client` sorts the events of
   a session by the interface of the object in the frame header: those of core
   interfaces go to the [display server backend](#display-server-backend), which
   returns them from the platform interface's `wait`; those of extension
   interfaces go to a receiver that the shell registers when it sets up the
   session, never through the platform interface. The transport does not tell
   them apart. The receiver's interface, and how the shell takes the extension's
   events in the same round, are defined with the shell.
7. **Its crates.** Its wire definition, with interface numbers, message types
   and codec, is a module of `kosmo-proto`, under the same provisional mark and
   fuzz tests as the core. Its role checks and dispatch are in `kosmo-server`,
   its policy in `kosmo-wm`, and its client interface in `kosmo-client`; the
   shell uses it through `kosmo-sdk`.
8. Its objects and messages are not counted among the six objects of v0.

#### Wayland table

Every protocol object has a row in a table that maps it to its nearest Wayland
counterpart:

- One row per object, ordered by interface number, with six columns: the kosmo
  object, its interface number, its status, the Wayland counterpart (interface
  and protocol, or none), the difference in meaning in one sentence, and the
  difficulty of translation (low, medium or high, as a judgement).
- The status is "v0 (provisional)", "shell extension v0 (provisional)" or
  "reserved". The table is written in English.
- In the first release it has the six core rows and the rows of the shell
  extension; the rows of the reserved objects are added with those objects.
- The table and the interface table of `kosmo-proto` are kept in the same
  commit, and a check fails when their rows, numbers and statuses do not match
  one to one.
- The Buffer row maps each pixel format to its DRM fourcc code, which, like the
  names of the [pixel formats](#pixel-formats-and-buffers), names components
  from the high bits of a little-endian word: `Xrgb8888`, `Rgb565` and
  `Argb8888Premul` have codes of the same names, `Rgba8Premul` has the code that
  names A, B, G and R, and the row notes that the two premultiplied formats are
  premultiplied.

The six core rows are, with the status "v0 (provisional)":

| Object | Number | Wayland | Difference | Difficulty |
| --- | --- | --- | --- | --- |
| Session | 1 | `wl_display` and `wl_registry`, with security-context | Identity comes from the authenticated endpoint, which core Wayland lacks; only protocol violations close a session, while every Wayland protocol error ends the connection; the protocol has a major version, and objects take their versions when they are bound or created, as in Wayland | Low |
| Buffer | 2 | `wl_buffer`, through `wl_shm` or dma-buf | Exactly one `Released` for each commit, while a Wayland compositor may read the pixels at any time after a commit until it releases the buffer; formats map to DRM fourcc codes | Medium |
| Surface | 3 | `wl_surface` | Content and damage come in one `Commit` with a sequence number, where Wayland has separate attach, damage and commit requests | Low |
| AppView | 4 | `xdg_toplevel` | Position and size come only from the window policy; no configuration acknowledgement in the first release | Low |
| Seat | 5 | `wl_seat` | One logical seat per session; v0 reports no capabilities | Medium |
| Keyboard | 6 | `wl_keyboard` | The server delivers the physical usage and the key meaning and sends no key map, and it produces repeats itself; a compatibility service generates XKB key maps for Linux programs | Medium |

The reserved objects will have these counterparts: EmbeddedView none, with high
difficulty; Pointer and Touch `wl_pointer` and `wl_touch`, medium; TextInput and
ImeSession text-input-v3 and input-method-v2, medium.

### Trusted core and execution

#### Tiers and trusted modules

The display server's modules fall into three tiers. Tier A decides who gets
input and what on the screen can be trusted; it is the trusted base of the GUI.
Tier B affects usability and whether pictures are right, but decides no
permission. Tier C is policy and appearance.

The trusted core is the three pure cores of tier A, `kosmo-scene`, `kosmo-focus`
and `kosmo-surface`, and an outer layer: the tier A modules of `kosmo-server`,
which are sessions and roles, the part of request dispatch that carries out
transitions of the cores, the wiring of global keys and the copy entry for
client buffers, and later the trusted presentation. The trusted base holds only
these.

| Module | Tier | Crate |
| --- | --- | --- |
| Sessions and roles: identity, the set of roles, and quotas granted and checked | A, outer layer | A module of `kosmo-server` |
| The part of request dispatch that carries out transitions of the cores | A, outer layer | `kosmo-server` |
| Lifecycle of buffers and surfaces, quota counts | A, pure core | `kosmo-surface` |
| View tree, stacking order, occlusion and placement constraints | A, pure core | `kosmo-scene` |
| Focus routing, focus generation, the foreground session, the reserved key table and the server's part of global keys | A, pure core | `kosmo-focus` |
| Wiring of global keys: stop requests through the stop-request port, switch reports through the shell extension | A, outer layer | `kosmo-server` |
| Copy entry for client buffers: shared memory is read only through the copy function that it calls | A, outer layer | `kosmo-server`, calling the transport's copy function |
| Input pipeline: device records to key meanings, layers, and the timing of repeat and long press | B | A module of `kosmo-server` |
| Software compositing, damage, format conversion, private copies and rotation | B | `kosmo-compositor` |
| Wiring of the device ports, the headless and nested backends | B | `kosmo-server`, `kosmo-server-headless`, `kosmo-server-host` |
| Window and layout policy | C | `kosmo-wm` |
| Protocol decoding | Entry, in no tier | `kosmo-proto` |

- **Narrow interface.** Tiers B and C reach the core in two ways only: read-only
  queries of the pure cores (`&self`), and messages to the outer layer, which
  turns them into inputs of the cores. They never hold a mutable reference to
  the state of a core. `kosmo-compositor` depends on the pure cores, and they do
  not depend on it.
- **The entry.** `kosmo-proto` parses untrusted bytes. A defect in it can at
  most misjudge the requests of one session and cannot get past the rules of the
  cores; it has `#![forbid(unsafe_code)]`, bounded decoding and fuzz tests. It
  is held to level A1 but is not part of the trusted base, although the layout
  table marks it pure core.
- **Sessions and roles** are a tier A module of `kosmo-server`, written as a
  pure core is, only through inputs and outputs, without input and output of its
  own and without reading a clock, but it is no crate of its own and not subject
  to the layout step's pure-core checks. Quota counts are kept in the pure core
  that holds the objects: surfaces, buffers and buffer bytes in `kosmo-surface`,
  views in `kosmo-scene`.
- Later releases add tier A modules for the trusted indicator area and trusted
  presentation, screenshot authorization and clipboard mediation, and tier B
  modules for frame scheduling, presentation feedback, output management and
  reconnection.

The first release has exactly five trusted modules:

| Module | Where | Holds | Takes | Gives | Never |
| --- | --- | --- | --- | --- | --- |
| Sessions and roles | `kosmo-server`, tier A outer layer | For each session its identity, from the authenticated endpoint and simulated on a host, its set of roles and its quota limits | Connections with their identities, the grant table, session closes | Results of role checks, the interfaces offered in `Welcome`, the release of child objects at close | Trusts an identity that a message states; accepts a client's request for a role |
| Lifecycle of buffers and surfaces | `kosmo-surface` | The buffer table with owners, states and commit sequences, the role state of surfaces, and each session's counts of surfaces, buffers and buffer bytes, private copies included | Creation, commits, superseding, latches, finished copies, destruction, session closes | Exactly one `Released` for each commit, quota rejections, violations | Holds pointers or reads pixels |
| View tree | `kosmo-scene` | Outputs, layers (background, application, shell, overlay and trusted), views, stacking order and placement | View creation, the window policy's placement decisions, destruction | A read-only scene for compositing, hit tests, rejected placements | Decides placement: the policy decides, and it enforces the constraints |
| Focus and input routing | `kosmo-focus` | The holder of focus, the focus generation, the epoch, the pressed keys and the foreground session | Delivered key events, device records only to look up the reserved key table, the policy's focus decisions, the holder's disappearance, requests that carry an input generation | `Enter`, `Leave`, synthetic releases, keys for the holder, rejected requests | Resolves key meanings, which tier B has done; reads a clock |
| The server's part of global keys | State in `kosmo-focus`, the reserved key table; wiring in `kosmo-server` | The reserved key table and what judging a short press needs | Device records that hit a reserved key | A stop decision for the foreground session; switch reports for the shell | Recognizes the long press of a reserved key, which belongs to krepi's input driver |

The four parts of the outer layer belong to these modules: sessions and roles is
the first module; the wiring of global keys is the outer layer of the server's
part of global keys; the copy entry, which decides when to copy which buffer and
judges a failed copy a violation, is the outer layer of the lifecycle of buffers
and surfaces; and dispatch belongs to the module whose core it drives. On a
host, the code that really reads mapped memory is the server end of the client
transport port, in the tier B backend crate `kosmo-server-headless`, and on the
device in krepi's integration; it is outside the five modules.

#### Sessions, roles and placement

Roles:

1. Roles are granted once, when a session is set up: the outer layer looks up
   the authenticated identity in a grant table and grants what the table says.
   On a host the table is given when the server starts, in tests it is a table
   of simulated identities, and on the device it will come from krepi's
   application registration. Afterwards only the shell's policy and
   notifications of krepi's policy service may change roles; the first release
   has neither, so a session's roles never change.
2. The first release has two roles: application, which `CreateAppView` needs,
   and shell, which the shell extension needs. Screenshot, input method and
   accessibility roles come later. Injected input is no role: scripted input on
   host and headless backends enters through the device ports and has nothing to
   do with client sessions.
3. At most one live session holds the shell role at a time. While one does, a
   new session of another identity entitled to it gets only the other roles that
   the table grants; the shell role can be granted again once the earlier shell
   session has closed.
4. A session can never raise its own roles, and closing a session releases all
   its child objects.

Placement constraints, enforced by the trusted core:

- Application views are only in the application layer. The shell's views are
  only in the shell layer and are created only through the shell extension. The
  overlay layer has no views in the first release. The trusted layer holds only
  the server's own trusted content.
- Once a reserved screen area is configured, which comes with the trusted
  indicator area, no layer other than the trusted layer intersects it.
- A decision that breaks a constraint is rejected and counted, and nothing
  changes.

When the holder's view is destroyed or its session closes, the focus core first
makes the holder none, increasing the focus generation, and then reports the
holder's disappearance to the window policy. If the policy names a new holder,
focus changes again by the same rules; otherwise it stays with none.

Global keys are recognized by their physical usage, before the key maps:

1. The reserved key table, the stop key and later system keys such as switch and
   home, is state of `kosmo-focus`, keyed by HID usage. On the device it comes
   from the physical keys registered in the configuration of krepi's input
   device services, on a host from the server's configuration. Neither the shell
   nor applications can change it; the shell can only configure shell actions
   through its extension.
2. The focus core looks up the usage of each device record in the reserved key
   table before the key map engine. A record that hits goes to the server's part
   of the [stop key](#stop-key): it never reaches the key map engine or routing,
   is never delivered as `Key` and cannot be grabbed. Only records that miss go
   on to the engine, so reserved keys need no entries in key maps.
3. The usage of a delivered key event is the physical usage of its device
   record.

The key maps are in tier B, so recognizing the stop key must not depend on them.

The compositor converts pixel formats only with the one conversion function of
`kosmo-types` that [pixel formats and buffers](#pixel-formats-and-buffers)
describe, both from client buffers into private copies and from the composed
buffer into a device's scan-out format, and never dithers. Its conversion
goldens use the same test vectors as the framework's. Ordered dithering, if ever
wanted, would first change the conversion function's rules.

#### Epoch and focus generation

The epoch identifies an instance of the display server:

1. The server does not produce its epoch: whoever starts it gives it. On the
   device it is the epoch that krepi's supervisor gives this service instance
   through krepi's start and recovery interface; on a host it is a start
   argument; in headless tests an injected constant. It is a constructor
   argument of `kosmo-focus`, never changes while the server runs, and is the
   same for every session, as `Welcome.epoch`.
2. It is at least 1; 0 is no epoch. The giver ensures that different instances
   have different epochs and that later ones are larger. The server only
   compares epochs for equality and never increases one, so it never runs out.
3. It is not the epoch of a device service, which changes when the device's
   holder changes; the server keeps its own epoch when a device service instance
   changes.
4. The first release has no reconnection, so a client sees one epoch in a
   session. `kosmo-client` drops events of another epoch; reconnection to a new
   epoch comes later.

The focus generation:

1. The first release has one seat with one counter in `kosmo-focus`,
   `FocusGen(u64)`, a newtype of `kosmo-types`. At start the holder is none and
   the generation 0.
2. Each change of holder, to none included, increases the generation by 1; a
   request that leaves the holder unchanged leaves the generation unchanged.
   Every `Enter`, `Leave` and `Key` carries the epoch and the generation at the
   time it is sent.
3. Focus changes for four reasons only: user input, which the server reports to
   the shell as a global key and the window policy decides on; a decision of the
   window policy; the holder's disappearance, since a destroyed view cannot keep
   focus; and a request that carries a recent input generation. The application
   protocol has no such request in the first release; if the shell extension has
   one, it is defined with the extension and still goes through the focus core,
   which applies the rules of generations and releases to the shell's requests
   too.
4. A change first sends a synthetic release with the old stamp for each key
   pressed on the leaving side, then `Leave`, then `Enter` to the new holder,
   and empties the set of pressed keys.
5. Running out: like the [version types](#three-version-types), the generation
   only increases and never wraps. A change to none because the holder has
   disappeared always takes place and increases the generation; any other change
   takes place only if the increased generation is below `u64::MAX`, and is
   otherwise rejected and counted, with holder and generation unchanged. So the
   generation reaches `u64::MAX` only with the holder none, and focus then never
   changes until the server restarts with a new epoch. The pure core's
   constructor can set the starting generation, so tests start from
   `u64::MAX - 2`.
6. The foreground session, the target of the stop key's short press, is a
   separate item of `kosmo-focus`: the window policy decides it, the core
   records it, and it becomes none when its session closes. The first policy
   makes it the session of the full-screen application view. It does not change
   when the shell's switcher takes focus.
7. The stop key cancels the jobs of the foreground session. This is the session
   in focus while focus is on an application view, and differs only while focus
   is on one of the shell's views, where cancelling the shell's own jobs would
   make no sense.

#### Pure-core rules

These rules hold for `kosmo-scene`, `kosmo-focus` and `kosmo-surface`, and for
later trusted pure cores of the same layer:

1. **Dependencies.** They depend only on `kosmo-types`, not on each other, not
   on `kosmo-proto`, which is provisional, and on no external package; they have
   no build script and no procedural macro. Facts that concern several cores
   come from the outer layer as inputs to each, such as "surface destroyed" to
   both `kosmo-scene` and `kosmo-focus`.
2. **Attributes.** `#![no_std]`, with `alloc` allowed;
   `#![forbid(unsafe_code)]`; no interior mutability, so no `Cell`, `RefCell`,
   atomics, `OnceCell` and the like. State changes only through step functions
   that take `&mut self`.
3. **Bounded.** The capacity of every collection is a constructor argument, such
   as the number of clients and objects per session from the quotas. Beyond it a
   step returns a distinct rejection and nothing changes.
4. **Deterministic.** No clock, since times are `MonoTime` inputs; no random
   numbers; no floating point; no hash containers, only ordered ones such as
   `BTreeMap`, or arrays. The same input sequence gives the same output
   sequence, and frozen vectors replay it.
5. **Total.** No input sequence makes them panic: arithmetic is checked, values
   from inputs are never unwrapped and indices never go out of bounds; errors
   are return values.
6. **Interface.** Each core has one step function,
   `step(&mut self, input) -> Outputs`, whose outputs are a bounded list of
   values, with no callbacks and no trait objects of the outer layer. Queries
   are read-only functions of `&self`; the compositor reads the scene through
   them.
7. **Keys.** The keys of objects in a core are newtypes of `u64` of that core,
   which the outer layer allocates and never reuses within an epoch, by the rule
   of protocol identifiers. The outer layer maps protocol identifiers to the
   keys of each core; keys shared by several cores are not added to
   `kosmo-types`.
8. **Checks.** The layout step checks rules 1 and 2 by the layout table's
   definition of pure core. That the cores depend only on `kosmo-types` and not
   on each other follows from the layers and the table: below layer D1 only
   `kosmo-types` and `kosmo-proto` exist, and pure-core crates never depend on a
   provisional crate; the cores list no same-layer crates. Review and contract
   tests, with numbered requirements, bounded exhaustive enumeration and frozen
   vectors, check rules 3 to 7.

#### Window policy module

1. **Tier and place.** `kosmo-wm` is in tier C and layer D2, outside the trusted
   base. It depends only on `kosmo-types`, neither on `kosmo-server` nor on the
   pure cores, and sees state only through the messages below, in which views
   and sessions carry the policy's own identifiers that the outer layer maps.
2. **Interface.** The policy trait, provisionally `WindowPolicy`, and its
   message types are in `kosmo-wm`. `kosmo-server` depends on `kosmo-wm` and is
   generic over the trait; the first policy and the stand-in policy of tests
   implement it.
3. **Messages.** The server tells the policy that a view appeared, with its
   session and the session's roles; that a view disappeared; that a session
   closed; that the holder of focus disappeared; the global keys reported to it,
   switch and the like but never the stop key; the requests that the shell
   extension forwards; and the output size. The policy decides placements (view,
   layer and rectangle), focus (a view or none), the foreground session (a
   session or none) and the stacking order. All of these are plain values.
4. **The core enforces.** The outer layer turns each decision into an input of
   the cores, which reject and count a decision that breaks the placement
   constraints or the focus rules, and nothing changes. The policy cannot give
   input to a view without focus or bypass the rules of generations and
   releases.
5. **No pixels, no input.** The policy never sees client buffers or key events,
   only the messages above.
6. **Execution.** The server calls it synchronously in its event loop, at the
   priority of requests, and each call does bounded work. It does no input or
   output, reads no clock, since times come in messages, and starts no threads.
   A panic in it ends the server.
7. **Later.** When the policy moves into the shell's process, these messages
   become part of the shell extension; the first release keeps it in the server.
8. **Before `kosmo-wm` exists.** Until then, the trait and its messages are in a
   module of `kosmo-server`, and a stand-in policy, such as every application
   view full screen with focus and the foreground session given to the session
   of the newest application view, is in `kosmo-server-headless`. They move into
   `kosmo-wm` with the first policy, and the server's tests then pass unchanged
   with the stand-in policy.

The first policy shows applications full screen, one region per page.

#### Trusted presentation

The trusted indicator area and the stop and switch entries are drawn by the
server itself from pre-rendered bitmaps. The first release has the rules only:
the trusted layer exists in its code but stays empty.

1. **Layer and area.** The trusted layer is above every other layer. Once a
   reserved area is configured, every other layer is clipped to leave it out.
2. **Who writes the pixels.** The trusted presentation module, a tier A module
   of the outer layer of `kosmo-server`, writes the trusted layer into the
   composed buffer last in each frame. The compositor, in tier B, composes the
   other layers and never writes the reserved area. The reserved area's pixels
   are a function of trusted state only, and a defect of the compositor cannot
   change them.
3. **Bitmaps only.** Trusted presentation only copies bitmaps of a fixed asset
   table to fixed positions: it parses no fonts or images, lays nothing out and
   depends neither on `kosmo-text` nor on `kosmo-render`. The assets are
   generated at build time and ship with the server program, with pixel formats
   and descriptions of `kosmo-types`; nothing is generated at run time. They
   live in the server's own memory.
4. **Software compositing.** Trusted content is written while composing in
   software, never through a hardware plane.
5. **Failing closed.** While the trusted state is unknown, the indicator never
   shows normal.
6. **Variable content.** Digits, short hexadecimal codes, timers and status
   symbols, such as the short code and timer of a restricted mode, use a fixed
   glyph set of the asset table: one bitmap per glyph at a fixed advance,
   without shaping. kosmo draws these glyphs itself, under its MIT license, and
   does not generate them from an OFL font. An application's display name comes
   from a name bitmap that a trusted party renders in advance and keeps with the
   application's registration, which kosmo asks krepi to provide; on a host a
   stand-in registration gives it. Fixed interface text, such as "Stop" and
   "Switch", and icons are bitmap assets rendered at build time from the two
   interface fonts; the rendered images are not font files.

#### Buffer sealing and private copies

The server seals a committed buffer by copying it:

1. **Meaning.** The content of a client buffer is taken when the server copies
   it. The server sends `Released` when the copy is done and never reads the
   buffer again. Compositing reads only the server's private copies, never
   client buffers. A client that writes its buffer before the copy tears only
   its own picture.
2. **When.** Handling a `Commit`, at the priority of requests, only marks the
   buffer committed; a newer commit of the same surface releases an earlier one
   that has not been copied, unread, and merges its damage. The copy happens at
   the output's next latch: the latch takes each surface's newest commit and
   copies its merged damage from the client buffer into the private copy in
   slices; each buffer copied is released at once, and then composing starts. So
   a private copy never changes during a frame, and of several commits within
   one frame only the newest is copied.
3. **One reading point.** Shared memory is read only in one copy function, which
   takes a buffer, a source rectangle and a target, and which the transport
   provides: the in-process channel's in safe Rust; the host transport's in
   `kosmo-server-headless`, the only unsafe code of kosmo that reads mapped
   memory, in a module of its own with a review of its own; on the device
   krepi's integration's. It reads word by word and never forms a `&[u8]` of the
   shared bytes. `kosmo-server`, `kosmo-compositor` and the three pure cores
   have `#![forbid(unsafe_code)]`. The function is `copy_rows` of the server end
   of the [client transport port](#client-transport-port). The client end of the
   host transport has its own code that creates memory objects and writes its
   own mapping, and never reads the server's memory.
4. **A failed read.** When the copy function reports that it cannot read, after
   a revocation on krepi or a mapping fault on a host, the server does not
   crash: it judges the session a violator and closes it with `BufferFault`. The
   transport must make such a failure a return value, never a signal: on a Linux
   host a buffer must carry the `F_SEAL_SHRINK` seal, and on a macOS host its
   descriptor must be a POSIX shared memory object.
5. **What krepi provides.** A client can grant a shared memory object to the
   server read-only, and while the server copies a committed buffer, a
   revocation by the client or its death causes no fatal fault in the server, at
   most a failed copy. How is krepi's decision.

Writing a buffer after the server has copied it, even before the client takes
`Released`, leaves the composed picture unchanged.

Private copies:

1. Each application view has one private copy in `kosmo-compositor`, always in
   `Rgba8Premul` and of the size of the view's `Configure`, which in the first
   release is the output's logical size. It is the source of the view's content
   in compositing, and it is allocated again when the view's size changes.
2. **Copy, then convert.** The conversion function reads a byte slice, and
   shared memory never becomes one, so conversion never reads a client buffer
   directly. For an `Rgba8Premul` client buffer, the copy function writes the
   damaged rows straight into the private copy. For the other formats, it copies
   each row into a row buffer of the outer layer, private memory of one
   `stride`, and the conversion function then writes it into the private copy.
3. **Other sizes.** A buffer is aligned with the view's top left corner, and
   what lies beyond the view is clipped. Parts of the view that the buffer does
   not cover keep the private copy's previous pixels; a new view's private copy
   is opaque black, `Rgba8Premul` 0, 0, 0, 255. This is no protocol error.
4. **Quota.** The bytes of a private copy count toward the session's quota of
   buffer bytes from `CreateAppView`, which otherwise fails with `QuotaExceeded`
   for buffer bytes, until the view is destroyed; `kosmo-surface` counts them.

#### Execution model

1. **One thread.** The server has one thread and one event loop, and every
   module, the window policy and compositing included, runs on it; it starts no
   threads.
2. **Four priorities,** the lower number first:
   - 0, control: the server's part of the stop key and the other reserved keys,
     sending stop requests and taking their reports;
   - 1, input: the other device records through the key maps and layers to the
     focus core and into the clients' event queues; the timers of repeat and
     long press; the display events of the device ports, such as completions,
     released buffers and epochs;
   - 2, requests: request frames and calls of clients, calls of the window
     policy, requests of the shell extension;
   - 3, compositing: latches, copies, slices of compositing and device commits.
3. **A round.** Each round (a) does all work of priorities 0 and 1 that has
   arrived, from the bounded queues of the device ports; (b) serves priority 2,
   rotating over the sessions; and (c) does one slice of priority 3. After each
   slice of (b) it checks the sources of priorities 0 and 1 and, when there is
   new work, does it as in (a) before continuing (b) where it stopped. After (c)
   the round ends, and the next starts with (a). New input thus waits at most
   one slice, a request at most one slice of compositing, compositing advances
   at least one slice per round, and every session gets its turn in every round.
   With no work, the server waits for any source until the nearest timer of
   repeat, long press or a frame's deadline; ports never block, and the only
   wait is the root port's `wait`.
4. **Control never queues.** When a device record hits the stop key, its
   handling runs at the step that reads it, and the request goes out at once,
   without waiting for the other records of the batch to be routed; the port to
   krepi's supervisor reserves room for these requests. Records are handled in
   the order they arrive and never reordered, which would change the order of
   key states.
5. **Never blocking.** The server never waits for a client. Calls to the
   stop-request port and the device ports do not block the loop; their reports
   arrive as events of priority 0 or 1.

Slices, rotation and the frame budget:

1. **Slices.** A slice of priority 2 is one request frame, with the window
   policy calls it causes. A slice of priority 3 is the copy after a latch or
   compositing of at most `SLICE_PX` pixels; copying and compositing have the
   same granularity, divided by damage rectangle and by row segments from top to
   bottom within it, in a fixed order, copying first. `SLICE_PX` is 16,384 in
   the first release, so a whole frame of 800 by 480 pixels takes about 24
   slices.
2. **Rotation.** Each round, priority 2 starts with the session after the one
   where the previous round started, in the order of session keys, and serves at
   most `REQ_PER_ROUND`, 8, slices of each session, in the order that the
   session sent them. Window policy calls caused by events of priority 0 or 1,
   such as the holder's disappearance, are made while handling that event, with
   the same bounded work.
3. **Flooding.** However much client A submits, client B's input is routed at
   priority 1 at the next check, at most one slice after it arrives; B's `q`
   queued request frames are processed within $\lceil q / 8 \rceil$ rounds. A's
   extra submissions wait in A's own request queue, and A gets `QueueFull` when
   it is full.
4. **Frame budget.** Each output has a budget `T` in nanoseconds, its refresh
   interval: $10^{12}$ divided by the refresh rate in millihertz from the device
   port's output description, rounded to whole nanoseconds. On the device the
   refresh rate is that of the mode that the display device service reports, and
   on QEMU the interval of its timer-generated vblank. The headless and nested
   backends take the mode of their simulated output from their start
   configuration, by default 60,000 mHz, which gives 16,666,667 ns, and tests
   inject it; the nested backend never takes the host window's refresh interval.
5. **Clock.** The server reads one monotonic clock, the same source as the
   timing of repeat and long press, in `MonoTime`. The clock is injected, and
   backend crates build the real clock. The server reads it at the start of each
   round and after each slice. Headless tests use a virtual clock that only the
   driver advances, and may inject a duration per slice, so that compositing
   becomes slow in virtual time.
6. The constants `SLICE_PX`, `REQ_PER_ROUND` and the default `T` are not based
   on measurements. Changing their values after measuring on the device does not
   change these rules.

When compositing is slow, frames are dropped, never input or the stop key:

1. **One frame at a time.** Each output has at most one frame in progress and at
   most one device commit that has not completed.
2. **Latch.** When an output has damage to compose, no frame in progress and its
   previous device commit completed, priority 3 starts a frame for it: it takes
   each surface's newest committed buffer, and the frame's damage is the union
   of the surfaces' damage since the previous frame, the changes of the view
   tree and the changes of trusted content. It records the latch time `L`, and
   the frame's deadline is `L + T`.
3. **Finish, then commit.** When the last slice of a frame is done, the frame
   goes out as one device commit of its update regions. A frame past its
   deadline is not given up: it is finished slice by slice and committed. The
   deadlines it misses are the dropped frames.
4. **No queue, only the newest.** Commits that arrive after a latch wait for the
   next latch, which takes only each surface's newest commit and releases older
   ones unread, merging their damage. When compositing is slow, fewer frames go
   out, and clients get `Released` later, so single-buffered clients slow down
   too; the server builds up no backlog.
5. **Input and stop are unaffected.** Priorities 0 and 1 come first in each
   round and are always done completely, slices are bounded, compositing never
   waits and frames never queue. So the number of records handled and of
   routings in a period depends only on the records that arrive, not on the
   speed of compositing, as long as the queues of the input device ports do not
   overflow; an overflow there is reported by the device service as `Dropped`
   and is never caused by compositing. A stop request goes out at priority 0
   whatever frame is in progress. When an injected duration per slice makes a
   frame longer than `T`, fewer frames go out in a period than deadlines pass,
   while the records handled and the routings at priorities 0 and 1 are the same
   as without it. The frame state of rules 1 to 4 is in `kosmo-compositor`, with
   times and the copy function given by the caller, so it does not depend on
   `kosmo-server`, whose loop calls the same code; a test driver can run the
   same check without the server's loop, routing each round's records through
   `kosmo-focus` and then letting compositing do one slice.
6. **Trusted content does not wait.** Once there is trusted content, the
   reserved area, which no other layer intersects, does not wait for the frame
   in progress: at the next moment without an uncompleted device commit, trusted
   presentation writes the reserved area and commits it alone, and the frame in
   progress goes out afterwards without it. The feedback of the stop key thus
   does not slow down with compositing.

Input routing and compositing may become two execution contexts once krepi
offers threads to the display server; the window thread of the nested backend
stays as it is.

#### Fault scope

The trusted core, the compositor and the window policy share the server's
process:

- A defect of the compositor can garble the picture, but cannot give input to
  the wrong client, because tiers A and B meet only through the narrow
  interface.
- A defect of the window policy can bring the server down.
- A panic of any module ends the server's process. The first release has no
  reconnection, so its clients get `Disconnected` and end.

### Device ports

The display server reaches devices, clients and krepi's supervisor only through
ports that kosmo defines. What kosmo needs from krepi behind these ports is
listed in [requirements on krepi: display and
devices](requirements/display-and-devices.md).

#### Port structure

1. **Two layers.** kosmo's side is Rust traits and plain values, whose
   signatures contain no krepi, IPC or host types. krepi defines and versions
   the interfaces of its device services and of its supervisor's control
   endpoint, and krepi's integration holds the glue between the two layers.
   kosmo's traits change only by a design change of kosmo; a new version of
   krepi's interfaces is adapted in the integration.
2. **Place.** The traits of the device ports for display and input, of the
   session transport, which is the server end of the client transport port, and
   of the stop-request port are in a port module of `kosmo-server`. The client
   end of the client transport port is in `kosmo-client`. The ports' own value
   types, such as identifiers of instances and outputs, the epoch of a device
   service, events, errors and the peer's identity, are defined in the same
   module, not in `kosmo-types`; the ports use only types that `kosmo-types`
   already has, such as `InputRecord`, `HidUsage`, `MonoTime` and `PixelFormat`,
   and change neither the [event record](#event-record) nor the [pixel
   formats](#pixel-formats-and-buffers).
3. **The root port.** The server is generic over one root trait, provisionally
   `ServerHost`. Through associated types and accessors it gives every display
   device instance, every input device instance, the session transport and the
   stop-request port, and it has two methods: `now(&self) -> MonoTime`, the
   server's monotonic clock, and
   `wait(&mut self, deadline: Option<MonoTime>) -> ServerWaitEnd`, which ends
   with `Ready`, `Deadline` or `Idle`. No method of any port blocks; `wait` is
   the only call of the server that blocks, and it returns `Ready` as soon as
   any source, a device event, an input record, a frame or connection of a
   session, or a stop report, has work. The headless backend never blocks:
   without work it returns `Deadline` or `Idle`, as the deadline has passed or
   not, and only its driver advances its clock. The headless backend, the nested
   backend, whose window events and host transport share one wait through a
   wake-up handle, and later krepi's integration implement it.
4. **Clock domain.** The time of input records, the times of device events and
   `now()` are in one monotonic clock domain. The port's implementation converts
   interrupt timestamps into it.
5. **Instances.** The display and input instances are given when the root value
   is built and do not grow in the first release; their epoch changes and
   removal are part of the ports, and hot plugging comes later as a compatible
   revision.
6. **No external memory in the server.** `kosmo-server` has
   `#![forbid(unsafe_code)]`. It reads and writes external memory, scan-out
   buffers and client buffers, only through methods of the ports, and the unsafe
   code that these methods need is in the backend crates or in krepi's
   integration.
7. **Inbound values are checked.** The server checks the handles, identifiers,
   counts and array lengths that ports hand it against the bounds of this
   section; unknown handles and values out of range are port errors and never
   cause a panic. Sequences of device events are fuzz tested.
8. **Size.** The root port has 2 methods besides its accessors, the display
   device port 6 operations and 5 events, and the input device port 4 operations
   and 2 events; the client transport and stop-request ports state their own
   sizes.

A single-threaded server cannot block on several ports, so all sources share one
wait, as the framework's backend does with its platform interface's one `wait`.

#### Display device port

The trait is provisionally `DisplayDevice`, and one value is one instance of
krepi's display device service.

1. **Instances and outputs.** An instance has its own epoch, a `DeviceEpoch`
   newtype of `u64` compared only for equality, and from 1 to `MAX_OUTPUTS`
   outputs, 4 in the first release. On the Raspberry Pi 5 one instance drives
   both HDMI outputs, and each DSI display and QEMU's virtio-gpu is an instance
   of its own.
2. **Output enumeration.** `info()` gives the instance's epoch; its
   capabilities, which state the items that the [minimal frame-buffer
   subset](#minimal-frame-buffer-subset) may lack: whether it sends `Vblank` and
   `Released`, how many scan-out buffers each output can allocate
   (`max_scanouts`) and whether it has panel power; and for each output its
   identifier, its current mode (width, height and refresh rate in millihertz),
   the formats it can scan out, a subset of the four pixel formats, and the
   transforms that its scan-out hardware can do, a subset of the eight
   symmetries of the plane that always contains the identity. v0 has only the
   current mode, without a mode list or mode switching.
3. **Scan-out buffers.**
   `alloc_scanout(output, format) -> Result<Scanout, DeviceError>` returns a
   handle and the stride of a buffer of the current mode's size, which the
   device service allocates; the server provides no pages. The server never maps
   scan-out buffers:
   `write_scanout(scanout, rect, src, src_stride) -> Result<(), DeviceError>`
   copies a rectangle of the server's private composed result, already in the
   scan-out format, into the scan-out buffer. How scan-out buffers are mapped is
   krepi's detail behind this method.
4. **Atomic commit.**
   `commit(output, scanout, damage) -> Result<CommitId, DeviceError>` takes a
   list of at most `MAX_COMMIT_RECTS` update regions, 16 in the first release
   like `MAX_DAMAGE`, and the server merges more before committing. An output
   has at most one commit that has not completed; another commit returns `Busy`,
   which is backpressure, not an error. A commit takes effect atomically per
   output: its scan-out buffer and update regions take effect together with
   `Completed`, and when it commits another scan-out buffer, the device shows
   nothing of it before `Completed`. With a single scan-out buffer,
   `write_scanout` writes the buffer that is being scanned out and may be seen
   before the commit, so the port does not prevent tearing then. Commits across
   outputs are not atomic in v0. A commit has no plane argument: each output
   scans out one plane.
5. **Panel power.** `set_power(output, PanelPower)` turns a panel on, to standby
   or off, and returns `Unsupported` when the instance has no panel power. In
   the first release only the conformance suite calls it; the brightness and
   screen-off policy comes later.
6. **Events.** `take_events(out) -> usize` never blocks and gives at most
   `out`'s length events: `Completed { commit, time }`,
   `Vblank { output, time }`, `Released { scanout }`, `HolderLost` and
   `EpochChanged { epoch }`. The enum and each variant are `#[non_exhaustive]`,
   so hot plugging, time sources and mode events can be added as compatible
   revisions. `time` is in the ports' clock domain. Whether a completion states
   its time source is krepi's decision, and v0 has no such field. The vblank
   events that the device service generates with a timer on QEMU are the same
   events as those of hardware vblank.
7. **Errors.** `DeviceError`, `#[non_exhaustive]`, has `Busy`, `Unsupported`,
   `NotHolder`, `StaleHandle` for a handle of an earlier epoch,
   `InvalidArgument`, `Exhausted` for more scan-out buffers than `max_scanouts`,
   and `Failed`.
8. **Modes and rotation.** The server holds the policy: it chooses the mode, the
   orientation and the brightness; krepi's driver carries out clocks, scan-out
   and mode switching. v0 has no mode list, no mode switching, no brightness and
   no backlight operation, and the server uses the current mode that `info()`
   gives. When the chosen orientation is in an output's transforms, it will be
   requested from the hardware through the port; otherwise the server rotates
   while it composes, and the scan-out buffer it commits is already in the
   panel's native orientation. In the first release the orientation is always
   the identity and compositing does not rotate. Runtime mode switching,
   rotation and hot plugging will add a mode list and a request for hardware
   transforms as compatible revisions. The brightness and screen-off policy
   belongs to kosmo and the backlight mechanism to krepi.

#### Minimal frame-buffer subset

krepi implements a minimal frame-buffer subset of the display device service
first: one plane, a fixed mode, update regions, completion events and a single
holder. Its profile states for each item of the display device port whether the
subset has it; an item that it may lack is reported by the instance's
capabilities, and the server chooses its fallback from them, never by waiting
for an event that does not come.

| Item | In the subset | When it is missing, the server |
| --- | --- | --- |
| Output enumeration: current mode, formats, transforms, capabilities | Always | — |
| Allocation of scan-out buffers, at least one per output | Always | — |
| `write_scanout` | Always | — |
| Commits with update regions | Always | — |
| `Completed` | Always | — |
| `HolderLost` and `EpochChanged`, which in the subset come only when the device service restarts | Always | — |
| `Vblank` | May be missing | Computes the frame deadline from the current mode's refresh rate, without waiting for a vblank, and commits the next frame once the previous commit has completed |
| `Released` | May be missing | Treats the `Completed` of a commit on an output as the release of the scan-out buffer of the previous commit there; with a single scan-out buffer, it writes again after `Completed` |
| A second scan-out buffer per output | May be missing | Uses one buffer: writes the damage into it only while no commit is pending and then commits, without protection from tearing |
| Panel power | May be missing | Never calls `set_power`; the panel stays on |
| Hardware transforms other than the identity | May be missing | Performs every orientation while composing in software; the first release always uses the identity |

- The subset has no second holder; the [holder handover](#holder-handover) is
  written for the full device service and checked with a configuration of the
  headless backend.
- One plane and a fixed mode are not items of the profile, since the v0 port has
  only the current mode and one plane per output.
- In its subset configuration, the headless backend lacks the last five items
  and has the first six.
- The epoch events are always present because after the device service restarts,
  old handles are no longer valid and the server must allocate its scan-out
  buffers again.

#### Holder handover

The display server holds the display device service, and krepi's minimal
recovery client is the only other holder that may take over scan-out; when it
does is krepi's rule.

1. **While the server is gone,** the display device service keeps the last
   frame, and the server does nothing for that: it neither clears the screen nor
   withdraws its last commit when it exits. The headless backend keeps the last
   committed frame after the server stops, for tests to read back.
2. **`HolderLost`:** a second holder has taken over scan-out, or the device
   service no longer serves this server. The instance's scan-out buffers and
   commit handles are no longer valid. A commit that has not completed counts as
   ended, and the server stops waiting for its `Completed`, stops writing
   scan-out buffers and committing, and stops composing for these outputs.
   Clients, input and the server's part of the stop key carry on: the recovery
   client takes no input, and the server still holds the input device services.
   A write or a commit while the server does not hold the instance returns
   `NotHolder`, and the server does not retry.
3. **`EpochChanged { epoch }`:** the device service hands scan-out back with a
   new epoch, or gives it to the server after restarting. Handles of the old
   epoch are no longer valid, and the port delivers no events of the old epoch
   after `EpochChanged`. The server reads `info()` again, allocates its scan-out
   buffers again, composes each output whole and commits it once with the whole
   output as its update region; it assumes nothing about what the screen shows.
   If an output's size or format has changed, it follows the new enumeration;
   the first release has a fixed mode, and the headless backend does not
   simulate such a change.
4. **Two epochs.** The epoch of a device service and the [server's
   epoch](#epoch-and-focus-generation) are separate: a handover changes neither
   the server's epoch nor the focus generation, and v0 sends clients nothing
   about it.
5. **Never both.** The server writes and commits only while it holds the
   instance; who holds it and when it is handed back are krepi's rules. kosmo
   neither implements nor calls the minimal recovery client and prepares no
   bitmaps for it; its bitmaps are prepared in the panel's native orientation.
6. The holder of the input device services does not change during a handover.
   Restarting the server and reconnecting clients come later.

#### Input device port

The trait is provisionally `InputDevice`, and one value is one instance of
krepi's input device service.

1. **One queue per instance.** Each input device service is an instance with its
   own epoch, description, queue of records and state query, and the port never
   merges instances into one queue. [Device records](#device-record) therefore
   carry no device identity; the server tells instances apart by their port
   values.
2. **Records.** `take_records(out) -> usize` never blocks and gives
   `InputRecord` values in the order they arrived, at most `out`'s length per
   call; a packet may span two calls. Their times are already converted into the
   ports' clock domain.
3. **Description.** `info()` gives the epoch; the class, `Keyboard`, or
   `Buttons` for an instance with keys only, such as a driver of buttons on
   general-purpose pins; a layout hint, which chooses the key map and tells
   apart keyboards that send the same usages; and the reserved keys registered
   in the instance's configuration, with usage and kind, which in the first
   release is only the stop key. The seat's reserved key table is the union of
   the usages that its instances register. Debouncing and keyboard lights are
   not in v0.
4. **State query.** `query_state(out) -> Result<(), DeviceError>` gives the
   usages that the instance currently reports pressed, at most `MAX_PRESSED`, 32
   in the first release.
5. **After a loss.** When the server reads `Dropped`, it drops the instance's
   records up to the next `PacketEnd`, queries the state, and synthesizes a
   release, marked `synthetic`, for each key that it believes pressed but the
   instance reports released. It never adds a press for a key that the instance
   reports pressed but the server never saw pressed, and never sends a stop
   request because of a query.
6. **Epoch changes and removal.** `take_events(out)` gives
   `EpochChanged { epoch }` and `Removed`, `#[non_exhaustive]`. After either,
   the server synthesizes releases for the keys pressed on that instance,
   empties its set of pressed keys and its unfinished packets, and drops every
   record of the old epoch; layer keys held on that instance count as released.
   Adding instances, hot plugging, is not in v0.
7. **Order across instances.** Each round the server first takes the instances
   that registered reserved keys and then the others in the order of
   construction, each up to what has arrived. Packets never interleave: an
   instance's packet reaches the key maps only after its `PacketEnd`, while
   complete packets of other instances may go first; records that hit a reserved
   key are still handled when they are read.
8. **One seat.** The first release has one seat, to which every `Keyboard` and
   `Buttons` instance belongs. When several instances press the same usage, only
   the first press is delivered, and the release is delivered with the last
   release.

#### Stop-request port

The trait is provisionally `StopPort`, in the port module of `kosmo-server`. It
carries the stop key's short press to whoever cancels the jobs of the foreground
session.

1. **Request.**
   `request(&mut self, req: StopRequest) -> Result<StopId, StopError>` never
   blocks. `StopRequest { peer: PeerId, scope: StopScope }`: `PeerId` is the
   peer identity that the session transport gave when it accepted the session's
   connection, from the authenticated endpoint on the device and simulated on a
   host, and the port's implementation finds the session's restricted stop
   capability on the device, or the stand-in's target on a host, from it.
   `StopScope` has only `FocusedJobs` in v0 and is `#[non_exhaustive]`. v0 has
   no trace number.
2. **Reports.** `take_reports(out) -> usize` never blocks and gives
   `StopReport { id, progress }`, where `StopProgress` is `Accepted` or
   `Rejected(StopRejected)` in v0 and is `#[non_exhaustive]`; the stages that
   krepi's supervisor reports will be added as a compatible revision.
3. **Room.** The port reserves room for requests. When it is full, `request`
   returns `StopError::Full`; the server counts it and reports it to the shell,
   and neither retries nor blocks.
4. **Wiring.** `kosmo-focus` gives the decision to stop the foreground session
   as a plain value, and the outer layer of `kosmo-server` takes that session's
   `PeerId` and calls `request`.
5. **On the device** the request goes to the control endpoint of krepi's
   supervisor, with the session's restricted stop capability, which can stop
   only that session. The supervisor first asks the job service to cancel
   cooperatively and, past a grace period, ends the session's isolated worker
   processes, reporting each stage. Who carries this out, how long the grace
   period is and how the server gets the capability are krepi's rules.
6. **On a host** a stand-in implements the port: it calls
   [`cancel_session`](#cancelling-a-session) of the job layer of the foreground
   session's client and reports `Accepted`. Tests of the server alone use a
   stand-in that records requests.
7. **Not the application's cancel key.** The `Cancel` key goes through the
   framework's `CancelJob` to the application's own job layer, and has nothing
   to do with the stop key.
8. Registering the foreground session with krepi is not in the v0 port; it will
   be a method with a default implementation, added as a compatible revision.
9. The port has 2 methods and 2 kinds of report.

#### Client transport port

The client transport port is provisional with protocol v0, whose frames it
carries.

1. **Two ends.** It is a pair of traits, implemented independently: the client
   end `ClientTransport` in `kosmo-client`, and the server end
   `SessionTransport` in the port module of `kosmo-server`. By the direction
   rules of the [source layout](#source-layout), no crate depends on both
   `kosmo-client` and a server crate, so each end's in-process implementation is
   tested against stand-ins, and a real client and a real server connect only
   through the [host transport](#host-transport), in two processes.
2. **Frames and capabilities.** Both ends carry byte frames without decoding
   them. A transport keeps message boundaries: one message is one frame, whose
   length bound is a constructor argument, `MAX_FRAME`; a received frame is
   copied into the caller's private buffer before it is handed over. Memory
   objects never enter frames; they are attached to calls, as the associated
   type `Memory` of both ends. The signatures thus do not depend on the types of
   `kosmo-proto`, and changes of the protocol's object model do not change the
   port.
3. **Client end.** A value is the connection of one session; connecting is
   construction, not a method of the trait.

   | Method (sketch) | What it does |
   | --- | --- |
   | `call(frame, memory: Option<Self::Memory>, deadline: MonoTime, reply) -> Result<(), TransportError>` | A synchronous call: sends one frame with at most one memory object and waits for its reply until the deadline; at most one call at a time |
   | `send(frame) -> Result<(), TransportError>` | Puts a frame into the request queue of `C_R` frames; when it is full, returns `QueueFull` and sends nothing |
   | `take_event(out) -> Result<bool, TransportError>` | Takes one event without blocking; taking it advances the read position that the server sees |
   | `wait(deadline: Option<MonoTime>) -> Result<WaitEnd, TransportError>` | Waits for an event, the deadline or a disconnection; `WaitEnd` is `Events`, `Deadline` or `Idle`, as for the platform interface's `wait` |
   | `create_memory(len) -> Result<Self::Memory, TransportError>`; `memory_mut(&m) -> Result<&mut [u8], TransportError>` | Creates a memory object that can be attached to a call, and gives a writable view of it, into which the client writes a buffer's pixels |

   Deadlines are `MonoTime` in the clock domain of the framework's `Clock`, and
   `wait`'s deadline may be `None` when the framework waits without one.
   `TransportError`, `#[non_exhaustive]`, has `QueueFull`, `Timeout`, `Closed`,
   `TooLarge` and `Exhausted`. After a call times out the connection is closed,
   and a late reply is never delivered. `kosmo-client` turns these errors, its
   own decoding failures and a `Violation` from the server into the three
   transport results of [protocol errors](#protocol-errors). The client end has
   6 methods.
4. **Server end.** No method blocks; the server waits in the root port's `wait`.

   | Method (sketch) | What it does |
   | --- | --- |
   | `accept() -> Option<Accepted>` | A new connection with its peer identity `PeerId`, from the authenticated endpoint on the device and simulated on a host |
   | `take_call(conn, out) -> Option<Call<Self::Memory>>` | Takes the session's call without a reply: its frame and its memory object |
   | `take_queued(conn, out) -> Option<usize>` | Takes the next frame of the request queue, in order |
   | `reply(conn, frame)` | Replies through the session's one reply slot |
   | `post(conn, frame)` | Puts an event into the ring of `C_E + R` frames; the server counts ordinary and reserved events by the [transport rules](#transport-rules) |
   | `events_taken(conn) -> u64` | The number of events that the client has taken, its read position |
   | `close(conn)`; `ended() -> Option<ConnId>` | Closes a session; gives a connection whose peer has gone |
   | `copy_rows(mem, src, dst, dst_stride) -> Result<(), CopyFault>`; `drop_memory(mem)` | The only place that reads shared memory: copies a rectangle, by the source's offset, stride and bytes per pixel, into the server's private bytes; out of bounds or unreadable gives `CopyFault`, and the server judges the session a violator |

   The server end has 10 methods.
5. **What both ends guarantee.** One message is one frame. Frames put into the
   request queue before a call can be taken when the server takes that call.
   When the client takes an event and then sends a request or a call,
   `events_taken` has counted that event when the server reads it. Each session
   has one reply slot, `C_R` request slots and `C_E + R` event slots, with
   capacities given as constructor arguments. The in-process implementations,
   the host transport and a later transport on krepi all guarantee the same.
6. **In-process implementations.** The client end's is in `kosmo-client`:
   deterministic, with injectable time and in safe Rust only. Its peer is a
   stand-in that the test gives; `call` gets its reply on the same thread
   through an advance function given at construction, and returns `Timeout` if
   the stand-in becomes idle without replying, never looking at the wall clock;
   `wait` never blocks, as in the headless backend. The server end's is in
   `kosmo-server-headless`, and its peer is a stand-in client that encodes with
   `kosmo-proto`.
7. The events of the shell extension take the path that [shell
   extension](#shell-extension) describes.

### Key map engine and the stop key

#### Key maps

1. **Place.** The key map engine is the input pipeline module of `kosmo-server`,
   in tier B and tested at level A0. Its judgements of time use only `now()` and
   the time of records, never a real clock.
2. **Pipeline.** An instance's records go to the reserved key table of
   `kosmo-focus`, which sends those that hit to the server's part of the [stop
   key](#stop-key); the others go to the engine, which keeps the pressed keys,
   the PC modifiers and the layer state, resolves meanings and times repeat and
   long press; it delivers `KeyEvent` values, as the [delivered key
   event](#delivered-key-event) defines, to the focus core.
3. **Data model.** A key map is constant data with an identifier and a set of
   key definitions, keyed by HID usage, the physical key. Each key is exactly
   one of:
   - a **layer key**, Shift, Alpha or Fn, which only changes the layer state;
   - a **PC modifier**, usages `0xE0` to `0xE7` of the keyboard page `0x07`,
     which changes the low 8 bits of `ModSet` and is delivered on press and
     release;
   - an **ordinary key**, with one entry for each set of layers that the map can
     produce, a subset of Shift, Alpha and Fn. An entry is one key meaning, two
     key meanings told apart by the PC's second level, which any PC Shift
     selects, or explicitly empty. The key also has a kind of timing: no repeat,
     repeating, or long press.

   Caps Lock, Num Lock and a third level such as AltGr are not in the first
   release; the United States host key map needs none of them. Combinations,
   sequences, dead keys and sticky keys are not in the first release's engine
   and are added by a design change. The source form of the data, Rust constants
   or a text table of kosmo that generates them, is chosen with the
   implementation, but it expresses exactly this model.
4. **Resolution and delivery.**
   - When an ordinary key is pressed, the engine takes its entry for the current
     set of layers, those whose state is not off, and then the meaning for the
     PC's level. A key without an entry or with an empty one is not delivered,
     since `KeyMeaning` has no "none", and neither is its release. Pressing any
     ordinary key, with or without a meaning, consumes latched layers.
   - The meaning is fixed at the press: the release, the repeats and the long
     press of one press carry it, whatever happens to layers and modifiers
     afterwards.
   - Layer keys are not delivered in v0; they show only in the meanings of later
     keys and in the high 8 bits of `ModSet`. When a client needs them, such as
     a compatibility service that wants raw key codes, they will get `NamedKey`
     entries and be delivered, with a new Keyboard version.
   - PC modifiers are delivered on press and release, synthetic releases
     included. Their meaning is one of four `NamedKey` entries, provisionally
     `ModShift`, `ModCtrl`, `ModAlt` and `ModGui`, the same for the left and the
     right key, which the usage tells apart; they neither repeat nor long-press.
     Scripts run through both the framework and the server need them: such a
     script can press Shift only with a modifier line, whose device record gives
     the next character its shifted meaning, and the test compares the server's
     meanings and modifiers with the script's.
   - When several instances press the same usage, only the first press is
     delivered, and the release with the last release.
   - Each delivered press records the focus generation. When focus changes, the
     press's repeat and long-press timing is cancelled and it produces no
     further events: its physical release does not reach the new focus. The
     focus core sends the synthetic release and `Leave`.
5. **Choosing a map.** Each input instance gets a key map by the layout hint of
   its description, from the maps in the server's configuration: start arguments
   on a host, injected in tests. The first release has only the [host key
   map](#host-key-map), which is also used for a hint that the server does not
   know. The layer state belongs to the seat and is shared by all instances.
6. **Checks.** A unit test checks each key map: every ordinary key has exactly
   one entry, possibly empty, for every set of layers that the map can produce;
   a key has one kind only; no key both repeats and long-presses; and the usages
   of reserved keys are not in the map. The last check covers only the default
   reserved keys; a reserved key given by a device description or a start
   argument that has an entry in the chosen map is caught first by the reserved
   key table, so its entry is never reached.
7. **Settings.** The server is the authority for input settings. In the first
   release the parameters and maps are given when the server is built, and never
   change at run time or persist. Changes of the layer state are not reported to
   the shell.

An entry is looked up for exactly the current set of layers, never falling back
to fewer layers, so the check of rule 6 can enumerate every cell.

#### Layer keys and layer state

1. There are three layer keys: Shift and Alpha, the calculator's shift or 2nd
   and alpha keys, and Fn. Each is off, held, latched or locked, and the current
   set of layers is the layer keys that are not off.
2. **Shift and Alpha latch and lock.** Pressed while off, one becomes latched.
   While it is latched, pressing an ordinary key resolves that key with a set of
   layers that includes it and then turns it off, and pressing it again locks
   it. Pressed while locked, it turns off. Releasing a layer key changes
   nothing.
3. **Fn is held.** Pressing Fn makes it held and releasing it turns it off;
   every ordinary key pressed meanwhile is resolved with Fn, and ordinary keys
   never clear it. Fn's press and release do not change Shift and Alpha, and a
   latched Shift or Alpha is consumed by the next ordinary key while Fn is held.
4. **Focus changes** clear latched layers, keep locked ones, and leave held ones
   as the physical keys are.
5. **An instance's epoch change or removal** releases the layer keys held on it,
   so Fn turns off; latched and locked layers stay. A restart of the server
   resets the layer state.
6. The engine has these two actions only, latch and lock for Shift and Alpha and
   hold for Fn; latch only, lock only and sticky keys are added by a design
   change.
7. The first release's host key map has no layer keys, so key maps in tests
   check the layer states.

The high 8 bits of `ModSet` carry the layer state:

1. Bits 8 and 9 are Shift, bits 10 and 11 Alpha and bits 12 and 13 Fn, each pair
   0 for off, 1 held, 2 latched and 3 locked. Bits 14 and 15 are reserved and
   always 0 in v0. Shift and Alpha take only 0, 2 and 3, and Fn only 0 and 1.
2. The low 8 bits follow the [event record](#delivered-key-event): bit `i` means
   that usage `0x07:0xE0 + i` is pressed on the seat, on any instance. The press
   of a PC modifier has its own bit set, and its release no longer has it.
3. `mods` is the seat's state when the event is delivered, after the record or
   the timer that produced it has been handled. A latched layer that a press
   consumes is therefore already 0 in that press's `mods`; a repeat or a long
   press takes the state when it is produced, and a synthetic release when it is
   synthesized.
4. Three layer keys of two bits each leave 2 bits free; more layer keys would
   widen `ModSet` by a change of the event record.

#### Repeat and long press

1. **Data.** Each ordinary key has one kind of timing: no repeat, repeating or
   long press. Layer keys, PC modifiers and reserved keys never repeat or
   long-press.
2. **Parameters,** for the seat, with provisional names:
   `RepeatParams { delay: u64, interval: u64 }` and
   `LongPressParams { threshold: u64 }`, in nanoseconds and each above 0, in
   `InputSettings { repeat: Option<RepeatParams>, long_press: LongPressParams }`,
   where a `repeat` of `None` turns repeat off. An interval rather than a rate
   keeps every planned time a whole number of nanoseconds without division or
   rounding; a settings screen can show it as a rate.
3. **Repeat.** For a repeating key pressed at $t_0$, repeat $k \ge 1$ is planned
   at $t_0 + \mathit{delay} + (k - 1) \cdot \mathit{interval}$, and its `time`
   is the planned time. Only the repeating key pressed last repeats: pressing
   another ordinary key stops the earlier key's repeats, and so do its release,
   a focus change, its instance's epoch change or removal, and a
   resynchronization after a loss that shows it released. If the server is late,
   with several planned times already past at the start of a round, it delivers
   one repeat with the last planned time that has passed, drops the others and
   counts them, so it never sends a burst of repeats after a load. Repeats are
   ordinary events and are dropped during an overflow.
4. **Long press.** A long-press key held until $t_0 + \mathit{threshold}$, that
   instant included, gives one `LongPress` with that planned time. Released
   earlier, it gives only its press and release; released later, its release
   too. A focus change cancels it. Only time decides, not the order of handling:
   when the server reads a release, a long press planned no later than the
   release's time and not yet delivered is delivered first, and likewise one
   repeat by rule 3. A release exactly at the threshold therefore gives a
   `LongPress`, and one a nanosecond earlier gives none.
5. **Timing.** The engine gives its nearest planned time, which is one of the
   deadlines of the server's `wait`, and the timing runs at the priority of
   input.
6. **Host defaults** of the host key map and the headless backend, which tests
   can override: a repeat delay of 500 ms, an interval of 33 ms, about 30
   repeats a second, and a long-press threshold of 600 ms. They are examples,
   not measurements; the device's defaults are set after measuring on the
   device.
7. **Host repeats.** The nested backend drops the repeats of the host's window
   library and uses only the server's.

#### Host key map

The host key map, provisionally `HOST_US`, is a PC keyboard with the United
States ANSI layout, on the keyboard page `0x07`. It has no layer keys, so its
set of layers is always empty.

| Host keys | Usages | Key meaning | Timing |
| --- | --- | --- | --- |
| Letters a to z | `0x04` to `0x1D` | `Char`: lower case at the first level, upper case at the second | Repeating |
| Digits 1 to 0 | `0x1E` to `0x27` | `Char`: the digit at the first level, the symbol of the same key of the United States layout at the second | Repeating |
| Space | `0x2C` | `Char(' ')` | Repeating |
| Punctuation of the main block | `0x2D` to `0x31`, `0x33` to `0x38` | `Char`, both levels as in the United States layout | Repeating |
| Keypad `/`, `*`, `-`, `+`, digits and decimal point | `0x54` to `0x57`, `0x59` to `0x63` | `Char`, whatever Num Lock says | Repeating |
| Left and right arrows | `0x50`, `0x4F` | `Named(Left)`, `Named(Right)` | Repeating |
| Backspace | `0x2A` | `Named(Delete)` | Repeating |
| Return and keypad Enter | `0x28`, `0x58` | `Named(Execute)` | No repeat |
| Escape | `0x29` | `Named(Cancel)` | No repeat |
| Left and right Ctrl, Shift, Alt and GUI | `0xE0` to `0xE7` | PC modifiers: `Named(ModCtrl)`, `Named(ModShift)`, `Named(ModAlt)`, `Named(ModGui)`, the same for left and right | No repeat |

- Other keys, such as Tab, the up and down arrows, forward delete, the function
  keys and the lock keys, have no entries and are not delivered; nothing in the
  first release uses them.
- Escape gives `Cancel`, the application's cancel key, which the framework binds
  to `CancelJob`; Escape is not a reserved key and has nothing to do with the
  stop key.
- The reserved stop key on a host is F12, usage `0x07:0x45`, by default, and a
  start argument can change it. It is not in the host key map; if it is changed
  to a key that has an entry, the entry is never reached.
- The map uses the five `NamedKey` entries of the framework's [commands and key
  bindings](#commands-and-key-bindings) and adds the four PC modifier entries,
  numbered when they are written into `kosmo-types`, only growing. The
  framework's minimal key map has no bindings for the four, so they match no
  command; names start with `Mod` to keep them apart from later entries of
  calculator layer keys. Other entries are proposed by the same rules when a key
  map needs them.
- The keys that repeat agree with the commands that repeat: `Left`, `Right`,
  `Delete` and characters repeat, `Execute` and `Cancel` do not, so behavior
  does not depend on the framework ignoring repeats.
- The nested backend maps host key codes to usages by physical position, not by
  the character that the host's layout gives. A host keyboard with another
  layout therefore types the characters of the United States layout; this is a
  limit of the first release.

#### Stop key

A short press of the reserved stop key goes through the display server and
cancels the jobs of the foreground session; its long press belongs to krepi's
input driver and reaches krepi's supervisor even when the display server does
not respond.

1. **When a short press is judged.** As soon as the server reads a `Press`
   record of the reserved stop key, which the reserved key table recognizes
   before the key maps, it judges one short press and, at that same step at
   priority 0, sends a stop request for the foreground session through the
   [stop-request port](#stop-request-port). The judgement ignores how long the
   key is held: the server's part needs no long-press duration, reads no clock
   and sets no timer.
2. **No duplicates from the server.**
   - One press gives at most one request; reserved keys neither repeat nor
     long-press.
   - A `Release` record sends nothing and withdraws nothing.
   - During a long press, and after krepi's driver has recognized one, the
     server's part does nothing. Whether the driver still sends this key's
     presses and releases afterwards is krepi's decision: a release is ignored
     as above, and a further `Press` without a release in between is a new press
     and sends a request by rule 1.
   - After a loss, a state query that shows the stop key pressed without a
     `Press` that the server read sends no request.
   - Two short presses in a row send two requests; the server's part does not
     merge them.
   - With no foreground session, no request is sent and the press is only
     counted; whether the shell then gets feedback, and through which message of
     the shell extension, is defined with the extension.
3. **The receiver merges the two paths.** When a short-press request and the
   emergency path of a long press both arrive, the receiver publishes one final
   state: on the device krepi's supervisor, which both paths reach and which
   kosmo asks to merge concurrent stops of one session, to treat an emergency
   stop as a superset of cancelling and to publish one final state for each job;
   on a host the stand-in's cancelling of a session, which is idempotent.
4. **What krepi's input driver does.** It passes the reserved key's `Press` on
   at once and never holds it back to judge a long press; otherwise rule 1 could
   not react at once.
5. **Feedback.** The first release has no trusted presentation; the server
   forwards the stop request's reports to the shell through the shell extension,
   which shows them. Later the trusted presentation draws immediate feedback at
   the same step that sends the request.

A long press therefore first sends one short-press cancel, which the receiver
merges with the emergency path.

### Host form

On a host, kosmo runs the display server with two backends, simulates the
device's display, and connects clients and servers in separate processes through
a host transport.

#### Host backends and device model

1. **One root port.** The headless backend `kosmo-server-headless` and the
   nested backend `kosmo-server-host` each implement the [root
   port](#port-structure) and the display, input, session transport and
   stop-request ports it gives. `kosmo-server`, `kosmo-compositor` and the three
   pure cores are the same code with both backends, which differ only in the
   ports, so nested runs and headless tests never diverge.
2. **The host's device model.** Both backends simulate a display device service
   with the settled shape of krepi's:
   - **Instance and outputs.** One simulated display engine is one instance.
     Each window of the nested backend is one of its outputs, up to
     `MAX_OUTPUTS`, but the first release's server uses one, because `Welcome`
     has the size of one output; one output is the default.
   - **Mode.** The start configuration gives each output's current mode, its
     width, height and refresh rate in millihertz, by default 60,000 mHz.
   - **Format.** The scan-out format is one of the four pixel formats:
     `Xrgb8888` by default, or `Rgb565` to simulate a product screen.
   - **Transforms.** The configuration gives the transforms that scan-out can
     do, by default the identity only, so every other orientation is rotated in
     software.
   - **One plane** per output.
   - **Timing.** The backend generates `Vblank` and `Completed` with a timer at
     the mode's refresh rate on its own clock, as the device service does on
     QEMU, never from the host's vertical sync.
   - **Capabilities.** The nested backend reports every capability, and a panel
     turned off shows a black window. The headless backend's capabilities,
     subset profile and handover configuration follow the [minimal frame-buffer
     subset](#minimal-frame-buffer-subset) and the [holder
     handover](#holder-handover).
   - Bandwidth and tearing-effect timing are not simulated.
3. **Host input.** The nested backend has one input instance of class
   `Keyboard`, the host's keyboard, whose description gives the host key map as
   its layout hint and registers the reserved stop key from the server's
   configuration, F12 by default. The headless backend's instances are given by
   the test when it is built.
4. **Where things are checked.** The rules of the server and the clients are
   checked on the headless backend. The nested backend's own rules are checked
   on hosted CI only through the stand-in of the [host window
   interface](#host-window-interface), with one more case when a virtual display
   is available. Runs in a host window on macOS are only local checks.

#### Server-side script runner

Scripts run through both the headless framework and the headless display server,
as [headless backend and scripts](#headless-backend-and-scripts) describe, have
a runner on each side:

1. **Split.** Only the framework's runner in `kosmo-testkit` parses the text of
   a script; the server's side never reads script text, since `display/` depends
   on no other workspace. The server's runner is the driver interface of the
   headless backend, with exactly four commands:
   - inject one device record;
   - advance the virtual clock to `t`;
   - run until idle, which is one step;
   - give one output's pixel digest, the SHA-256 of the buffer description and
     the raw bytes, as the [determinism rules](#determinism-rules) state,
     computed with an exactly pinned package and never written by hand.

   Tests of the server alone call these commands in process; tests across
   processes send them through a control connection, as [stepping two
   processes](#stepping-two-processes) describes.
2. **Key lines.** The framework's side projects key lines into device records as
   the [event record](#time-decoding-and-scripts) states, with the lines of one
   time forming a packet that ends with `PacketEnd`, and injects them one by
   one. These scripts hold only presses and releases.
3. **Time lines.** For a time line `t`, the server's side advances the headless
   backend's virtual clock to `t`, which only increases, handling the timers
   that fall due on the way, such as repeat, long press and frame deadlines, in
   time order. The client's side advances its injected clock to the same number.
   The two are separate clock domains: the same number only makes scripts easy
   to write, and release plans never use times.
4. **Checkpoint lines.** First both sides run to a common idle. Then the
   server's side takes the pixel digest of each output, and the client's side
   writes its three records. The server is idle when priorities 0 to 3 have no
   work, no timer is due at the current virtual time and no device commit is
   pending, that is, every `Completed` of the headless backend at the current
   virtual time has been delivered. For this, scripts run on both sides use a
   device model in which `Completed` comes at the same virtual time as its
   commit, as the framework's headless backend gives buffers back at once. The
   model timed by the refresh rate is for tests of the frame budget and dropped
   frames, and for nested runs.
5. Neither the server's runner nor the step mode of the headless server program
   sleeps or reads the wall clock; they use only the virtual clock.

#### Host window interface

The nested backend uses host windows only through one interface, so that it can
be checked on hosted CI without opening windows.

1. **The interface.** The trait, provisionally `HostWindow`, is internal to
   `kosmo-server-host` and not exported.

   | Method (sketch) | What it does |
   | --- | --- |
   | `open(spec) -> Result<WindowId, HostWindowError>` | Opens a window; `spec` has the width and height in device pixels, a whole scale factor and a title, which says that the window is a preview and does not show the behavior of the device |
   | `present(id, frame) -> Result<(), HostWindowError>` | Shows a frame: pixels of the device's resolution in the host format `Xrgb8888`, with rows, size, stride and update regions; scaling happens below the interface |
   | `take_events(out) -> usize` | Takes host events without blocking |
   | `close(id)` | Closes a window |

   `HostEvent`, `#[non_exhaustive]`, has four kinds:
   `Key { usage, down, host_repeat }`, where `usage` is a HID usage of the
   keyboard page `0x07`; `FocusLost { id }` and `FocusGained { id }`; and
   `CloseRequested { id }`. Pointer events are dropped below the interface. The
   interface is single-threaded; the parts across threads are inside its
   implementation.
2. **Below the interface,** an adapter of the window library is the only place
   with the library's types. It maps the library's physical key codes to HID
   usages through a data table and drops keys without a mapping; its unit tests
   build key code values directly, never library events. It scales by whole
   factors with the nearest neighbor, never interpolating, into the library's
   buffer. It sizes windows in the host's physical pixels, and the host's scale
   factor never enters kosmo's coordinates.
3. **Above the interface,** the core of the nested backend is checked on hosted
   CI through the stand-in:
   - **Normalization of host keys.** It keeps the pressed state of each usage. A
     press of a key already pressed, host repeats included, and a release of a
     key not pressed are dropped. Every remaining event becomes one device
     record followed by `PacketEnd`, with the backend's clock at normalization
     as its time, which in nested runs is the real monotonic clock.
   - **Focus lost.** It puts `Dropped` and `PacketEnd` into the input instance's
     queue and empties its pressed state. The server then queries the state,
     which is empty, and synthesizes the releases of the keys that it still
     believes pressed, so synthetic releases come only from the server.
   - The device model of the host, with its timer, commits and completions.
   - **Format.** It converts scan-out buffers to the host's `Xrgb8888` with the
     shared conversion function, the same as compositing; pixel digests are
     taken from the scan-out buffer before conversion.
4. **The stand-in** of `HostWindow` has no threads and no window library: it
   records the frames it gets, whose digests tests can take, and tests inject
   `HostEvent` values. The nested backend passes the server's conformance suite
   with it.
5. **Closing.** `CloseRequested` stops the nested server program, and its
   clients get `Disconnected`.

#### Main thread and window library

macOS requires its window system to run on the main thread, and the server's
event loop is single-threaded. The nested server program therefore gives the
main thread to the window library:

1. **Startup.** The nested server program first starts the server thread, on
   which the server's event loop runs and which is the only thread that runs
   server modules, and then hands the main thread to the window library's event
   loop, winit's `run_app`.
2. **The window thread does window work only.** It opens and closes windows,
   shows frames, turns host events into `HostEvent` values in a bounded queue
   and wakes the server's wait. It never reads or writes the server's state.
3. **Handing over.** From the server to the window thread, `present` copies a
   frame into a buffer that the window side holds, at most one waiting frame per
   window, and wakes the window library's event loop through winit's
   `EventLoopProxy::send_event`. From the window thread to the server, a
   `HostEvent` goes into the bounded queue, and the wake-up handle of the [host
   transport](#host-transport) makes the root port's `wait` return `Ready`. The
   server thus still blocks only in that `wait`, which covers the host's sockets
   and the window events at once; the real `HostWindow` is this handle on the
   server thread's side.
4. **Stopping.** On `CloseRequested` the window thread tells the server through
   the same queue. The server ends its loop and closes every session, whose
   clients get `Disconnected`, and once its thread has ended the main thread
   ends the window library's loop.
5. The window thread is part of a port's implementation, as a device service's
   separate process is on the device. It runs no server modules, so the server
   remains a single-threaded event loop.

The window library is winit with softbuffer:

1. **Dependencies.** `kosmo-server-host` has exactly two direct external
   dependencies: `winit` `=0.30.13` without default features, with the features
   `rwh_06` and `x11`, and `softbuffer` `=0.4.8` without default features, with
   the features `x11` and `x11-dlopen`. macOS needs no features.
2. **Closure.** When chosen, the closure of these two for the two target
   platforms was 74 packages, seven of whose names appear in more than one
   version; the deps step counts it again when the crate is added. The allowlist
   of `display/` holds it.
3. **Exceptions.** The allowlist of `display/` has seven exceptions for it, each
   with its reason:

   | Package | Exception | Reason |
   | --- | --- | --- |
   | objc-sys 0.3.5 | `links` | Links the Objective-C runtime that macOS has, which every binding of its window system needs, only on macOS; compiles no C and downloads nothing |
   | x11-dl 2.21.0 | Native code through pkg-config | Only reads the library directory with pkg-config and links libdl on Linux; the X11 libraries are loaded at run time with `dlopen`, so no X11 development packages are needed to build |
   | tiny-xlib 0.2.5 | Native code through pkg-config | The same, with its `dlopen` feature |
   | bitflags 1.3.2 | License | States `MIT/Apache-2.0`, read as `MIT OR Apache-2.0`; only on macOS |
   | foreign-types 0.5.0 | License | The same |
   | foreign-types-macros 0.2.4 | License | The same |
   | foreign-types-shared 0.3.1 | License | The same |

   The other ten packages of the closure with build scripts only probe the
   compiler's version or emit configuration flags; they compile no C or C++ and
   download nothing at build time.
4. **Linux.** Building needs no X11 development packages and links only libdl.
   At run time, winit and softbuffer load libX11.so.6, libXcursor.so.1,
   libX11-xcb.so.1, libXi.so.6, libxkbcommon.so.0, libxkbcommon-x11.so.0 and
   libxcb.so.1 with `dlopen`. On hosted Ubuntu runners these libraries and Xvfb
   are installed as packages.
5. **macOS.** The nested backend builds on macOS; windows there are checked
   locally.
6. **Wayland.** On Linux only X11 is enabled, and on Wayland desktops the nested
   backend runs through XWayland. Native Wayland would add the features
   `wayland` and `wayland-dlopen`, 26 more packages, two more native-code
   exceptions and three more license exceptions, and is a separate decision.
7. **Versions.** These versions and features are fixed. An upgrade, for a new
   winit release with its changed interface, a security advisory or a build that
   fails with a new toolchain, follows the allowlist rules, and one that needs
   an exception beyond this table is a design change.

Only `kosmo-server-host` in `display/` allows `winit` and `softbuffer`; no other
crate depends on them directly, and the closures of `framework/` and `desktop/`
never contain them. No crate depends on `kosmo-server-host`, a program and
backend in layer D3, so the window library enters `display/`'s lock file and
allowlist only through it. The layout step's negative case for this rule uses
these two package names.

#### Host transport

On a host, a client and a server in separate processes connect through local
sockets, and buffers travel as shared memory whose descriptors are passed over
the socket, as a capability is granted on krepi. Sealing a buffer is still the
copy that [buffer sealing](#buffer-sealing-and-private-copies) describes; this
is how the host carries it out.

| Item | Linux | macOS |
| --- | --- | --- |
| Connection | `AF_UNIX`, `SOCK_STREAM` | The same; `SOCK_SEQPACKET` is not available |
| Frame boundaries | Each transport message is a header of fixed size, with kind, length and counts, followed by one protocol frame; messages are split by length, at most `MAX_FRAME` | The same |
| Creating a memory object, `create_memory(len)` | `memfd_create` with `MFD_CLOEXEC` and `MFD_ALLOW_SEALING`, `ftruncate(len)`, then the seals `F_SEAL_SHRINK`, `F_SEAL_GROW` and `F_SEAL_SEAL` | `shm_open` with `O_CREAT`, `O_EXCL` and `O_RDWR`, mode `0600`, under a short unique name that `shm_unlink` removes at once, then `ftruncate(len)` once |
| Writing pixels, `memory_mut` | `mmap` once with `PROT_READ`, `PROT_WRITE` and `MAP_SHARED` | The same |
| Passing it with `CreateBuffer` | The descriptor travels as `SCM_RIGHTS` with that call's message, exactly one per call | The same |
| The server's checks | `F_GET_SEALS` must include `F_SEAL_SHRINK`, which only a memfd can have, so ordinary files are excluded too; `fstat`'s size is at least `stride × height`; otherwise `InvalidArgument` | The descriptor must be a POSIX shared memory object, and `fstat`'s size at least `stride × height`; otherwise `InvalidArgument`. A size check alone would let a client pass an ordinary file and shrink it, so that the server gets a signal when it reads the mapping |
| The server's mapping | `mmap` once with `PROT_READ` and `MAP_SHARED`, then the descriptor is closed; `drop_memory` unmaps | The same; the mapping stays readable after the client closes its descriptor |
| Copying, `copy_rows` | Reads the read-only mapping word by word into the server's private bytes, after checking that the rectangle and stride lie inside the object, and returns `CopyFault` otherwise; an object that passed the checks cannot shrink, so reading never gets a signal | The same, as far as observed; see below |
| The server's wait | One `poll(2)` over every descriptor of the process, the listening socket, the sessions, the wake-up handle and the control connection, with its timeout from the deadline. Sockets never block; bytes that cannot be written wait in the connection's outgoing queue until it can take them, so writing never blocks the server | The same |
| The client's wait | It has one connection: the standard library's `UnixStream` with a read timeout from the deadline | The same |
| A closed peer | Sending with `MSG_NOSIGNAL` gives `EPIPE` | The socket has `SO_NOSIGPIPE`, and sending gives `EPIPE` |

On macOS, telling a shared memory object from an ordinary file relies on `fstat`
reporting no file type for it, which is an observation on current systems rather
than a documented guarantee; and that the object's size cannot change has been
observed while the client still maps it, but not after the client unmaps it. If
a client could shrink it after unmapping it, a local client could make the
server get a signal while copying and end. The host transport is for development
hosts and is not the device's security boundary.

1. **The server end** is in `kosmo-server-headless` and uses `rustix` `=1.1.6`
   without default features, with the features `std`, `net`, `mm`, `shm`, `fs`
   and `event`; `shm` can go, since the server creates no objects. rustix gives
   safe interfaces for receiving descriptors, reading seals and `poll`; only
   mapping needs unsafe code. The server end has unsafe code in two places only,
   the read-only mapping and the reading of mapped memory in `copy_rows`, each
   in a module of its own with a review of its own. Its closure, rustix,
   linux-raw-sys, bitflags, libc and errno, needs no exceptions and lies within
   the window library's closure.
2. **The client end** is in `kosmo-client`, written directly over the system
   calls. `kosmo-client` has no external dependency, and the framework's
   backends are core libraries without unsafe code, so the client end can use
   neither rustix nor libc, nor live in `kosmo-backend-display`.
   - Connecting, reading, writing and waiting with a timeout use the standard
     library's `UnixStream`, and error codes come from
     `std::io::Error::last_os_error()`.
   - `extern "C"` declarations give the calls that the standard library lacks:
     `memfd_create`, `fcntl`, `ftruncate`, `mmap` and `sendmsg` on Linux;
     `shm_open`, `shm_unlink`, `ftruncate`, `mmap` and `sendmsg` on macOS.
   - `msghdr`, `cmsghdr` and the alignment of control messages differ between
     the two systems, so each has its own `repr(C)` layout, checked against the
     system headers by compile-time assertions of sizes and field offsets. Only
     local runs check the macOS layout.
   - These declarations, calls and the access to mappings are in one module,
     compiled only on Linux and macOS and reviewed on its own; the trait, the
     in-process implementation and the other modules of `kosmo-client` have no
     unsafe code. The allowlist of `framework/` does not grow.
   - kosmo otherwise takes such general functions from exactly pinned packages;
     this is the one exception, because export crates have no external
     dependency.
3. **A memory object is passed once.** The client keeps its descriptor and
   mapping, and the server refers to the object by its handle in the session;
   later commits are ordinary queued frames without descriptors. When a session
   ends, the server unmaps all its objects.
4. **Counts travel with frames.**
   - The header of each message from client to server carries the number of
     events that the client has taken, `events_taken`, so the server sees each
     taking when it reads a later request.
   - The header of each message from server to client carries the number of
     request frames that the server has taken, from which the client computes
     `QueueFull` with `C_R`.
   - When the server has taken requests in a round and has nothing else to send,
     it sends a message with only the counts, at most one per round. Counts are
     totals, so at most one such message waits in the outgoing queue, and a
     newer one replaces it.
   - The client does the same: when it has taken events and has nothing else to
     send, it sends a message with only the counts before it blocks in `wait`.
     Otherwise a client that only reads events would make the server believe
     that the event queue is full, and the server would never see its `Overflow`
     taken.
   - Each session has one reply slot.
   - Both directions are bounded: a connection's outgoing queue in the server
     holds at most `C_E + R` events, one reply and one count message, so a
     client that stops reading does not grow the server's memory; more than
     `C_R` unprocessed request frames from a client mean that it ignored
     `QueueFull`, a protocol violation.
5. **Deadlines.** The client end turns the deadlines of `call` and `wait` into a
   socket read timeout, and the server end turns the deadline of the root port's
   `wait` into the timeout of `poll`. A timeout is the deadline minus the
   injected clock's reading. In nested and production runs the program's entry
   injects the real monotonic clock that it builds; `kosmo-client` never builds
   one. In step mode the deadlines are virtual.
6. **Wake-up handle.** The server end gives a handle that another thread can
   call, the writing end of a local socket pair, through which the nested
   backend's window thread wakes the root port's `wait`.

#### Host programs

Placement:

- The server end of the host transport, the headless server program and its step
  mode are in `kosmo-server-headless`, in layer D3. `kosmo-server-host` uses its
  server end through a same-layer dependency listed in the layout table.
  `kosmo-server` has no host transport.
- The client end is in `kosmo-client`, in layer F0 and exported, compiled only
  on Linux and macOS and without external dependencies; its trait and its
  in-process implementation are unchanged.
- `kosmo-backend-display` remains a core library with `#![forbid(unsafe_code)]`,
  generic over the transport and without the host transport.
- The program `kosmo` builds the client end of the host transport with a
  constructor of `kosmo-client`, which `desktop/` may use directly, injects the
  real clock that it builds, and hands it to the display server backend that it
  gets through `kosmo-sdk`; how `kosmo-sdk` re-exports the backend's constructor
  is decided with it.
- `kosmo-client`, `kosmo-server-headless` and `kosmo-server-host` are boundary
  crates, registered by the changes that create them as [platform
  boundary](#platform-boundary) states, with these categories: `kosmo-client`
  for local sockets and shared memory; `kosmo-server-headless` for local
  sockets, shared memory and its program entry's arguments, environment and exit
  code; `kosmo-server-host` for the window library, threads, the real clock and
  its program entry's arguments, environment, exit code and child processes. The
  program `kosmo` adds no category: it takes the rendezvous path only as an
  argument, and its real clock is already registered.

Rendezvous and identity:

- A server listens on the path that a start argument gives. Without one, the
  nested server uses a default path in the user's runtime directory,
  `$XDG_RUNTIME_DIR` on Linux and `$TMPDIR` on macOS, and removes it when it
  exits.
- A second path, the first with `.shell` appended, is only for the shell.
- Clients get the path as an argument, never from an environment variable. Tests
  use a unique path in a temporary directory for each case.
- Peer identities, `PeerId`, are assigned in the order of acceptance. A session
  that connects through the `.shell` path is set up with the shell's identity,
  any other with an application's; this is the simulated authenticated identity
  of a host. A host does not check the credentials of the peer process.

Program entries:

- Both server programs take the path to listen on, the configuration of the
  simulated outputs and the server's epoch.
- The headless server program has only its step mode in the first release; a
  test driver starts it, and it starts no clients.
- The nested server program may take a client command after `--`: it starts that
  command with `--display <path>`, a provisional name, appended, and does not
  wait for it when the server ends.
- The calculator subcommand of `kosmo`, given `--display <path>`, connects to
  that server through the host transport, and without it runs through the
  headless framework. The path is an argument rather than an environment
  variable because `kosmo` is registered for arguments but not for the
  environment.
- The commands that run these programs will be given in the display stack's
  feature documents, which are planned.

#### Stepping two processes

Scripts run through the headless server with a client in another process give
byte-identical results only if both processes advance under the test's control:

1. **Step mode.** Started in step mode, the headless server program reads no
   wall clock, only its virtual clock, and advances only on the driver's
   commands, the four commands of the [server-side script
   runner](#server-side-script-runner), which come through a control connection
   whose path is given at start. While it waits for a command, it still reads
   the bytes that have arrived on each connection into that connection's receive
   buffer without handling them, so a client never blocks on a full socket
   buffer; what it reads is bounded by the counts of the host transport. A step
   that runs until idle works like this:
   - The command carries the number of transport messages that the client has
     sent on each session, and the server first reads until all of them have
     arrived, without relying on when the kernel delivers them.
   - It then runs rounds of the [execution model](#execution-model) until idle.
   - It replies with the numbers of request frames it took and events it sent in
     the step.
2. **The client end's step hook.** In test builds, the client end of the host
   transport can take a step hook, an inherent method of that implementation and
   not part of `ClientTransport`. With a hook, `call` sends its call, asks the
   driver through the hook to let the server take a step, and then takes the
   reply; `wait` never blocks; and deadlines are virtual.
3. **Checkpoints.** The driver alternates between letting the server take a step
   and running the client's side until idle, as the framework's single-round
   entry allows, until a round moves no frame in either direction: the server
   took no request frame and sent no event, and the client sent no message. It
   then takes the checkpoint's records. Both processes advance only under
   commands and the hook, and the server rotates over sessions in the order of
   their keys, so two runs are the same, also with two clients in one test
   process.
4. **The stop stand-in.** On a host, the stand-in of the stop-request port in
   `kosmo-server-headless` turns a `PeerId` and a scope into a cancel message of
   the transport, not a protocol frame, on that session's connection, and
   reports `Accepted` if the connection exists and `Rejected` otherwise. The
   client end hands the message to a handler that the program's entry registers,
   the program `kosmo` or a test driver, and the handler calls `cancel_session`
   on the job-layer thread. The handler, too, is an inherent method, not part of
   the trait. Nested runs and step mode use the same path.
5. **Formats.** The commands of the control connection and the kinds of
   transport messages, the count message and the cancel message, exist only on
   hosts and are not part of the protocol. Their encoding is fixed with the host
   transport and its feature document.

## Frontends of the first release

The first release has exactly two frontends: the line-mode terminal of
`kosmo` without arguments, and the framework's headless frontend, the script
runner with the headless backend. A host window appears only through the nested
display server. Frontends hold only input and output buffers; session state is
in the core libraries.

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
- The script runner of `kosmo-testkit` reads scripts of delivered key events
  and drives the headless backend, which receives only the events injected
  into it. Tests through it never sleep or read the wall clock; the backend
  has no threads, and only its driver, here the runner, advances its virtual
  clock, as [headless backend and scripts](#headless-backend-and-scripts)
  describes.

## Components

A component is a unit with its own role, contract and level.

| Level | Meaning |
| --- | --- |
| A0 | Tested: tests, with each run recorded with its environment; stated as tested, per environment |
| A1 | Contract: numbered requirements, a contract, contract tests with negative cases, and fuzz tests for untrusted input; pure state machines use bounded exhaustive enumeration instead of fuzz tests; stated as contract tested |
| A2 | Specification checked: an executable specification of the component and its properties is checked; stated as specification checked, never as verified. No component of the first release is at A2 |

- Every planned crate and `xtask/` appear in at least one row; crates with
  components of different roles (`kosmo-math-model`, `kosmo-math-kalyx`,
  `kosmo-proto`, `kosmo-server` and `kosmo-client`, the last three also for the
  privileged shell extension) appear in several.
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
| One-line editing structure | `kosmo-math-model` | Data-structure core | A1 | [One-line editing](features/math/one-line-editing.md); tests in `framework/kosmo-math-model/src/` and `framework/kosmo-math-model/tests/` |
| Exact display | `kosmo-math-model`, `kosmo-math-kalyx` | Presentation | A0 | `planned` |
| Formula conversion (linear text, range mapping, read-back) | `kosmo-math-kalyx` | Cross-project format | A1 | `planned` |
| Job layer and protocol stub (kalyx and controllable backends) | `kosmo-jobs`, `kosmo-math-kalyx` | Cross-project protocol core | A1 | `planned` |
| Scratchpad | `kosmo-scratchpad` | Composition | A0 | `planned` |
| SDK facade (re-exports only) | `kosmo-sdk` | Facade | A0 | `planned` |
| Shared types (geometry and the 26.6 fixed-point type, logical time, focus generation and epoch, event records, pixel formats, buffer descriptions and their conversion, damage regions) | `kosmo-types` | Shared types | A0 | `planned` |
| Framework runtime (control tree and passes, event loop and clock, effects and the execution port, result slot, timers, dispatch, focus checks and key bindings) | `kosmo-ui` | Framework core | A0 | `planned` |
| Layout (rows, columns and scrolling; hit testing) | `kosmo-ui` | Framework core | A0 | `planned` |
| Accessibility tree (AccessKit nodes and tree updates) and test queries | `kosmo-ui`, `kosmo-testkit` | Framework core | A0 | `planned` |
| Rendering (renderer interface, display lists, rasterization with vello_cpu, per-pixel reference implementation, wrapper of transcendental functions) | `kosmo-render` | Presentation | A0 | `planned` |
| Text (shaping and line breaking with parley, glyph outlines and coverage cache, the two bundled fonts and their asset manifest) | `kosmo-text` | Presentation | A0 | `planned` |
| Platform interface (nine traits, with AccessKit re-exported) and conformance suite | `kosmo-platform` (the suite's crate is chosen with the suite) | Platform contract | A1 | `planned` |
| Headless backend (no threads, a virtual clock that only the driver advances) | `kosmo-backend-headless` | Backend | A0, passing the platform conformance suite | `planned` |
| Test support (script runner with checkpoint records, clock control, call probes, pixel digests) | `kosmo-testkit` | Tool | A0 | `planned` |
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
| Headless server backend, with the server end of the host transport, the headless server program and its step mode (unsafe code only in the read-only mapping and in `copy_rows`) | `kosmo-server-headless` | Backend | A0, passing the session and device port suite | `planned` |
| Nested backend | `kosmo-server-host` | Backend | A0 | `planned` |
| Client library and client transport, with the client end of the host transport (hand-written system calls in one module, a platform boundary) | `kosmo-client` | Port | A1 | `planned` |
| Display server backend (through the client library and the client transport port) | `kosmo-backend-display` | Backend | A1 | `planned` |
| Window policy module | `kosmo-wm` | Policy | A0 | `planned` |
| Shell (skeleton, switch and stop entries, lifecycle presentation, status area, notifications) | `kosmo-shell` | Shell | A0 | `planned` |
| Privileged shell extension (wire definition and codec; role checks and dispatch; client interface) | `kosmo-proto`, `kosmo-server`, `kosmo-client` | Protocol extension | A1, with fuzz tests of extension message decoding | `planned` |
