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

kosmo has no usable capability yet: none of the features above work today.
The repository holds documentation, the kosmo logo, a commit hook, the
repository checks, the layout table `layout.toml` and the `desktop/` workspace
with the `kosmo` program, which so far only reports its version. CI runs the
repository checks on every pull request to `main` and every push to `main`.
These commands run from the repository root:

```sh
cargo run --locked --manifest-path desktop/Cargo.toml -p kosmo -- --version
cargo xtask check
```

The first prints `kosmo` and its version; the second runs the repository
checks. No version has been released yet: a release is a tagged commit, and
the version that `main` shows is not a release until it is tagged.

kosmo is written in Rust: its programs use the standard library, and its
pure-core crates will use only `core` and `alloc`. Its first releases will run
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
- **kalyx** (`casxi-lab/kalyx`): the casξ computer algebra library. kosmo will
  take it from crates.io, turn what you type into expressions for it and show
  the results it computes.
- **kyto** (`casxi-lab/kyto`): the casξ hardware, including the screens and
  keyboards that kosmo's display server will reach through krepi's drivers.

## Documentation

- [Scope](docs/scope.md): the complete target scope of kosmo and the status of
  each capability.
- [Architecture](docs/architecture.md): the design rules, the source layout and
  the components.
- [Contributing](CONTRIBUTING.md) and [repository instructions](AGENTS.md).

## License

kosmo is released under the [MIT License](LICENSE). Copyright (c) 2026 The
casξ Authors; see [AUTHORS](AUTHORS).
