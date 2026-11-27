# Platform interface

The framework reaches surfaces, input, its clock, resources and the
accessibility tree through the nine traits of `kosmo-platform`, in layer F0
of the `framework/` workspace. A backend implements them: the headless
backend `kosmo-backend-headless`, in layer F1, implements them without
threads or a display, with a clock and input that only its driver gives it;
the display server backend `kosmo-backend-display` is planned.
`kosmo-testkit`, in layer F0, holds the conformance suite of the interface,
which the headless backend passes, the script runner, which drives the
framework on the headless backend from event scripts, and the call probes of
tests of call relations. `kosmo-platform` depends only on `kosmo-types` and
on AccessKit 0.25.2, which it re-exports as `kosmo_platform::accesskit` so
that every crate uses the same version. `kosmo-platform` and the headless
backend are core libraries with `#![forbid(unsafe_code)]` that read no
files, and `kosmo-testkit` is a boundary crate for files. The design rules
that they follow are in the [platform
interface](../../architecture.md#platform-interface), [headless backend and
scripts](../../architecture.md#headless-backend-and-scripts) and [platform
boundary](../../architecture.md#platform-boundary) sections of the
architecture. This document states the contract of the interface with its
numbered requirements and bounds, describes the headless backend, event
scripts, the runner with the records that it writes and call probes, and
lists the limits and the commands that run the tests.

## Using it

A driver builds the headless backend with its capabilities and resources and
creates the main surface; the backend queues `Configure` and `FocusIn` for
it. A presented buffer is busy until a wait returns its release. This is the
example of the crate documentation of `kosmo-backend-headless`, which
`cargo test` runs:

```rust
use std::collections::BTreeMap;

use kosmo_backend_headless::HeadlessBackend;
use kosmo_platform::{
    EventSource, PixelSize, PlatformCaps, PlatformEvent, Presenter, SurfacePort,
    SurfaceRequest, SurfaceRole, WaitEnd,
};
use kosmo_types::{DamageRect, MonoTime, PixelFormat};

let size = PixelSize::new(4, 2).unwrap();
let caps = PlatformCaps::new(vec![PixelFormat::Rgba8Premul], size);
let mut backend = HeadlessBackend::new(caps, BTreeMap::new());
let request = SurfaceRequest { role: SurfaceRole::Main, size, format: PixelFormat::Rgba8Premul };
let surface = backend.create_surface(request).unwrap();

let mut events = Vec::new();
assert_eq!(backend.wait(None, &mut events), Ok(WaitEnd::Events));
assert!(matches!(events[0], PlatformEvent::Configure { .. }));
assert!(matches!(events[1], PlatformEvent::FocusIn { .. }));

backend.buffer(surface).unwrap().bytes.fill(0xFF);
backend.present(surface, &[DamageRect::new(0, 0, 4, 2).unwrap()]).unwrap();
events.clear();
backend.wait(Some(MonoTime::ZERO), &mut events).unwrap();
assert_eq!(events, [PlatformEvent::Released { surface }]);
assert_eq!(backend.wait(Some(MonoTime::ZERO), &mut events), Ok(WaitEnd::Deadline));
```

## The contract

`kosmo-platform` declares the nine traits in `framework/platform/src/ports.rs`;
the last exists only with its feature `test-hooks`, which is off by default:

| Trait | Methods |
| --- | --- |
| `Platform` | The supertraits `EventSource`, `Clock`, `SurfacePort`, `Presenter`, `Capabilities`, `ResourceProvider` and `AccessibilitySink`; `fn shutdown(&mut self) -> Result<(), PlatformError>` |
| `EventSource` | `fn wait(&mut self, deadline: Option<MonoTime>, out: &mut Vec<PlatformEvent>) -> Result<WaitEnd, PlatformError>` |
| `Clock` | `fn now(&self) -> MonoTime` |
| `SurfacePort` | `fn create_surface(&mut self, req: SurfaceRequest) -> Result<SurfaceId, PlatformError>`; `fn destroy_surface(&mut self, s: SurfaceId) -> Result<(), PlatformError>` |
| `Presenter` | `fn buffer(&mut self, s: SurfaceId) -> Result<PixelBufferMut<'_>, PlatformError>`; `fn present(&mut self, s: SurfaceId, damage: &[DamageRect]) -> Result<(), PlatformError>` |
| `Capabilities` | `fn caps(&self) -> PlatformCaps` |
| `ResourceProvider` | `fn resource(&self, id: ResourceId) -> Result<&[u8], PlatformError>` |
| `AccessibilitySink` | `fn update(&mut self, surface: SurfaceId, update: TreeUpdate) -> Result<(), PlatformError>`, with AccessKit's `TreeUpdate` |
| `PlatformTestHooks` | The supertrait `Platform`; `fn inject(&mut self, e: PlatformEvent) -> Result<(), PlatformError>`; `fn advance_to(&mut self, time: MonoTime) -> Result<(), PlatformError>`; `fn read_back(&self, s: SurfaceId) -> Result<Option<ReadBack<'_>>, PlatformError>`; `fn inject_fault(&mut self, port: FaultPort, error: PlatformError) -> Result<(), PlatformError>`; `fn a11y_tree(&self, s: SurfaceId) -> Result<Option<&AccessibilityTree>, PlatformError>` |

A backend is one value. The shell's `run` and its single-round entry
`Shell::run_round` are generic over `P: Platform`, and the conformance suite
and the script runner over `P: PlatformTestHooks`. `kosmo-testkit` turns on
the feature `test-hooks` of `kosmo-platform`, and a backend implements
`PlatformTestHooks` under a feature `test-hooks` of its own, which only test
builds turn on; no manifest of the framework core, controls or programs
turns them on, so outside test builds their code cannot inject events,
advance the clock or read buffers back; test builds that include
`kosmo-testkit`, such as the probe pass, in which the `call-probes` of
`kosmo-ui` turns on `kosmo-testkit`, build the test hooks of
`kosmo-platform` too. A driver that builds the headless backend may use its
inherent methods instead.

Every backend keeps these rules, which the crate documentation of
`kosmo-platform` states:

- Nothing blocks the caller's thread longer than `wait` with its deadline,
  no method needs a thread, and no method calls back into the caller. A
  backend that never blocks returns `WaitEnd::Idle` instead of waiting
  without a deadline.
- Events come back in the order they arose, each once. Injected events keep
  any focus stamp they carry.
- Each operation either does all it says or returns a `PlatformError` and
  changes nothing.
- The numbers of events per wait, of rectangles per presentation, of
  surfaces, of injected events waiting to be returned and of accessibility
  nodes per surface are bounded, as [bounds](#bounds) lists. Events beyond
  the first bound wait for the next wait; any other operation beyond a bound
  returns `PlatformError::Exhausted`.
- A handle of a destroyed surface is stale: every operation on it returns
  `PlatformError::StaleHandle`.
- Events, requests and capabilities are plain values that can be compared
  and recorded; they hold no host types and no callbacks.

A wait appends `PlatformEvent`s to `out` and ends with `WaitEnd::Events`
when it returned at least one, `Deadline` when it returned none and the
deadline is not later than the clock, and `Idle` when it returned none and
has nothing to wait for. The event enum is `#[non_exhaustive]`; frame events
and presentation feedback will be new kinds of it, and lifecycle,
clipboard, memory pressure and pointer events are not returned:

| Event | Meaning |
| --- | --- |
| `Key { surface, stamp, event }` | A delivered key event of `kosmo-types` for a surface, with its focus stamp |
| `FocusIn { surface, stamp }` | The surface gained focus with the stamp |
| `FocusOut { surface, stamp }` | The surface lost the focus that it gained with the stamp |
| `Configure { surface, size }` | The surface has the size in pixels |
| `Released { surface }` | The buffer that the surface presented last is given back, so `buffer` hands it out again |

A `Stamp` is the epoch of the display server and the focus generation of the
surface. `PlatformError` is `#[non_exhaustive]`, and an operation that
returns one leaves the backend as it was:

| Error | When |
| --- | --- |
| `Unsupported` | What was asked is not supported, such as a pixel format outside the capabilities |
| `StaleHandle` | The surface was destroyed or never existed |
| `InvalidArgument` | An argument breaks the operation's rules, such as a clock reading earlier than the clock or a first tree update without the whole tree |
| `Busy` | The surface's buffer is presented and not given back yet, or, for the test hooks, a fault already waits at the port |
| `Exhausted` | The operation would pass one of the bounds |
| `Disconnected` | The connection to the platform is gone |
| `NotFound` | The resource does not exist |
| `Backend` | The backend failed for a reason of its own |

The other value types:

- `SurfaceRequest { role, size, format }` asks for a surface of a
  `SurfaceRole`, so far only `Main` in a `#[non_exhaustive]` enum, a
  `PixelSize` of 1 to 8,192 pixels a side, `MAX_BUFFER_SIDE` of
  `kosmo-types`, and a pixel format of the capabilities. `SurfaceId` wraps
  a `u32`, which a backend never gives two surfaces.
- `PlatformCaps::new(formats, output)` holds the accepted pixel formats, in
  the backend's order of preference with each format once, and the output
  size; `accepts` tells whether a format is one of them.
- `PixelBufferMut { desc, bytes }` borrows the backend until the caller
  presents, so nothing writes to the buffer after presentation; `desc` has
  the surface's size and format.
- `ResourceId::new(name)` names a resource by its file name in the asset
  manifest of the crate that owns it, as a `&'static str`.
- `AccessibilityTree` is the tree that a backend keeps for a surface: `new`
  builds it from a first update that holds the tree information and every
  node, and `apply` applies a later one, whose nodes replace those with
  their identifiers, whose focus becomes the tree's and after which the
  nodes no longer reachable from the root go. An update of a subtree is
  `Unsupported`; a result that names a missing child, reaches a node twice
  or leaves a node of the update or the focus unreachable is
  `InvalidArgument`; one beyond the bound of nodes is `Exhausted`; on an
  error the tree stays as it was. Nodes are kept in an ordered map.
- With `test-hooks`, `ReadBack { desc, bytes, damage }` is what a surface
  presented last, and `FaultPort` names the port method at which
  `inject_fault` makes the next call fail: `Wait`, `CreateSurface`,
  `DestroySurface`, `Buffer`, `Present`, `Resource`, `Update` or
  `Shutdown`.

## Numbered requirements

The module `conformance` of `kosmo-testkit`, in
`framework/testkit/src/conformance.rs`, states 16 numbered requirements that
every backend keeps and checks them through the test hooks, generic over
`P: PlatformTestHooks`. `check(n, new)` checks requirement `n` on a new
backend that `new` builds from the suite's `Setup`, and when the backend
does not keep it, returns a `Failure` with the number and what the check
observed; `run(new)` checks every requirement in
order, each on a new backend, and returns a `Report`, whose `failing` lists
the numbers of the requirements that failed. `Setup` holds the capabilities,
the formats `Rgba8Premul` and `Xrgb8888` on an output of 64 by 48 pixels,
and a resource table of two resources; `TITLES` holds the title of each
requirement. A requirement about a bound checks it exactly at the bound and
one over it. Before injecting events, a requirement takes the events that
the backend queued itself, such as those of a new surface, by waiting until
a wait returns none.

The tests are in `framework/backend-headless/tests/conformance.rs`. For each
requirement, its positive test checks it on the headless backend, and its
negative test runs the whole suite on the headless backend wrapped in
`Defective`, of `tests/support/mod.rs`, with a defect against that
requirement alone, and checks that the report lists that requirement and no
other. The defect wrappers are tests of the backend because `kosmo-testkit`,
in layer F0, cannot depend on a backend of layer F1.
`every_requirement_has_a_test` and `every_requirement_has_a_defect` check
that both lists hold the requirements 1 to 16.

1. A wait returns injected events in the order they were injected, each
   once and as injected, whatever their focus stamps. Positive:
   `requirement_01_injected_events_come_back_in_order`; negative:
   `defect_01_reversed_events`, which returns the key and focus events of a
   wait in reverse order.
2. A wait that returns no event ends with `Deadline` when its deadline is
   not later than the clock, and otherwise, or without a deadline, with
   `Idle`, without blocking. Positive:
   `requirement_02_a_wait_ends_at_its_deadline_or_is_idle`; negative:
   `defect_02_idle_at_the_deadline`, whose wait that reaches its deadline is
   idle.
3. A wait returns at most `MAX_EVENTS_PER_WAIT` events; later events stay
   queued in order for the next wait. Positive:
   `requirement_03_a_wait_returns_at_most_the_bound_of_events`; negative:
   `defect_03_one_event_over_the_bound`, whose wait after one that returned
   the bound returns one event more.
4. The clock reads 0 on a new backend and then the time that the driver
   advanced it to; waits and other port calls do not change it, and
   advancing to an earlier time returns `InvalidArgument` and changes
   nothing. Positive: `requirement_04_the_clock_reads_what_the_driver_advanced`;
   negative: `defect_04_waits_advance_the_clock`, whose waits advance the
   clock by 1 ns.
5. The identities of surfaces are distinct, and the identity of a destroyed
   surface is stale: `destroy_surface`, `buffer`, `present` and `update` on
   it return `StaleHandle` and change nothing. Positive:
   `requirement_05_destroyed_surfaces_are_stale`; negative:
   `defect_05_presents_on_a_destroyed_surface`.
6. At most `MAX_SURFACES` surfaces exist at once; creating one more returns
   `Exhausted`, and the others keep working. Positive:
   `requirement_06_at_most_the_bound_of_surfaces`; negative:
   `defect_06_a_surface_over_the_bound`, which returns a made-up identity
   for a surface over the bound.
7. Two surfaces keep their presentations and read-backs, their
   accessibility trees and their events apart. Positive:
   `requirement_07_two_surfaces_stay_apart`; negative:
   `defect_07_one_read_back_for_all_surfaces`, whose every read-back
   returns the surface that presented last.
8. A buffer is described with its surface's size and format, and after a
   presentation `read_back` returns that description and the presented
   bytes, byte for byte, also after a later presentation. Positive:
   `requirement_08_a_buffer_reads_back_as_presented`; negative:
   `defect_08_reads_back_the_previous_presentation`.
9. After a presentation, `buffer` returns `Busy` until a wait returns the
   surface's `Released`; then it hands out the buffer again. Positive:
   `requirement_09_a_presented_buffer_is_busy_until_released`; negative:
   `defect_09_hands_out_a_busy_buffer`, which hands out a busy buffer as a
   separate copy.
10. A presentation has at most `MAX_RECTS_PER_PRESENT` rectangles; one with
    more returns `Exhausted`, presents nothing and leaves the read-back as
    it was. Positive: `requirement_10_at_most_the_bound_of_rectangles`;
    negative: `defect_10_accepts_a_rectangle_over_the_bound`, which
    presents the first rectangles of a presentation with more.
11. Every reading of `caps` is the same and holds the formats and the output
    size that the backend was built with; a surface in another format is
    `Unsupported` and is not created. Positive:
    `requirement_11_other_formats_are_unsupported`; negative:
    `defect_11_accepts_an_unsupported_format`, which creates such a surface
    in the preferred format.
12. `resource` returns the bytes of the backend's table; an identity outside
    the table and a missing resource injected with `inject_fault` return
    `NotFound`, and later calls work as before. Positive:
    `requirement_12_resources_come_from_the_table`; negative:
    `defect_12_a_missing_resource_is_empty`, which returns empty bytes for
    a missing resource.
13. Tree updates apply in order to the tree that the backend keeps for a
    surface, which `a11y_tree` returns; a first update without the whole
    tree returns `InvalidArgument` and creates no tree. Positive:
    `requirement_13_tree_updates_apply_in_order`; negative:
    `defect_13_keeps_only_the_last_update`, whose tree keeps only the nodes
    of the last update.
14. The tree of a surface has at most `MAX_A11Y_NODES` nodes; an update that
    would pass the bound returns `Exhausted` and leaves the tree as it was.
    Positive: `requirement_14_at_most_the_bound_of_nodes`; negative:
    `defect_14_changes_the_tree_over_the_bound`, whose update over the bound
    still renames the root.
15. At most `MAX_INJECTED` injected events wait to be returned; one more
    returns `Exhausted`, the waits return exactly the injected ones, and
    once they are returned, injecting works again. Positive:
    `requirement_15_at_most_the_bound_of_injected_events`; negative:
    `defect_15_queues_injected_events_over_the_bound`.
16. A disconnection injected with `inject_fault` ends the next wait with
    `Disconnected`, and `shutdown` then returns `Ok` without panicking.
    Positive: `requirement_16_an_injected_disconnection_ends_the_wait`;
    negative: `defect_16_drops_an_injected_disconnection`.

The display server backend, when it exists, is to pass the same suite.

## Bounds

The bounds are constants of `kosmo-platform`:

| Constant | Value | What it bounds |
| --- | --- | --- |
| `MAX_EVENTS_PER_WAIT` | 64 | The events that one wait returns; later events stay queued, in order, for the next wait |
| `MAX_RECTS_PER_PRESENT` | 16, `MAX_DAMAGE_RECTS` of `kosmo-types` | The update rectangles of one presentation; more return `Exhausted` |
| `MAX_SURFACES` | 4 | The surfaces that exist at once; one more returns `Exhausted` |
| `MAX_INJECTED` | 1,024 | The injected events that wait to be returned; one more returns `Exhausted` |
| `MAX_A11Y_NODES` | 4,096 | The nodes of the accessibility tree of one surface, one for each control of the control tree, whose bound `MAX_WIDGETS` of `kosmo-ui` is the same; an update beyond it returns `Exhausted` and leaves the tree as it was |

A surface's buffer also keeps to the buffer limits of `kosmo-types`: the
headless backend refuses a surface whose buffer would hold more than
`MAX_BUFFER_BYTES` with `InvalidArgument`.

## The headless backend

`HeadlessBackend::new(caps, resources)` builds a headless backend with its
capabilities and its resource table, with its clock at 0, no surfaces and no
events. It has no threads, reads no files and reads no real clock:

- Its clock moves only when the driver advances it, through the test hooks'
  `advance_to` or the inherent `HeadlessBackend::advance_to`; a time
  earlier than the clock is `InvalidArgument`, and waiting never advances
  it.
- `wait` never blocks: it returns the queued events, at most 64, otherwise
  `Deadline` when the deadline is not later than the clock, otherwise
  `Idle`. Events come only from the driver, through the test hooks'
  `inject` or the inherent `HeadlessBackend::inject`, which accept any
  stamp, and from the backend itself. So a driver of a build without
  `test-hooks` injects events and advances the clock as the hooks do.
- It numbers its surfaces from 1 in the order it creates them and never
  gives a number twice. Once a surface of the role `Main` exists, it queues
  `Configure` with the surface's size and `FocusIn` with the epoch
  `HEADLESS_EPOCH`, 1, and a focus generation that counts from 1, so a
  script of key presses needs no focus line. Each main surface gets its own
  `FocusIn`; the backend does not move focus between them. Destroying a
  surface drops the events that the backend queued for it; injected events
  stay.
- Each surface has one buffer. `present` keeps a copy of the buffer's bytes
  and the update rectangles as what the surface presented, which
  `read_back` returns, and queues `Released` behind the events already
  queued. The buffer is given back when a wait returns that `Released`,
  which is the next wait unless at least `MAX_EVENTS_PER_WAIT` events are
  queued before it; until then `buffer` and `present` return `Busy`. A
  presentation's rectangles lie inside the buffer, or `present` returns
  `InvalidArgument`.
- It keeps the accessibility tree of each surface as an
  `AccessibilityTree`, built from the updates in order, which `a11y_tree`
  returns, and `resource` returns the bytes of the table given at
  construction.
- After `shutdown`, every other port method that can fail returns
  `Disconnected`, except `read_back` and `a11y_tree`, which find no surface
  and return `StaleHandle`; `shutdown` itself returns `Ok` again.
- With `test-hooks`, a fault injected at a port makes that port's next call
  return the error once, and a second fault at a port that has one waiting
  returns `Busy`. With the feature, the backend is not `Sync`: it keeps the
  fault of `resource`, which takes `&self`, in a `Cell`.

Its tests besides the suite are in `framework/backend-headless/tests/`;
those of `headless.rs` use the inherent methods only, so they run with and
without `test-hooks`, and those of `hooks.rs` need the hooks. The tests of
`AccessibilityTree` are in the module `a11y::tests` of `framework/platform`:

1. A new main surface is configured and focused, and the next wait is idle.
   Test: `tests/headless.rs`: `a_main_surface_is_configured_and_focused`.
2. The inherent methods inject and advance the clock as the test hooks do,
   in the sequences of requirements 1 and 4. Tests: `tests/headless.rs`:
   `inherent_methods_inject_like_the_test_hooks`,
   `inherent_methods_advance_the_clock_like_the_test_hooks`.
3. Destroying a surface drops only the events that the backend queued for
   it, a release comes back in the order it arose, and a presentation keeps
   its rectangles inside the buffer. Tests: `tests/headless.rs`:
   `destroying_a_surface_drops_only_the_events_that_the_backend_queued_for_it`,
   `a_release_comes_back_in_the_order_it_arose`,
   `a_presentation_keeps_its_rectangles_inside_the_buffer`.
4. A surface beyond the buffer limit is an invalid argument and queues
   nothing. Test: `tests/headless.rs`:
   `a_surface_beyond_the_buffer_limit_is_an_invalid_argument`.
5. After `shutdown` every port is disconnected, and the hooks find no
   surface and take no fault. Tests: `tests/headless.rs`:
   `after_shutdown_every_port_is_disconnected`; `tests/hooks.rs`:
   `after_shutdown_the_hooks_find_no_surface_and_take_no_fault`.
6. A second fault at a port is `Busy`, and the first takes effect once.
   Test: `tests/hooks.rs`:
   `a_second_fault_at_a_port_is_busy_and_the_first_takes_effect_once`.
7. A first update needs the tree information and every node, updates
   replace nodes and drop what is no longer reachable, the focus is that of
   the last update, a rejected update leaves the tree as it was, and a tree
   holds at most the bound of nodes. Tests: `a11y::tests::`
   `a_first_update_needs_the_tree_information_and_every_node`,
   `updates_replace_nodes_and_drop_what_is_no_longer_reachable`,
   `the_focus_is_that_of_the_last_update`,
   `a_rejected_update_leaves_the_tree_as_it_was`,
   `a_tree_holds_at_most_the_bound_of_nodes`.

The shell's tests on the headless backend, in `tests/shell.rs`, are among
the [requirements](runtime.md#requirements) of the framework runtime.

## Event scripts

An event script is the input of a run of the framework on the headless
backend: UTF-8 text with one record per line and a line feed after every
line, the last one included. The first line is the header, and each later line
is a key line, a time line, a focus line or a checkpoint line. This script
creates a 64 by 8 surface, types `1`, moves focus away and back, presses Right
with the stamp of the earlier focus, and writes a record:

```text
kosmo-script 1 surface 64x8 xrgb8888
at 1000000 press 7:1e char:1 mods=0000
at 1000000 release 7:1e char:1 mods=0000
time 6000000
focus out 1 stamp=1/1
focus in 1 stamp=1/2
at 7000000 press 7:4f named:right mods=0000 stamp=1/1
checkpoint
```

| Line | Form | What the runner does |
| --- | --- | --- |
| Header | `kosmo-script 1 surface <width>x<height> <format>` | Creates the main surface, of the role `Main`, with the size and pixel format; only the first line, and every script has one |
| Key line | `at <time> <action> <page>:<id> <meaning> mods=<bits> [synthetic] [stamp=<epoch>/<generation>]` | Moves the clock to the time and injects the delivered key event `Key` for the main surface |
| Time line | `time <time>` | Moves the clock to the time |
| Focus line | `focus in <surface> stamp=<epoch>/<generation>` or `focus out <surface> stamp=<epoch>/<generation>` | Injects `FocusIn` or `FocusOut` for the surface with the stamp |
| Checkpoint line | `checkpoint` | Runs the framework until it is idle and writes a record, as [the next section](#the-script-runner-and-its-records) describes |

The fields:

- the format version is `1`; a header with another version fails as such,
  whatever follows it;
- the width and height of the main surface are 1 to 8,192 pixels,
  `MAX_BUFFER_SIDE` of `kosmo-types`, and must fit the output size of the
  backend's capabilities; the format is `rgba8-premul`, `xrgb8888`, `rgb565`
  or `argb8888-premul`, and must be one of the capabilities' formats;
- a time is a `MonoTime` in nanoseconds, on the virtual clock of the run;
- the action of a key is `press`, `release`, `repeat` or `long-press`;
- the HID usage is the usage page and the usage on it, each 1 to 4 lowercase
  hexadecimal digits, as `7:4` for the key A;
- the meaning is `char:<c>` with exactly one character, `char:U+<code>` with
  a code point in 4 to 6 uppercase hexadecimal digits, for characters such as
  a space that a field cannot hold, or `named:<name>` with a named key of
  `kosmo-types`: `left`, `right`, `delete`, `execute`, `cancel`,
  `mod-shift`, `mod-ctrl`, `mod-alt` or `mod-gui`. The two forms of a
  character mean the same: `char:U+<code>` may spell any character, so
  `char:A`, `char:U+0041` and `char:U+00041` give the same meaning;
- the modifiers are the 16 bits of `ModSet` in 4 lowercase hexadecimal
  digits: the low byte is the HID modifier byte and the high byte the layer
  state;
- `synthetic` marks a release that the display server made up;
- the stamp is the epoch and the focus generation, the outer focus stamp of
  the event; on a key line it is optional;
- a surface is the identity of a surface that the script created. The
  headless backend numbers its surfaces from 1, so on a new headless backend
  the main surface is surface 1.

The rules:

- Fields are separated by one space. No line is empty, starts or ends with a
  space, or holds a control character, tabs and carriage returns included.
  Optional fields come in the order of the table, `synthetic` before the
  stamp.
- Times and the other decimal numbers have no sign and no leading zero and
  fit their types: `u64` for times, epochs and focus generations and `u32`
  for surfaces.
- The time of a key line or a time line is never earlier than the time of
  the key line or time line before it, and never earlier than the backend's
  clock when the run starts. Lines of one time are injected one after the
  other, without a round between them.
- A key line without a stamp carries the stamp of the latest focus line that
  entered the main surface, or, before one, the stamp of the surface's focus
  that the framework recorded: the focus that the headless backend gives a
  new main surface, with the epoch `HEADLESS_EPOCH`, 1, and the focus
  generation 1 on a new backend. So the example leaves that focus with
  `stamp=1/1`, enters again with `stamp=1/2`, and its last key, with
  `stamp=1/1`, is dropped.
- Focus lines and the stamps of key lines, together with repeat and long
  press, are only for scripts run through the framework alone. A test that a
  key of an earlier focus generation never reaches the new view leaves
  focus, enters again with a new generation and then delivers a key with the
  old stamp, as the example does. A script run through both the headless
  framework and the headless display server holds only the header, key
  lines of presses and releases without stamps, time lines and checkpoint
  lines, as the
  [server-side script runner](../../architecture.md#server-side-script-runner)
  describes; the runner of `kosmo-testkit` reads every line and does not
  check this.
- Scripts hold input only. Platform failures are injected through the
  `inject_fault` hook of `PlatformTestHooks`, never written in scripts. The
  order in which jobs complete is not written either: the test gives the
  controllable backend of the job layer a release plan, keyed only on what
  the client sees, the submission order of jobs, the gates of their plans
  and the conditions that release them, such as another job reaching its
  terminal state or the n-th key event of the event handling record being
  handled, never on virtual time.

A script has at most `MAX_LINES` lines, 65,536, of at most `MAX_LINE_BYTES`
bytes, 256, without the line feed. `Script::parse` reads a script from its
bytes, and `Script::read` reads a script file, at most `MAX_SCRIPT_BYTES`,
16,842,752 bytes, every line at its longest with its line feed; a larger file
is not read. Neither panics: they return the first problem as a
`ScriptError`, with the number of its line, counted from 1, or 0 for a file
that cannot be read or is too large. `kosmo-testkit` is a boundary crate for
files: it reads script files with `std::fs` and converts the errors of
reading where it reads them.

| Problem | When |
| --- | --- |
| `Unreadable` | The file cannot be read; line 0, with the path and the reason |
| `TooLarge` | The file has more than `MAX_SCRIPT_BYTES` bytes; line 0 |
| `TooManyLines` | A line after the first 65,536 |
| `TooLong` | A line of more than 256 bytes |
| `NoLineFeed` | The last line has no line feed |
| `NotUtf8` | A line that is not UTF-8 |
| `Control` | A line with a control character |
| `EmptyField` | An empty line, a space at the start or the end, or two spaces in a row |
| `NoHeader` | The first line is not a header, or the script is empty |
| `SecondHeader` | A header after the first line |
| `Version` | A header with another version than 1 |
| `Unknown` | A line whose first word starts no kind of line |
| `Missing` | A line that ends before one of its fields |
| `Extra` | A field after the last one |
| `Invalid` | A field with a value that it cannot have |
| `Earlier` | A time earlier than the time before it |

The tests of scripts are in `framework/testkit`, in the module
`script::tests` and in `tests/script_files.rs`:

1. Every kind of line is read with its fields, at the largest values too,
   both forms of a character give the same meaning, each named key and
   format has its name, and the names cover the nine named keys of
   `kosmo-types` once each. Tests:
   `every_kind_of_line_is_read_with_its_fields`,
   `every_named_key_and_format_has_its_name`,
   `the_names_cover_the_named_keys_of_kosmo_types`.
2. Unknown lines, missing fields and extra fields are errors with their line
   numbers, and so are fields with invalid values. Tests:
   `unknown_lines_are_errors_with_their_line_numbers`,
   `missing_fields_are_errors`, `extra_fields_are_errors`,
   `fields_with_invalid_values_are_errors`.
3. A key line or time line earlier than the line before it is an error, and
   the same time again is not. Test:
   `a_time_earlier_than_the_line_before_is_an_error`.
4. The header comes first, only once and with version 1. Test:
   `the_header_comes_first_with_version_1`.
5. Lines are UTF-8, end with a line feed, have single spaces and hold no
   control characters. Test:
   `lines_are_utf8_with_line_feeds_single_spaces_and_no_control_characters`.
6. A script of 65,536 lines and a line of 256 bytes are read, one line or
   one byte more is an error. Tests: `a_script_has_at_most_max_lines_lines`,
   `a_line_has_at_most_max_line_bytes_bytes`.
7. 2,000 random scripts from a fixed seed give a script or an error and never
   panic. Test: `random_scripts_give_results_or_errors_and_never_panic`.
8. A script file is read, and an error of reading has line 0. Test:
   `a_script_file_is_read_or_its_error_says_why_without_a_line`.
9. A file of `MAX_SCRIPT_BYTES` and one more bytes is too large from its
   metadata alone, even when it cannot be read, and a named pipe through
   which that many bytes come is too large once they have been read. Tests:
   `tests/script_files.rs`:
   `a_script_file_larger_than_the_bound_is_not_read`,
   `a_file_with_more_bytes_than_its_metadata_states_is_refused_once_read`.

## The script runner and its records

`Script::run` replays a script on a platform with its test hooks, any
`P: PlatformTestHooks`, through a driver, `D: Driver<P>`, and returns a
`Record` with one section for each checkpoint. The runner reaches the
platform only through the platform interface and its test hooks, and the
framework only through the driver, which the test implements: `kosmo-testkit`
depends neither on the framework core nor on `kosmo-render`, so that crates
with call probes can depend on it.

| Method of `Driver` | What it does |
| --- | --- |
| `round` | Runs one round of the framework through the shell's single-round entry, `Shell::run_round`, with the shell's deadline, does what the test arranges between rounds, such as releasing a job, and returns how the round's wait ended |
| `pending` | Whether receipts of the last round's effects, or job reports that the release plan let through and the framework has not taken, wait for the next round |
| `next_deadline` | The deadline that the next round waits with |
| `focus_stamp` | The stamp of the latest focus in of a surface that the framework took, as `Runtime::focus` of `kosmo-ui` gives it |
| `view` | The lines of the view model record, as the application defines them, without line feeds, tabs or other control characters |
| `display` | The text of the display list that a surface presented last |

A run:

1. The header's main surface must fit the output size and be in one of the
   formats of the platform's capabilities; the runner creates it and runs the
   framework until it is idle.
2. Before the clock moves to a later time, the framework runs until it is
   idle. The clock then stops at each deadline of the driver that comes
   before the time, such as the polling deadline while a job runs, and the
   framework runs until it is idle there. Key lines and time lines move the
   clock this way; then a key line injects its event.
3. A focus line injects its event at the current time.
4. A checkpoint line runs the framework until it is idle and writes the next
   section.

The framework is idle when a round that starts with nothing pending and no
deadline due finds no platform event, its wait returning `Idle`, and leaves
nothing pending or due. The runner advances the clock only through
`advance_to` of the test hooks and reads no other clock, and the headless
backend never advances its clock itself. A line
that does not let the framework become idle within `MAX_ROUNDS_PER_LINE`
rounds, 65,536, fails. The shell's rounds do not run the drawing pass yet, so
a driver draws the frames itself: it lays its controls out, builds the
display list with the drawing pass `paint` of `kosmo-ui` and presents it with
`Shell::present`.

A section has the three records of a checkpoint in this order:

```text
checkpoint 1 at 0
view 4
region main focus no
line ""
cursor 0
job none
display 1 5
push-clip 0,0 4096x512 offset 0,0
fill-rect #101010ff 0,0 4096x512
fill-rect #606060ff 3712,64 320x384
fill-rect #ffffffff 0,64 64x384
pop-clip
pixels 1 64x8 stride=256 xrgb8888 sha256=7e46ac6e89095a9a1ecfe004520e86220f60922a9155ce86ef8efa7fe4f7ad75
```

- `checkpoint <n> at <time>`: the number of the checkpoint, from 1, and the
  time of the virtual clock.
- `view <count>` and that many lines: the view model record that the driver
  writes; what its lines say is the application's. The lines of this example
  are those of the line editor of the tests.
- For each surface that the script created, `display <surface> <count>` and
  that many lines: the text of the display list that it presented last, one
  command per line, as [the text form of display
  lists](rendering.md#the-text-form-of-display-lists) states, with
  coordinates as integers and colors in hexadecimal; no lines before the
  surface presented a list.
- For each surface,
  `pixels <surface> <width>x<height> stride=<stride> <format> sha256=<digest>`:
  the description of the buffer that it presented
  last, which `read_back` of the test hooks returns, and the SHA-256 of the
  16 bytes of the description, its width, height, stride and format code
  each as a little-endian `u32`, followed by the raw bytes, in 64 lowercase
  hexadecimal digits. This is the encoding of the [pixel
  goldens](rendering.md#pixel-goldens), so the digest of a frame equals the
  digest that `MANIFEST` lists for the same frame. `PixelDigest` of
  `kosmo-testkit` computes it with the `sha2` package, pinned exactly at
  0.11.1. A surface that never presented a buffer has `pixels <surface>
  none`.

Surfaces come in the order of their identities, every record of the runner
is kept in ordered containers, and nothing in a section depends on the wall
clock or on the order of a hash: the only times are those of the virtual
clock. So two runs of one script, each with a new backend, application and
driver, write byte-identical sections. The repository keeps no expected
records; the tests compare two runs and check parts of the records.

A run stops at the first problem, as a `ScriptError` with the number of the
line that ran, 1 for the header:

| Problem | When |
| --- | --- |
| `OutsideCaps` | The header's main surface does not fit the output size or has a format outside the capabilities |
| `Platform` | A port or hook of the platform returned an error, such as one that `inject_fault` injected |
| `BeforeClock` | The backend's clock is later than the time of a key line or time line |
| `Driver` | A round of the driver failed |
| `NoSurface` | A focus line names a surface that the script did not create |
| `NoStamp` | A key line without a stamp, with no focus line and no recorded focus to take one from |
| `NotIdle` | The framework did not become idle within 65,536 rounds of the line |
| `View` | A view line holds a control character |
| `Display` | Display text that is not lines, each followed by a line feed |
| `Format` | A read-back in a pixel format without a code of pixel digests; every format of this version has one |

With the feature `call-probes` of `kosmo-testkit`, which only the command
line turns on, each run records the call probe `kosmo-testkit::run`, as
[call probes](#call-probes) describes, and [running the
tests](#running-the-tests) gives the commands that run the tests below.

The tests of the runner are in `framework/backend-headless/tests/` with the
line editor of `tests/editor/`, whose jobs run on the controllable backend
of the job layer, and the frozen script `tests/scripts/editing-and-a-job.script`,
and the tests of digests in the module `pixels::tests` of
`framework/testkit`:

1. Two runs of the frozen script, which holds every kind of line and a job
   that the release plan lets finish after the 15th key event, each with a
   new backend, editor and driver, write the same bytes at all four
   checkpoints. Test: `tests/frozen.rs`:
   `two_runs_of_the_frozen_script_write_byte_identical_records`.
2. Sections keep their order, and the rounds read only the script's times
   and the deadlines on the way. Test: `tests/frozen.rs`:
   `records_follow_their_fixed_order_and_hold_only_virtual_times`.
3. After leaving focus and entering it with a new generation, a key with
   the old stamp is not delivered, the event handling record shows it
   dropped, and a key without a stamp takes the new one. Test:
   `tests/runner.rs`:
   `a_key_with_the_stamp_of_an_earlier_focus_is_not_delivered`.
4. Lines of one time run no round between them, so a key at the time of two
   focus lines takes the stamp of the one that entered the surface. The
   framework is idle only after a round that starts quiet, waits `Idle` and
   leaves nothing pending or due: 65 keys of one time, more than one wait
   returns, are all handled before the checkpoint, rounds with a due
   deadline go on, a frame presented in a round that took a job's report
   has been released before the checkpoint, and what a quiet round leaves
   pending or due is taken before it. The bound of 65,536 rounds holds for
   each line, and a line that is never idle stops after exactly that many.
   Tests:
   `tests/runner.rs`:
   `lines_of_one_time_run_no_round_between_them`,
   `a_key_at_the_time_of_a_focus_line_takes_its_stamp`,
   `more_keys_than_one_wait_returns_are_all_handled_before_the_checkpoint`,
   `a_due_deadline_keeps_the_framework_running`,
   `a_checkpoint_comes_after_the_release_of_the_last_frame`,
   `what_a_quiet_round_leaves_pending_is_taken_before_the_checkpoint`,
   `what_a_quiet_round_leaves_due_is_taken_before_the_checkpoint`,
   `the_bound_of_rounds_holds_for_each_line`,
   `a_line_runs_at_most_max_rounds_per_line_rounds`.
5. A header outside the capabilities, a focus line for another surface, an
   injected failure, a later clock, a framework that is never idle, a
   failing round, a key without a stamp, and view lines or display text out
   of form stop the run at their lines, and a surface that never presented
   has no display lines and `none`. Tests: `tests/runner.rs`:
   `a_header_outside_the_capabilities_stops_the_run_at_line_1`,
   `a_focus_line_for_a_surface_that_the_script_did_not_create_stops_the_run`,
   `platform_failures_come_from_the_test_hooks_and_stop_the_run`,
   `a_framework_that_never_becomes_idle_stops_the_run_after_the_bound`,
   `a_failing_round_and_a_key_without_a_stamp_stop_the_run`,
   `view_lines_and_display_text_keep_their_form`.
6. The digest of a golden's frame is the digest that its `MANIFEST` lists,
   and the description is part of the digest. Tests:
   `the_digest_of_a_golden_frame_is_its_manifest_digest`,
   `the_description_is_part_of_the_digest`,
   `every_format_has_a_code_and_a_name`.
7. A run of the frozen script reaches the runner's entry and the
   single-round entry, and without the feature `call-probes` a run records
   no probe. Tests: `tests/call_relations.rs`:
   `a_script_run_reaches_the_runner_and_the_single_round_entry`;
   `tests/runner.rs`: `without_call_probes_a_run_records_no_probe`.

## Call probes

With the feature `call-probes` of `kosmo-testkit`, each run of the script
runner records the call probe `kosmo-testkit::run`, and with the feature
`call-probes` of `kosmo-ui`, each call of the single-round entry
`Shell::run_round` records `kosmo-ui::run_round`. Only the command line
turns these features on, as `--features kosmo-ui/call-probes`, directly or
through the feature `call-probes` of another crate, such as that of the
headless backend; no dependency declaration turns them on, dev-dependencies
included, so ordinary builds and tests compile no probe code. A probe is the
statement
`kosmo_testkit::probe!("<crate>::<entry>")` under
`#[cfg(feature = "call-probes")]` at the start of an entry. It adds the
name to an ordered record of the whole process, and
`kosmo_testkit::probes::check`, given the entries of a test, returns those
that no probe recorded as `Missing`. A crate of layer F0 reaches
`kosmo-testkit` through an optional dependency in its own layer and a crate
of layers F1 to F3 through one on layer F0; `kosmo-platform` and the facade
have no probes, and the entries of pure-core crates count through probes in
the crates that call them. [Call probes](../../../CONTRIBUTING.md#call-probes)
states these rules in full, with the checks of the layout step that keep
probes out of ordinary builds and the probe pass of `cargo xtask check`
with its time in CI.

The test of call relations owns its test binary, `tests/call_relations.rs`
of the headless backend: it runs the frozen script and checks that the
record holds exactly `kosmo-testkit::run` and `kosmo-ui::run_round`. It
compiles empty without the features `test-hooks` and `call-probes` of the
headless backend, whose `call-probes` turns on those of `kosmo-testkit` and
`kosmo-ui` for its tests, and `cargo xtask check` runs it in its probe
pass, which runs every test of `framework/` with every probe on. Item 7 of
the runner's tests above lists it, and the tests of the record are in the
module `probes::tests` of `framework/testkit`: `a_recorded_entry_passes`
and `a_missing_entry_is_listed`.

## Limits

- The display server backend is planned, so the suite runs on the headless
  backend alone.
- Each surface of the headless backend has one buffer, so a driver waits for
  its release before it draws the next frame, and the backend keeps only
  the last presentation of each surface. No backend repaints by buffer age
  until a backend has more than one buffer per surface.
- The headless backend focuses each main surface when it creates it and
  never moves focus; later focus changes come only from injected events.
- The interface has no pointer, clipboard, text input, lifecycle or memory
  pressure events, and no frame events or presentation feedback other than
  `Released`.
- The accessibility trees that a backend keeps are connected to no
  assistive technology, there are no action requests, and queries of a
  tree by role and name in `kosmo-testkit` are planned.
- Resources are named with `&'static str` names fixed at compile time, and
  the headless backend holds their bytes from its construction.
- `inject_fault` holds one fault per port at a time, and a fault takes
  effect once, at the next call of its port.
- The suite sees a backend only through the platform interface and its test
  hooks: whether a backend starts threads or calls back into its caller is
  not observable there. The layout step rejects `std::thread` in the
  `src/` of `kosmo-platform` and the headless backend, which are core
  libraries, as the [platform boundary](../../architecture.md#platform-boundary)
  states, and review covers the rest.
- Only the runner's entry and the single-round entry have call probes, and
  only `framework/` has crates with probes and tests of call relations;
  the applications of `desktop/` and the crates of `display/` have none
  yet. Other public entries get probes with the tests of call relations
  that cover them. A test of call relations checks that its entries were
  reached, not how often or in which order, and the record holds the calls
  of its whole test binary.
- The probe pass runs every test of `framework/` once more, with the
  probes compiled in, so its time adds to the check.

## Running the tests

`cargo xtask check` runs the tests of the platform interface, the headless
backend, scripts and call probes with these commands from the repository
root, offline, with `CARGO_NET_OFFLINE=true` after its fetch step:

```sh
cargo test --workspace --locked --manifest-path framework/Cargo.toml
cargo test --locked --package kosmo-backend-headless --features test-hooks --manifest-path framework/Cargo.toml
cargo test --locked --workspace --features kosmo-testkit/call-probes,kosmo-ui/call-probes,kosmo-backend-headless/test-hooks,kosmo-backend-headless/call-probes --manifest-path framework/Cargo.toml
```

The first runs the tests of every crate of `framework/` without features
from the command line, among them the tests of `kosmo-platform` with its
test hooks, which `kosmo-testkit` turns on, the tests of scripts, digests
and probes of `kosmo-testkit`, and `tests/headless.rs` of the headless
backend without its test hooks. The second builds the headless backend
with its test hooks and runs, without call probes, the suite in
`tests/conformance.rs`, the runner's tests in `tests/runner.rs` and
`tests/frozen.rs`, and the tests of `tests/hooks.rs`, `tests/shell.rs` and
`tests/headless.rs`. The third, the probe pass, runs every test of
`framework/` once more with every call probe on, the test of call
relations in `tests/call_relations.rs` among them. The `clippy` step lints
the same three builds with `--all-targets`, and `framework/` once more
without `kosmo-testkit` and without test targets, as ordinary builds
compile it.

`--test` runs one test file alone; these commands run the suite and the test
of call relations:

```sh
cargo test --locked --manifest-path framework/Cargo.toml -p kosmo-backend-headless --features test-hooks --test conformance
cargo test --locked --manifest-path framework/Cargo.toml -p kosmo-backend-headless --features test-hooks,call-probes --test call_relations
```
