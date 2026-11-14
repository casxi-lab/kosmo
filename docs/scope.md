# Scope

This page describes the complete target scope of kosmo: what it will provide,
which other parts of casξ it builds on, and the status of each capability.
No capability is usable yet, so every capability has the status `planned`.

| Status | Meaning |
| --- | --- |
| `planned` | Described on this page; nothing is implemented |
| `partial` | Part of the capability works and is tested; its feature documents state which part |
| `implemented` | The capability works and is tested as its feature documents describe |

A change that delivers a feature document under `docs/features/` sets the
matching row to `partial` and links the document from it. A row can link
several feature documents, and it becomes `implemented` only when the whole
capability works and is tested as its feature documents describe.

## Responsibilities

kosmo is the desktop environment of krepi and the GUI framework for its
desktop applications. It has four parts and a compatibility layer: the display
protocol and the display server; the framework and its SDK; the desktop
environment and system interface, that is, the shell and the system screens;
the first-party applications; and display compatibility for Linux desktop
programs. Formula editing, typesetting, result display, the Scratchpad and
document regions are controls of the framework or applications built on it.
kosmo will have its own framework core and controls, built on Rust
building-block crates for text, rasterization, accessibility and
localization, and its own display protocol, built on krepi's capabilities and
IPC. Its shell is the graphical shell of the desktop environment; the command
shell and terminal devices run on krepi.

kosmo builds on these other parts of casξ:

- **kalyx**, the computer algebra library, will parse and compute mathematics.
- **krepi**, the operating system, will provide:
  - the kernel, shared buffers and IPC;
  - the display and input drivers. They will offer kosmo's display server a
    display device service (output enumeration, scan-out buffers, atomic
    commits, completion and vblank events) and an input device service
    (normalized input events and device descriptions), and they will carry
    out mode setting and scan-out. The input driver will recognize the long
    press of the reserved stop key and pass it straight to krepi's
    supervisor;
  - the supervisor, with application registration and lifecycle;
  - the document service (documents, problems, versioned values and their
    dependencies), the job service, language runtimes such as Python as
    separate worker programs, storage, and power with the backlight mechanism
    and automatic sleep;
  - the enforcement of usage restrictions at every service entry;
  - the mechanisms for Linux compatibility;
  - the sysroot, the platform SDK, installation on the device and the joint
    tests on the device.

  kosmo's display server is the only user of the input device service.
  Besides it, only krepi's minimal recovery client may use the display device
  service; that client shows only pre-rendered images and status codes, takes
  no input and does no layout. kosmo does not depend on krepi's crates: kosmo
  will define the device, client transport, job and document ports, and krepi
  will implement them and build kosmo's programs for the device.
- The mathematics of plots and geometric constructions will come from kalyx.
  The plotting application kamp, the dynamic geometry application kyklo and
  the formal workbench krino, which checks proofs, will build their interfaces
  on kosmo's framework. The framework will provide the coordinate plane, the
  code editor, the diagnostics and goal views and the region protocol that
  they use.

## Target scope by capability

The capabilities are grouped by the parts of kosmo. Every table has the same
columns.

### Display protocol and display server

| Capability | What kosmo will provide | Builds on | Status |
| --- | --- | --- | --- |
| Display protocol | kosmo's own protocol between applications and the display server, built on krepi's capabilities and IPC. A session is a capability whose identity comes from the kernel, and privileged roles are granted one at a time. The protocol covers surfaces and buffers; views and embedding slots that place one client's surface inside another's; input events with focus generations; the clipboard; restricted canvases; outputs and presentation feedback; screenshot authorization; the trusted indicator area; reconnection after a restart of the display server; version negotiation and errors. A privileged extension is granted only to the shell's session. A table maps every protocol object to its nearest Wayland counterpart. The first protocol version is provisional | krepi capabilities, IPC and shared buffers | `planned` |
| Compositing and screen output | Compositing with damage regions, frame scheduling and presentation feedback; output management for resolution, orientation, pixel format and refresh, with the policy for mode, rotation, brightness and turning the screen off, and panel power; the pointer cursor; memory quotas per client and fairness under load; recovery after a crash of the display server, with clients reconnecting; headless and nested backends for hosts. The display server holds krepi's display device service: krepi's display driver allocates the scan-out buffers and carries out mode setting and scan-out, and the backlight mechanism and automatic sleep belong to krepi. The product screens have not been chosen | krepi display device service, shared buffers | `planned` |
| Trusted screen areas, focus and clipboard | The parts of the display server that no application can fool, each written as a small pure state machine: the view tree and stacking order; focus routing and focus generations; the lifecycle of surfaces and buffers; a trusted indicator area and the stop and switch entries, which the display server draws itself and no application can draw over; clipboard mediation and screenshot authorization. A written threat model of the GUI states what they protect against | krepi capabilities | `planned` |
| Keyboard and key maps | kosmo's display server, the only user of krepi's input device service, turns key presses and releases into key events: key maps with several layers, modifier keys and combinations, key repeat and long press, timed by an injected clock. Every key event carries both the physical key and its meaning and goes to the view in focus. Global keys and the stop key: a short press goes through the display server and cancels the jobs in focus; a long press of the reserved stop key is recognized by krepi's input driver and reaches krepi's supervisor even when the display server does not respond. Debouncing belongs to krepi's driver or the keyboard controller. The product keyboards have not been chosen | krepi input device service | `planned` |

### Framework and SDK

| Capability | What kosmo will provide | Builds on | Status |
| --- | --- | --- | --- |
| Framework runtime and application model | kosmo's own framework core: a tree of controls with fixed passes; state with versioned result slots; a single-threaded event loop with an injected clock; layout; commands and actions; undo; themes and styles; animation timing; an accessibility tree with test queries; localization; the application side of the clipboard; several surfaces, overlays and hosted regions with the region protocol; the application model and lifecycle; clients of the job and document services; queries of the current usage policy; and accounting of GUI caches apart from computation memory | kosmo display protocol; krepi job and document services on the device | `planned` |
| Rendering, text and fonts | Display lists and update regions; software rasterization on the CPU; text shaping, line breaking and bidirectional text; bundled fonts with glyph caches, two interface fonts first and a math font later; image decoding and encoding; the same output on every host. Text and rasterization come from Rust building-block crates behind kosmo's own interfaces, and fonts are unmodified OFL-1.1 font files | Rust building-block crates | `planned` |
| Controls | The framework's controls: labels, lists, scrolling and other basic controls; text fields; menus, dialogs, pop-ups and a command palette; sliders and numeric controls; a table grid; a coordinate plane with versioned viewports; a code editor with highlighting and diagnostics; terminal emulation; a job output view with bounded output; a stop control that shows the state of its job; diagnostics and proof-goal views; a host for restricted canvases. kosmo's own applications use the same controls as any other application | kosmo framework runtime | `planned` |
| Math input and formula editing | An editing structure kept apart from the semantic expressions of the CAS, so that incomplete input such as `1/□` can exist and simplification never changes the structure or the cursor you are editing; conversion of the editing structure into expressions; one-line input first, then two-dimensional editing with fractions, radicals, subscripts and superscripts; matrix and vector entry. The editing structure and the formula editing controls do not depend on the CAS and are open to other applications; only the kalyx adapter crate depends on kalyx | kalyx | `planned` |
| Result display | Typeset results in exact or approximate form; scientific and engineering notation; angle, number base and display modes stated explicitly; every result tagged with the source, environment and settings versions it was computed from | kalyx, krepi job service | `planned` |
| Focus, navigation and commands | Several independently focused regions on a page; semantic commands such as evaluate selection, cancel job, zoom viewport, run program and check proof, delivered to the focused region; a background page never takes the current input; a global stop and switch entry; shared selection, clipboard and undo | kosmo display server (focus routing), krepi job service | `planned` |
| SDK and client libraries | An SDK crate for applications that at first only re-exports the framework, is marked unstable and is not published; project templates, developer documentation and developer tools; the client library of the display protocol with its conformance tests; a C interface to the client library, with bindings for other languages such as Go. Until version 1.0, applications use the framework only through its Rust API | kosmo framework and display protocol | `planned` |
| Host preview | kosmo on a host computer: the line-mode terminal frontend; the framework's headless backend, driven by event scripts; the display server with its headless and nested backends, where a host window appears only as an output of the nested display server. The code above the framework's platform interface and the display server's device ports is the same as on the device. A host run does not test system-call isolation, device drivers, memory limits or power loss | Host operating system | `planned` |

### Desktop environment and system interface

| Capability | What kosmo will provide | Builds on | Status |
| --- | --- | --- | --- |
| Shell | The graphical shell: the home screen and launcher; switching between applications; the window model and layout policy, where a first release shows one application at a time, full screen; the global stop and switch entries; the status area, notifications and system dialogs; and how applications start, pause and close as you see it. The window and layout policy is a replaceable module inside the display server; the rest of the shell is a client that uses the protocol's privileged shell extension | kosmo display protocol (shell extension); krepi supervisor (application registration and lifecycle) | `planned` |
| Application lifecycle | Creating or opening, activating and deactivating, pausing and resuming, saving and closing as you see them. Closing an application handles unsaved content explicitly. Closing a view is separate from stopping its jobs: a job belongs to the document session and continues or is cancelled according to an explicit background policy | krepi supervisor (application registration and lifecycle), krepi job service | `planned` |
| Settings and system screens | A settings application for keys and key repeat, the display, the brightness and sleep policy and the language; sessions; installing and registering applications; browsing documents and files. The display server is the authority for input settings; the settings will be kept in a system configuration area of krepi's storage, which krepi is asked to provide | krepi storage, session and power services | `planned` |
| Device management | Screens for managing the device: file organization, storage capacity and clean-up, battery level and charging state, backlight and automatic sleep, controlled shutdown and restart, system update and recovery. Each screen shows only states that krepi has confirmed; krepi carries out every action, and kosmo shows the state and passes on your requests | krepi storage, power and update services | `planned` |
| Usage restrictions | A visible indication of a restricted mode, for example one that turns off the CAS, program execution or access to existing documents, and a way to check entering and leaving it. krepi enforces the restriction at every service entry; hiding a menu entry in kosmo is not a restriction. The indication is drawn by the display server in a reserved screen area that no application can draw over. This is a product feature and implies no approval for any examination | krepi policy and session services | `planned` |

### First-party applications

| Capability | What kosmo will provide | Builds on | Status |
| --- | --- | --- | --- |
| Calculator | The calculator application, built through the SDK like any other application: one-line input with a result view and history first, then two-dimensional input. The Scratchpad is part of it, as its mode without a document. It is the first application on kosmo's display server | kosmo framework, kalyx | `planned` |
| Scratchpad and discovery | A Scratchpad that switches between calculation and graph; history and reuse of subexpressions; undo; formula templates; a function catalog with argument help; context menus and settings. Calculation and graph share a temporary context that never changes a document; Scratchpad work is saved as a document only when you ask, and variable conflicts are resolved at that point | krepi document service, kalyx | `planned` |
| Compound documents and split views | Documents made of problems, pages and application regions; reordering and copying pages; layouts with several views on one page, where a first release may show one application per page full screen; saving and reopening restores source content, layout and variable scopes | krepi document service | `planned` |
| Linked values | Variables, functions and data shared by every region of one problem; formulas, graphs, tables, measurements, notes and sliders updating each other; edits, including dragging a graph or geometry object, sent as semantic edits that the document service commits; dependency cycles reported; undo; problems isolated by default, with values from another problem only through an explicit import or library reference | krepi document service, kalyx | `planned` |
| Notes and scientific typesetting | Text, math boxes that can be recalculated or turned off, annotations, images and chemical formula typesetting; opening a graph page from a selected expression. Chemical boxes are typesetting only; they do not balance or solve reactions | kalyx, krepi document service | `planned` |
| Graph view | Graph pages in documents, in which the plotting application kamp shows its plots, drawn with the framework's coordinate plane control and its versioned viewports. Each pan or zoom fixes the expression, parameter environment, viewport and output size as a new version, and older samples can be discarded; point counts are bounded; undefined regions and discontinuities are kept, and curves are never joined across an asymptote | kamp, kalyx | `planned` |
| Table, geometry and proof views | The framework's table grid, diagnostics list and proof-goal view, and the document regions in which the geometry application kyklo and the formal workbench krino show their views. They share selection, clipboard, undo and the command framework with the other views and keep their own editing models | krepi document service, kyklo, krino | `planned` |
| Programming | Python editing, execution and an interactive terminal; user functions, programs and reusable libraries called from a document; further languages later. Python units with side effects run only when you run them, never because a view redraws or a variable changes. Output is bounded. The stop control first requests cancellation and ends the isolated worker when its limit is exceeded | krepi language runtimes, job service, terminal devices and command shell | `planned` |
| Interactive widgets | Reusable sliders, canvases and interactive experiment or simulation pages; scripts that read the document variables they are granted and receive events; installation and registration, permission grants, drawing budgets, saving and restoring. Scripts draw only on restricted canvases of kosmo's display protocol. The script language has not been chosen | krepi script runtime and permissions; kosmo display protocol (restricted canvases) | `planned` |

### Display compatibility for Linux desktop programs

| Capability | What kosmo will provide | Builds on | Status |
| --- | --- | --- | --- |
| Linux desktop programs | Unmodified Linux desktop programs on kosmo's display server: a Wayland compatibility service, one instance for each program or security context, which is an ordinary client of kosmo's protocol and stays outside the trusted parts of the display server; X11 programs, each through its own rootless Xwayland; D-Bus with a few portals, such as the file chooser and screenshot authorization; pointer input for these programs; their windows in the shell; native backends for SDL3 and winit. Qt and GTK programs run unmodified this way instead of being ported. This part will live in a separate repository for compatibility and ports | krepi Linux compatibility (Unix sockets, descriptor passing, sealed shared memory); Linux user-space components such as Xwayland and the D-Bus daemon, which krepi is asked to package | `planned` |

## Rules every view follows

- Drawing reads only view models. Rendering never runs a computation;
  computations are versioned jobs that return typed results: mathematical
  values, plot data, output text or diagnostics.
- Data flows from input to the editing model and its commands, to a versioned
  job, to a computation backend, to a result for the matching version, to a
  view model and then to rendering. Editors, the terminal, graph pages and
  proof regions use the same lifecycle and events.
- A result or diagnostic is shown only against the source version it was
  computed from. Text diagnostics carry the source file or unit, the source
  version and a half-open range of UTF-8 bytes; the math editor uses stable node
  positions, which kosmo maps to the screen.
- Computation backends never draw on the screen. A program that needs its own
  graphics gets a restricted canvas whose size, command count, buffer size and
  lifetime kosmo's display server controls.
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
| Geometry exploration | Construct a triangle and dependent objects; drag a vertex; measurements and loci update; a degenerate position reports an error; after recovering, editing continues | The coordinate plane and the region that hosts the geometry application, drags as semantic edits, the error display | `planned` |
| Science experiment | Capture sensor data; data columns with units; fit, residuals and graph; a Notes explanation; export to a computer. Disconnections and dropped samples keep their quality marks | Table and graph views, Notes, visible quality marks. Capture and export are provided by krepi and other casξ components | `planned` |
| Programming and proof | Run Python or a proof job in a document; read versioned diagnostics; stop the job when it exceeds its limit; the GUI stays editable; the source content and an explicit result state are saved | Code editor, terminal view, diagnostics, the stop control | `planned` |

## First integration target

kosmo will first be integrated with krepi once krepi runs interactively on
the Raspberry Pi 5 with a development display and a USB keyboard. There
kosmo's display server will hold krepi's display and input device services: a
calculation is entered into the one-line calculator on a physical keyboard,
and its result appears on the display. Editing, error display, long-press and
modifier keys and cancellation are checked, together with input and display
during a long calculation and the rejection of stale results. kosmo's own
tests will run the same scenarios on a host, through the headless backend and
the session and device port suite; the results on the device will come from
krepi's tests.

Two examples will run there:

1. A derivative of $x^2$ is requested, the input is changed to $x^3$ and
   submitted again. The old job can be cancelled, and the old result, even when
   it arrives late, belongs to the old source version only and never replaces
   the new result.
2. A job in an isolated worker prints output continuously or loops; a test
   worker comes first, and Python and proof jobs come later. The GUI stays
   editable, output is bounded, the region's stop control first requests
   cancellation and ends the worker when its limit is exceeded, and the
   document keeps its source content and reports the final state of the job.
   A minimal program application hosts the region.

Panning and zooming a graph is not part of this integration; it comes with the
graph view.

## Open questions

- **Screen and keyboard.** The device comes in two forms, a landscape handheld
  and a clamshell. The screen and keyboard of each form are not chosen; no key
  count or size is fixed.
- **Display protocol details.** The object model, wire format, version
  negotiation, errors and transport of the first protocol version are
  provisional. They will be reviewed when krepi settles its side of the
  display path: the development display path, the form of the drivers and the
  hardware side of rendering.
- **Math font and image formats.** The math font will be chosen with
  two-dimensional typesetting, and image formats with the features that need
  them. The rasterizer, the text stack, the determinism rules and the two
  interface fonts are chosen; the
  [architecture](architecture.md#rendering-text-and-determinism) describes
  them.
- **Widget script language.** Not chosen. Lua is a reference.
- **Response times.** No numeric targets for start-up, cancellation or response
  time have been set. Each target is set before the measurement that checks
  it, together with the long-press duration of the reserved stop key; until
  then, tests count deterministic work instead of timing it.
- **TI-Nspire compatibility.** The capabilities draw on the TI-Nspire CX II CAS
  handheld as a reference. Reading or writing its `.tns` documents, TI-Basic and
  the TI Lua API are open compatibility questions and are not planned features.
