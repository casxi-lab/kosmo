# Repository instructions

- Build kosmo as an ordinary Rust program that uses `std`. It runs as a host
  preview and on krepi through the GUI bridge. Keep the platform adaptation
  apart from the document, command, focus, job-protocol and layout code, so
  that the preview and the device share that code.
- Drawing reads only view models. Rendering never runs a computation and never
  drives a display device; computations are versioned jobs, and kosmo draws into
  surfaces that the display service owns.
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
  `#[cfg(test)]` modules next to the code they test; integration tests live
  under `tests/`. Generated output belongs under the ignored `artifacts/` or
  `target/`.
- Use English Conventional Commit subjects of at most 72 characters and the
  local `.githooks/commit-msg` hook. Preserve history. Do not create remotes,
  tags or releases without authorization.
- The repository has no Cargo package yet, so no build or test command exists.
  Before committing, run `git diff --check` and `git diff --cached --check` and
  review the staged diff. The change that adds the Cargo package also adds its
  check command to `CONTRIBUTING.md` and to these instructions.
