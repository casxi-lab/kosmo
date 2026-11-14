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
2. `fetch` and `deps`: the packages of each workspace, downloaded once and
   then checked offline, as
   [the fetch and deps steps](#the-fetch-and-deps-steps) describes;
3. `fmt`, `clippy` with warnings as errors, and `test --locked`, in each
   workspace that `WORKSPACES` in `xtask/src/check.rs` lists: `xtask` and
   `desktop`;
4. `docs`: relative links and anchors in the Markdown files;
5. `commits`: the header of every commit message in `<base>..HEAD` keeps to
   the rules of the commit message hook. The base is the commit that the
   environment variable `CHECK_BASE` names; when it is unset, empty or all
   zeros, the base is the merge base of `origin/main` and `HEAD`;
6. `workflows`: `.github/workflows/` holds only regular files, and every
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
the later steps run with `CARGO_NET_OFFLINE=true`. The `layout`, `docs` and
`workflows` steps read only files that Git tracks, and so does the `deps` step
for the files it reads itself, so build output and ignored files do not change
the result; add a new file to the index with `git add` before running the
check.

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
`<workspace>_ms`; for the `fetch` step and the `cargo metadata` commands of
the `deps` step, the duration of each command as `<workspace>_<target>_ms`;
for the `deps` step also the observations that its section lists), the first
line of `rustc --version`, the runner image (`ImageOS`, `ImageVersion`) and
the GitHub Actions run (`GITHUB_RUN_ID`, `GITHUB_RUN_ATTEMPT`,
`GITHUB_EVENT_NAME`, `GITHUB_REF`, `GITHUB_SHA`); a value the environment
does not provide is `null`.

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
What Cargo resolves from the manifests is checked by the deps step, not by this
one.

A change that adds a crate registers it in `layout.toml` with its directory,
layer and marks, adds it to the `members` of its workspace root, and declares
it in the root's `[workspace.dependencies]` when other crates depend on it.
The first crate of a workspace also adds the workspace's root manifest,
`Cargo.lock` and allowlist and adds the workspace to `WORKSPACES`.

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
  [licenses of dependencies](#licenses-of-dependencies) describes.

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
entries nor the licenses of the workspace's crates.io packages, whose
exceptions it holds, are checked.

It also reports, without failing, these facts of the receipt: `drift`, the
crates.io packages whose versions differ between the workspaces, `xtask`
included, or `none`; and for each workspace `<workspace>_members`, the number
of its members, `<workspace>_exports`, of its export crates in the table,
`<workspace>_crates_io`, of its crates.io packages, and
`<workspace>_internal_edges`, of the distinct edges between crates of the
repository, counted once per kind.

An allow entry is a `[[crates.<name>.external]]` table in `layout.toml`, for
example:

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
together; `crates`, the crates of this repository whose crates.io dependencies
bring the package in: each member, or crate of `layout.toml` reached through a
`path` dependency, from which a chain of crates.io packages alone leads to it;
and `duplicate`, why the workspace resolves several versions of the package.
Both lists are sorted. A workspace without crates.io packages has an allowlist
without entries. The license exceptions of the workspace follow the entries.

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

#### Limits of the fetch and deps steps

The fetch and deps steps have limits:

- They see what Cargo resolves for the targets in `layout.toml`. A package
  that only other platforms use is neither listed nor checked. Build
  dependencies and procedural macros run on the host; they are resolved for
  the listed targets, which include the host of CI, `aarch64-unknown-linux-gnu`,
  and `aarch64-apple-darwin`.
- Resolved features are those that Cargo's resolver gives a package for all
  its dependents in the resolution together, dev-dependencies included, so
  they can be more than a normal build turns on.
- `cargo xtask` builds xtask before any step runs, so the build scripts and
  procedural macros of xtask's own dependencies run before the check reads
  `xtask/allowlist.toml` and checks their licenses.
- The license rules read only the `license` of each package, not its license
  files.
- The allowlists record whether each package has a build script, but the
  deps step does not judge build scripts yet.
- The checks find dependencies that were added by mistake. They cannot stop a
  build script, a procedural macro or configuration outside the repository
  that hides a change on purpose.

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

The layout step checks how the manifests declare dependencies, and the deps
step checks what Cargo resolves from them: the sources, the exact requirements
and the allowed packages of each crate and workspace, as
[the fetch and deps steps](#the-fetch-and-deps-steps) describes. A change that
adds, removes or updates an external dependency updates the allow entries of
the crate in `layout.toml` and the allowlist of the workspace in the same
commit; the deps step prints the entries to add, replace or remove.

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
