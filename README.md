<picture>
  <source media="(prefers-color-scheme: dark)" srcset=".github/assets/logo-dark.svg">
  <img alt="kosmo" src=".github/assets/logo.svg" height="64">
</picture>

# kosmo

kosmo is the desktop environment of krepi, the casξ operating system, and the
GUI framework for its desktop applications. casξ is a portable workstation for
mathematics, science and programming built on the Raspberry Pi 5. kosmo covers
the graphical stack above krepi's drivers: the display protocol and the
display server; the framework and its SDK; the desktop environment and system
interface, that is, the shell and the system screens; the first-party
applications; and display compatibility for Linux desktop programs. The
formula editor, typeset results, the Scratchpad and document regions are
controls of the framework or applications built on it. The name comes from
the Greek κόσμος.

## What it will provide

### Display protocol and display server

Applications will draw into their own surfaces and hand them to kosmo's
display server. The display server will compose them, route input and focus,
mediate the clipboard, authorize screenshots and keep a trusted indicator area
that no application can draw over. It will speak kosmo's own display protocol,
built on krepi's capabilities and IPC, with a table that maps each protocol
object to its nearest Wayland counterpart.

### Framework and SDK

kosmo will provide a GUI framework with controls, layout, text and fonts,
software rendering, commands and undo, accessibility, themes and an
application model, together with the formula editing and typesetting
controls. Applications will build on it through an SDK; programs in other
languages will use a C interface to the client library of the display
protocol.

### Desktop environment and system interface

A shell will start, switch and lay out applications, keep the global stop and
switch entries reachable, and show a status area, notifications and system
dialogs. The system interface will add screens for settings, device
management and restricted modes.

### First-party applications

The first-party applications will be built on the framework like any other
application: the calculator, which includes the Scratchpad; Notes and
documents; and a programming application with a terminal view. They will let
you:

- Type and edit mathematics. The formula editor keeps incomplete input such as
  `1/□` while you work, and simplification never rearranges the formula you are
  editing. Results are typeset with fractions, radicals, subscripts and
  superscripts, in exact or approximate form, with the angle, number base and
  display modes stated explicitly.
- Keep your work in documents. A document holds problems; a problem holds
  pages; a page holds application regions such as a calculator, a graph, a
  table, a geometry view, a Notes page or a program editor. Saving and reopening
  a document restores its source content, its layout and its variable scopes.
- Link values. Variables, functions and data defined in one problem are shared
  by every region of that problem. Change a parameter or drag a slider, and the
  formulas, graphs, tables, measurements and notes that depend on it update.
  Undo takes you back. Other problems stay unaffected unless you import from
  them explicitly.
- Write Notes with text, math boxes that recalculate, annotations, images and
  chemical formulas, and open a graph of an expression you select.
- Use the Scratchpad for quick calculations and graphs that never touch your
  documents, with history, reuse of earlier results, formula templates, a
  function catalog with argument help, and context menus. Save Scratchpad work
  as a document when you want to keep it.
- Write and run programs. Edit Python code, run it, use an interactive
  terminal, and call your own functions and libraries from a document.
- Build interactive widgets: reusable sliders, canvases and interactive
  experiment pages whose scripts read only the document variables you allow.
- Keep working while a long computation runs. The stop control is always
  reachable, and a result that arrives after you changed the input is never
  shown against the new input.

### Display compatibility for Linux desktop programs

Unmodified Linux desktop programs, X11 programs included, will be shown on
kosmo's display server through a compatibility service that will live in a
separate repository.

[Scope](docs/scope.md) describes each of these in detail, with what kosmo
relies on from the other parts of casξ.

## Status

kosmo has no application on its framework yet. On a host computer, the
`kosmo` program computes one line at a time in a terminal: the
[line-mode terminal](docs/features/desktop/line-mode-terminal.md) reads
expressions and a few commands from standard input and writes exact results,
such as `2*x` for the derivative of `x^2`, to standard output. Of the features
above, the parts that it builds on work, as libraries: the editing structure
of one-line formula input, its conversion into the text that kalyx reads, the
job layer that runs that text with kalyx, and the Scratchpad's history of
submissions with the reuse of their inputs and results. The repository holds
documentation, the kosmo logo, a commit hook, the repository checks, the
layout table `layout.toml` and three product workspaces. The `display/`
workspace has `kosmo-types`, the shared types of the framework and the display
server: fixed-point geometry, logical time, focus generations and epochs,
keyboard event records, pixel formats, buffer descriptions with the conversion
of pixels between them, and damage regions. The `framework/` workspace has the
libraries `kosmo-math-model`, which holds
[one-line formula input](docs/features/math/one-line-editing.md) while you edit
it, `kosmo-jobs`, which runs computations as [versioned
jobs](docs/features/framework/job-layer.md), `kosmo-render`, which holds
[display lists](docs/features/framework/rendering.md) with their text
form, the renderer interface, the rasterizer on vello_cpu that draws display
lists on the CPU and the wrapper of the transcendental functions that
rendering and layout are to call, `kosmo-text`, which holds the [two
fonts](docs/features/framework/text.md) that kosmo bundles, Fira Sans and
Fira Mono, with their license files and
their asset manifest, the font set that registers their bytes for the text
stack parley without system fonts, the shaping and line breaking of
paragraphs into glyph runs of display lists and the glyph cache of their
coverage masks, `kosmo-platform`, the [platform
interface](docs/features/framework/platform-interface.md) through which the
framework reaches surfaces, input, its clock, resources and the
accessibility tree, with AccessKit re-exported, `kosmo-math-kalyx`,
which [converts](docs/features/math/formula-conversion.md) the input into the
text that kalyx reads and runs the jobs with kalyx, `kosmo-scratchpad`, the
[Scratchpad](docs/features/framework/scratchpad.md), which keeps a history of
the submissions of a line with their results and reuses their inputs and
results in the line, the framework core `kosmo-ui`, which holds an
application's [control tree](docs/features/framework/runtime.md) and runs
the rounds of its event loop with result slots, timers, focus checks and
the dispatch of commands, with a shell that fetches the two fonts through
the platform interface, drives rounds through it and presents frames,
drawing text from the glyph cache of its font set, so far without layout
and with a drawing pass that rounds do not run yet, the
headless backend `kosmo-backend-headless`, which implements the platform
interface without threads or a display, with a clock and input that only
its driver gives it, `kosmo-testkit`, test support with the conformance
suite of the platform interface, which the headless backend passes, with
the runner of
[event scripts](docs/features/framework/platform-interface.md#event-scripts),
which drives the framework on the headless backend and writes view model
records, display lists and pixel digests at each checkpoint, with the digests
of presented buffers and with call probes for tests of call relations, and
the facade `kosmo-sdk`, which re-exports
`kosmo-math-model`, `kosmo-jobs`, `kosmo-math-kalyx` and `kosmo-scratchpad`
and through which the `desktop/` workspace uses them. The
`desktop/` workspace has the `kosmo` program. CI runs the repository checks on every pull request to
`main` and every push to `main`, and a weekly job audits the lock files
against the RustSec advisory database. These commands run from the
repository root:

```sh
printf ':diff x x^2\n:diff x x^3\n:history\n' | cargo run --quiet --locked --manifest-path desktop/Cargo.toml -p kosmo
cargo run --locked --manifest-path desktop/Cargo.toml -p kosmo -- --version
cargo test --locked --manifest-path desktop/Cargo.toml -p kosmo
cargo test --locked --manifest-path framework/Cargo.toml -p kosmo-math-model
cargo test --locked --manifest-path framework/Cargo.toml -p kosmo-jobs
cargo test --locked --manifest-path framework/Cargo.toml -p kosmo-render
cargo test --locked --manifest-path framework/Cargo.toml -p kosmo-text
cargo test --locked --manifest-path framework/Cargo.toml -p kosmo-platform --features test-hooks
cargo test --locked --manifest-path framework/Cargo.toml -p kosmo-math-kalyx
cargo test --locked --manifest-path framework/Cargo.toml -p kosmo-scratchpad
cargo test --locked --manifest-path framework/Cargo.toml -p kosmo-ui
cargo test --locked --manifest-path framework/Cargo.toml -p kosmo-backend-headless --features test-hooks
cargo test --locked --manifest-path framework/Cargo.toml -p kosmo-testkit
cargo test --locked --manifest-path framework/Cargo.toml -p kosmo-sdk
cargo test --locked --manifest-path display/Cargo.toml -p kosmo-types
cargo xtask check
```

The first runs two derivatives and lists the history in the terminal; without
the pipe, `cargo run --quiet --locked --manifest-path desktop/Cargo.toml -p kosmo`
reads the lines that you type. The second prints `kosmo` and its version. The
next thirteen run the tests of the terminal, of the editing structure, of
the job layer, of rendering, with its pixel goldens and the wrapper of
transcendental functions, of the text crate, with its font set, shaping,
line breaking, glyph cache and pixel goldens, of the platform interface
with its test hooks, of formula conversion with the kalyx runner, of the
Scratchpad, of the framework core, of the headless
backend with its test hooks, the conformance suite, the shell and the
script runner, of the event scripts, the pixel digests and the call probes
of the test support, of the facade and of the shared types, and the last
runs the repository checks. No version has been released yet: a release is
a tagged commit, and the version that `main` shows is not a release until
it is tagged.

kosmo is written in Rust: its programs use the standard library, and its
pure-core crates use only `core` and `alloc`. Its first releases will run
only on a host computer: in a terminal in line mode, headless, or in a host
window through the nested display server. Later, kosmo will run on krepi,
where its display server will hold krepi's display and input device services
and its applications will use krepi's document and job services. The code
above the framework's platform interface and the display server's device
ports will be the same on a host and on the device; only the backends behind
them will differ. A run on a host tells you nothing about system-call
isolation, device drivers, memory limits or power loss on the device. The
scope describes the
[first integration target](docs/scope.md#first-integration-target): what will
be checked first when kosmo runs on the device.

## Related repositories

- **krepi** (`casxi-lab/krepi`): the casξ operating system. It will provide the
  microkernel; the display and input drivers, whose device services kosmo's
  display server will hold; shared buffers and IPC; the supervisor; the
  document and job services, language runtimes, storage and power; the
  enforcement of usage restrictions; and the sysroot and platform SDK. kosmo
  does not depend on krepi's crates: kosmo will define the ports, and krepi
  will implement them and build kosmo for the device.
- **kalyx** (`casxi-lab/kalyx`): the casξ computer algebra library. kosmo
  takes it from crates.io and turns what you type into the text that it
  reads, and will show the results it computes.
- **kyto** (`casxi-lab/kyto`): the casξ hardware, including the screens and
  keyboards that kosmo's display server will reach through krepi's drivers.

## Documentation

- [Scope](docs/scope.md): the complete target scope of kosmo and the status of
  each capability.
- [Architecture](docs/architecture.md): the design rules, the source layout and
  the components.
- [Requirements on krepi: display and
  devices](docs/requirements/display-and-devices.md): what kosmo needs from
  krepi for the display path on the device.
- [Requirements on typed values](docs/requirements/typed-values.md): what kosmo
  saves or exchanges and what it needs from the typed values that hold it.
- [Contributing](CONTRIBUTING.md) and [repository instructions](AGENTS.md).

## License

kosmo is released under the [MIT License](LICENSE). Copyright (c) 2026 The
casξ Authors; see [AUTHORS](AUTHORS). The two fonts that `kosmo-text`
bundles in `framework/text/assets/fonts/` are not covered by the MIT License:
Fira Sans and Fira Mono are distributed under the SIL Open Font License 1.1
(`OFL-1.1`) with their original license files,
[`firasans/OFL.txt`](framework/text/assets/fonts/firasans/OFL.txt) and
[`firamono/OFL.txt`](framework/text/assets/fonts/firamono/OFL.txt).
