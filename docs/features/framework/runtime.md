# Framework runtime

The library `kosmo-ui` in the `framework/` workspace is the core of kosmo's
framework. It holds an application's control tree and applies each new view
tree to it, runs the event loop one round at a time with the time that its
caller passes in, holds the current result of a region in a versioned result
slot, keeps one-shot timers, checks the focus stamps of key events,
dispatches commands from the focused control to its ancestors and the
application, resolves keys with binding tables and the minimal key map, and
finds the control at a point of a layout result.

The round function covers the event, update and view passes. Layout,
drawing, accessibility and presentation, the shell `run` that waits for
platform events and reads the clock, and the platform interface
`kosmo-platform` with its backends are planned; until they exist, tests and
other drivers build the input of each round in code. `kosmo-ui` depends on
`kosmo-types` for geometry, logical time, focus generations, epochs and key
events, and on the versions of `kosmo-math-model`; it depends on neither the
job layer nor kalyx. The design rules that it follows are in the
[application model and passes](../../architecture.md#application-model-and-passes),
[control tree](../../architecture.md#control-tree),
[effects and the result slot](../../architecture.md#effects-and-the-result-slot),
[event loop and clock](../../architecture.md#event-loop-and-clock),
[dispatch and hit testing](../../architecture.md#dispatch-and-hit-testing),
[focus](../../architecture.md#focus) and
[commands and key bindings](../../architecture.md#commands-and-key-bindings)
sections of the architecture.

## Using it

An application is a model that implements `App`, with controls that
implement `Control` and an execution port type that implements `JobPort`. A
`Runtime` runs rounds: `Runtime::round` takes an `Input` and runs the event
and update passes, and `Round::finish` runs the view pass. Between the two,
the driver hands the round's effects to the execution port, here with
`execute`, and passes the receipts to the next round. This is the example of
the crate documentation, which `cargo test` runs: a one-line application
that submits a job when Execute is pressed.

```rust
use kosmo_types::{Epoch, FocusGen, HidUsage, KeyAction, KeyEvent, KeyMeaning, ModSet, MonoTime};
use kosmo_ui::{
    App, Command, Context, Control, DrawContext, EVALUATE_SELECTION, Event, Input, JobPort,
    Key, Kind, Message, Out, Receipt, Runtime, Stamp, SurfaceId, View, execute,
};

// A port that accepts every submission and numbers it.
#[derive(Default)]
struct Port { next: u64 }
impl JobPort for Port {
    type Request = &'static str;
    type Submission = u64;
    type Rejection = ();
    type CancelAnswer = ();
    type Report = (u64, &'static str);
    fn submit(&mut self, _: &'static str) -> Result<u64, ()> { self.next += 1; Ok(self.next) }
    fn cancel(&mut self, _: &u64) {}
    fn take_reports(&mut self, _: &mut Vec<(u64, &'static str)>) -> usize { 0 }
}

#[derive(Clone, PartialEq, Debug)]
struct Line;
impl Control<Calc> for Line {
    type State = ();
    fn kind(&self) -> Kind { Kind::new("line") }
    fn draw(&self, _: &mut DrawContext<'_, ()>) {}
}

// The application submits a job on Execute and keeps the identity it gets.
#[derive(Default)]
struct Calc { submitted: Option<u64> }
impl App for Calc {
    type Message = ();
    type Control = Line;
    type Port = Port;
    fn update(&mut self, message: Message<Self>, out: &mut Out<Self>) {
        match message {
            Message::Own(()) => out.submit("2*x"),
            Message::Accepted(id) => self.submitted = Some(id),
            _ => {}
        }
    }
    fn view(&self) -> View<Line> { View::new(Key::new(0), Line).focused() }
    fn command(&self, command: &Command) -> Option<()> {
        (command.id == EVALUATE_SELECTION).then_some(())
    }
}

fn main() {
    let surface = SurfaceId::new(1);
    let stamp = Stamp::new(Epoch::new(1), FocusGen::new(1));
    let execute_key = KeyEvent {
        usage: HidUsage::new(0x07, 0x28),
        meaning: KeyMeaning::Named(kosmo_types::NamedKey::EXECUTE),
        mods: ModSet::EMPTY,
        action: KeyAction::Press,
        time: MonoTime::from_nanos(5),
        synthetic: false,
    };
    let (mut runtime, mut app, mut port) = (Runtime::new(), Calc::default(), Port::default());

    // Round 1: focus comes in and Execute is pressed; the effect goes to the port.
    let mut input = Input::new(MonoTime::from_nanos(5));
    input.events.push(Event::FocusIn { surface, stamp });
    input.events.push(Event::Key { surface, stamp, event: execute_key });
    let mut round = runtime.round(&mut app, input);
    let receipts: Vec<Receipt<Port>> = execute(&mut port, round.take_effects());
    let first = round.finish(&app);
    assert!(first.record.text().contains("handled kosmo.EvaluateSelection by app"));

    // Round 2: the receipt comes back as the first message of the round.
    let mut input = Input::new(MonoTime::from_nanos(6));
    input.receipts = receipts;
    let second = runtime.round(&mut app, input).finish(&app);
    assert_eq!(app.submitted, Some(1));
    assert_eq!(second.record.lines()[1], "receipt accepted 1");
}
```

Run the library's tests from the repository root with:

```sh
cargo test --locked --manifest-path framework/Cargo.toml -p kosmo-ui
```

## Applications and controls

- `App::update` handles one `Message` at a time and is the only place that
  changes the model. It writes effects and timer requests into its `Out`:
  `submit(request)`, `cancel(submission)` and `timer(deadline, label)`.
- `App::view` reads the model and returns a `View` tree. A node has a key,
  unique among its siblings, and the control that the view gives; one node
  may be marked `focused()`, the focused control, and one `region()`, the
  region's root. Without a region mark the root is the region's root.
- `App::bindings` is the application's binding table, and `App::command`
  handles a command that no control handled, by returning a message.
- A `Control` has a `Kind`, a constant name, and a presentation state of its
  own type, which starts at its default. Its event handlers, `handle` for
  commands, `focus` for focus changes of the region's root and `timer` for
  its due timers, get a `Context`: the control's `WidgetId`, its
  presentation state, and messages, effects and timer requests to output.
  None of them reaches the model, the execution port or the platform.
- Messages of the framework are `Message::Own` for the application's own
  messages, `Accepted` and `Rejected` for receipts, `Report` for job reports
  and `Timer` for the application's due timers.

## The control tree

The `Tree` holds the controls in a slot table. A `WidgetId` is a slot and its
generation, which `to_bits` writes as one `u64`. Removing a control increases
its slot's generation and puts the slot on a free list, which is used last
in, first out, so the same sequence of views gives the same identities. A
slot whose generation would pass `u32::MAX` is retired. Traversals go in
preorder, following the children lists, never in slot order, and the tree
uses no hash containers.

When the view pass applies a view tree, a control under the same parent with
the same key and kind keeps its identity and presentation state and takes
the control that the view gives; every other node becomes a new control, and
the controls that the view no longer has are removed with their subtrees.

| Bound | Value | Beyond it |
| --- | --- | --- |
| `MAX_WIDGETS` | 4,096 controls | `RebuildError::TooManyControls` |
| `MAX_DEPTH` | 64 controls on the longest path from the root down, both ends included | `RebuildError::TooDeep` |

The view tree is checked before anything changes, and a rejected view leaves
the control tree as it was, so a round never panics on a view. Two siblings
with one key give `RebuildError::DuplicateKey`, and a second focused or
region mark `SecondFocus` or `SecondRegion`.

The tests check eight properties of the tree:

1. Parents and children agree: the children of each control's parent hold it
   exactly once, and the parent of each child is the control.
2. No cycle: from any control, following parents reaches the root within
   `MAX_DEPTH` controls.
3. Each control is in the tree exactly once: a preorder traversal from the
   root visits each control once, as many as the tree holds.
4. Traversal follows the children lists, in the order of the view tree.
5. An identity of a removed control finds no control, and its slot's
   generation has increased or the slot is retired.
6. Rebuilding round by round gives the same tree as building it once from
   the last view: the same keys, kinds, controls and order of children.
7. The same sequence of views gives the same identities.
8. The tree holds at most `MAX_WIDGETS` controls and is at most `MAX_DEPTH`
   deep.

## Rounds

The `Runtime` holds the control tree, the focus of each surface, the timer
table and the counts of what it dropped. It never reads a clock, never waits
and never calls a port, and it creates no threads. A round has two steps.

`Runtime::round(app, input)` runs the event pass and the update pass. Its
`Input` holds the round's `time`, a `MonoTime` that every judgement of the
round uses; the `receipts` of the previous round's effects; the platform
`events` in the order they were delivered; the job `reports`; and
`unfinished_jobs`, the number of jobs whose terminal state has not been
taken, as the execution port last reported it. The event pass queues
messages in this order:

1. the receipts: an acceptance or a rejection becomes a message, and the
   answer to a cancellation goes only into the record;
2. the messages that key events give through [dispatch](#dispatch);
3. the messages of focus changes, which the region's root receives;
4. the job reports;
5. the due timers.

Platform events are taken in their order, so a focus change applies to the
key events after it. The update pass then calls `update` once for each
message. The returned `Round` holds the effects of the event handlers and of
`update` in the order they were produced; `take_effects` hands them to the
driver.

`Round::finish(app)` runs the view pass: in a round that handled a message,
and in every round until a view tree has been applied, it calls `view` once
and applies its tree; a round in which only presentation state changed
skips it. It returns an `Output` with
the round's record, what the view pass did, as `ViewPass::Rebuilt`,
`Skipped` or `Rejected`, and the `next_deadline`: the earliest timer and,
while `unfinished_jobs` is above 0, the round's time plus
`JOB_POLL_INTERVAL`, 16 ms; `None` when there is neither.

## Effects and the execution port

`JobPort` is the execution port: `submit`, which answers at once with a
submission identity or a rejection; `cancel`, whose answer only the record
keeps; and `take_reports`, which also returns the number of unfinished jobs.
The request, submission identity, rejection, answer and report are its
associated types. The framework defines the port and never holds one; the
application's assembly implements it on its job layer. `Effect::Submit` and
`Effect::Cancel` are the effects, and `execute(port, effects)` hands them to
the port in order and returns one `Receipt` for each, `Accepted`, `Rejected`
or `Cancelled`, for the next round.

## The result slot

`ResultSlot` holds at most one result with its versions and submission. The
slot is part of the application model, beside the holder of the region,
which keeps the region's current `Versions` and its latest accepted
submission and passes them in.

- `deliver(result, current, latest)` judges a `Tagged` result with
  `kosmo_math_model::version::judge`, the function that the job layer uses
  too. A current result is written into the slot; any other result leaves
  the slot as it was. The `Delivered` value carries the judgement and the
  result, for the caller to write into the record of its submission or to
  count toward the region's discard count; it is `#[must_use]`.
- `shown(current, latest)` gives the display value: `Shown::Empty`,
  `Current` with the held result, or `Stale` once the region's versions or
  latest submission have changed, until a current result is written.
- Results are delivered in the update pass, when `update` handles a job
  report, so the acceptance receipts of the same round have always updated
  the latest submission first.
- The slot's operations take no editing structure and no cursor, so writing
  a result never changes what is being edited.

## Timers

`Out::timer` and `Context::timer` request a one-shot timer with an absolute
deadline and a `TimerLabel`. The requests of a round are registered after
the update pass. Every timer whose deadline is not later than a round's time
is due: due timers come by deadline and then by the order of registration,
an application's timer as `Message::Timer` and a control's through its
`timer` handler. A timer of a control that no longer exists is dropped and
counted. The table holds at most `TIMER_CAPACITY`, 64, timers; a request to a
full table is refused, recorded and counted, and the timers already there
stay. A periodic timer is registered again by its requester.

## Focus

Each surface has one region. For each surface the runtime records the stamp,
the epoch and focus generation, of its latest `Event::FocusIn`, and whether
it holds focus; `Event::FocusOut` with that stamp ends the holding, and a
focus out with another stamp is ignored. A key event is checked once, in one
place, before dispatch, and is dropped with its reason when:

| `Dropped` | When |
| --- | --- |
| `Epoch` | its epoch differs from the epoch of the surface's latest focus in |
| `Generation` | its focus generation differs from that of the latest focus in |
| `NoFocus` | its surface does not hold focus |

So a key of an earlier focus generation never reaches the new view.
`Runtime::counts` gives the number of drops for each reason.

## Dispatch

A key event that passes the check is resolved to a command with the binding
tables of the focus chain: the focused control's table, then its ancestors'
up to the region's root, then the application's, then the minimal key map.
The command goes first to the focused control in `Phase::Target`, then to
each ancestor up to the region's root in `Phase::Bubble`, then to
`App::command`, and the first that handles it ends dispatch. When the
region's root is not an ancestor of the focused control, the chain goes up
to the root; without a focused control, it has only the application. Controls never
receive raw key events. Every key event has exactly one outcome in the
record: dropped with its reason, handled with its handler, or unhandled.

## Commands and key bindings

A `Command` is a `CommandId`, a constant name in a namespace that states
whether the command repeats, with an optional character. The framework's
commands are in the namespace `kosmo`. A `Binding` names a `Trigger`, a
named key, one character or any character, and the `Mods` that it needs,
Shift, Ctrl, Alt and GUI without telling left and right apart. The event's
modifiers must equal them; a character binding ignores Shift, and the high 8
bits of `ModSet`, the layer state, never take part. The first binding whose
key meaning and modifiers match gives the command: a press triggers it, a
repeat only if the command repeats, and a release, synthetic or not, or a
long press never does. `resolve` does this for a key event and a list of
tables, and `check_table` reports a table that maps one key meaning and set
of modifiers to two commands.

`MINIMAL_KEY_MAP` has six bindings, all without modifiers:

| Key meaning | Command | Repeats |
| --- | --- | --- |
| `Named(NamedKey::LEFT)` | `MOVE_LEFT`, `kosmo.MoveLeft` | Yes |
| `Named(NamedKey::RIGHT)` | `MOVE_RIGHT`, `kosmo.MoveRight` | Yes |
| `Named(NamedKey::DELETE)` | `DELETE_BACKWARD`, `kosmo.DeleteBackward` | Yes |
| `Named(NamedKey::EXECUTE)` | `EVALUATE_SELECTION`, `kosmo.EvaluateSelection` | No |
| `Named(NamedKey::CANCEL)` | `CANCEL_JOB`, `kosmo.CancelJob` | No |
| Any character `c` | `INSERT_TEXT`, `kosmo.InsertText`, with `c` | Yes |

## Hit testing

`hit_test(layout, point)` returns the path of identities from the root to
the control at `point`, or `None`. A `LayoutResult` holds a `LayoutBox` for
each control in preorder, which is the drawing order: its parent's index,
its rectangle in the content coordinates of its parent, its scroll offset
and whether it draws. Each box is moved by the scroll offsets of all its
ancestors and clipped by all their boxes; it holds its left and top edges
but not its right and bottom edges. Of overlapping boxes the last in
drawing order is hit, and a control that draws nothing is never hit.
Coordinates are `Fx` values; a position that does not fit gives
`LayoutError::Overflow`, a box out of preorder `Order`, and a layout result
holds at most `MAX_WIDGETS` boxes. The function only reads its input; so
far only tests build layout results, and the layout pass will produce them.

## The drawing entry

`Control::draw` takes the control and a `DrawContext`, which gives only the
control's identity and its presentation state, to read. The drawing pass
will call it in preorder; it is not called yet. Through its parameters the
drawing entry reaches neither a mutable model, nor the execution port, nor a
port of the platform interface. Three examples of the documentation of
`Control` check this: each tries one of the three in the drawing entry and
fails to compile, and the same code without that line compiles. The
platform interface does not exist yet, so the third uses a stand-in trait
for one of its ports. The toolchain does not check the error code of an
example that fails to compile, so the examples state none.

## The event handling record

`Output::record` is the event handling record of a round: UTF-8 text, one
entry per line, with its fields in a fixed order. `Record::lines` gives the
lines and `Record::text` the text, each line followed by a line feed. It
holds no wall-clock time, no address and nothing in hash order, so the same
rounds give the same record byte for byte. Values of the application's
types, such as requests and reports, appear in their debug form, with line
feeds and carriage returns written as `\n` and `\r`. A round's lines come in
this order:

| Line | Meaning |
| --- | --- |
| `round <n> time <t>` | The round's number, from 1, and its time in nanoseconds |
| `receipt accepted <submission>` | An acceptance |
| `receipt rejected <rejection>` | A rejection |
| `cancel answered <answer>` | The answer to a cancellation |
| `key surface <s> epoch <e> generation <g> <meaning> <action> mods <bits> time <t> <outcome>` | A key event and its outcome, in the order of the platform events |
| `focus-in surface <s> epoch <e> generation <g>` | A focus in |
| `focus-out surface <s> epoch <e> generation <g>` | A focus out; `focus-out ignored` for one with another stamp |
| `report <report>` | A job report |
| `timer due <requester> deadline <t> label <l>` | A due timer |
| `timer dropped missing <requester> deadline <t> label <l>` | A due timer of a control that no longer exists |
| `effect submit <request>`, `effect cancel <submission>` | An effect, in the order they were produced |
| `timer set <requester> deadline <t> label <l>` | A registered timer |
| `timer refused full <requester> deadline <t> label <l>` | A request to a full timer table |
| `view rebuilt controls <n>`, `view rejected <error>`, `view skipped` | What the view pass did |
| `deadline <t>`, `deadline none` | The next deadline |

In a key line, `<meaning>` is `named <number>` or `char '<c>'`, `<action>` is
`press`, `release`, `repeat` or `long-press`, followed by `synthetic` for a
release that the display server made up, and `<bits>` is the whole `ModSet`
in hexadecimal, such as `0x0002`. `<outcome>` is `dropped epoch`,
`dropped generation`, `dropped no-focus`, `unhandled` for a key without a
command, `unhandled <command>` for a command that nobody handled,
`handled <command> by control <slot>:<generation>` or
`handled <command> by app`. A command is written `<namespace>.<name>`,
followed by the character in quotes for `kosmo.InsertText`. A requester is
`app` or `control <slot>:<generation>`, and a view error is
`too-many-controls`, `too-deep`, `duplicate-key <key>`, `second-focus` or
`second-region`. For example:

```text
round 8 time 8000000
key surface 1 epoch 1 generation 1 named 4 press mods 0x0000 time 0 handled kosmo.EvaluateSelection by app
effect submit Request { text: "11", versions: Versions { source: SourceVersion(2), context: ContextVersion(0), settings: SettingsVersion(0) } }
timer set app deadline 10500000 label 1
view rebuilt controls 2
deadline 10500000
```

Tests build the input of each round in code. The text format of event
scripts, their runner and its records are planned in `kosmo-testkit`.

## Determinism and the platform boundary

`kosmo-ui` is a core library with `#![forbid(unsafe_code)]`. Its `src/`,
unit tests included, uses none of the interfaces that the
[platform boundary](../../architecture.md#platform-boundary) forbids, among
them threads, clocks, the environment, files and input and output, which
the layout step of `cargo xtask check` checks. It uses no floating-point
arithmetic, no hash containers and no locale, and its record holds no
wall-clock time; review checks these, as the
[determinism rules](../../architecture.md#determinism-rules) describe.

## Limits

- The round function has no layout, drawing, accessibility or presentation
  pass; the shell `run`, its single-round entry and the platform interface
  are planned. Drivers build each round's input and hand effects to the
  execution port themselves.
- One region per surface, and the runtime has one control tree for all
  surfaces; focus does not move within a region, and there is no
  region-level focus generation.
- Only key events are dispatched; there are no pointer events and no
  long-press bindings.
- The event handling record has no format version, and the debug form of
  an application's values is part of it.
- The timer table holds 64 timers and refuses more.
- Hit testing has no caller outside tests.

## Requirements

The tests are in `framework/kosmo-ui`: the unit tests in the `tests`
modules under `src/` (named here with their module, such as
`tree::tests::…`), the random rounds in `src/random.rs`, and the
integration tests in `tests/` (named with their file), which run a one-line
calculator whose model holds a line of `kosmo-math-model` and a result
slot. No test sleeps or reads the wall clock.

1. The control tree keeps the eight properties of
   [the control tree](#the-control-tree) while views add, remove and change
   controls, nest them six deep, remove whole subtrees and reorder
   children, and a test that breaks each property on purpose through an
   entry of test builds finds it broken. Tests: `tree::tests::`
   `parents_and_children_agree_while_views_change`,
   `a_child_that_names_its_sibling_as_parent_breaks_the_agreement`,
   `no_cycle_with_six_levels_of_nesting`,
   `a_parent_and_child_that_are_each_others_parent_form_a_cycle`,
   `each_control_is_in_the_tree_once_after_a_subtree_is_removed`,
   `a_control_taken_out_of_its_parents_children_is_not_in_the_tree`,
   `traversal_follows_the_views_order_after_a_reorder`,
   `swapping_two_children_without_the_view_breaks_the_order`,
   `an_old_identity_finds_nothing_when_its_slot_is_used_again`,
   `a_removal_that_keeps_the_generation_lets_an_old_identity_find_a_control`,
   `a_tree_built_once_equals_the_tree_built_round_by_round`,
   `trees_that_differ_in_one_key_are_told_apart`,
   `the_same_sequence_gives_the_same_identities`,
   `sequences_that_differ_in_one_removal_give_other_identities`.
2. A view of exactly 4,096 controls, or exactly 64 deep, is built; one
   control more, or one level deeper, gives its distinct error and leaves
   the tree as it was, without panicking; repeated keys and second marks
   are rejected; a slot at its last generation is retired. Tests:
   `tree::tests::a_tree_exactly_at_its_bounds_is_built`,
   `tree::tests::one_control_beyond_a_bound_leaves_the_tree_as_it_was`,
   `tree::tests::views_with_repeated_keys_or_marks_are_rejected`,
   `tree::tests::a_slot_at_its_last_generation_is_retired`,
   `tree::tests::identities_fit_in_64_bits`,
   `round::tests::a_rejected_view_leaves_the_tree_and_is_recorded`.
3. In 1,000 groups of up to 64 random rounds from fixed seeds, with random
   changes of the view and random key events and focus changes, nothing
   panics, the properties hold after every round, and at the end of each
   group the tree equals the tree built once from the last view and a
   second run gives the same identities. Test:
   `random::random_rounds_keep_the_invariants`.
4. A result is written into the slot only when it is current. The receipt
   and the report of one submission in the same round give a current
   result, because the receipt comes first; a late result of a superseded
   submission, or after the source, settings or context version changed,
   is returned unchanged and not written; an older result after a newer
   one does not replace it; a written result is shown as stale after an
   edit or a new submission until a current one is written. Tests:
   `tests/result_slot.rs`:
   `a_receipt_and_its_report_in_one_round_give_a_current_result`,
   `a_late_result_of_a_superseded_submission_is_not_written`,
   `a_late_result_after_an_edit_is_stale`,
   `a_late_result_after_a_settings_change_is_stale`,
   `a_late_result_after_a_context_change_is_stale`,
   `an_older_result_that_arrives_after_a_newer_one_does_not_replace_it`,
   `a_written_result_is_shown_as_stale_after_an_edit`,
   `a_written_result_is_shown_as_stale_after_a_new_submission`.
5. Writing a result leaves the editing structure, its cursor and its source
   version as they were, and the slot's operations take no editing
   structure or cursor. Test: `tests/result_slot.rs`:
   `writing_a_result_leaves_the_editing_structure_as_it_was`.
6. A refused submission does not become the latest, and a cancellation
   gives the application no receipt. Effects keep the order in which they
   were produced, and their receipts come first in the next round. Tests:
   `tests/result_slot.rs`:
   `a_refused_submission_keeps_the_latest_and_a_cancellation_gives_no_receipt`;
   `round::tests::effects_keep_their_order_and_receipts_come_first_in_the_next_round`.
7. Two runs of one event script of twelve rounds write the same record byte
   for byte, with effects, receipts, the outcomes of keys with their
   handlers, a drop reason, and a timer set and due. Test:
   `tests/event_script.rs`: `two_runs_of_one_script_write_the_same_record`.
8. A command goes to the focused control, then bubbles to the parent, the
   region's root and the application, and the first handler ends dispatch;
   a command that nobody handles is unhandled; a key of another epoch or for
   a surface without focus is dropped and counted. Tests: `tests/dispatch.rs`:
   `the_focused_control_handles_its_command`,
   `an_unhandled_command_bubbles_to_the_parent`,
   `an_unhandled_command_bubbles_to_the_regions_root`,
   `an_unhandled_command_bubbles_to_the_application`,
   `a_command_that_nobody_handles_is_unhandled`,
   `a_key_of_another_epoch_is_dropped`,
   `a_key_for_a_surface_without_focus_is_dropped`.
9. A key of an earlier focus generation never reaches the new view: it is
   dropped before dispatch, `update` does not receive it and the view stays
   as it was. Test: `tests/dispatch.rs`:
   `a_key_of_an_earlier_focus_generation_never_reaches_the_new_view`.
10. Hit testing finds the control at a point: inside a box, not on its right
    and bottom edges, on its left and top edges, the later of overlapping
    boxes, clipped by all ancestors, moved by an ancestor's scroll offset,
    never a control that draws nothing, and nothing outside every box.
    Tests: `hit::tests::`
    `a_point_in_a_box_hits_it`, `right_and_bottom_edges_are_outside`,
    `left_and_top_edges_are_inside`,
    `of_overlapping_boxes_the_later_in_drawing_order_is_hit`,
    `a_box_is_clipped_by_all_its_ancestors`,
    `a_box_moves_by_the_scroll_offset_of_its_ancestors`,
    `a_control_that_draws_nothing_is_not_hit`,
    `a_point_outside_every_box_hits_nothing`,
    `boxes_come_in_preorder_and_positions_fit`.
11. Each binding of the minimal key map gives its command when the tables of
    the focus chain have no match: on press and repeat for the commands that
    repeat, on press only for Execute and Cancel, and for any character
    whatever its Shift and never on release; a long press and a synthetic
    release give no command; modifiers must be equal, the layer state takes
    no part, and a table with a conflict is reported. Tests:
    `tests/key_map.rs`: `left_moves_left_on_press_and_repeat`,
    `right_moves_right_on_press_and_repeat`,
    `delete_deletes_backwards_on_press_and_repeat`,
    `execute_evaluates_the_selection_on_press_only`,
    `cancel_cancels_the_job_on_press_only`,
    `a_character_inserts_itself_whatever_its_shift_and_not_on_release`,
    `long_press_and_synthetic_releases_are_unhandled`,
    `the_minimal_key_map_has_six_bindings_without_conflict`;
    `command::tests::` `the_minimal_key_map_has_no_conflict`,
    `a_table_that_maps_one_key_to_two_commands_is_reported`,
    `modifiers_fold_left_and_right_and_ignore_the_layer_state`,
    `modifiers_must_equal_and_the_layer_state_takes_no_part`,
    `long_press_and_synthetic_releases_trigger_nothing`,
    `the_first_table_with_a_match_gives_the_command`.
12. Timers: due timers come by deadline and then by registration order; a
    full table refuses the next timer distinctly and keeps the others; a
    timer of a removed control is dropped and counted; the next deadline is
    the round's time plus 16 ms while jobs are unfinished, otherwise the
    earliest timer, otherwise none. Tests:
    `timer::tests::due_timers_come_by_deadline_then_by_registration_order`,
    `timer::tests::a_full_table_refuses_a_timer_and_keeps_the_others`,
    `round::tests::a_full_timer_table_refuses_the_next_timer_distinctly`,
    `round::tests::a_timer_of_a_removed_control_is_dropped_and_counted`,
    `round::tests::the_next_deadline_follows_unfinished_jobs_then_timers`.
13. A round in which only presentation state changed does not call `view`
    and leaves the tree as it was. Test:
    `round::tests::a_round_that_changes_only_presentation_state_skips_the_view`.
14. The drawing entry reaches neither a mutable model, nor the execution
    port, nor a port of the platform interface: three examples of the
    documentation of `Control` fail to compile, and the same three without
    the offending line compile; its context gives the control's identity
    and presentation state. Tests: the examples of `Control`, which
    `cargo test` runs; `tree::tests::the_drawing_entry_gets_the_identity_and_presentation_state`.
15. `kosmo-ui` is a core library: the layout step of `cargo xtask check`
    reads its `src/` for the items that the platform boundary forbids, and
    the crate forbids unsafe code.
