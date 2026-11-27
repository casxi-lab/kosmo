# Platform interface

The framework reaches surfaces, input, its clock, resources and the
accessibility tree through the nine traits of `kosmo-platform` in the
`framework/` workspace. The headless backend `kosmo-backend-headless`
implements them without threads or a display, with a clock and input that only
its driver gives it, and `kosmo-testkit` holds the conformance suite of the
interface and the script runner, which drives the framework on the headless
backend from event scripts. The traits, their methods and bounds and the rules
that every backend keeps are stated in the crate documentation of
`kosmo-platform` and in the [platform
interface](../../architecture.md#platform-interface) and [headless backend and
scripts](../../architecture.md#headless-backend-and-scripts) sections of the
architecture. This document describes event scripts and the runner with the
records that it writes.

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
  `mod-shift`, `mod-ctrl`, `mod-alt` or `mod-gui`;
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
  new main surface.
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
`script::tests`:

1. Every kind of line is read with its fields, at the largest values too,
   and each named key and format has its name. Tests:
   `every_kind_of_line_is_read_with_its_fields`,
   `every_named_key_and_format_has_its_name`.
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
deadline due finds no platform event: its wait returns `Idle`. The runner
advances the clock only through `advance_to` of the test hooks and reads no
other clock, and the headless backend never advances its clock itself. A line
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
| `Earlier` | The backend's clock is later than the time of a key line or time line |
| `Driver` | A round of the driver failed |
| `NoSurface` | A focus line names a surface that the script did not create |
| `NoStamp` | A key line without a stamp, with no focus line and no recorded focus to take one from |
| `NotIdle` | The framework did not become idle within 65,536 rounds of the line |
| `View` | A view line holds a control character |
| `Display` | Display text that is not lines, each followed by a line feed |

With the feature `call-probes` of `kosmo-testkit`, which only the command
line turns on, each run records the call probe `kosmo-testkit::run`, and the
single-round entry records `kosmo-ui::run_round`. The test of call relations
owns its test binary, `tests/call_relations.rs` of the headless backend, and
compiles empty without the feature `call-probes` of the headless backend,
which turns on the call probes of `kosmo-testkit` and `kosmo-ui` for its tests
only. These commands run from the repository root; the first two run the
tests of scripts and the digests, and of the runner on the headless backend,
and the third the test of call relations:

```sh
cargo test --locked --manifest-path framework/Cargo.toml -p kosmo-testkit
cargo test --locked --manifest-path framework/Cargo.toml -p kosmo-backend-headless --features test-hooks
cargo test --locked --manifest-path framework/Cargo.toml -p kosmo-backend-headless --features test-hooks,call-probes --test call_relations
```

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
   framework is idle only after a round that starts quiet and waits `Idle`:
   65 keys of one time, more than one wait returns, are all handled before
   the checkpoint, and rounds with a due deadline go on. The bound of 65,536
   rounds holds for each line, and a line that is never idle stops after
   exactly that many. Tests: `tests/runner.rs`:
   `lines_of_one_time_run_no_round_between_them`,
   `a_key_at_the_time_of_a_focus_line_takes_its_stamp`,
   `more_keys_than_one_wait_returns_are_all_handled_before_the_checkpoint`,
   `a_due_deadline_keeps_the_framework_running`,
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
   single-round entry. Test: `tests/call_relations.rs`:
   `a_script_run_reaches_the_runner_and_the_single_round_entry`.
