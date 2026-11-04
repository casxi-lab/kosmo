# Contributing

kosmo is newly initialized. It has no Cargo package yet, so there is nothing to
build or test. These are the commands that apply today:

```sh
git config --local core.hooksPath .githooks
git diff --check
git diff --cached --check
```

The first command enables the commit message hook in `.githooks/commit-msg`.
The other two report whitespace errors in unstaged and staged changes.

## Building and checking

kosmo will be an ordinary Rust program that uses `std`. The change that adds
the Cargo package also adds `Cargo.lock`, the check entry point and the
commands to run, and updates this section and [AGENTS](AGENTS.md) to match.
Continuous integration for this repository is planned to require
`cargo xtask check`, as in krepi; that command does not exist here yet.

## Commit messages

Commit messages use English Conventional Commits with a header of at most 72
characters, for example `docs(scope): describe the graph view`. The accepted
types are `feat`, `fix`, `docs`, `style`, `refactor`, `perf`, `test`, `build`,
`ci`, `chore` and `revert`. Do not bypass the hook or change global Git
configuration.

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

## Private repositories

This repository is public. Documents, code comments and commit messages do not
name or link the project's private repositories. Refer to their content in
neutral terms (the coordination records, the CAS project) and give only commit
hashes and paths where a reference must be traceable. The documentation check
rejects lines that match the case-insensitive pattern
`casxi-(system|cas|build)|krepi-h[i]l|erthoaers[/]`.

## License

kosmo is released under the [MIT License](LICENSE). The copyright holders are
the casξ Authors, listed in [AUTHORS](AUTHORS). Contributions are accepted
under the same license.
