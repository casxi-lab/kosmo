# Repository instructions

- kosmo's programs use `std`; crates that the layout table marks pure core
  are `no_std` with `alloc`. The framework reaches the screen and input only
  through its platform interface and its two backends, the headless backend
  and the display server backend; a host window appears only through the
  nested display server. The display server reaches devices only through
  its device ports, which krepi will implement on the device on top of its
  display and input device services. Keep the code above the platform
  interface and the device ports the same on a host and on the device, and
  keep platform types out of core libraries, as the platform boundary in
  `docs/architecture.md` states.
- Drawing reads only view models. Rendering never runs a computation;
  computations are versioned jobs. Applications draw only into their own
  surfaces and submit them to kosmo's display server. Only the display server
  composes surfaces and presents them to outputs. On the device it does so
  through krepi's display device service: krepi's display driver allocates
  the scan-out buffers and carries out mode setting and scan-out, and the
  display server decides the mode policy.
- Keep the editing structure of a formula apart from the semantic expressions of
  the CAS. Incomplete input such as `1/□` stays representable, and
  simplification never changes the structure or the cursor being edited.
- Show a result or diagnostic only against the source, environment and settings
  versions it was computed from. A late result of an older version never
  replaces a newer one.
- Send changes to shared values, including drags in graph and geometry views,
  as semantic edits that the document service commits. Never write another
  view's objects directly.
- Keep input, editing and the stop control usable while jobs run. Output,
  plot points and messages are bounded. GUI caches are accounted apart from
  computation memory, and rebuildable caches are dropped first when memory is
  low.
- Usage restrictions are enforced by krepi services. kosmo shows them; hiding a
  menu entry is not a restriction.
- Write all documentation and code comments in English, from the point of view
  of a person who uses or builds on kosmo: what a feature does, how to use it,
  its limits, and commands that actually run. `docs/scope.md` holds the target
  scope and the status of each capability; mark anything that does not exist
  as planned.
- Keep milestone, task, proposal and decision numbers, capability and interface
  codes, stage codes, agent narratives, review histories and evidence ledgers
  in the project's coordination records outside this repository. Documents and
  comments here state the technical content:
  the rule, the condition that lifts a limit, or the module that will
  hold a planned item.
- Keep implementation, tests and fixtures here. Unit tests live in
  `#[cfg(test)]` modules next to the code they test; integration tests live in
  the `tests/` directory of the crate they test. Generated output belongs under
  the ignored `artifacts/` or `target/`.
- Use English Conventional Commit subjects of at most 72 characters and the
  local `.githooks/commit-msg` hook. Preserve history. Do not create remotes,
  tags or releases without authorization.
- kosmo's product code is divided into three Cargo workspaces, `display/`,
  `framework/` and `desktop/`, each with its own `Cargo.lock`, and the
  repository tasks are the separate `xtask/` workspace, as the source layout in
  `docs/architecture.md` describes. So far only `desktop/`, with the program
  `kosmo`, and `xtask/` exist; a workspace root is added with the workspace's
  first crate. There is no `Cargo.toml` at the repository root. `layout.toml`
  lists every crate, and a change that adds a crate registers it there in the
  same commit.
- Crates of this repository depend on each other only through `path`
  dependencies, only in the directions that the layout table allows, and with
  `version = "=<the version of kosmo>"`. External dependencies come only from
  crates.io, pinned with `=x.y.z`; no git dependencies, no paths outside the
  repository and no krepi crates. A change that adds, removes or updates an
  external dependency updates the crate's allow entries in `layout.toml` and
  the workspace's allowlist in the same commit; the deps step of
  `cargo xtask check` prints the entries to add, replace or remove.
- Before committing, run `cargo xtask check`, `git diff --check` and
  `git diff --cached --check` and review the staged diff. CI runs
  `cargo xtask check` on every pull request to `main` and every push to
  `main`.
