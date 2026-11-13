# Contributing

kosmo's source is divided into Cargo workspaces, as the
[source layout](docs/architecture.md#source-layout) describes. Before proposing
a change, enable the commit message hook once and run these commands from the
repository root:

```sh
git config --local core.hooksPath .githooks
cargo xtask check
git diff --check
git diff --cached --check
```

The first command enables the commit message hook in `.githooks/commit-msg`.
`cargo xtask check` runs the required checks that the next section describes.
The last two report whitespace errors in unstaged and staged changes.

## Building and checking

The Rust toolchain is pinned in `rust-toolchain.toml`, and rustup installs it
on first use. Each workspace has its own `Cargo.lock`: so far the product
workspace `desktop/` with the program `kosmo`, and the repository tasks in
`xtask/`, run as `cargo xtask`. Run the program with
`cargo run --locked --manifest-path desktop/Cargo.toml -p kosmo -- --version`.
`cargo xtask check` runs these steps in order and stops at the first one that
fails:

1. `layout`: the source layout, as [the layout step](#the-layout-step)
   describes;
2. `fmt`, `clippy` with warnings as errors, and `test --locked`, in each
   workspace that `WORKSPACES` in `xtask/src/check.rs` lists: `xtask` and
   `desktop`;
3. `docs`: relative links and anchors in the Markdown files;
4. `commits`: the header of every commit message in `<base>..HEAD` keeps to
   the rules of the commit message hook. The base is the commit that the
   environment variable `CHECK_BASE` names; when it is unset, empty or all
   zeros, the base is the merge base of `origin/main` and `HEAD`;
5. `workflows`: `.github/workflows/` holds only regular files, and every
   `.yml` or `.yaml` file there, in any subdirectory, is UTF-8 and one YAML
   document with scalar keys, without anchors, aliases, merge keys, repeated
   keys or characters that YAML parsers read differently. No workflow
   mentions `pull_request_target` in any letter case, not even in a comment
   or through an escape; every job sets its own `permissions`; and in a
   workflow that runs on `pull_request`, no job requests write access and
   nothing outside `on`, neither the workflow `env` nor any job, names
   `secrets` in an expression, an `if` condition or a key, even inside a
   string literal.

The `layout`, `docs` and `workflows` steps read only files that Git tracks, so
build output and ignored files do not change the result; add a new file to the
index with `git add` before running the check.

Every workspace denies the Clippy lints `unwrap_used`, `expect_used`, `panic`,
`todo`, `unimplemented` and `allow_attributes_without_reason`, and `desktop/`
and `xtask/` also forbid unsafe code. The root `clippy.toml` lets tests unwrap,
expect and panic: code in `#[cfg(test)]` modules and in `#[test]` functions.
`todo!()` is rejected in tests too. A helper function in `tests/` outside a
`#[test]` function returns a `Result` or carries an `#[allow]`, and every
`#[allow]` states its reason with `reason = "..."`.

`cargo xtask check --receipt <path>` also writes a JSON receipt of the run to
`<path>`, whether the check passes or fails. It records the commit, whether
the working tree was clean before and after the run, each step with its
status, duration and the values it observed (for the `fmt`, `clippy` and
`test` steps, the duration of each workspace's command in milliseconds, as
`<workspace>_ms`), the first line of
`rustc --version`, the runner image (`ImageOS`, `ImageVersion`) and the GitHub
Actions run (`GITHUB_RUN_ID`, `GITHUB_RUN_ATTEMPT`, `GITHUB_EVENT_NAME`,
`GITHUB_REF`, `GITHUB_SHA`); a value the environment does not provide is
`null`.

CI runs the check as the job `cargo xtask check` of
`.github/workflows/check.yml` on a GitHub-hosted `ubuntu-24.04-arm` runner,
for the head commit of every pull request to `main` and for every push to
`main`. It sets `CHECK_BASE` to the base commit of the pull request or to the
commit before the push, installs the toolchain of `rust-toolchain.toml` with
rustup, and uploads the receipt as the artifact
`check-receipt-<commit>-<attempt>`.

`cargo xtask check` has a time budget in CI: its target is 10 minutes and its
upper limit is 20 minutes. The budget is not a timeout, and the job sets no
timeout of its own. A run in which the check takes longer than 16 minutes,
80% of the upper limit, is reported to the maintainers; the receipt shows
which step and workspace took the time. When the check comes close to the
limit, the number of cases of fuzz tests and bounded exhaustive tests is
lowered first and caching is added. Steps are added to the check without
renaming the job or changing the branch rules of `main`. Splitting the job or
making another CI job required for merging needs the maintainers' approval,
and the branch rules of `main` are changed together with the pull request
that does it.

### The layout step

`layout.toml` at the repository root is the layout table: the product
workspaces with their layers, the workspaces each one uses, their target
platforms and `lock-allowlist` locations, the tool workspace `xtask`, and every
crate that exists with its directory, layer and marks. Its comments describe
each field. The `layout` step reads it and the tracked files, without running
Cargo, and reports every problem it finds, one per line as
`<path>[:<line>]: <message>`, sorted by path and then by line number. Problems
at a place in a Rust file and in a table that does not parse carry a line
number; problems in manifests and problems of a whole file, such as a missing
attribute of a library root, do not. It checks:

- the table: it parses, without unknown keys; layer identifiers are unique
  across workspaces; each crate's layer exists, its directory lies inside the
  layer's workspace, and no two crates share a directory; `uses` names known
  workspaces; a `provisional` condition is not empty; a facade is an export
  crate; no crate of a layer marked `publish = false` is publishable;
  `same-layer` lists crates of the same layer; no workspace or tool directory
  is the repository root or lies inside another; each `lock-allowlist` lies
  inside its workspace; every path is relative to the repository root and
  written without `.`, `..`, empty parts or a trailing `/`; `targets` lists at
  least one target platform, each once. The layers of each workspace and their
  order, as the [source layout](docs/architecture.md#source-layout) lists
  them, `uses`, `publish = false` on layers D4 and F4 (conformance and
  integration tests), `facade-only` on layer S1 (first-party applications) and
  the crate `kosmo` in layer S2 (the program) are as the source layout fixes
  them, so changing them in the table fails until the rules change. When the
  table does not parse or is not consistent, only the checks marked "also
  without a table" below run;
- workspace roots: there is no `Cargo.toml` at the repository root (also
  without a table); each product workspace with crates in the table has a
  tracked root manifest that declares `[workspace]` and no `[package]`, and a
  `Cargo.lock`; a product workspace without crates has no root manifest; each
  tool workspace has a `Cargo.lock`, and no other directory has one. The
  crates of a workspace whose root manifest is missing or declares no
  `[workspace]` are not checked further;
- members: every tracked file whose name is `Cargo.toml` in any letter case is
  spelled exactly `Cargo.toml` and is a workspace root, a tool manifest or the
  manifest of a crate of the table; each crate of the table has a tracked
  manifest with the crate's name; the `members` of a root list exactly its
  crates, without patterns;
- the form of the manifests: members inherit every dependency from the
  `[workspace.dependencies]` of their root with `workspace = true`, adding at
  most `features` and `optional`. There, an external dependency has an exact
  version, `version = "=x.y.z"` with an optional pre-release or build suffix,
  and a `path` dependency names a crate of the table and has
  `version = "=<the version of kosmo>"`; a crate of the table is declared only
  with its `path`. A tool workspace has the same exact versions, no `path`
  dependency and no dependency on a crate of the table, also in its
  `[workspace.dependencies]`. No dependency uses `git`, `branch`, `tag`,
  `rev`, `registry`, `registry-index` or `package`, and no manifest has
  `[patch]` or `[replace]` or the spellings `dev_dependencies`,
  `build_dependencies`, `default_features`, `proc_macro` and `crate_type`;
- directions: every internal dependency that a manifest declares, in
  `[dependencies]`, `[dev-dependencies]`, `[build-dependencies]` and their
  `[target.*]` forms under any condition, follows the directions of the
  source layout. A crate of another workspace is an export crate of a
  workspace that `uses` names, and a facade when the dependent's layer is
  marked `facade-only`; a crate of the same workspace is in a lower layer, or
  in the same layer and listed in the dependent's `same-layer`; a pure-core
  crate depends only on pure-core crates. Each crate that `same-layer` lists
  is a dependency, and the internal dependency graph, dev-dependencies
  included, has no cycle;
- the repository boundary: no package or dependency is named `krepi` or starts
  with `krepi-` or `krepi_`; no `path` and no symbolic link leaves the
  repository (symbolic links also without a table); there is no submodule
  (also without a table). Symbolic links, the literal paths of `include!`,
  `include_str!`, `include_bytes!` and `#[path = "..."]` in Rust files,
  `#[path]` inside `cfg_attr`, an inner `#![path]` at the start of an inline
  module written out in the source and names written as raw identifiers such
  as `r#path` included, and the target paths and `build` of manifests stay
  inside their workspace, or inside the repository outside the workspaces; an
  absolute path leaves the repository. A `#[path]` inside an inline module is
  resolved from the module's directory, and a `#![path]` like a `#[path]` on
  the module; in a file other than a `mod.rs` file, which may be a crate root
  or a module file, both of the directories that rustc may use are checked.
  The `workspace`, `readme` and `license-file` of `[package]` and the `readme`
  and `license-file` of `[workspace.package]` do not leave the repository.
  Paths are joined as written, without following symbolic links, so none of
  these paths, no dependency `path`, no `members` entry and no target of a
  symbolic link (also without a table) passes through a tracked symbolic link
  before its last part. Every tracked `.rs` file can be read as Rust tokens,
  so a fixture that is not Rust does not end in `.rs`;
- the Cargo configuration (also without a table): the only Cargo
  configuration is the regular file `.cargo/config.toml`, which holds only
  `[alias]` with the single alias `xtask`; a `.cargo` entry or configuration
  file anywhere else, in any letter case, fails;
- pure-core crates: everything under `src/` is a regular file, not a symbolic
  link; the library root has `#![no_std]`, not behind `cfg_attr`, and
  `#![forbid(unsafe_code)]`; the library root and every file that `include!`
  or `#[path = "..."]` brings in are `.rs` files under `src/`, and the
  directory that a `#[path]` or an inner `#![path]` gives an inline module
  lies under `src/`; there is no build script and no external dependency in
  any dependency table; no Rust file under `src/`, tests included, contains
  the names `Cell`, `RefCell`, `UnsafeCell`, `OnceCell`, `cell` or `atomic`,
  raw identifiers and macro bodies included, so not even a local variable is
  named `cell`; `extern crate std` appears only with `#[cfg(test)]` directly
  on the `extern crate` item, not merely inside a `#[cfg(test)]` module.
  `alloc` may be used;
- build scripts and procedural macros: a crate has a build script
  (`package.build` names a file, or `build.rs` is tracked and `build` is not
  `false`) or is a procedural-macro library (`[lib]` sets `proc-macro = true`
  or lists `proc-macro` in `crate-type`) only when the table marks it
  `build-script` or `proc-macro`, and a crate with such a mark has one; a tool
  workspace has neither;
- versions and publishing: the table lists `kosmo`, which has a version;
  every crate has the version of `kosmo`, after inheritance from
  `[workspace.package]`; `publish` resolves to `false` exactly for the crates
  that the table does not mark publishable;
- provisional crates: no pure-core crate depends on a crate marked
  `provisional`, through any dependency table;
- export crates: an export crate has no external dependency;
- the shape of the workspace roots: a product workspace root sets
  `resolver = "3"`, sets the six Clippy lints above to `deny` or `forbid` in
  `[workspace.lints.clippy]` and the `restriction` group that holds them to
  neither `allow` nor `warn`, sets `overflow-checks = true` in
  `[profile.release]` and `false` nowhere in `[profile]`, package overrides
  and `build-override` included, and has no `exclude` or `default-members`;
  each member sets `[lints] workspace = true`;
- coverage: `WORKSPACES` lists exactly the tool workspaces and the product
  workspaces with crates.

The layout step has limits. It sees only tracked files, so a new manifest is
checked only after `git add`. It checks the dependencies that manifests
declare, not the graph that Cargo resolves, and counts a dependency under a
`[target.*]` condition for every platform and a `#[path]` inside `cfg_attr`
under any condition. It finds only literal paths and knows the macros and
attributes only by their own names: paths built by macros such as
`include!(concat!(...))`, and a macro imported under another name, as with
`use core::include_str as text;`, are not checked. Modules that a macro
generates are not recognized either, so the `#[path]` and `#![path]` of an
inline module such as `mod $name { ... }` in a `macro_rules!` body are not
checked. It cannot see what build scripts, procedural macros or configuration
outside the repository, such as `$CARGO_HOME/config.toml`, `RUSTFLAGS` or
`CLIPPY_CONF_DIR`, change, and a changed `xtask` alias no longer runs the
check at all; review covers these.
The sources, licenses and allowed packages of the dependencies are checked by
the planned dependency step, not by this one.

A change that adds a crate registers it in `layout.toml` with its directory,
layer and marks, adds it to the `members` of its workspace root, and declares
it in the root's `[workspace.dependencies]` when other crates depend on it.
The first crate of a workspace also adds the workspace's root manifest and
`Cargo.lock` and adds the workspace to `WORKSPACES`.

## Dependencies

Crates of this repository depend on each other only through `path`
dependencies, in the directions that the
[source layout](docs/architecture.md#source-layout) allows, with the common
version written as `version = "=<the version of kosmo>"`. External
dependencies come only from crates.io, each with an exact version,
`version = "=x.y.z"`, and the workspace's `Cargo.lock` records its checksum.
In a product workspace, each is declared once in the
`[workspace.dependencies]` of its root, and members inherit it with
`workspace = true`; the tool workspace `xtask` declares them in its own
manifest. Git dependencies, other registries, `path` dependencies outside the
repository, `[patch]` and `[replace]` sections and krepi's crates are never
used. kalyx will also come from crates.io, and only the kalyx adapter crate
will depend on it.

The layout step checks how the manifests declare dependencies. The planned
dependency step will check what Cargo resolves from them: the sources, the
allowed packages of each crate and workspace, and their licenses.

Security advisories are checked by hand before each version is tagged,
pre-release versions such as `0.1.0-dev.1` included: a maintainer checks the
crates.io packages in every `Cargo.lock` of the repository against the RustSec
advisory database and records, with the release decision, the commit of the
advisory database that was used. A weekly advisory job will be added once
the dependencies grow beyond about 20 packages, which is expected with the
framework's text stack. It will not be required for merging and will change
neither the `cargo xtask check` job nor the branch rules of `main`.

## Commit messages

Commit messages use English Conventional Commits with a header of at most 72
characters, for example `docs(scope): describe the graph view`. The accepted
types are `feat`, `fix`, `docs`, `style`, `refactor`, `perf`, `test`, `build`,
`ci`, `chore` and `revert`. Headers that start with `Merge `, `Revert `,
`fixup! ` or `squash! ` are accepted as they are. The `commits` step of the
check applies the same rules to the commits of a branch. Do not bypass the hook
or change global Git configuration.

## Documentation

Documentation and code comments are English and written for a person who uses
or builds on kosmo. A document states what a feature does, how to use it, its
limits, and commands that actually run. Nothing is described as working before
it does.

Mathematics in Markdown is written in LaTeX. Inline formulas are written as
`$...$` on one line, without a backslash followed by ASCII punctuation and
without any of `* | < > & ~`; display formulas go in a fenced block with the
info string `math`, never inside a list item, and `$$` is not used. Commands,
program input and output, identifiers, file paths and option values keep code
formatting. Rust doc comments use plain text, because rustdoc does not render
LaTeX.

[Scope](docs/scope.md) describes the complete target scope of kosmo, one row
per capability, with its status. A change that delivers a feature document,
at `docs/features/<area>/<feature>.md` with its interface, examples,
limitations and tests, sets the status of the matching row in the scope to
`partial` and links the document from that row. A row can link several
feature documents; it becomes `implemented` only when the whole capability
works and is tested as its feature documents describe.

Plans and their records belong to the project's coordination records, kept
outside this repository:
milestone and task numbers, proposal and decision numbers, capability and
interface codes, stage codes, agent narratives, review histories and evidence
ledgers. No document or code comment here uses them. A document states the
technical content:

- an item that does not exist yet is named by the module or file that will hold
  it and marked planned;
- a limit that waits for other work states the condition that lifts it;
- a decision is stated as the rule it made and where the rule applies.

## Tests

Unit tests live in `#[cfg(test)]` modules next to the code they test.
Integration tests of a crate live in that crate's `tests/` directory. The
integration tests of the framework as a whole will live in the unpublished
integration-test crates of `framework/` (layer F4), and the conformance tests
of the display protocol and the display server in the unpublished conformance
crates of `display/` (layer D4). Fixtures stay in the repository. Raw output
belongs under the ignored `artifacts/` or `target/`.

Test editing, layout, commands and focus, the filtering of stale results, and
interface regressions. Test the display protocol and the platform interface
against their conformance suites, rendering against golden images, and
interaction with the shell. Test errors, cancellation and resource limits
beside the successful paths.

Fuzz tests and bounded exhaustive tests take their number of cases from named
constants, so that a count can be lowered in one place when the check comes
close to its time budget.

A host run does not replace tests on the device: system-call isolation,
device drivers, memory limits and power loss are tested on krepi. Integration
tests that run on a host with the kalyx release pinned from crates.io belong
to kosmo; joint tests that connect kosmo to krepi's services or to the device
belong to krepi, and the results on the device come from krepi's tests.

Tests produce observations; they do not admit a release. The release decision
is recorded separately in the coordination records. Creating a tag or
publishing is a
separate authorized action.

## License

kosmo is released under the [MIT License](LICENSE). The copyright holders are
the casξ Authors, listed in [AUTHORS](AUTHORS). Contributions are accepted
under the same license.
