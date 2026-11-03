<picture>
  <source media="(prefers-color-scheme: dark)" srcset=".github/assets/logo-dark.svg">
  <img alt="kosmo" src=".github/assets/logo.svg" height="64">
</picture>

# kosmo

kosmo is the user interface and shell of casξ, a portable workstation for
mathematics, science and programming built on the Raspberry Pi 5. It is the
part of casξ you see and touch: the formula and code editors, typeset results,
graphs, tables, the terminal view, documents with their pages, and the shell
that moves you between them. The name comes from the Greek κόσμος.

## What it will let you do

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
- Build widgets: reusable sliders, canvases and interactive experiment pages
  whose scripts read only the document variables you allow.
- Keep working while a long computation runs. The stop control is always
  reachable, and a result that arrives after you changed the input is never
  shown against the new input.

[Scope](docs/scope.md) describes each of these in detail, with what kosmo
relies on from the other parts of casξ.

## Status

kosmo is newly initialized. The repository holds documentation, the kosmo logo
and a commit hook. It has no code, so there is nothing to build or run yet, and
none of the features above work today.

kosmo will be an ordinary Rust program that uses the standard library. It will
run first as a preview on a host computer and then on krepi, the casξ operating
system, where it will connect through krepi's GUI bridge to the document, job,
display and input services. The preview and the device will share the document,
command, focus, job-protocol and layout code; only the platform adaptation
differs. A preview run tells you nothing about system-call isolation, device
drivers, memory limits or power loss on the device.

The first integration target is krepi's interactive-device stage on the
Raspberry Pi 5. There a calculation is entered on a physical keyboard and its
result appears on a display. Editing, errors, long-press and modifier keys and
cancellation are checked, together with input and display during a long
calculation and the rejection of stale results. One-line input comes first; two-dimensional formula editing
is checked separately. The product screen and keyboard have not been chosen.

## Related repositories

- **krepi** (`casxi-lab/krepi`): the casξ operating system. It will provide the
  microkernel, the document, job, display and input services, the GUI bridge
  and the SDK that kosmo is built on. It is newly initialized as well.
- **kalyx** (`casxi-lab/kalyx`): the casξ computer algebra library. kosmo will
  turn what you type into expressions for it and show the results it computes.
  kalyx is newly initialized; the library is developed in `casxi-cas` until its
  code moves into kalyx.
- **casxi-build**: plans and cross-repository coordination for casξ.

## Documentation

- [Scope](docs/scope.md): the complete target scope of kosmo and the status of
  each capability.
- [Contributing](CONTRIBUTING.md) and [repository instructions](AGENTS.md).

A license has not been selected yet.
