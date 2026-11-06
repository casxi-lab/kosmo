# Contributing

kosmo is newly initialized. kosmo itself has no code yet; the only Cargo
package is the repository-task workspace in `xtask/`. Before proposing a
change, enable the commit message hook once and run these commands from the
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
on first use. The repository tasks are a Cargo workspace of their own in
`xtask/`, with its own `Cargo.lock`, run as `cargo xtask`. `cargo xtask check`
runs these steps in order and stops at the first one that fails:

1. `fmt`, `clippy` with warnings as errors, and `test`, in the `xtask`
   workspace;
2. `docs`: relative links and anchors in the Markdown files;
3. `commits`: the header of every commit message in `<base>..HEAD` keeps to
   the rules of the commit message hook. The base is the commit that the
   environment variable `CHECK_BASE` names; when it is unset, empty or all
   zeros, the base is the merge base of `origin/main` and `HEAD`;
4. `workflows`: `.github/workflows/` holds only regular files, and every
   `.yml` or `.yaml` file there, in any subdirectory, is UTF-8 and one YAML
   document with scalar keys, without anchors, aliases, merge keys, repeated
   keys or characters that YAML parsers read differently. No workflow
   mentions `pull_request_target` in any letter case, not even in a comment
   or through an escape; every job sets its own `permissions`; and in a
   workflow that runs on `pull_request`, no job requests write access and
   nothing outside `on`, neither the workflow `env` nor any job, names
   `secrets` in an expression, an `if` condition or a key, even inside a
   string literal.

The `docs` and `workflows` steps read only files that Git tracks, so build
output and ignored files do not change the result; add a new file to the index
with `git add` before running the check.

`cargo xtask check --receipt <path>` also writes a JSON receipt of the run to
`<path>`, whether the check passes or fails. It records the commit, whether
the working tree was clean before and after the run, each step with its
status, duration and the values it observed, the first line of
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

kosmo will be an ordinary Rust program that uses `std`. The change that adds
its Cargo package also adds the package to the check and updates this section
and [AGENTS](AGENTS.md) to match.

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

[Scope](docs/scope.md) describes the complete target scope of kosmo, one row
per capability, with its status. When a capability is implemented, it gets one
feature document at `docs/features/<area>/<feature>.md` that describes its
interface, examples, limitations and tests, and the same change links that
document from its row in the scope and updates the row's status.

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
Integration tests live under `tests/`, and fixtures stay in the repository.
Raw output belongs under the ignored `artifacts/` or `target/`.

Test editing, layout, commands and focus, the filtering of stale results, and
interface regressions. Test errors, cancellation and resource limits beside the
successful paths. A host preview run does not replace tests on the device:
system-call isolation, device drivers, memory limits and power loss are tested
on krepi, and the joint tests that connect the real CAS and kosmo belong to
krepi.

Tests produce observations; they do not admit a release. The release decision
is recorded separately in the coordination records. Creating a tag or
publishing is a
separate authorized action.

## License

kosmo is released under the [MIT License](LICENSE). The copyright holders are
the casξ Authors, listed in [AUTHORS](AUTHORS). Contributions are accepted
under the same license.
