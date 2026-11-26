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
workspaces `display/`, with the shared types `kosmo-types`, `framework/`, with
the libraries `kosmo-math-model`, `kosmo-jobs`, `kosmo-render`,
`kosmo-platform`, `kosmo-testkit`, `kosmo-math-kalyx`, `kosmo-ui`,
`kosmo-backend-headless` and `kosmo-scratchpad` and the facade
`kosmo-sdk`, and `desktop/`, with the program `kosmo`, and the repository tasks
in `xtask/`, run as `cargo xtask`.
`cargo run --locked --manifest-path desktop/Cargo.toml -p kosmo` runs the
program's line-mode terminal, and `cargo xtask check` runs these steps in
order and stops at the first one that fails:

1. `layout`: the source layout, as [the layout step](#the-layout-step)
   describes;
2. `goldens`: the pixel goldens against their manifests, as
   [the goldens step](#the-goldens-step-and-cargo-xtask-golden) describes;
3. `fetch` and `deps`: the packages of each workspace, downloaded once and
   then checked offline, as
   [the fetch and deps steps](#the-fetch-and-deps-steps) describes;
4. `fmt`, `clippy` with warnings as errors, and `test --locked`, in each
   workspace that `WORKSPACES` in `xtask/src/check.rs` lists: `xtask`,
   `display`, `framework` and `desktop`. `clippy` and `test` then build each
   package that `FEATURE_PASSES` lists once more with its feature on, so far
   `kosmo-backend-headless` with `test-hooks`: the workspace's command builds
   it without its test hooks and skips the test targets whose
   `required-features` name them, and this pass builds the hooks and runs
   those targets;
5. `docs`: relative links and anchors in the Markdown files;
6. `commits`: the header of every commit message in `<base>..HEAD` keeps to
   the rules of the commit message hook. The base is the commit that the
   environment variable `CHECK_BASE` names; when it is unset, empty or all
   zeros, the base is the merge base of `origin/main` and `HEAD`;
7. `workflows`: `.github/workflows/` holds only regular files, and every
   `.yml` or `.yaml` file there, in any subdirectory, is UTF-8 and one YAML
   document with scalar keys, without anchors, aliases, merge keys, repeated
   keys or characters that YAML parsers read differently. No workflow
   mentions `pull_request_target` in any letter case, not even in a comment
   or through an escape; every job sets its own `permissions`; and in a
   workflow that runs on `pull_request`, no job requests write access and
   nothing outside `on`, neither the workflow `env` nor any job, names
   `secrets` in an expression, an `if` condition or a key, even inside a
   string literal.

The `fetch` step is the only one that uses the network: the Cargo commands of
the later steps run with `CARGO_NET_OFFLINE=true`. The `layout`, `goldens`,
`docs` and `workflows` steps read only files that Git tracks, and so does the
`deps` step for the files it reads itself, so build output and ignored files
do not change the result; add a new file to the index with `git add` before
running the check.

Every workspace denies the Clippy lints `unwrap_used`, `expect_used`, `panic`,
`todo`, `unimplemented` and `allow_attributes_without_reason`, and
`display/`, `framework/`, `desktop/` and `xtask/` also forbid unsafe code. The root
`clippy.toml` lets tests unwrap, expect and panic: code in `#[cfg(test)]`
modules and in `#[test]` functions. `todo!()` is rejected in tests too. A
helper function in `tests/` outside a `#[test]` function returns a `Result`
or carries an `#[allow]`, and every `#[allow]` states its reason with
`reason = "..."`. The product workspaces also deny `disallowed_methods` and
`disallowed_types`, whose lists the root `clippy.toml` holds, as
[the determinism lints](#the-determinism-lints) describes.

`cargo xtask check --receipt <path>` also writes a JSON receipt of the run to
`<path>`, whether the check passes or fails. It records the commit, whether
the working tree was clean before and after the run, each step with its
status, duration and the values it observed (for the `fmt`, `clippy` and
`test` steps, the duration of each workspace's command in milliseconds, as
`<workspace>_ms`, and of each feature pass, as `<package>_<feature>_ms`; for the `fetch` step and the `cargo metadata` commands of
the `deps` step, the duration of each command as `<workspace>_<target>_ms`;
for the `deps` step also the observations that its section lists; for the
`goldens` step, the number of goldens that the manifests list, as
`goldens`), the first line of `rustc --version`, the runner image
(`ImageOS`, `ImageVersion`) and the GitHub Actions run (`GITHUB_RUN_ID`,
`GITHUB_RUN_ATTEMPT`, `GITHUB_EVENT_NAME`, `GITHUB_REF`, `GITHUB_SHA`); a
value the environment does not provide is `null`.

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
crate that exists with its directory, layer, marks and allow entries. Its
comments describe each field. The `layout` step reads it and the tracked files,
without running Cargo, and reports every problem it finds, one per line as
`<path>[:<line>]: <message>`, sorted by path and then by line number. Problems
at a place in a Rust file and in a table that does not parse carry a line
number; problems in manifests and problems of a whole file, such as a missing
attribute of a library root, do not. It checks:

- the table: it parses, without unknown keys; layer identifiers are unique
  across workspaces; each crate's layer exists, its directory lies inside the
  layer's workspace, and no two crates share a directory; the last part of
  each crate's path is the crate name without the prefix `kosmo-`, or the
  whole name when it does not start with the prefix, and a crate elsewhere
  fails with the directory where it belongs; `uses` names known
  workspaces; a `provisional` condition is not empty; a facade is an export
  crate; no crate of a layer marked `publish = false` is publishable;
  `same-layer` lists crates of the same layer; `platform` lists at least one
  known platform category, each once, and no pure-core crate has it; no
  workspace or tool directory is the repository root or lies inside another;
  each `lock-allowlist` lies inside its workspace; every path is relative to
  the repository root and written without `.`, `..`, empty parts or a
  trailing `/`; `targets` lists at least one target platform, each once. The
  layers of each workspace and their
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
  manifest with the crate's name, which declares neither `[workspace]` nor
  `package.workspace`, so the crate belongs to the root of its workspace;
  the `members` of a root list exactly its crates, without patterns;
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
- kalyx: no crate of the table other than the kalyx adapter
  `kosmo-math-kalyx`, and not the tool workspace `xtask`, declares the
  package `kalyx`, in any dependency table, `[target.*]` forms and the
  `[workspace.dependencies]` of `xtask` included. The package is found by its
  name, as Cargo finds it: for an entry inherited with `workspace = true`, the
  `package` that its declaration in the root renames, or else its key,
  whatever `package` the entry itself names; for any other entry, the
  `package` that it renames, or else its key. So a renamed `kalyx` fails here
  as well as being a rename. A crate that depends on `kosmo-math-kalyx`
  reaches kalyx through it, which this rule does not check;
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
- the Clippy configuration (also without a table): the only Clippy
  configuration is the regular file `clippy.toml` at the repository root.
  Clippy reads the nearest one at or above a crate's directory, so a
  `clippy.toml` or `.clippy.toml` anywhere else, in any letter case, fails,
  and so does a `.clippy.toml` at the root;
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
- export crates: an export crate has no direct external dependency;
- the platform boundary: a library crate, one with a `[lib]` table or a
  tracked `src/lib.rs`, is a core library unless its entry lists platform
  categories in `platform`, the platform boundary column, which a crate
  without a library target leaves out. In every Rust file
  under the `src/` of a library crate, unit tests and inline modules
  included, no path, macro or type that the
  [platform boundary](docs/architecture.md#platform-boundary) forbids is
  used, unless one of the crate's categories allows it; the comments of
  `layout.toml` list what each category allows. The step expands `use`
  trees with their renames and glob imports, follows `extern crate` renames,
  type aliases, paths from the crate root, paths that start with `self` or,
  in an `impl`, with `Self`, and qualified paths such as
  `<kalyx::Context>::new`, lets items and `macro_rules!` macros hide the
  names they define, and reports each use with its file and line: an import
  of a forbidden item, a path that names one or an item inside one, and a
  call of a forbidden macro. Compile-time `env!`, comments, doc comments and string
  literals are not uses;
- the deterministic path: the crates that the architecture names on the
  path, `kosmo-types`, `kosmo-ui`, `kosmo-render`, `kosmo-text`,
  `kosmo-controls`, `kosmo-math-controls` and `kosmo-compositor`, are marked
  `deterministic` when the table lists them. In every Rust file in the
  directory of a crate that the table marks `deterministic`, in any of its
  targets, no lint attribute `allow`, `expect` or `warn`, also inside
  `cfg_attr`, in the bodies of macros and when written with raw identifiers
  such as `r#allow`, names `clippy::disallowed_methods` or
  `clippy::disallowed_types`, their former names `clippy::disallowed_method`
  and `clippy::disallowed_type`, or the groups `clippy::style` and
  `clippy::all` that hold them. The one
  exception is a single unconditional inner attribute `#![allow(...)]` or
  `#![expect(...)]` of `clippy::disallowed_methods` alone at the start of
  `src/det_math.rs` of `kosmo-render`, the wrapper module that calls libm,
  which declares no module in another file and includes no file; the
  exception covers every listed method in the module and in the inline
  modules inside it, so review reads that the module only forwards to
  `libm`; a deterministic crate keeps its files as regular files, has no
  build script, names in its manifest only `.rs` files under its directory
  as the roots of its targets, and `include!` and `#[path]` in it bring in
  only `.rs` files under its directory, so that, outside the limits below,
  the step reads every file that the crate compiles;
- the test clock: tests take time only from an injected clock, so no test
  code of a crate of the table or of a tool workspace calls
  `std::thread::sleep` or `std::time::SystemTime::now`. Test code is every
  Rust file under a crate's `tests/`, `benches/` and `examples/`; under its
  `src/`, the items under `#[test]` or under a `cfg` attribute whose
  condition names `test` outside `not(...)`, as `#[cfg(test)]` and
  `#[cfg(all(test, unix))]` do, all of a module with an inner
  `#![cfg(test)]`, and the files of the modules that these declare or
  include, with the modules that those declare or include in turn; the
  whole `src/` of `kosmo-testkit` and of the crates of layers D4 and F4;
  and the doc tests under `src/`: the fenced code blocks of doc comments,
  written with `///`, `//!`, `/** */` or `#[doc = "..."]`, that rustdoc
  runs as Rust, without a tag or with only `rust`, `compile_fail`,
  `no_run`, `should_panic` and the other tags of Rust tests, but not
  `ignore`, read with their hidden lines. The uses are found as the
  platform boundary check finds them, so imports, renames and glob imports
  are followed, an import of either function is reported as well as each
  use, comments and string literals do not match, and `SystemTime` used as
  a type is not a use. Each use is reported with its file and line, and so
  is a doc test whose tokens cannot be read;
- call probes and test hooks: in every manifest of the product workspaces,
  a dependency on `kosmo-testkit` other than a dev-dependency is optional
  and turned on only by the crate's feature `call-probes`, as
  `call-probes = ["dep:kosmo-testkit"]`; no dependency declaration,
  dev-dependencies and the `[workspace.dependencies]` of the roots
  included, turns on the `call-probes` of a crate, as
  `features = ["call-probes"]` or `features = ["kosmo-ui/call-probes"]`
  would; none but a dev-dependency, and the dependency of `kosmo-testkit` on
  `kosmo-platform`, turns on the `test-hooks` of `kosmo-platform` or of a
  backend, a crate whose name starts with `kosmo-backend-`; and the default
  features of a crate, expanded through its feature table, turn on neither
  its own `call-probes` or `test-hooks` nor `kosmo-testkit` or the
  `call-probes` or `test-hooks` of a dependency. So call probes are
  compiled only when the command line turns them on, and test hooks only
  then and in tests;
- the shape of the workspace roots: a product workspace root sets
  `resolver = "3"`, sets the six Clippy lints above and `disallowed_methods`
  and `disallowed_types` to `deny` or `forbid` in `[workspace.lints.clippy]`
  and the groups that hold them, `restriction`, `style` and `all`, to
  neither `allow` nor `warn`, writes every key of `[workspace.lints.clippy]`
  with `_` and the lints above under their current names, since Clippy
  reads `disallowed-methods` as `disallowed_methods`, `disallowed_method` as
  `disallowed_methods` and `option_unwrap_used` as `unwrap_used`, sets
  `overflow-checks = true` in `[profile.release]` and `false` nowhere in
  `[profile]`, package overrides and `build-override` included, and has no
  `exclude` or `default-members`;
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
checked. Lint attributes are found by their own names too, so one that a
macro assembles from parts or a procedural macro adds is not found, nor is
one in a crate that the table does not mark `deterministic`, also when a
`macro_rules!` macro of such a crate carries it into a deterministic crate
that calls the macro, or brings in, with a literal `include!` that rustc
resolves from the file that calls the macro, a file that the step does not
read. The rule of the wrapper module reads only the tokens
of `src/det_math.rs`, so a module declaration or an `include!` that a macro
defined in another file expands inside the wrapper module is not found,
and the module's exception covers the code that it brings in; review reads
the macros that deterministic crates call. The test clock check has
the limits of the platform boundary check and counts the lines of a test
item, from its first attribute to its end, as test code, other code on
those lines included. It finds test targets by their directories, not by
a `path` of `[[test]]`, `[[bench]]` or `[[example]]` elsewhere, and does
not read indented code blocks, which rustdoc also runs, doc comments that a
macro writes, as `#[doc = include_str!("...")]` does, or a test attribute
that `cfg_attr` adds. It cannot see
what build scripts, procedural macros or configuration outside the
repository, such as `$CARGO_HOME/config.toml`, `RUSTFLAGS` or
`CLIPPY_CONF_DIR`, change, and a changed `xtask` alias no longer runs the
check at all; review covers these.
What Cargo resolves from the manifests is checked by the deps step, not by this
one.

The platform boundary check reads tokens, not the names that the compiler
resolves. It does not find a forbidden item in a path that a macro assembles
from parts, as in `$krate::fs` in a `macro_rules!` body; in an item that a
crate reaches through a re-export, whether another crate's or its own, as in
`clock::Context::new` after `pub use kalyx::Context;` in a module `clock`,
through a path that starts with `crate` or `super`, through `self::` in a
block that has an item of that name, which the step reads in the block
instead of its module, or through a type alias with generic parameters;
through an `extern crate` rename in another file or an import in the body of
a macro call; through a glob import whose path goes through another glob
import of the same module or block, directly or through a rename, as
`use time::*;` after `use std::*;`; in a path from the crate root right after
a comparison `>` that follows a `<` in the same statement; or in code that
`include!` or `#[path]` brings in from outside `src/`. It reads only the
`src/` of a crate, even when `[lib] path` puts the library root elsewhere,
and does not check a crate without a library target. A path of one segment
does not say whether it names a type or a value, so a function, constant or
static hides a forbidden type of the same name, a module, type or trait
hides a forbidden value of the same name, as `mod UNIX_EPOCH` beside
`use std::time::*;`, and an imported name hides a forbidden macro of the
same name. The other way round, the step reports
some code that uses no forbidden item. It counts a name that a module
imports, one by one or with a glob import, also in the modules inside it,
and does not tell a path from a name that is not one: a variable,
parameter, field, enum variant or generic parameter with the name of an
item that an import of its module or of an outer module brings in, as the
variant `Instant` beside `use std::time::*;`, is reported as that item. It
does not know the macros that `#[macro_use]` brings out of a module, so a
call of such a macro with the name of a forbidden macro is reported as that
macro. Renaming one of them resolves it. Review covers these. The step
reads nested brackets with one level of recursion for each level of
nesting, so a file whose brackets nest some thousands of levels deep stops
the check with a stack overflow, which fails the check rather than passing
it.

The source layout gives the export crates of `display/` no external
dependency at all, not even through other crates, and the layout step checks
only the external dependencies that each crate declares itself. For an export
crate of `display/` that is pure core, as `kosmo-types` is, the rest follows
from its checks: a pure-core crate depends only on pure-core crates, and no
pure-core crate declares an external dependency. For an export crate of
`display/` that is not pure core, review checks the crates it depends on.

A change that adds a crate puts it in a directory of its workspace named
after the crate without the prefix `kosmo-`, such as `framework/render` for
a crate `kosmo-render`, or after the whole name when it does not start with
the prefix: crate names keep the prefix for publishing on crates.io, and
directories need not. The change registers the crate in `layout.toml` with
its directory, layer and marks, with its platform categories when it is
a library crate at the platform boundary and with `deterministic = true`
when it writes layout coordinates, layout trees, display lists, glyph
positions, coverage or pixels, adds its directory to the
`members` of its workspace root, and declares it in the root's
`[workspace.dependencies]`, under its name and with the path of its
directory, when other crates depend on it.
The first crate of a workspace also adds the workspace's root manifest,
`Cargo.lock` and allowlist and adds the workspace to `WORKSPACES`.

### The goldens step and `cargo xtask golden`

A crate with pixel goldens, so far `kosmo-render`, keeps them in its
directory `tests/golden/`: a file `<name>.px` for each golden, which holds a
drawn buffer as the [digests rule](docs/architecture.md#determinism-rules)
encodes it, and `MANIFEST`, with one line `<digest>  <name>` for each
golden: the SHA-256 of the file in 64 lowercase hexadecimal digits, two
spaces and the name, sorted by name. Names are lowercase letters, digits and
`-`. A test refers to a golden by calling `matches_golden` with the name as
a string literal; the call draws the golden's scene, encodes it in memory and
compares it with the file byte for byte. These commands write and verify the
goldens:

```sh
cargo xtask golden
cargo xtask golden --verify
```

The first runs the ignored test `write_goldens` of each crate with goldens,
which writes its `.px` files, and then writes `MANIFEST` from the `.px`
files in `tests/golden/`; run without a change to the drawing code, it
leaves every file as it was. It removes the `.px` files of the goldens
before it runs the test, so the file of a golden whose scene is gone goes
too, and it fails when the test writes no golden. When it fails, `MANIFEST`
is as it was, the files it removed stay removed and those that the test
wrote stay written, until it runs again without failing or
`git checkout -- <crate>/tests/golden` restores them. Add a new golden's
file with `git add`. The second runs the ignored test `redraw_goldens`, which
draws the goldens again into `target/golden/<package>/` of the workspace,
and prints for each golden the new digest and the digest in `MANIFEST`. It
fails when one differs and changes no tracked file. Both leave out files
whose names start with `.`, such as `.DS_Store`. The ignored tests are run
only through these commands.

The `goldens` step reads the tracked files of the golden directories and of
the crates with goldens, without running Cargo, and reports every problem it
finds, one per line as `<path>[:<line>]: <message>`, sorted by path and then
by line number:

- a golden whose file has another SHA-256 than `MANIFEST` lists;
- a golden of `MANIFEST` without a file, or that no test refers to;
- a `.px` file that `MANIFEST` does not list, and any other file in a golden
  directory but `MANIFEST`;
- a line of `MANIFEST` that is not a digest of 64 lowercase hexadecimal
  digits, two spaces and a name, a name listed twice and lines out of order;
- a call of `matches_golden` with a name that `MANIFEST` does not list, and
  a call in `tests/golden.rs` outside its top-level tests;
- a crate with goldens whose `tests/golden.rs` lacks the ignored tests
  `write_goldens` and `redraw_goldens`, ignores another top-level test, or
  has a `cfg` or `cfg_attr` attribute on itself or on a top-level function
  or module, since the tests of the goldens run on every target and only
  its top-level tests compare goldens;
- a directory `tests/golden` of a crate that `cargo xtask golden` does not
  write.

It finds the calls of `matches_golden` in the tokens of the crate's Rust
files, so calls in comments and string literals do not count, nor do calls
whose first argument is not a string literal. A golden directory without
`MANIFEST` lists no goldens. The step does not check that a test returns
or checks the result of each call, nor that the crate's manifest runs
`tests/golden.rs`, which `autotests = false` without a `[[test]]` table
for it would turn off; review covers these. In CI, the goldens step
compares every file with `MANIFEST` and the test step every drawing with
its file, byte for byte, so the digests of what CI draws are those of
`MANIFEST`.

### The fetch and deps steps

The `fetch` step downloads the packages that each workspace of the check needs:
the tool workspace `xtask` and each product workspace with crates in
`layout.toml`, in this order, for each target platform in the workspace's
`targets`. For each workspace and target it runs

```sh
cargo fetch --locked --manifest-path <workspace>/Cargo.toml --target <target>
```

It runs every command, also after one fails, and reports each failure with
Cargo's message.

The `deps` step runs, for the same workspaces and targets,

```sh
CARGO_NET_OFFLINE=true cargo metadata --locked --offline --all-features \
    --filter-platform <target> --format-version 1 \
    --manifest-path <workspace>/Cargo.toml
```

and takes the union of the resolutions of a workspace's targets by package id.
Normal, build and dev dependencies all count, optional dependencies of members
count as if their features were on, and a package that only other platforms
use, such as a dependency under `cfg(windows)`, is not part of the union
although `Cargo.lock` records it. On the union of each workspace it checks:

- sources: every package is a member of the workspace, a crate of
  `layout.toml` reached through a `path` dependency from a product workspace,
  or a package whose source is exactly crates.io,
  `registry+https://github.com/rust-lang/crates.io-index`, which Cargo reports
  for crates.io with the sparse protocol too. The directions below decide, edge
  by edge, whether a crate of the table may be reached: a crate of another
  workspace that is not an export crate may be reached through an export crate
  of its own workspace, such as a facade that re-exports it, but no crate of
  the workspace being checked may depend on it directly. Git packages, packages
  of other registries, `path` packages that are not crates of the table, inside
  or outside the repository, and any crate of the table in the tool workspace
  `xtask` fail, and are not checked further. No package is named `krepi` or
  starts with `krepi-` or `krepi_`. Every crates.io package has a checksum in
  the workspace's `Cargo.lock`;
- exact requirements: every dependency of a member on a crates.io package, in
  any dependency table, has the requirement `=x.y.z`, with an optional
  pre-release or build suffix, as `cargo metadata` reports it (`"0.13"` in a
  manifest is reported as `^0.13`);
- directions: every resolved edge between crates of the table, for each
  target, follows the directions that the layout step checks on the declared
  edges, and the resolved graph, dev-dependencies included, has no cycle;
- allow entries: the direct dependencies of each crate of the table on
  crates.io packages are exactly its `external` entries in `layout.toml`,
  matched by package, kind and target condition and compared in requirement,
  whether the dependency is optional, default features, the features that the
  declaration turns on and the resolved features of the package. The tool
  workspace `xtask` has no allow entries; its allowlist alone lists its
  packages;
- allowlists: the crates.io packages of the union are exactly the entries of
  the workspace's allowlist, the file that its `lock-allowlist` names, with the
  fields below. A package that the workspace resolves in several versions,
  over all its targets together, has a `duplicate` reason on the entry of each
  version, and only such a package has one;
- licenses: every member has the license of a kosmo crate, compared as
  written: `MIT`, or `MIT AND OFL-1.1` when the crate has font assets, that is,
  when a `.ttf`, `.otf`, `.ttc`, `.woff` or `.woff2` file, in any letter case,
  is tracked in its directory outside the directories of other packages. The
  license expression of every crates.io package is in the allowed set, or the
  workspace's allowlist has a license exception for it, as
  [licenses of dependencies](#licenses-of-dependencies) describes;
- build scripts: the build script of a crates.io package declares no `links`
  and has no build dependency that compiles native code, looks for or binds to
  system libraries or can download, unless the package's allowlist entry
  states the exception, as
  [build scripts of dependencies](#build-scripts-of-dependencies) describes.

The step reports every problem it finds, one per line as
`<path>[:<line>]: <message>`, sorted by path and then by line number; the line
is given for an allowlist whose tables do not have the fields below. When the
allow entries of a crate differ from what Cargo resolves, it also prints every
`external` entry that the crate should have, to replace all its entries in
`layout.toml`, or says that they all go. When an allowlist differs, it prints
the entries to add, each replacing the entry with the same name and version if
there is one, and the entries to remove. The printed entries keep the reasons
of the entries they replace; the other entries and the license exceptions stay
as they are. A workspace for which `cargo metadata` fails is reported with
Cargo's message, indented on the lines that follow, and not checked further. An
allowlist that is missing or cannot be read is reported, and neither its
entries nor the licenses and build scripts of the workspace's crates.io
packages, whose exceptions it holds, are checked.

It also reports, without failing, these facts of the receipt: `drift`, the
crates.io packages whose versions differ between the workspaces, `xtask`
included, or `none`; and for each workspace `<workspace>_members`, the number
of its members, `<workspace>_exports`, of its export crates in the table,
`<workspace>_crates_io`, of its crates.io packages, and
`<workspace>_internal_edges`, of the distinct edges between crates of the
repository, counted once per kind.

An allow entry is a `[[crates.<name>.external]]` table in `layout.toml`. For
example, the crate `kosmo-render` has this entry for `libm`:

```toml
[[crates.kosmo-render.external]]
name = "libm"
version = "=0.2.16"
default-features = false
features = []
resolved-features = ["arch", "default"]
```

Its fields are the package `name`; the requirement `version`; `kind`,
`"build"` or `"dev"`, left out for a normal dependency; `target`, the
condition of a `[target.<condition>.*]` table, left out otherwise;
`optional = true` for an optional dependency, left out otherwise;
`default-features`; `features`, the features that the declaration turns on
after inheritance from the workspace root; and `resolved-features`, the
features of the package after resolution, for all targets of the workspace
together. Both lists are sorted.

An allowlist has one `[[package]]` table for each package, for example:

```toml
[[package]]
name = "serde"
version = "1.0.229"
source = "crates.io"
license = "MIT OR Apache-2.0"
build-script = true
features = ["alloc", "derive", "serde_derive"]
crates = ["xtask"]
```

Its fields are `name`; `version`, as `Cargo.lock` records it, build metadata
included; `source`, always `crates.io`; `license`, the license expression of
the package's manifest as written, left out when it has none; `build-script`,
whether it has a build script; `proc-macro = true` for a procedural-macro
library, left out otherwise; `links`, the value of `links` in its manifest,
which usually names the native library that its build script links, left out
when it declares none; `features`, its resolved features for all targets
together, which an allow entry calls `resolved-features` (its `features` are
those that the declaration turns on); `crates`, the crates of this repository
whose crates.io dependencies bring the package in: each member, or crate of
`layout.toml` reached through a `path` dependency, from which a chain of
crates.io packages alone leads to it; and `duplicate`, why the workspace
resolves several versions of the package. Both lists are sorted. Three more
fields state the exceptions of a build script with their reasons:
`links-exception`, `native-code-exception` and `download-exception`. A
workspace without crates.io packages has an allowlist without entries. The
license exceptions of the workspace follow the entries.

#### Licenses of dependencies

The license expression of a crates.io package, its `license` as
`cargo metadata` reports it, is parsed in the strict mode of the `spdx` crate
at the version that `xtask/Cargo.toml` pins, 0.13.6, with the SPDX License List
3.29.0 that it carries; the check has no parser of its own. The allowed set is
`MIT`, `Apache-2.0`, `BSD-2-Clause`, `BSD-3-Clause`, `ISC`, `Zlib` and
`Unicode-3.0`.

- In the allowed set: a term is in the set when it is one of these
  identifiers, without `+` and without `WITH`. `A OR B` is in the set when one
  of its operands is, and `A AND B` when both are. Parentheses group; without
  them, `+` binds tighter than `WITH`, `WITH` than `AND`, and `AND` than `OR`,
  so `A OR B AND C` reads as `A OR (B AND C)`. `and`, `or` and `with` in lower
  case are the same operators. So `MIT OR Apache-2.0`,
  `(MIT OR Apache-2.0) AND Unicode-3.0`, `Apache-2.0 OR GPL-3.0-only` and
  `Apache-2.0 WITH LLVM-exception OR MIT` are in the set.
- Valid, but outside the set: every other identifier of the list, a term with
  `WITH` or `+`, such as `Apache-2.0 WITH LLVM-exception` or `Apache-2.0+`, and
  `LicenseRef-`, `DocumentRef-` and `AdditionRef-` terms.
- Not valid: identifiers in another letter case or spelling, such as `mit` or
  `Apache 2.0`; deprecated identifiers, such as `GPL-2.0`; operators in mixed
  case, such as `Or`; and the legacy form of crates.io with `/`, such as
  `MIT/Apache-2.0`.

A package whose expression is outside the set or not valid, or that has none
(only a `license-file`, or neither), needs a license exception in the allowlist
of each workspace that resolves it, for example:

```toml
[[license-exception]]
name = "arraydeque"
version = "0.5.1"
license = "MIT/Apache-2.0"
reading = "MIT OR Apache-2.0"
reason = "The legacy form of crates.io for a choice of licenses, in which `/` reads as OR."
```

Its fields are `name` and `version`, as in the package's entry; `license`, the
expression as written, left out for a package without one; `reading`, how the
workspace reads the license, as an expression in the allowed set; and `reason`.
The legacy form, an expression that is not valid as written but is once each
`/` reads as `OR`, has a reading when that expression is in the allowed set,
and the reading is that expression, with each `/` written as ` OR `. The two
are compared after parsing: the letter case of the operators does not matter,
nor do parentheses that leave the grouping as it is. Any other exception may
leave the reading out. The step fails when an exception has no reason, records
another expression than the package declares or has a reading that breaks these
rules, when its package is not resolved for the workspace's targets, and when
the package's expression is in the set. An exception holds for one version of
one package in one workspace.

#### Build scripts of dependencies

For each crates.io package with a build script, the step reads two things from
`cargo metadata`: its `links`, and the packages that its build dependencies
bring in, through every kind of dependency of those packages, for the
workspace's targets. It fails when:

- the package declares `links`, which usually names the native library that its
  build script links; a package that declares it for another reason needs the
  exception too;
- they include `cc`, `cmake`, `cxx-build` or `autotools`, which compile C or
  C++, `nasm-rs`, which assembles native code, `bindgen`, which generates
  bindings to C headers, or `pkg-config`, `vcpkg` or `system-deps`, which look
  for system libraries;
- they include `reqwest`, `ureq`, `curl`, `attohttpc`, `minreq`, `http_req`,
  `hyper`, `isahc` or `git2`, with which a build script can download at build
  time.

Each failure names the build dependency, and the chain of packages that leads
to it. The package's allowlist entry may state an exception to each of the
three, with its reason, in `links-exception`, `native-code-exception` or
`download-exception`. An exception without a reason fails, and so does an
exception that the package does not need. The step judges no other build
script; review reads them.

#### Limits of the fetch and deps steps

The fetch and deps steps have limits:

- They see what Cargo resolves for the targets in `layout.toml`. A package
  that only other platforms use is neither listed nor checked. Build
  dependencies and procedural macros run on the host, but they are resolved
  for the listed targets, `aarch64-unknown-linux-gnu`, the host of CI, and
  `aarch64-apple-darwin`; build dependencies that only another host, such as
  an x86-64 one, uses are not seen.
- The fetch step downloads the packages for the listed targets only, and the
  later Cargo commands run offline. On a host that is not one of them, such as
  an x86-64 or a Windows one, `clippy` and `test` fail when they need a package
  that only that host uses, such as one under `cfg(windows)`; run
  `cargo fetch --locked --manifest-path <workspace>/Cargo.toml` once, without
  `--target`, before the check.
- `cargo metadata` runs with `--all-features`, so every feature of every
  member is on: the resolution has every optional dependency of a member, and
  the resolved features include those that only features off by default turn
  on. Resolved features are those that Cargo's resolver gives a package for
  all its dependents in the resolution together, dev-dependencies included,
  so they can be more than a normal build turns on. The resolution also has
  an optional dependency of a crates.io package that only a weak dependency
  feature of the package names, such as `glifo?/libm` in the `libm` feature
  of vello_cpu, although no feature turns the dependency on: the allowlist
  lists it and the packages that only it brings in, the features that they
  turn on count in the resolved features of the packages they depend on,
  and the build compiles none of those packages.
- Cargo compares a package with the checksum in `Cargo.lock` only when it
  downloads it. It does not compare again the sources that it unpacked
  into a local `CARGO_HOME` or the `.crate` files that it keeps there, so a
  changed cache changes a local build without failing a step. CI starts
  from an empty `CARGO_HOME`, and a local comparison of golden digests
  uses a fresh one.
- `cargo xtask` downloads and builds xtask before any step runs, outside the
  fetch step, so the build scripts and procedural macros of xtask's own
  dependencies run before the check reads `xtask/allowlist.toml` and checks
  their licenses and build scripts.
- The license rules read only the `license` of each package, not its license
  files. The build-script rules read only `links` and the names of the build
  dependencies: a build script that runs a compiler or a download tool itself,
  opens a network connection, prints `cargo:rustc-link-lib` without `links`,
  or reads environment variables to use data from outside the repository is
  not found, nor is a package that does such work under a name the lists do
  not have. Procedural macros also run at build time; the allowlists mark
  them with `proc-macro = true`, but the step does not judge them. Review
  covers these.
- The build-script rules cover crates.io packages. Which members may have a
  build script is set per crate in `layout.toml`, as the layout step checks,
  and their build dependencies are allow entries there; review reads what
  their build scripts do.
- The checks find dependencies that were added by mistake. They cannot stop a
  build script, a procedural macro or configuration outside the repository
  that hides a change on purpose.

### The determinism lints

Clippy's lints `disallowed_methods` and `disallowed_types`, with the lists of
the root `clippy.toml`, keep deterministic code to the rules of
[transcendental functions](docs/architecture.md#transcendental-functions)
and [hash containers](docs/architecture.md#determinism-rules). Each product
workspace denies both in `[workspace.lints.clippy]`, so the `clippy` step
fails on a call of a listed method or function and on a use of a listed
type in any crate of `display/`, `framework/` and `desktop/`, tests
included; in `xtask`, which reads the same file, the lints keep their
default level, warn, which `-D warnings` of the step makes an error.
Clippy reports by the path that the compiler resolves: a method call such
as `x.sin()`, a path call such as `f32::sin(x)` or `<f32>::sin(x)`, a
function as a value such as `f32::sin`, and a type through any re-export,
such as `std::collections::hash_map::HashMap`; a method of the same name of
another type is not reported. The lists are:

- methods of `f32` and `f64`, in `std` and in `core` outside its unstable
  module `math`, whose documentation has an "Unspecified precision"
  section, calls its result non-deterministic or refers to algebraic
  operators: in toolchain 1.99.0 the 41 methods of each type that the
  [transcendental functions](docs/architecture.md#transcendental-functions)
  list, among them the unstable `gamma`, `ln_gamma`, `erf` and `erfc`;
- every function that `libm` 0.2.16 exports, 124 of them;
- the types `std::collections::HashMap`, `std::collections::HashSet`,
  `std::hash::RandomState` and `libm::Libm`, through which the functions
  of `libm` can be called too.

The layout table marks the crates of the deterministic path
`deterministic`, and in their Rust files the layout step allows no
attribute that lowers either lint, except one in the wrapper module
`det_math` of `kosmo-render`, the one place that calls `libm`, as
[the layout step](#the-layout-step) describes. A crate off the
deterministic path may allow them with a reason.

The methods and functions are generated, in this order, by this command,
run from the repository root with the `rust-src` component of the pinned
toolchain (`rustup component add rust-src`) and with `libm` 0.2.16 in
Cargo's registry, which the fetch step downloads:

```sh
library="$(rustc --print sysroot)/lib/rustlib/src/rust/library"
for type in f32 f64; do
    awk -v type="$type" '
        /^pub mod math / { exit }
        /^ *\/\/\// { if (!doc) text = ""; doc = 1; text = text $0 "\n"; next }
        { doc = 0 }
        /^ *pub (const )?fn / {
            if (text ~ /# Unspecified precision/) why = "its precision is unspecified and differs between hosts; deterministic code calls det_math in kosmo-render"
            else if (text ~ /algebraic-operators/) why = "its result is not deterministic; deterministic code uses the basic operations"
            else if (text ~ /non-deterministic/) why = "it may return either of two inputs that compare equal, such as +0.0 and -0.0; deterministic code compares explicitly"
            else why = ""
            if (why != "") {
                match($0, /fn [a-z0-9_]+/)
                printf "    { path = \"%s::%s\", reason = \"%s\" },\n", type, substr($0, RSTART + 3, RLENGTH - 3), why
            }
        }
        /^ *pub (const )?fn / || /^ *$/ || /^ *}/ { text = "" }
    ' "$library/std/src/num/$type.rs" "$library/core/src/num/$type.rs"
done
set -- "${CARGO_HOME:-$HOME/.cargo}"/registry/src/*/libm-0.2.16/src/math/mod.rs
sed -n 's/^pub use self::[a-z0-9_]*::{*\([a-z0-9_, ]*\)}*;$/\1/p' "$1" | tr ',' '\n' | tr -d ' ' |
    awk '{ printf "    { path = \"libm::%s\", reason = \"deterministic code calls libm only through det_math in kosmo-render\" },\n", $0 }'
```

Its output, one entry per line, replaces the entries of `disallowed-methods`
in `clippy.toml`; the four types are written by hand. A pull request that
upgrades the toolchain or `libm` runs the command again and states the
versions in the comment above the list. The tests of the lints are in
`xtask/src/layout/lints/fixture.rs`: they write a crate that denies both
lints into `xtask/target/`, which Git ignores, and run the pinned Clippy on
it with the root `clippy.toml`, offline. A method call, a path call and a
function pointer of methods with unspecified precision, `algebraic_add`,
`f32::max`, `libm::sinf` outside a module that expects the lint and
`std::collections::HashMap`, also through a re-export, are each reported
once, the method `sin` of a type of the crate is not, and every entry of
the lists is reported where the crate names it, except the four unstable
methods, which stable code cannot name. Another test checks that the lists
hold exactly the 41 methods of `f32` and of `f64` and the 124 functions of
libm, so that a regenerated list that drops an entry fails.

The lints have limits:

- Clippy reads the nearest `clippy.toml` at or above a crate's directory,
  or the directory that `CLIPPY_CONF_DIR` names; configuration outside the
  repository can make it read another file, and review covers that.
- Clippy warns about an entry whose crate resolves but whose item does
  not, and `-D warnings` makes that an error. It ignores without a warning
  an entry whose crate does not resolve, such as the functions of `libm`
  in a crate that does not reach it, and a method that `f32` or `f64` does
  not have, among them their methods of `std` in a `no_std` crate, whose
  code cannot call them; the fixture names every entry, so an entry that
  names nothing fails its test.
- The command finds the methods by the words of their documentation; a
  method of a new toolchain that states an unspecified or
  non-deterministic result in other words is not found, so the pull
  request of the upgrade reads the new methods of `f32` and `f64`.
- Only `libm` among the math crates is listed. The methods of traits of
  dependencies that compute transcendental functions, such as the
  `FloatFuncs` trait of kurbo, which vello_cpu re-exports, and the hash
  containers of other crates, such as hashbrown, are not; a direct
  dependency on a crate such as core_maths or hashbrown needs an allow
  entry, which review judges.
- Clippy does not check the examples in documentation comments, which
  `cargo test` runs as doc tests, so a doc test that calls a listed method
  passes the `clippy` step; review covers them.
- The lints see kosmo's code only. Floating point inside dependencies is
  examined when a library is chosen, and the features of the raster
  library and the text stack are compared by the deps step, as the rules
  of [golden images and determinism](#golden-images-and-determinism) state.

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
used. kalyx 0.1.0 comes from crates.io too, and only the kalyx adapter crate
`kosmo-math-kalyx` declares it, as the layout step checks. A change that moves
kosmo to another kalyx version also replaces the copy of kalyx's answer
vectors that
[`framework/math-kalyx/tests/data/README.md`](framework/math-kalyx/tests/data/README.md)
describes; no step of the check compares the copy byte for byte with the
package.

The layout step checks how the manifests declare dependencies, and the deps
step checks what Cargo resolves from them: the sources, the exact requirements,
the allowed packages of each crate and workspace, and their licenses and build
scripts, as [the fetch and deps steps](#the-fetch-and-deps-steps) describes. A
change that adds, removes or updates an external dependency updates the allow
entries of the crate in `layout.toml` and the allowlist of the workspace,
with the license and build-script exceptions that the new packages need, in
the same commit; the deps step prints the entries to add, replace or remove.

Security advisories are checked by hand before each version is tagged,
pre-release versions such as `0.1.0-dev.1` included: a maintainer checks the
crates.io packages in every `Cargo.lock` of the repository against the RustSec
advisory database and records, with the release decision, the commit of the
advisory database that was used. A weekly advisory job, not required for
merging, will be added with the framework's text stack; with the framework's
raster library, the allowlists of the product workspaces together list more
than about 20 distinct packages, each name and version counted once however
many allowlists list it, the packages of the tool workspace `xtask` not
counted. The job will change neither the `cargo xtask check` job nor the
branch rules of `main`.

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

### Golden images and determinism

Rendering is tested against pixel goldens, and code on the deterministic
path keeps to these rules, each following the paragraph of the
architecture that it links:

1. Deterministic code calls transcendental functions only through
   `det_math` in `kosmo-render`, which calls `libm` 0.2.16, uses otherwise
   only the basic floating-point operations, `sqrt`, `mul_add` and the
   operations whose result is fully determined, and never calls a method of
   unspecified precision, an `algebraic_*` method or a floating-point `min`,
   `max` or `clamp`, as [the determinism lints](#the-determinism-lints) check
   ([transcendental functions](docs/architecture.md#transcendental-functions)).
2. The raster library and the text stack are built without `std` and with
   the same `libm`, and the deps step compares their resolved features with
   the allow entries and the allowlists, which detects a change such as
   `std` turned on but passes once the entries are rewritten as the step
   prints them, so review rejects `std` and threading features in such a
   change ([transcendental functions](docs/architecture.md#transcendental-functions)).
3. The SIMD level of the pixel and text paths depends only on the
   compilation target and is given to the raster library explicitly, and
   builds add neither `-C target-cpu` nor `-C target-feature`
   ([SIMD and code generation](docs/architecture.md#determinism-rules)).
4. Deterministic code uses ordered containers and never `HashMap`,
   `HashSet` or `RandomState` of the standard library, also through
   re-exports, as the determinism lints check, and the hash containers
   inside dependencies are covered by comparing golden digests across
   separate processes ([hash containers](docs/architecture.md#determinism-rules)).
5. The deterministic path starts no threads, the threading features of the
   libraries stay off, and no package of the rayon or crossbeam families is
   allowed ([threads](docs/architecture.md#determinism-rules)).
6. Colors are 8-bit sRGB-encoded constants without color management, which
   the raster library composites with its `u8_pipeline`, and RGB565 is
   produced only by the conversion function of `kosmo-types` when a buffer is
   submitted, without dithering
   ([color](docs/architecture.md#determinism-rules)).
7. Fonts come only from the asset directory of `kosmo-text` through
   `ResourceProvider`, with system font discovery off
   ([font sources](docs/architecture.md#determinism-rules)).
8. Output depends only on input: nothing reads the locale or environment
   variables, options are constants, and records hold no wall-clock time
   ([environment](docs/architecture.md#determinism-rules)).
9. Layout trees, display lists and their serialization hold no NaN and no
   infinity ([finite values](docs/architecture.md#determinism-rules)).
10. Pixel goldens come only from the CPU rasterizer and are compared by the
    SHA-256 of the buffer description and the raw bytes, without image
    encoding, as [the goldens step](#the-goldens-step-and-cargo-xtask-golden)
    describes ([digests](docs/architecture.md#determinism-rules)).
11. The toolchain and the chosen libraries are pinned exactly, and each
    upgrade is a pull request of its own that updates the goldens by
    category with `cargo xtask golden` and, for the toolchain, generates the
    lists of the determinism lints again
    ([pinning](docs/architecture.md#determinism-rules)).
12. Pixel goldens have zero tolerance, and the check on GitHub-hosted runners
    is authoritative: the digests that `cargo xtask golden --verify` prints
    on a local host, run with a fresh `CARGO_HOME` and reported with whether
    the volume of the checkout is case-sensitive, are compared with those of
    CI one by one, which are those of `MANIFEST`, as
    [the goldens step](#the-goldens-step-and-cargo-xtask-golden) states for
    CI, and a local digest that differs stops the work and goes to the
    maintainers, without taking CI's digest instead
    ([authority](docs/architecture.md#determinism-rules)).

## License

kosmo is released under the [MIT License](LICENSE). The copyright holders are
the casξ Authors, listed in [AUTHORS](AUTHORS). Contributions are accepted
under the same license.
