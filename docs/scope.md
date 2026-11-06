# Scope

This page describes the complete target scope of kosmo: what it will provide,
which other parts of casξ it builds on, and the status of each capability.
kosmo has no code yet, so every capability has the status `planned`.

| Status | Meaning |
| --- | --- |
| `planned` | Described on this page; nothing is implemented |
| `partial` | Part of the capability works and is tested; its feature document states which part |
| `implemented` | The capability works and is tested as its feature document describes |

When a capability is implemented, the same change gives its row the new status
and a link to its feature document under `docs/features/`.

## Responsibilities

kosmo owns editing, formula typesetting, layout, the views of a page, focus and
navigation, and the interactive state of the casξ user interface. Its shell is
the graphical shell: it switches between documents, pages and applications and
keeps the stop control reachable. The command shell and terminal devices run
on krepi.

Other parts of casξ will provide what kosmo shows:

- **kalyx**, the computer algebra library, will parse and compute mathematics.
- **krepi**, the operating system, will provide the document service (documents,
  problems, versioned values and their dependencies), the job service, the
  display and input services, usage restrictions, language runtimes such as
  Python as separate worker programs, the GUI bridge and the SDK.
- Plots, geometry constructions and proof checking will be computed by their
  own components: the plotting component kamp, the dynamic-geometry component
  kyklo and the formal workbench krino. These are planned components without a
  repository yet. kosmo will show their results in document regions.

## Target scope by capability

| Capability | What kosmo will provide | Builds on | Status |
| --- | --- | --- | --- |
| Math input and formula editing | An editing structure kept apart from the semantic expressions of the CAS, so that incomplete input such as `1/□` can exist and simplification never changes the structure or the cursor you are editing; conversion of the editing structure into expressions; one-line input first, then two-dimensional editing with fractions, radicals, subscripts and superscripts; matrix and vector entry | kalyx | `planned` |
| Result display | Typeset results in exact or approximate form; scientific and engineering notation; angle, number base and display modes stated explicitly; every result tagged with the source, environment and settings versions it was computed from | kalyx, krepi job service | `planned` |
| Compound documents and split views | Documents made of problems, pages and application regions; reordering and copying pages; layouts with several views on one page, where a first release may show one application per page full screen; saving and reopening restores source content, layout and variable scopes | krepi document service | `planned` |
| Linked values | Variables, functions and data shared by every region of one problem; formulas, graphs, tables, measurements, notes and sliders updating each other; edits, including dragging a graph or geometry object, sent as semantic edits that the document service commits; dependency cycles reported; undo; problems isolated by default, with values from another problem only through an explicit import or library reference | krepi document service, kalyx | `planned` |
| Notes and scientific typesetting | Text, math boxes that can be recalculated or turned off, annotations, images and chemical formula typesetting; opening a graph page from a selected expression. Chemical boxes are typesetting only; they do not balance or solve reactions | kalyx, krepi document service | `planned` |
| Scratchpad and discovery | A standalone Scratchpad that switches between calculation and graph; history and reuse of subexpressions; undo; formula templates; a function catalog with argument help; context menus and settings. Calculation and graph share a temporary context that never changes a document; Scratchpad work is saved as a document only when you ask, and variable conflicts are resolved at that point | krepi document service, kalyx | `planned` |
| Graph view | Plot data shown with pan and zoom. Each view change fixes the expression, parameter environment, viewport and output size as a new version, and older samples can be discarded; point counts are bounded; undefined regions and discontinuities are kept, and curves are never joined across an asymptote | kamp, kalyx | `planned` |
| Table, geometry and proof views | Data tables, geometry views, and regions for proof goals and diagnostics. They share selection, clipboard, undo and the command framework with the other views and keep their own editing models | krepi document service, kyklo, krino | `planned` |
| Programming | Python editing, execution and an interactive terminal; user functions, programs and reusable libraries called from a document; further languages later. Python units with side effects run only when you run them, never because a view redraws or a variable changes. Output is bounded. The stop control first requests cancellation and ends the isolated worker when its limit is exceeded | krepi language runtimes and job service | `planned` |
| Widgets | Reusable sliders, canvases and interactive experiment or simulation pages; scripts that read the document variables they are granted and receive events; installation and registration, permission grants, drawing budgets, saving and restoring. The script language has not been chosen | krepi restricted canvas and permissions | `planned` |
| Focus, navigation and commands | Several independently focused regions on a page; semantic commands such as evaluate selection, cancel job, zoom viewport, run program and check proof, delivered to the focused region; a background page never takes the current input; a global stop and switch entry; shared selection, clipboard and undo | krepi GUI bridge | `planned` |
| Application lifecycle | Creating or opening, activating and deactivating, pausing and resuming, saving and closing as you see them. Closing an application handles unsaved content explicitly. Closing a view is separate from stopping its jobs: a job belongs to the document session and continues or is cancelled according to an explicit background policy | krepi application and job services | `planned` |
| Keyboard and display | Key layout, press, release and repeat, modifier keys and combinations, debouncing and the response to cancel; resolution, orientation, pixel format, update regions, backlight and refresh targets. kosmo draws into surfaces that the display service owns and composes. The product screen and keyboard have not been chosen | krepi display and input services | `planned` |
| Usage restrictions | A visible indication of a restricted mode, for example one that turns off the CAS, program execution or access to existing documents, and a way to check entering and leaving it. krepi enforces the restriction at every service entry; hiding a menu entry in kosmo is not a restriction. This is a product feature and implies no approval for any examination | krepi policy and session services | `planned` |
| Host preview | The same document, command, focus, job-protocol and layout code running on a host computer, with only the platform adaptation replaced. A preview does not test system-call isolation, device drivers, memory limits or power loss | Host platform adaptation | `planned` |

## Rules every view follows

- Drawing reads only view models. Rendering never runs a computation;
  computations are versioned jobs that return typed results: mathematical
  values, plot data, output text or diagnostics.
- Data flows from input to the editing model and its commands, to a versioned
  job, to a backend, to a result for the matching version, to a view model and
  then to rendering. Editors, the terminal, graph pages and proof regions use
  the same lifecycle and events.
- A result or diagnostic is shown only against the source version it was
  computed from. Text diagnostics carry the source file or unit, the source
  version and a half-open range of UTF-8 bytes; the math editor uses stable node
  positions, which kosmo maps to the screen.
- Backends never draw on the screen. A program that needs its own graphics gets
  a restricted canvas whose size, command count, buffer size and lifetime the
  service controls.
- Input that belongs to an earlier focus generation never reaches a new view.
- Memory for frames, fonts, images and history caches is accounted apart from
  computation memory. Caches that can be rebuilt are dropped when memory runs
  low.
- After a restart, kosmo rebuilds its views. Documents and jobs never depend
  on pointers into kosmo's views. Edits that were not committed to the document
  are not guaranteed to survive.

## Whole-machine flows

casξ as a whole is accepted against four flows. Each one passes through kosmo.

| Flow | Steps | kosmo's part | Status |
| --- | --- | --- | --- |
| Interactive math document | Define parameters and functions; graph, table and Notes stay in sync; adjust a slider; undo; save and reopen. Another problem and the Scratchpad are unaffected | Document regions, linked views, the slider, undo and layout restore | `planned` |
| Geometry exploration | Construct a triangle and dependent objects; drag a vertex; measurements and loci update; a degenerate position reports an error; after recovering, editing continues | The geometry view, drags as semantic edits, the error display | `planned` |
| Science experiment | Capture sensor data; data columns with units; fit, residuals and graph; a Notes explanation; export to a computer. Disconnections and dropped samples keep their quality marks | Table and graph views, Notes, visible quality marks. Capture and export are provided by krepi and other casξ components | `planned` |
| Programming and proof | Run Python or a proof job in a document; read versioned diagnostics; stop the job when it exceeds its limit; the GUI stays editable; the source content and an explicit result state are saved | Code editor, terminal view, diagnostics, the stop control | `planned` |

## First integration target

kosmo is first integrated with krepi in krepi's interactive-device stage, on the
Raspberry Pi 5. At that stage a calculation is entered on a physical keyboard
and its result appears on a display. Editing, error display, long-press and
modifier keys and cancellation are checked, together with input and display
during a long calculation and the rejection of stale results.

Three examples run there:

1. A derivative of $x^2$ is requested, the input is changed to $x^3$ and
   submitted again. The old job can be cancelled, and the old result, even when
   it arrives late, belongs to the old source version only and never replaces
   the new result.
2. A graph is panned and zoomed. Each step fixes the expression, parameter
   environment, viewport and output size as a new version; older samples can be
   discarded, the number of points is bounded, and undefined regions and
   discontinuities are kept.
3. A Python or proof job prints output continuously or loops. The GUI stays
   editable and can switch pages, output is bounded, the stop control first
   requests cancellation and ends the isolated worker when its limit is
   exceeded, and the document keeps its source content and reports the final
   state of the job.

## Open questions

- **Screen, keyboard and form.** Not chosen; no key count or size is fixed.
- **GUI, display and input stack and rendering path.** Not decided. kosmo's
  platform adaptation follows that decision.
- **Font, text and image libraries.** Chosen with the font rendering,
  typesetting and image formats that kosmo needs; FreeType is one candidate.
- **Widget script language.** Not chosen. Lua is a reference.
- **Response times.** No numeric targets for start-up, cancellation or response
  time have been set; they are set after measurements on a prototype.
- **TI-Nspire compatibility.** The capabilities draw on the TI-Nspire CX II CAS
  handheld as a reference. Reading or writing its `.tns` documents, TI-Basic and
  the TI Lua API are open compatibility questions and are not planned features.
