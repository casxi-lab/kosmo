//! Tests of the layout step.
//!
//! Most tests change one thing in a valid layout that has crates in all
//! three product workspaces and expect exactly the problems that the change
//! causes. A few change a second valid layout instead, where only `desktop/`
//! has a crate, the program `kosmo`, as in this repository so far. This
//! repository's own tree is checked as it is.

use std::collections::BTreeMap;

use super::*;

/// The workspaces and tools of the valid layout, as in `layout.toml`.
const WORKSPACES_TABLE: &str = r#"[workspaces.display]
path = "display"
uses = []
targets = ["aarch64-unknown-linux-gnu", "aarch64-apple-darwin"]
lock-allowlist = "display/allowlist.toml"
layers = [
    { id = "D0", name = "exports" },
    { id = "D1", name = "pure core" },
    { id = "D2", name = "server libraries" },
    { id = "D3", name = "server programs and backends" },
    { id = "D4", name = "conformance tests", publish = false },
]

[workspaces.framework]
path = "framework"
uses = ["display"]
targets = ["aarch64-unknown-linux-gnu", "aarch64-apple-darwin"]
lock-allowlist = "framework/allowlist.toml"
layers = [
    { id = "F0", name = "foundations and models" },
    { id = "F1", name = "core, adapters and backends" },
    { id = "F2", name = "controls" },
    { id = "F3", name = "composition and the facade" },
    { id = "F4", name = "integration tests", publish = false },
]

[workspaces.desktop]
path = "desktop"
uses = ["framework"]
targets = ["aarch64-unknown-linux-gnu", "aarch64-apple-darwin"]
lock-allowlist = "desktop/allowlist.toml"
layers = [
    { id = "S0", name = "shell and system interface" },
    { id = "S1", name = "first-party applications", facade-only = true },
    { id = "S2", name = "the program kosmo" },
]

[tools.xtask]
path = "xtask"
targets = ["aarch64-unknown-linux-gnu", "aarch64-apple-darwin"]
lock-allowlist = "xtask/allowlist.toml"
"#;

/// A crate of the valid layout: its name, workspace and layer, the marks
/// it has besides `export = false`, `pure-core = false` and
/// `publish = false`, and the crates it depends on.
type Spec = (
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static [&'static str],
);

/// The crates of the valid layout. The kalyx adapter `kosmo-math-kalyx`
/// also depends on kalyx, which the root of `framework/` declares. As in
/// this repository, `kosmo-jobs` is registered for threads and `kosmo` for
/// the standard streams, arguments, exit code and real clock.
const CRATES: [Spec; 14] = [
    (
        "kosmo-types",
        "display",
        "D0",
        "export = true\npure-core = true\n",
        &[],
    ),
    (
        "kosmo-proto",
        "display",
        "D0",
        "export = true\npure-core = true\nprovisional = \"until the protocol is reviewed\"\n",
        &[],
    ),
    (
        "kosmo-scene",
        "display",
        "D1",
        "pure-core = true\n",
        &["kosmo-types"],
    ),
    (
        "kosmo-server",
        "display",
        "D3",
        "",
        &["kosmo-scene", "kosmo-proto"],
    ),
    ("kosmo-math-model", "framework", "F0", "", &[]),
    (
        "kosmo-jobs",
        "framework",
        "F0",
        "same-layer = [\"kosmo-math-model\"]\nplatform = [\"threads\"]\n",
        &["kosmo-math-model"],
    ),
    ("kosmo-render", "framework", "F0", "", &[]),
    (
        "kosmo-client",
        "framework",
        "F0",
        "export = true\n",
        &["kosmo-proto"],
    ),
    (
        "kosmo-ui",
        "framework",
        "F1",
        "",
        &["kosmo-render", "kosmo-types"],
    ),
    (
        "kosmo-math-kalyx",
        "framework",
        "F1",
        "",
        &["kosmo-math-model", "kalyx"],
    ),
    (
        "kosmo-sdk",
        "framework",
        "F3",
        "export = true\nfacade = true\n",
        &["kosmo-ui"],
    ),
    (
        "kosmo-shell",
        "desktop",
        "S0",
        "",
        &["kosmo-sdk", "kosmo-client"],
    ),
    ("kosmo-calc", "desktop", "S1", "", &["kosmo-sdk"]),
    (
        "kosmo",
        "desktop",
        "S2",
        "platform = [\"standard-streams\", \"arguments\", \"exit-code\", \"real-clock\"]\n",
        &["kosmo-calc"],
    ),
];

/// The `[workspace.dependencies]` of each root: names and paths.
const DECLARED: [(&str, &[(&str, &str)]); 3] = [
    (
        "display",
        &[
            ("kosmo-types", "types"),
            ("kosmo-proto", "proto"),
            ("kosmo-scene", "scene"),
        ],
    ),
    (
        "framework",
        &[
            ("kosmo-math-model", "math-model"),
            ("kosmo-render", "render"),
            ("kosmo-ui", "ui"),
            ("kosmo-proto", "../display/proto"),
            ("kosmo-types", "../display/types"),
        ],
    ),
    (
        "desktop",
        &[
            ("kosmo-calc", "calc"),
            ("kosmo-sdk", "../framework/sdk"),
            ("kosmo-client", "../framework/client"),
        ],
    ),
];

/// The library root of each pure-core crate. Its comments and strings name
/// a type of interior mutability without using it, and it uses `alloc`.
const PURE_LIB: &str = "//! A pure-core crate. Comments and strings may name RefCell.
#![no_std]
#![forbid(unsafe_code)]
extern crate alloc;
/// The name of a type, not a use of it.
pub const NAME: &str = \"RefCell\";
";

/// The covered workspaces of the valid layout.
const ALL: [&str; 4] = ["xtask", "display", "framework", "desktop"];

/// The crate of the valid layout with only `desktop/`.
const DESKTOP_CRATES: [Spec; 1] = [("kosmo", "desktop", "S2", "", &[])];

/// The covered workspaces of the valid layout with only `desktop/`.
const DESKTOP_ONLY: [&str; 2] = ["xtask", "desktop"];

/// Files by path.
type Files = BTreeMap<String, String>;

/// The directory of the crate `name` inside its workspace: the name without
/// the prefix `kosmo-`, or the name itself when it has no such prefix.
fn dir_of(name: &str) -> &str {
    name.strip_prefix("kosmo-").unwrap_or(name)
}

fn root_manifest(workspace: &str, crates: &[Spec], declared: &[(&str, &[(&str, &str)])]) -> String {
    let members: Vec<String> = crates
        .iter()
        .filter(|(_, owner, ..)| *owner == workspace)
        .map(|(name, ..)| format!("\"{}\"", dir_of(name)))
        .collect();
    let declared = declared
        .iter()
        .find(|(owner, _)| *owner == workspace)
        .map(|(_, entries)| *entries)
        .unwrap_or_default();
    let mut text = format!(
        "[workspace]\nresolver = \"3\"\nmembers = [{}]\n\n[workspace.package]\nversion = \"0.1.0-dev.1\"\nedition = \"2024\"\nlicense = \"MIT\"\n\n[workspace.dependencies]\n",
        members.join(", ")
    );
    for (name, path) in declared {
        text.push_str(&format!(
            "{name} = {{ path = \"{path}\", version = \"=0.1.0-dev.1\" }}\n"
        ));
    }
    text.push_str("\n[workspace.lints.clippy]\n");
    for lint in DENIED_LINTS {
        text.push_str(&format!("{lint} = \"deny\"\n"));
    }
    text.push_str("\n[profile.release]\noverflow-checks = true\n");
    text
}

fn crate_manifest(name: &str, dependencies: &[&str]) -> String {
    let mut text = format!(
        "[package]\nname = \"{name}\"\nversion.workspace = true\nedition.workspace = true\nlicense.workspace = true\npublish = false\n\n[lints]\nworkspace = true\n"
    );
    if !dependencies.is_empty() {
        text.push_str("\n[dependencies]\n");
        for dependency in dependencies {
            text.push_str(&format!("{dependency}.workspace = true\n"));
        }
    }
    text
}

/// A valid layout with `crates`, whose roots declare `declared`.
fn layout_of(crates: &[Spec], declared: &[(&str, &[(&str, &str)])]) -> Files {
    let mut files = Files::new();
    let mut table = WORKSPACES_TABLE.to_string();
    for &(name, workspace, layer, marks, dependencies) in crates {
        let pure = marks.contains("pure-core = true");
        let dir = format!("{workspace}/{}", dir_of(name));
        let mut entry = format!("\n[crates.{name}]\npath = \"{dir}\"\nlayer = \"{layer}\"\n");
        if !marks.contains("export = true") {
            entry.push_str("export = false\n");
        }
        if !pure {
            entry.push_str("pure-core = false\n");
        }
        entry.push_str("publish = false\n");
        entry.push_str(marks);
        table.push_str(&entry);
        files.insert(
            format!("{dir}/Cargo.toml"),
            crate_manifest(name, dependencies),
        );
        let (source, text) = if name == PROGRAM {
            ("src/main.rs", "fn main() {}\n".to_string())
        } else if pure {
            ("src/lib.rs", PURE_LIB.to_string())
        } else {
            ("src/lib.rs", format!("//! The crate {name}.\n"))
        };
        files.insert(format!("{dir}/{source}"), text);
        if name == PROGRAM {
            // As in this repository, the program also has a library target.
            files.insert(
                format!("{dir}/src/lib.rs"),
                format!("//! The crate {name}.\n"),
            );
        }
    }
    files.insert(TABLE.to_string(), table);
    for workspace in ["display", "framework", "desktop"] {
        if !crates.iter().any(|(_, owner, ..)| *owner == workspace) {
            continue;
        }
        files.insert(
            format!("{workspace}/Cargo.toml"),
            root_manifest(workspace, crates, declared),
        );
        files.insert(
            format!("{workspace}/Cargo.lock"),
            "version = 4\n".to_string(),
        );
    }
    files.insert(
        "xtask/Cargo.toml".to_string(),
        "[package]\nname = \"xtask\"\nversion = \"0.0.0\"\nedition = \"2024\"\npublish = false\n\n[workspace]\n\n[dependencies]\nyaml-rust2 = { version = \"=0.13.0\", default-features = false }\n".to_string(),
    );
    files.insert("xtask/Cargo.lock".to_string(), "version = 4\n".to_string());
    files.insert(
        CARGO_CONFIG.to_string(),
        "[alias]\nxtask = \"run --quiet --locked --manifest-path xtask/Cargo.toml --\"\n"
            .to_string(),
    );
    files
}

/// The valid layout with crates in every product workspace.
fn valid() -> Files {
    let mut files = layout_of(&CRATES, &DECLARED);
    declare(&mut files, "framework", "kalyx = \"=0.1.0\"");
    // A pure-core crate may use the standard library in its tests.
    edit(
        &mut files,
        "display/types/src/lib.rs",
        "extern crate alloc;\n",
        "extern crate alloc;\n#[cfg(test)]\nextern crate std;\n",
    );
    files
}

/// The valid layout in which only `desktop/` has a crate, the program
/// `kosmo`, as in this repository so far.
fn valid_desktop_only() -> Files {
    layout_of(&DESKTOP_CRATES, &[])
}

/// Replaces the one occurrence of `from` in `text`.
#[track_caller]
fn replace(text: &mut String, from: &str, to: &str) {
    assert_eq!(text.matches(from).count(), 1, "{from}");
    *text = text.replacen(from, to, 1);
}

/// Replaces the one occurrence of `from` in the file at `path`.
#[track_caller]
fn edit(files: &mut Files, path: &str, from: &str, to: &str) {
    replace(files.get_mut(path).expect("the file exists"), from, to);
}

/// Appends `text` to the file at `path`.
fn append(files: &mut Files, path: &str, text: &str) {
    files.get_mut(path).expect("the file exists").push_str(text);
}

/// Adds `entry` at the start of the `[workspace.dependencies]` of the root
/// of `workspace`.
fn declare(files: &mut Files, workspace: &str, entry: &str) {
    edit(
        files,
        &format!("{workspace}/Cargo.toml"),
        "[workspace.dependencies]\n",
        &format!("[workspace.dependencies]\n{entry}\n"),
    );
}

fn tree(files: &Files) -> Tree {
    let borrowed: Vec<(&str, &str)> = files
        .iter()
        .map(|(path, text)| (path.as_str(), text.as_str()))
        .collect();
    Tree::from_files(&borrowed)
}

/// The problems of the valid layout after `change`.
fn problems_with(change: impl FnOnce(&mut Files)) -> Vec<String> {
    let mut files = valid();
    change(&mut files);
    check(&tree(&files), &ALL)
}

/// Asserts that `found` has one problem for each of `expected`, each
/// containing its fragment.
#[track_caller]
fn assert_problems(found: &[String], expected: &[&str]) {
    assert_eq!(found.len(), expected.len(), "{found:#?}");
    let mut unmatched: Vec<&String> = found.iter().collect();
    for fragment in expected {
        let at = unmatched
            .iter()
            .position(|problem| problem.contains(fragment))
            .unwrap_or_else(|| panic!("no problem contains `{fragment}`: {found:#?}"));
        unmatched.remove(at);
    }
}

/// The problems of the valid layout with only `desktop/` after `change`.
fn desktop_only_with(change: impl FnOnce(&mut Files)) -> Vec<String> {
    let mut files = valid_desktop_only();
    change(&mut files);
    check(&tree(&files), &DESKTOP_ONLY)
}

#[test]
fn the_valid_layout_passes() {
    assert_eq!(problems_with(|_| {}), Vec::<String>::new());
}

#[test]
fn the_valid_layout_with_only_the_desktop_passes() {
    assert_eq!(desktop_only_with(|_| {}), Vec::<String>::new());
}

#[test]
fn this_repository_passes() {
    let tree = Tree::load(&repo::root()).expect("the repository is a Git working copy");
    assert_eq!(check(&tree, &check::WORKSPACES), Vec::<String>::new());
}

// The seven cases below are the main failures that the layout step exists
// to catch: a dependency against the layer order, a dependency against the
// workspace direction, interior mutability in a pure-core crate, a
// pure-core crate that depends on a provisional crate, a crate that the
// layout table does not list, a crate other than the kalyx adapter that
// declares kalyx, and a core library that uses the standard streams.

#[test]
fn rejects_a_dependency_on_a_higher_layer() {
    let found = problems_with(|files| {
        append(
            files,
            "framework/math-model/Cargo.toml",
            "\n[dependencies]\nkosmo-ui.workspace = true\n",
        );
    });
    assert_eq!(
        found,
        [
            "framework/math-model/Cargo.toml: [dependencies] kosmo-ui: a crate of layer F0 may not depend on kosmo-ui of the higher layer F1"
        ]
    );
}

#[test]
fn rejects_a_dependency_against_the_workspace_direction() {
    let found = problems_with(|files| {
        declare(
            files,
            "display",
            "kosmo-ui = { path = \"../framework/ui\", version = \"=0.1.0-dev.1\" }",
        );
        append(
            files,
            "display/server/Cargo.toml",
            "kosmo-ui.workspace = true\n",
        );
    });
    assert_eq!(
        found,
        [
            "display/server/Cargo.toml: [dependencies] kosmo-ui: crates of display may not depend on crates of framework"
        ]
    );
}

#[test]
fn rejects_interior_mutability_in_a_pure_core_crate() {
    let found = problems_with(|files| {
        edit(
            files,
            "display/scene/src/lib.rs",
            "extern crate alloc;\n",
            "use core::cell::RefCell;\nextern crate alloc;\n",
        );
    });
    assert_eq!(
        found,
        ["display/scene/src/lib.rs:4: the pure-core crate kosmo-scene may not use cell, RefCell"]
    );
}

#[test]
fn rejects_a_pure_core_dependency_on_a_provisional_crate() {
    let found = problems_with(|files| {
        append(
            files,
            "display/scene/Cargo.toml",
            "kosmo-proto.workspace = true\n",
        );
    });
    assert_eq!(
        found,
        [
            "display/scene/Cargo.toml: [dependencies] kosmo-proto: the pure-core crate kosmo-scene may not depend on the provisional crate kosmo-proto"
        ]
    );
}

#[test]
fn rejects_a_crate_missing_from_the_layout_table() {
    let found = problems_with(|files| {
        files.insert(
            "desktop/extra/Cargo.toml".to_string(),
            crate_manifest("kosmo-extra", &[]),
        );
    });
    assert_eq!(
        found,
        ["desktop/extra/Cargo.toml: kosmo-extra is not a crate of the layout table"]
    );
}

#[test]
fn rejects_kalyx_in_the_dev_dependencies_of_the_math_model() {
    let found = problems_with(|files| {
        append(
            files,
            "framework/math-model/Cargo.toml",
            "\n[dev-dependencies]\nkalyx.workspace = true\n",
        );
    });
    assert_eq!(
        found,
        [
            "framework/math-model/Cargo.toml: [dev-dependencies] kalyx: declares the package kalyx, which only the kalyx adapter kosmo-math-kalyx may declare"
        ]
    );
}

#[test]
fn a_core_library_may_not_use_standard_io() {
    let found = problems_with(|files| append(files, RENDER_LIB, "use std::io::Write;\n"));
    assert_eq!(
        found,
        [
            "framework/render/src/lib.rs:2: the core library kosmo-render may not use std::io; only a crate registered for the platform category standard-streams may"
        ]
    );
}

// The layout table itself.

#[test]
fn a_table_that_does_not_parse_leaves_only_the_checks_without_a_table() {
    let found = problems_with(|files| {
        edit(
            files,
            TABLE,
            "layer = \"S2\"\nexport = false\npure-core = false\n",
            "layer = \"S2\"\nexport = false\npure_core = false\n",
        );
        files.insert("Cargo.toml".to_string(), "[workspace]\n".to_string());
    });
    assert_problems(
        &found,
        &[
            "unknown field `pure_core`",
            "Cargo.toml: the repository root has no Cargo.toml",
        ],
    );
    assert!(found[1].starts_with("layout.toml:"), "{found:#?}");
}

#[test]
fn rejects_unknown_and_repeated_layers() {
    let unknown = problems_with(|files| {
        edit(files, TABLE, "layer = \"S2\"", "layer = \"S9\"");
    });
    assert_eq!(unknown, ["layout.toml: crates.kosmo: unknown layer S9"]);
    let repeated = problems_with(|files| {
        edit(
            files,
            TABLE,
            "    { id = \"D1\", name = \"pure core\" },\n",
            "    { id = \"D1\", name = \"pure core\" },\n    { id = \"D0\", name = \"again\" },\n",
        );
    });
    assert_eq!(
        repeated,
        ["layout.toml: workspaces.display: layer D0 is defined twice"]
    );
}

#[test]
fn rejects_crate_paths_outside_their_workspace_or_shared() {
    let outside = problems_with(|files| {
        edit(
            files,
            TABLE,
            "path = \"desktop/kosmo\"\n",
            "path = \"framework/kosmo\"\n",
        );
    });
    assert_eq!(
        outside,
        ["layout.toml: crates.kosmo: the path framework/kosmo is not inside the workspace desktop"]
    );
    let shared = problems_with(|files| {
        edit(
            files,
            TABLE,
            "path = \"framework/render\"\n",
            "path = \"framework/math-model\"\n",
        );
    });
    assert_eq!(
        shared,
        [
            "layout.toml: crates.kosmo-math-model and crates.kosmo-render share the path framework/math-model"
        ]
    );
}

#[test]
fn rejects_unknown_workspaces_empty_conditions_and_outside_allowlists() {
    let found = problems_with(|files| {
        edit(files, TABLE, "uses = []\n", "uses = [\"dispaly\"]\n");
        edit(
            files,
            TABLE,
            "provisional = \"until the protocol is reviewed\"\n",
            "provisional = \"\"\n",
        );
        edit(
            files,
            TABLE,
            "lock-allowlist = \"display/allowlist.toml\"\n",
            "lock-allowlist = \"display-allowlist.toml\"\n",
        );
    });
    assert_problems(
        &found,
        &[
            "layout.toml: workspaces.display: uses names the unknown workspace dispaly",
            "layout.toml: crates.kosmo-proto: the provisional condition is empty",
            "layout.toml: workspaces.display: the lock-allowlist display-allowlist.toml is not inside display/",
        ],
    );
    let blank = problems_with(|files| {
        edit(
            files,
            TABLE,
            "provisional = \"until the protocol is reviewed\"\n",
            "provisional = \" \"\n",
        );
    });
    assert_eq!(
        blank,
        ["layout.toml: crates.kosmo-proto: the provisional condition is empty"]
    );
}

#[test]
fn table_paths_are_normalized() {
    for path in [
        "./framework",
        "/framework",
        "framework/",
        "desktop/../framework",
    ] {
        let found = desktop_only_with(|files| {
            edit(
                files,
                TABLE,
                "path = \"framework\"\n",
                &format!("path = \"{path}\"\n"),
            );
        });
        assert_eq!(
            found,
            [format!(
                "layout.toml: workspaces.framework: the path {path} is not a normalized path inside the repository"
            )]
        );
    }
    let krate = problems_with(|files| {
        edit(
            files,
            TABLE,
            "path = \"desktop/kosmo\"\n",
            "path = \"desktop/../desktop/kosmo\"\n",
        );
    });
    assert_eq!(
        krate,
        [
            "layout.toml: crates.kosmo: the path desktop/../desktop/kosmo is not a normalized path inside the repository"
        ]
    );
    let allowlist = problems_with(|files| {
        edit(
            files,
            TABLE,
            "lock-allowlist = \"display/allowlist.toml\"\n",
            "lock-allowlist = \"display/./allowlist.toml\"\n",
        );
    });
    assert_eq!(
        allowlist,
        [
            "layout.toml: workspaces.display: the lock-allowlist display/./allowlist.toml is not a normalized path inside the repository"
        ]
    );
}

#[test]
fn the_table_keeps_the_rules_of_the_source_layout() {
    let uses = problems_with(|files| {
        edit(files, TABLE, "uses = []\n", "uses = [\"framework\"]\n");
    });
    assert_eq!(
        uses,
        [
            "layout.toml: workspaces.display: uses must be []; the source layout fixes the directions between the workspaces"
        ]
    );
    let marks = problems_with(|files| {
        edit(
            files,
            TABLE,
            "\"conformance tests\", publish = false",
            "\"conformance tests\"",
        );
        edit(
            files,
            TABLE,
            "\"first-party applications\", facade-only = true",
            "\"first-party applications\"",
        );
    });
    assert_problems(
        &marks,
        &[
            "layout.toml: workspaces.display: the source layout requires layer D4 with publish = false",
            "layout.toml: workspaces.desktop: the source layout requires layer S1 with facade-only = true",
        ],
    );
    let targets = problems_with(|files| {
        edit(
            files,
            TABLE,
            "path = \"desktop\"\nuses = [\"framework\"]\ntargets = [\"aarch64-unknown-linux-gnu\", \"aarch64-apple-darwin\"]\n",
            "path = \"desktop\"\nuses = [\"framework\"]\ntargets = []\n",
        );
        edit(
            files,
            TABLE,
            "path = \"xtask\"\ntargets = [\"aarch64-unknown-linux-gnu\", \"aarch64-apple-darwin\"]\n",
            "path = \"xtask\"\ntargets = [\"aarch64-apple-darwin\", \"aarch64-apple-darwin\"]\n",
        );
    });
    assert_problems(
        &targets,
        &[
            "layout.toml: workspaces.desktop: targets lists no target platform",
            "layout.toml: tools.xtask: targets lists aarch64-apple-darwin twice",
        ],
    );
}

#[test]
fn the_table_keeps_the_layers_of_the_source_layout_in_order() {
    let rule = "the source layout fixes the layers and their order";
    let removed = problems_with(|files| {
        edit(
            files,
            TABLE,
            "    { id = \"D2\", name = \"server libraries\" },\n",
            "",
        );
    });
    assert_eq!(
        removed,
        [format!(
            "layout.toml: workspaces.display: the layers must be D0, D1, D2, D3, D4, from the lowest; {rule}"
        )]
    );
    let swapped = problems_with(|files| {
        edit(
            files,
            TABLE,
            "    { id = \"F1\", name = \"core, adapters and backends\" },\n    { id = \"F2\", name = \"controls\" },\n",
            "    { id = \"F2\", name = \"controls\" },\n    { id = \"F1\", name = \"core, adapters and backends\" },\n",
        );
    });
    assert_eq!(
        swapped,
        [format!(
            "layout.toml: workspaces.framework: the layers must be F0, F1, F2, F3, F4, from the lowest; {rule}"
        )]
    );
    // The marks of a layer are checked once the layers are right.
    let desktop = desktop_only_with(|files| {
        edit(
            files,
            TABLE,
            "    { id = \"S1\", name = \"first-party applications\", facade-only = true },\n    { id = \"S2\", name = \"the program kosmo\" },\n",
            "    { id = \"S2\", name = \"the program kosmo\" },\n    { id = \"S1\", name = \"first-party applications\" },\n",
        );
    });
    assert_eq!(
        desktop,
        [format!(
            "layout.toml: workspaces.desktop: the layers must be S0, S1, S2, from the lowest; {rule}"
        )]
    );
    let program = desktop_only_with(|files| {
        edit(files, TABLE, "layer = \"S2\"", "layer = \"S1\"");
    });
    assert_eq!(
        program,
        [
            "layout.toml: crates.kosmo: the program crate kosmo belongs in layer S2, the layer of the program, not S1"
        ]
    );
}

#[test]
fn a_table_that_is_not_a_regular_file_is_reported_as_such() {
    let mut tree = tree(&valid());
    tree.insert(TABLE, Entry::Symlink("other.toml".to_string()));
    assert_eq!(
        check(&tree, &ALL),
        ["layout.toml: not a UTF-8 regular file; the layout step reads it"]
    );
}

// Workspace roots.

#[test]
fn rejects_a_manifest_at_the_repository_root() {
    let found = problems_with(|files| {
        files.insert(
            "Cargo.toml".to_string(),
            "[workspace]\nmembers = [\"desktop/kosmo\"]\n".to_string(),
        );
    });
    assert_eq!(
        found,
        ["Cargo.toml: the repository root has no Cargo.toml; each workspace has its own root"]
    );
}

#[test]
fn a_workspace_with_crates_needs_a_virtual_root() {
    let missing = problems_with(|files| {
        files.remove("desktop/Cargo.toml");
    });
    assert_eq!(
        missing,
        ["desktop/Cargo.toml: missing; the workspace desktop has crates in the layout table"]
    );
    let package = problems_with(|files| {
        files.insert(
            "desktop/Cargo.toml".to_string(),
            "[package]\nname = \"desktop\"\nversion = \"0.1.0-dev.1\"\n".to_string(),
        );
    });
    assert_problems(
        &package,
        &[
            "desktop/Cargo.toml: the workspace root must declare [workspace]",
            "desktop/Cargo.toml: the workspace root must not declare [package]",
        ],
    );
}

#[test]
fn workspaces_do_not_contain_one_another_or_the_root() {
    for (path, expected) in [
        (
            "desktop/fw",
            "layout.toml: workspaces.framework: the workspace framework lies inside the workspace desktop",
        ),
        (
            "",
            "layout.toml: workspaces.framework: the path of the workspace framework is the repository root",
        ),
        (
            "xtask/framework",
            "layout.toml: workspaces.framework: the workspace framework lies inside xtask/",
        ),
    ] {
        let found = desktop_only_with(|files| {
            edit(
                files,
                TABLE,
                "path = \"framework\"\n",
                &format!("path = \"{path}\"\n"),
            );
            let allowlist = repo::join(path, "allowlist.toml").unwrap_or_default();
            edit(
                files,
                TABLE,
                "lock-allowlist = \"framework/allowlist.toml\"\n",
                &format!("lock-allowlist = \"{allowlist}\"\n"),
            );
        });
        assert_eq!(found, [expected]);
    }
}

#[test]
fn a_workspace_without_crates_has_no_root_manifest() {
    let found = desktop_only_with(|files| {
        files.insert(
            "framework/Cargo.toml".to_string(),
            "[workspace]\n".to_string(),
        );
    });
    assert_eq!(
        found,
        [
            "framework/Cargo.toml: the workspace framework has no crate in the layout table, so it has no root manifest yet"
        ]
    );
}

#[test]
fn only_workspace_roots_have_a_lock_file() {
    let found = problems_with(|files| {
        files.remove("desktop/Cargo.lock");
        files.insert(
            "desktop/kosmo/Cargo.lock".to_string(),
            "version = 4\n".to_string(),
        );
    });
    assert_problems(
        &found,
        &[
            "desktop/Cargo.lock: missing",
            "desktop/kosmo/Cargo.lock: only workspace roots have a Cargo.lock",
        ],
    );
}

// The shape of the workspace roots.

#[test]
fn a_root_uses_resolver_3_and_keeps_overflow_checks() {
    let found = problems_with(|files| {
        edit(
            files,
            "desktop/Cargo.toml",
            "resolver = \"3\"",
            "resolver = \"2\"",
        );
        append(
            files,
            "desktop/Cargo.toml",
            "\n[profile.dev]\noverflow-checks = false\n",
        );
    });
    assert_problems(
        &found,
        &[
            "desktop/Cargo.toml: [workspace] resolver must be \"3\"",
            "desktop/Cargo.toml: [profile.dev] may not set overflow-checks = false",
        ],
    );
}

#[test]
fn a_root_denies_the_lints_for_members_that_inherit_them() {
    let found = problems_with(|files| {
        edit(files, "desktop/Cargo.toml", "unwrap_used = \"deny\"\n", "");
        edit(
            files,
            "desktop/shell/Cargo.toml",
            "\n[lints]\nworkspace = true\n",
            "",
        );
        edit(
            files,
            "desktop/Cargo.toml",
            "resolver = \"3\"\n",
            "resolver = \"3\"\nexclude = [\"old\"]\n",
        );
    });
    assert_problems(
        &found,
        &[
            "desktop/Cargo.toml: [workspace.lints.clippy] unwrap_used must be \"deny\"",
            "desktop/shell/Cargo.toml: must set [lints] workspace = true",
            "desktop/Cargo.toml: the workspace root may not have workspace.exclude",
        ],
    );
}

#[test]
fn no_profile_setting_turns_off_overflow_checks() {
    let found = problems_with(|files| {
        append(
            files,
            "desktop/Cargo.toml",
            "\n[profile.release.package.kosmo]\noverflow-checks = false\n\n[profile.release.package.\"*\"]\noverflow-checks = false\n\n[profile.dev.build-override]\noverflow-checks = false\n",
        );
    });
    assert_problems(
        &found,
        &[
            "desktop/Cargo.toml: [profile.release.package.kosmo] may not set overflow-checks = false",
            "desktop/Cargo.toml: [profile.release.package.\"*\"] may not set overflow-checks = false",
            "desktop/Cargo.toml: [profile.dev.build-override] may not set overflow-checks = false",
        ],
    );
}

#[test]
fn the_denied_lints_may_be_forbidden_but_not_lowered() {
    let forbidden = problems_with(|files| {
        edit(
            files,
            "desktop/Cargo.toml",
            "unwrap_used = \"deny\"\n",
            "unwrap_used = \"forbid\"\n",
        );
    });
    assert_eq!(forbidden, Vec::<String>::new());
    let lowered = problems_with(|files| {
        append(
            files,
            "desktop/Cargo.toml",
            "\n[workspace.lints.clippy.restriction]\nlevel = \"allow\"\npriority = 1\n",
        );
    });
    assert_eq!(
        lowered,
        [
            "desktop/Cargo.toml: [workspace.lints.clippy] restriction may not be \"allow\"; the group holds the denied lints"
        ]
    );
}

// Members.

#[test]
fn every_crate_of_the_table_has_a_manifest_with_its_name() {
    let untracked = problems_with(|files| {
        files.remove("desktop/calc/Cargo.toml");
    });
    assert_eq!(
        untracked,
        ["desktop/calc/Cargo.toml: is not tracked; the layout table lists the crate kosmo-calc"]
    );
    let renamed = problems_with(|files| {
        edit(
            files,
            "desktop/calc/Cargo.toml",
            "name = \"kosmo-calc\"",
            "name = \"calc\"",
        );
    });
    assert_eq!(
        renamed,
        ["desktop/calc/Cargo.toml: the package is named calc, not kosmo-calc"]
    );
}

#[test]
fn a_crate_of_the_table_belongs_to_the_root_of_its_workspace() {
    // A workspace of its own would declare kalyx where the kalyx rule
    // does not look.
    let own = problems_with(|files| {
        append(
            files,
            "framework/render/Cargo.toml",
            "\n[workspace]\n\n[workspace.dependencies]\nkalyx = \"=0.1.0\"\n",
        );
    });
    assert_eq!(
        own,
        [
            "framework/render/Cargo.toml: the crate kosmo-render may not declare [workspace]; it is a member of the root of its workspace"
        ]
    );
    let named = problems_with(|files| {
        edit(
            files,
            "framework/render/Cargo.toml",
            "publish = false\n",
            "publish = false\nworkspace = \"..\"\n",
        );
    });
    assert_eq!(
        named,
        [
            "framework/render/Cargo.toml: the crate kosmo-render may not set package.workspace; it is a member of the root of its workspace"
        ]
    );
}

#[test]
fn members_list_exactly_the_crates_of_the_workspace() {
    let missing = problems_with(|files| {
        edit(
            files,
            "desktop/Cargo.toml",
            "members = [\"shell\", \"calc\", \"kosmo\"]",
            "members = [\"shell\", \"kosmo\"]",
        );
    });
    assert_eq!(
        missing,
        [
            "desktop/Cargo.toml: members must be exactly the crates of the workspace in the layout table: calc, kosmo, shell"
        ]
    );
    let pattern = problems_with(|files| {
        edit(
            files,
            "desktop/Cargo.toml",
            "members = [\"shell\", \"calc\", \"kosmo\"]",
            "members = [\"*\"]",
        );
    });
    assert_eq!(
        pattern,
        ["desktop/Cargo.toml: members may not use patterns; list each crate"]
    );
}

#[test]
fn a_manifest_in_any_letter_case_must_be_a_crate_of_the_table() {
    let found = problems_with(|files| {
        files.insert(
            "desktop/x/cargo.toml".to_string(),
            crate_manifest("kosmo-x", &[]),
        );
    });
    assert_eq!(
        found,
        ["desktop/x/cargo.toml: kosmo-x is not a crate of the layout table"]
    );
}

// The form of the manifests.

#[test]
fn members_inherit_external_dependencies() {
    let found = problems_with(|files| {
        append(files, "desktop/kosmo/Cargo.toml", "foo = \"=1.0.0\"\n");
    });
    assert_eq!(
        found,
        [
            "desktop/kosmo/Cargo.toml: [dependencies] foo: must be inherited with workspace = true from the workspace root"
        ]
    );
}

#[test]
fn external_versions_are_exact() {
    let found = problems_with(|files| {
        declare(
            files,
            "desktop",
            "a = \"1.2.3\"\nb = \"^1.2.3\"\nc = { version = \"=1.2\" }\nd = \"=1.2.3-rc.1+build.5\"",
        );
    });
    assert_problems(
        &found,
        &[
            "desktop/Cargo.toml: [workspace.dependencies] a: the version must be exact (=x.y.z), not 1.2.3",
            "desktop/Cargo.toml: [workspace.dependencies] b: the version must be exact (=x.y.z), not ^1.2.3",
            "desktop/Cargo.toml: [workspace.dependencies] c: the version must be exact (=x.y.z), not =1.2",
        ],
    );
}

#[test]
fn rejects_git_and_registry_sources() {
    let found = problems_with(|files| {
        declare(
            files,
            "desktop",
            "g = { git = \"https://example.org/g\", version = \"=1.0.0\" }\nr = { registry = \"other\", version = \"=1.0.0\" }",
        );
    });
    assert_problems(
        &found,
        &[
            "desktop/Cargo.toml: [workspace.dependencies] g: may not use git",
            "desktop/Cargo.toml: [workspace.dependencies] r: may not use registry",
        ],
    );
}

#[test]
fn rejects_patch_and_replace_sections() {
    let found = problems_with(|files| {
        append(
            files,
            "desktop/Cargo.toml",
            "\n[patch.crates-io]\nfoo = { path = \"vendor/foo\" }\n\n[replace]\n\"foo:1.0.0\" = { path = \"vendor/foo\" }\n",
        );
    });
    assert_problems(
        &found,
        &[
            "desktop/Cargo.toml: [patch] is not allowed",
            "desktop/Cargo.toml: [replace] is not allowed",
        ],
    );
}

#[test]
fn internal_dependencies_have_the_common_version() {
    let found = problems_with(|files| {
        edit(
            files,
            "framework/Cargo.toml",
            "kosmo-render = { path = \"render\", version = \"=0.1.0-dev.1\" }",
            "kosmo-render = { path = \"render\" }",
        );
        edit(
            files,
            "framework/Cargo.toml",
            "kosmo-math-model = { path = \"math-model\", version = \"=0.1.0-dev.1\" }",
            "kosmo-math-model = { path = \"math-model\", version = \"=0.1.0\" }",
        );
    });
    assert_problems(
        &found,
        &[
            "framework/Cargo.toml: [workspace.dependencies] kosmo-render: an internal dependency must have version = \"=0.1.0-dev.1\"",
            "framework/Cargo.toml: [workspace.dependencies] kosmo-math-model: an internal dependency must have version = \"=0.1.0-dev.1\"",
        ],
    );
}

#[test]
fn paths_name_crates_of_the_table() {
    let found = problems_with(|files| {
        declare(
            files,
            "desktop",
            "docs = { path = \"../docs\", version = \"=0.1.0-dev.1\" }\ntasks = { path = \"../xtask\", version = \"=0.1.0-dev.1\" }",
        );
    });
    assert_problems(
        &found,
        &[
            "desktop/Cargo.toml: [workspace.dependencies] docs: the path ../docs is not a crate of the layout table",
            "desktop/Cargo.toml: [workspace.dependencies] tasks: the path ../xtask is not a crate of the layout table",
        ],
    );
}

#[test]
fn rejects_renamed_dependencies_and_underscore_keys() {
    let found = problems_with(|files| {
        declare(
            files,
            "desktop",
            "other = { version = \"=1.0.0\", package = \"real\" }",
        );
        append(
            files,
            "desktop/kosmo/Cargo.toml",
            "\n[dev_dependencies]\n\n[build_dependencies]\n\n[target.'cfg(unix)'.dev_dependencies]\n",
        );
    });
    assert_problems(
        &found,
        &[
            "desktop/Cargo.toml: [workspace.dependencies] other: may not rename a dependency",
            "desktop/kosmo/Cargo.toml: [dev_dependencies] is not allowed; write dev-dependencies",
            "desktop/kosmo/Cargo.toml: [build_dependencies] is not allowed; write build-dependencies",
            "desktop/kosmo/Cargo.toml: [target.'cfg(unix)'.dev_dependencies] is not allowed; write dev-dependencies",
        ],
    );
}

#[test]
fn tool_versions_are_exact() {
    let found = problems_with(|files| {
        edit(
            files,
            "xtask/Cargo.toml",
            "yaml-rust2 = { version = \"=0.13.0\", default-features = false }",
            "yaml-rust2 = \"0.13\"",
        );
    });
    assert_eq!(
        found,
        [
            "xtask/Cargo.toml: [dependencies] yaml-rust2: the version must be exact (=x.y.z), not 0.13"
        ]
    );
}

#[test]
fn crates_of_the_table_are_declared_only_with_their_path() {
    let found = problems_with(|files| {
        declare(files, "desktop", "kosmo-server = \"=0.1.0-dev.1\"");
        append(
            files,
            "desktop/shell/Cargo.toml",
            "kosmo-server.workspace = true\n",
        );
        append(files, "xtask/Cargo.toml", "kosmo = \"=0.1.0-dev.1\"\n");
    });
    assert_problems(
        &found,
        &[
            "desktop/Cargo.toml: [workspace.dependencies] kosmo-server: a crate of the layout table is declared with its path, not from a registry",
            "xtask/Cargo.toml: [dependencies] kosmo: xtask may not depend on crates of the layout table",
        ],
    );
}

// Directions.

#[test]
fn same_layer_dependencies_are_listed_and_used() {
    let unlisted = problems_with(|files| {
        edit(files, TABLE, "same-layer = [\"kosmo-math-model\"]\n", "");
    });
    assert_eq!(
        unlisted,
        [
            "framework/jobs/Cargo.toml: [dependencies] kosmo-math-model: kosmo-math-model is in the same layer F0 and is not listed in same-layer of kosmo-jobs"
        ]
    );
    let unused = problems_with(|files| {
        edit(
            files,
            TABLE,
            "same-layer = [\"kosmo-math-model\"]\n",
            "same-layer = [\"kosmo-math-model\", \"kosmo-render\"]\n",
        );
    });
    assert_eq!(
        unused,
        [
            "layout.toml: crates.kosmo-jobs: lists kosmo-render in same-layer but does not depend on it"
        ]
    );
}

#[test]
fn other_workspaces_are_reached_only_through_export_crates() {
    let found = problems_with(|files| {
        declare(
            files,
            "framework",
            "kosmo-server = { path = \"../display/server\", version = \"=0.1.0-dev.1\" }",
        );
        append(
            files,
            "framework/ui/Cargo.toml",
            "kosmo-server.workspace = true\n",
        );
    });
    assert_eq!(
        found,
        [
            "framework/ui/Cargo.toml: [dependencies] kosmo-server: kosmo-server is not an export crate of the workspace display"
        ]
    );
}

#[test]
fn the_desktop_reaches_only_export_crates_of_the_framework() {
    for (dependency, path, expected) in [
        (
            "kosmo-ui",
            "../framework/ui",
            "desktop/calc/Cargo.toml: [dependencies] kosmo-ui: kosmo-ui is not an export crate of the workspace framework",
        ),
        (
            "kosmo-types",
            "../display/types",
            "desktop/calc/Cargo.toml: [dependencies] kosmo-types: crates of desktop may not depend on crates of display",
        ),
    ] {
        let found = problems_with(|files| {
            declare(
                files,
                "desktop",
                &format!("{dependency} = {{ path = \"{path}\", version = \"=0.1.0-dev.1\" }}"),
            );
            append(
                files,
                "desktop/calc/Cargo.toml",
                &format!("{dependency}.workspace = true\n"),
            );
        });
        assert_eq!(found, [expected]);
    }
}

#[test]
fn pure_core_crates_depend_only_on_pure_core_crates() {
    let found = problems_with(|files| {
        append(
            files,
            TABLE,
            "\n[crates.kosmo-d0]\npath = \"display/d0\"\nlayer = \"D0\"\nexport = false\npure-core = false\npublish = false\n",
        );
        edit(
            files,
            "display/Cargo.toml",
            "members = [",
            "members = [\"d0\", ",
        );
        declare(
            files,
            "display",
            "kosmo-d0 = { path = \"d0\", version = \"=0.1.0-dev.1\" }",
        );
        files.insert(
            "display/d0/Cargo.toml".to_string(),
            crate_manifest("kosmo-d0", &[]),
        );
        files.insert(
            "display/d0/src/lib.rs".to_string(),
            "//! A crate that is not pure core.\n".to_string(),
        );
        append(
            files,
            "display/scene/Cargo.toml",
            "kosmo-d0.workspace = true\n",
        );
    });
    assert_eq!(
        found,
        [
            "display/scene/Cargo.toml: [dependencies] kosmo-d0: the pure-core crate kosmo-scene may depend only on pure-core crates; kosmo-d0 is not one"
        ]
    );
}

#[test]
fn rejects_a_cycle_through_a_dev_dependency() {
    let found = problems_with(|files| {
        edit(
            files,
            TABLE,
            "[crates.kosmo-math-model]\npath = \"framework/math-model\"\nlayer = \"F0\"\nexport = false\npure-core = false\npublish = false\n",
            "[crates.kosmo-math-model]\npath = \"framework/math-model\"\nlayer = \"F0\"\nexport = false\npure-core = false\npublish = false\nsame-layer = [\"kosmo-jobs\"]\n",
        );
        declare(
            files,
            "framework",
            "kosmo-jobs = { path = \"jobs\", version = \"=0.1.0-dev.1\" }",
        );
        append(
            files,
            "framework/math-model/Cargo.toml",
            "\n[dev-dependencies]\nkosmo-jobs.workspace = true\n",
        );
    });
    assert_eq!(
        found,
        [
            "framework/jobs/Cargo.toml: the internal dependency graph has a cycle: kosmo-jobs -> kosmo-math-model -> kosmo-jobs"
        ]
    );
}

#[test]
fn target_and_build_dependencies_follow_the_directions() {
    for table in ["target.'cfg(unix)'.dev-dependencies", "build-dependencies"] {
        let found = problems_with(|files| {
            append(
                files,
                "framework/math-model/Cargo.toml",
                &format!("\n[{table}]\nkosmo-ui.workspace = true\n"),
            );
        });
        assert_eq!(
            found,
            [format!(
                "framework/math-model/Cargo.toml: [{table}] kosmo-ui: a crate of layer F0 may not depend on kosmo-ui of the higher layer F1"
            )]
        );
    }
}

#[test]
fn tools_have_no_path_dependencies() {
    let found = problems_with(|files| {
        append(
            files,
            "xtask/Cargo.toml",
            "kosmo = { path = \"../desktop/kosmo\" }\n",
        );
    });
    assert_eq!(
        found,
        ["xtask/Cargo.toml: [dependencies] kosmo: xtask may not have path dependencies"]
    );
}

// The repository boundary.

#[test]
fn rejects_krepi_packages() {
    let found = problems_with(|files| {
        declare(files, "desktop", "krepi-abi = \"=0.1.0\"");
        append(
            files,
            "desktop/shell/Cargo.toml",
            "krepi-abi.workspace = true\n",
        );
        append(files, "xtask/Cargo.toml", "krepi_sys = \"=0.1.0\"\n");
    });
    assert_problems(
        &found,
        &[
            "desktop/Cargo.toml: [workspace.dependencies] krepi-abi: kosmo does not depend on krepi: no package is named krepi or starts with krepi- or krepi_",
            "xtask/Cargo.toml: [dependencies] krepi_sys: kosmo does not depend on krepi: no package is named krepi or starts with krepi- or krepi_",
        ],
    );
}

#[test]
fn paths_leaving_the_repository_report_only_that() {
    let found = problems_with(|files| {
        declare(
            files,
            "desktop",
            "outside = { path = \"../../outside\", version = \"=0.1.0-dev.1\" }",
        );
        append(
            files,
            "xtask/Cargo.toml",
            "outside = { path = \"../../outside\" }\n",
        );
    });
    assert_problems(
        &found,
        &[
            "desktop/Cargo.toml: [workspace.dependencies] outside: the path ../../outside leaves the repository",
            "xtask/Cargo.toml: [dependencies] outside: the path ../../outside leaves the repository",
        ],
    );
}

#[test]
fn rejects_submodules() {
    let mut tree = tree(&valid());
    tree.insert("desktop/vendor", Entry::Submodule);
    assert_eq!(
        check(&tree, &ALL),
        ["desktop/vendor: submodules are not allowed"]
    );
}

#[test]
fn symbolic_links_stay_inside_their_workspace_or_the_repository() {
    let mut tree = tree(&valid());
    tree.insert(
        "desktop/kosmo/link",
        Entry::Symlink("../../framework".to_string()),
    );
    tree.insert("docs/out", Entry::Symlink("../../x".to_string()));
    tree.insert("docs/in", Entry::Symlink("../desktop".to_string()));
    assert_problems(
        &check(&tree, &ALL),
        &[
            "desktop/kosmo/link: the symbolic link to ../../framework leaves desktop/",
            "docs/out: the symbolic link to ../../x leaves the repository",
        ],
    );
}

#[test]
fn source_and_target_paths_stay_inside_their_workspace() {
    let found = problems_with(|files| {
        files.insert(
            "desktop/kosmo/src/main.rs".to_string(),
            "fn main() {\n    let _ = include_str!(\n        \"../../../framework/notes.txt\"\n    );\n}\n#[path = \"../../../display/shared.rs\"]\nmod shared;\n".to_string(),
        );
        append(
            files,
            "desktop/kosmo/Cargo.toml",
            "\n[lib]\npath = \"../../framework/lib.rs\"\n",
        );
    });
    assert_problems(
        &found,
        &[
            "desktop/kosmo/src/main.rs:3: the source path ../../../framework/notes.txt leaves desktop/",
            "desktop/kosmo/src/main.rs:6: the source path ../../../display/shared.rs leaves desktop/",
            "desktop/kosmo/Cargo.toml: the target path ../../framework/lib.rs leaves desktop/",
        ],
    );
}

#[test]
fn paths_in_cfg_attr_and_inline_modules_stay_inside_their_workspace() {
    let found = problems_with(|files| {
        files.insert(
            "desktop/kosmo/src/main.rs".to_string(),
            "#[cfg_attr(unix, path = \"../../../display/shared.rs\")]\nmod shared;\nmod inline {\n    #[path = \"../../../../framework/x.rs\"]\n    mod x;\n}\nfn main() {}\n".to_string(),
        );
    });
    assert_problems(
        &found,
        &[
            "desktop/kosmo/src/main.rs:1: the source path ../../../display/shared.rs leaves desktop/",
            "desktop/kosmo/src/main.rs:4: the source path ../../../../framework/x.rs leaves desktop/",
        ],
    );
}

#[test]
fn paths_behind_raw_identifiers_stay_inside_their_workspace() {
    let found = problems_with(|files| {
        files.insert(
            "desktop/kosmo/src/main.rs".to_string(),
            "#[r#path = \"../../../display/shared.rs\"]\nmod shared;\n#[r#cfg_attr(unix, path = \"../../../display/other.rs\")]\nmod other;\nr#include!(\"../../../framework/code.rs\");\nconst S: &str = r#include_str!(\"../../../framework/notes.txt\");\nfn main() {}\n".to_string(),
        );
    });
    assert_problems(
        &found,
        &[
            "desktop/kosmo/src/main.rs:1: the source path ../../../display/shared.rs leaves desktop/",
            "desktop/kosmo/src/main.rs:3: the source path ../../../display/other.rs leaves desktop/",
            "desktop/kosmo/src/main.rs:5: the source path ../../../framework/code.rs leaves desktop/",
            "desktop/kosmo/src/main.rs:6: the source path ../../../framework/notes.txt leaves desktop/",
        ],
    );
}

#[test]
fn inner_paths_of_inline_modules_stay_inside_their_workspace() {
    let found = problems_with(|files| {
        files.insert(
            "desktop/kosmo/src/main.rs".to_string(),
            "mod m {\n    #![path = \"../../../display\"]\n    mod n;\n}\nmod c {\n    #![cfg_attr(unix, path = \"../..\")]\n    #[path = \"../framework/x.rs\"]\n    mod x;\n}\nfn main() {}\n".to_string(),
        );
    });
    // The directory ../.. of c stays inside desktop/, but x.rs, read from
    // it, does not.
    assert_problems(
        &found,
        &[
            "desktop/kosmo/src/main.rs:2: the source path ../../../display leaves desktop/",
            "desktop/kosmo/src/main.rs:7: the source path ../framework/x.rs leaves desktop/",
        ],
    );
}

#[test]
fn paths_do_not_pass_through_symbolic_links() {
    let mut files = valid();
    files.insert(
        "desktop/kosmo/src/main.rs".to_string(),
        "const S: &str = include_str!(\"up/../../outside.txt\");\nconst A: &str = include_str!(\"/up/x.txt\");\nfn main() {}\n".to_string(),
    );
    append(
        &mut files,
        "desktop/kosmo/Cargo.toml",
        "\n[[bin]]\nname = \"other\"\npath = \"src/up/../other.rs\"\n",
    );
    edit(
        &mut files,
        "desktop/Cargo.toml",
        "kosmo-calc = { path = \"calc\"",
        "kosmo-calc = { path = \"kosmo/src/up/calc\"",
    );
    let mut tree = tree(&files);
    // The link itself stays inside desktop/.
    tree.insert("desktop/kosmo/src/up", Entry::Symlink("../..".to_string()));
    tree.insert(
        "desktop/kosmo/far",
        Entry::Symlink("src/up/../x".to_string()),
    );
    let rule = "passes through the symbolic link desktop/kosmo/src/up, which the layout step does not follow";
    assert_eq!(
        check(&tree, &ALL),
        [
            format!(
                "desktop/Cargo.toml: [workspace.dependencies] kosmo-calc: the path kosmo/src/up/calc {rule}"
            ),
            format!("desktop/kosmo/Cargo.toml: the target path src/up/../other.rs {rule}"),
            format!("desktop/kosmo/far: the symbolic link to src/up/../x {rule}"),
            format!("desktop/kosmo/src/main.rs:1: the source path up/../../outside.txt {rule}"),
            // An absolute path leaves the repository before any link.
            "desktop/kosmo/src/main.rs:2: the source path /up/x.txt leaves the repository"
                .to_string(),
        ]
    );
}

#[test]
fn manifest_paths_stay_inside_the_repository() {
    let found = problems_with(|files| {
        edit(
            files,
            "desktop/kosmo/Cargo.toml",
            "name = \"kosmo\"\n",
            "name = \"kosmo\"\nworkspace = \"../../../outside\"\nreadme = \"../../../outside/README.md\"\n",
        );
        edit(
            files,
            "desktop/Cargo.toml",
            "license = \"MIT\"\n",
            "license = \"MIT\"\nlicense-file = \"../../LICENSE\"\n",
        );
    });
    assert_problems(
        &found,
        &[
            // A crate of the table sets no package.workspace at all.
            "desktop/kosmo/Cargo.toml: the crate kosmo may not set package.workspace; it is a member of the root of its workspace",
            "desktop/kosmo/Cargo.toml: the path ../../../outside of package.workspace leaves the repository",
            "desktop/kosmo/Cargo.toml: the path ../../../outside/README.md of package.readme leaves the repository",
            "desktop/Cargo.toml: the path ../../LICENSE of workspace.package.license-file leaves the repository",
        ],
    );
}

#[test]
fn a_rust_file_that_is_not_rust_tokens_is_reported_with_its_line() {
    let found = problems_with(|files| {
        files.insert(
            "docs/example.rs".to_string(),
            "fn f() {}\nconst S: &str = \"open;\n".to_string(),
        );
    });
    assert_eq!(found.len(), 1, "{found:#?}");
    assert!(
        found[0].starts_with("docs/example.rs:2: cannot be read as Rust tokens"),
        "{found:#?}"
    );
}

// The Cargo configuration.

#[test]
fn the_cargo_configuration_holds_only_the_xtask_alias() {
    let found = problems_with(|files| {
        append(
            files,
            CARGO_CONFIG,
            "b = \"build\"\n\n[build]\nrustflags = [\"-Ctarget-cpu=native\"]\n",
        );
    });
    assert_problems(
        &found,
        &[
            ".cargo/config.toml: may hold only [alias], not [build]",
            ".cargo/config.toml: may define only the alias xtask, not b",
        ],
    );
}

#[test]
fn there_is_no_other_cargo_configuration() {
    let found = problems_with(|files| {
        for path in [
            "desktop/.cargo/config.toml",
            ".cargo/config",
            ".CARGO/Config.toml",
        ] {
            files.insert(path.to_string(), "[alias]\n".to_string());
        }
    });
    assert_problems(
        &found,
        &[
            "desktop/.cargo/config.toml: the only Cargo configuration is .cargo/config.toml, a regular file",
            ".cargo/config: the only Cargo configuration is .cargo/config.toml, a regular file",
            ".CARGO/Config.toml: the only Cargo configuration is .cargo/config.toml, a regular file",
        ],
    );
}

// Pure-core crates.

#[test]
fn a_pure_core_crate_is_unconditionally_no_std() {
    let found = problems_with(|files| {
        edit(
            files,
            "display/types/src/lib.rs",
            "#![no_std]",
            "#![cfg_attr(not(test), no_std)]",
        );
    });
    assert_eq!(
        found,
        [
            "display/types/src/lib.rs: the pure-core crate kosmo-types must have an unconditional #![no_std]"
        ]
    );
}

#[test]
fn a_pure_core_crate_forbids_unsafe_code() {
    let found = problems_with(|files| {
        edit(
            files,
            "display/types/src/lib.rs",
            "#![forbid(unsafe_code)]\n",
            "",
        );
    });
    assert_eq!(
        found,
        [
            "display/types/src/lib.rs: the pure-core crate kosmo-types must have #![forbid(unsafe_code)]"
        ]
    );
}

#[test]
fn a_pure_core_crate_has_no_build_script_even_when_marked() {
    let found = problems_with(|files| {
        files.insert(
            "display/types/build.rs".to_string(),
            "fn main() {}\n".to_string(),
        );
        edit(
            files,
            TABLE,
            "[crates.kosmo-types]\npath = \"display/types\"\n",
            "[crates.kosmo-types]\npath = \"display/types\"\nbuild-script = true\n",
        );
    });
    assert_eq!(
        found,
        ["display/types/Cargo.toml: the pure-core crate kosmo-types may not have a build script"]
    );
}

#[test]
fn a_pure_core_crate_has_no_external_dependencies() {
    let found = problems_with(|files| {
        declare(files, "display", "foo = \"=1.0.0\"");
        append(
            files,
            "display/scene/Cargo.toml",
            "\n[dev-dependencies]\nfoo.workspace = true\n",
        );
    });
    assert_eq!(
        found,
        [
            "display/scene/Cargo.toml: [dev-dependencies] foo: the pure-core crate kosmo-scene may not have external dependencies"
        ]
    );
}

#[test]
fn interior_mutability_is_found_in_macros_and_raw_identifiers() {
    let found = problems_with(|files| {
        append(
            files,
            "display/scene/src/lib.rs",
            "use core::sync::atomic;\nm!(Cell::new(1));\npub fn f(r#OnceCell: u8) {}\npub type U = UnsafeCell<u8>;\n",
        );
    });
    let rule = "the pure-core crate kosmo-scene may not use";
    assert_eq!(
        found,
        [
            format!("display/scene/src/lib.rs:7: {rule} atomic"),
            format!("display/scene/src/lib.rs:8: {rule} Cell"),
            format!("display/scene/src/lib.rs:9: {rule} OnceCell"),
            format!("display/scene/src/lib.rs:10: {rule} UnsafeCell"),
        ]
    );
}

#[test]
fn a_pure_core_crate_uses_std_only_in_tests() {
    let found = problems_with(|files| {
        append(files, "display/scene/src/lib.rs", "extern crate std;\n");
    });
    assert_eq!(
        found,
        [
            "display/scene/src/lib.rs:7: the pure-core crate kosmo-scene may not use extern crate std outside #[cfg(test)]; the attribute stands directly on the extern crate item"
        ]
    );
}

#[test]
fn a_pure_core_crate_keeps_its_code_under_src() {
    let root = problems_with(|files| {
        append(
            files,
            "display/scene/Cargo.toml",
            "\n[lib]\npath = \"lib.rs\"\n",
        );
        let text = files
            .remove("display/scene/src/lib.rs")
            .expect("the library root exists");
        files.insert(
            "display/scene/lib.rs".to_string(),
            format!("{text}use core::cell::RefCell;\n"),
        );
    });
    assert_eq!(
        root,
        [
            "display/scene/Cargo.toml: the library root display/scene/lib.rs of the pure-core crate kosmo-scene is not a .rs file under display/scene/src/"
        ]
    );
    let included = problems_with(|files| {
        append(
            files,
            "display/scene/src/lib.rs",
            "#[path = \"../imp.rs\"]\nmod imp;\ninclude!(\"../more.rs\");\n#[path = \"inner.rs\"]\nmod inner;\npub const DATA: &str = include_str!(\"../data.txt\");\n",
        );
    });
    let rule = "the pure-core crate kosmo-scene brings in Rust code only from .rs files under display/scene/src/";
    assert_eq!(
        included,
        [
            format!("display/scene/src/lib.rs:7: {rule}, not ../imp.rs"),
            format!("display/scene/src/lib.rs:9: {rule}, not ../more.rs"),
        ]
    );
    // A #[path] inside an inline module starts from the module's
    // directory, and a #[path] on an inline module gives the directory of
    // the modules inside it; one inside cfg_attr counts as well.
    let modules = problems_with(|files| {
        append(
            files,
            "display/scene/src/lib.rs",
            "mod a {\n    #[path = \"../b.rs\"]\n    mod b;\n}\n#[cfg_attr(all(), path = \"../imp.rs\")]\nmod imp;\n#[path = \"..\"]\nmod up {\n    mod hidden;\n}\n",
        );
        files.insert("display/scene/src/b.rs".to_string(), String::new());
    });
    assert_eq!(
        modules,
        [
            format!("display/scene/src/lib.rs:11: {rule}, not ../imp.rs"),
            format!("display/scene/src/lib.rs:13: {rule}, not .."),
        ]
    );
    // A library root outside the workspace is reported only as that.
    let outside = problems_with(|files| {
        append(
            files,
            "display/scene/Cargo.toml",
            "\n[lib]\npath = \"../../framework/ui/src/lib.rs\"\n",
        );
    });
    assert_eq!(
        outside,
        ["display/scene/Cargo.toml: the target path ../../framework/ui/src/lib.rs leaves display/"]
    );
}

#[test]
fn a_pure_core_crate_keeps_code_behind_raw_identifiers_under_src() {
    let found = problems_with(|files| {
        append(
            files,
            "display/scene/src/lib.rs",
            "#[r#path = \"../imp.rs\"]\nmod imp;\nr#include!(\"../more.rs\");\n#[cfg_attr(all(), r#path = \"../cfg.rs\")]\nmod cfg;\n#[r#cfg_attr(all(), path = \"..\")]\nmod up {\n    mod hidden;\n}\n",
        );
    });
    let rule = "the pure-core crate kosmo-scene brings in Rust code only from .rs files under display/scene/src/";
    assert_eq!(
        found,
        [
            format!("display/scene/src/lib.rs:7: {rule}, not ../imp.rs"),
            format!("display/scene/src/lib.rs:9: {rule}, not ../more.rs"),
            format!("display/scene/src/lib.rs:10: {rule}, not ../cfg.rs"),
            format!("display/scene/src/lib.rs:12: {rule}, not .."),
        ]
    );
}

#[test]
fn a_pure_core_crate_keeps_the_inner_paths_of_inline_modules_under_src() {
    let found = problems_with(|files| {
        append(
            files,
            "display/scene/src/lib.rs",
            "mod a {\n    #![path = \"..\"]\n    mod hidden;\n}\nmod b {\n    #![cfg_attr(all(), path = \"../..\")]\n    mod c {\n        #[path = \"d.rs\"]\n        mod d;\n    }\n}\nmod e {\n    #![path = \"f/h\"]\n    #[path = \"../../g.rs\"]\n    mod g;\n}\n",
        );
    });
    // Under its condition, b reads c from display/, so d.rs may be
    // display/c/d.rs. The unconditional f/h replaces the name e, so g.rs is
    // src/g.rs.
    let rule = "the pure-core crate kosmo-scene brings in Rust code only from .rs files under display/scene/src/";
    assert_eq!(
        found,
        [
            format!("display/scene/src/lib.rs:8: {rule}, not .."),
            format!("display/scene/src/lib.rs:12: {rule}, not ../.."),
            format!("display/scene/src/lib.rs:14: {rule}, not d.rs"),
        ]
    );
}

#[test]
fn a_pure_core_crate_keeps_its_sources_as_regular_files() {
    let rule = "the pure-core crate kosmo-scene keeps its sources under display/scene/src/ as regular files, not symbolic links";
    let mut files = valid();
    files.insert(
        "display/server/src/cells.rs".to_string(),
        "use core::cell::RefCell;\npub type R = RefCell<u8>;\n".to_string(),
    );
    append(&mut files, "display/scene/src/lib.rs", "mod imp;\n");
    let mut module = tree(&files);
    module.insert(
        "display/scene/src/imp.rs",
        Entry::Symlink("../../server/src/cells.rs".to_string()),
    );
    assert_eq!(
        check(&module, &ALL),
        [format!("display/scene/src/imp.rs: {rule}")]
    );
    let mut files = valid();
    let text = files
        .remove("display/scene/src/lib.rs")
        .expect("the library root exists");
    files.insert("display/server/src/core.rs".to_string(), text);
    let mut root = tree(&files);
    root.insert(
        "display/scene/src/lib.rs",
        Entry::Symlink("../../server/src/core.rs".to_string()),
    );
    assert_eq!(
        check(&root, &ALL),
        [format!("display/scene/src/lib.rs: {rule}")]
    );
    let mut files = valid();
    files.remove("display/scene/src/lib.rs");
    let mut directory = tree(&files);
    directory.insert(
        "display/scene/src",
        Entry::Symlink("../server/src".to_string()),
    );
    assert_eq!(
        check(&directory, &ALL),
        [format!("display/scene/src: {rule}")]
    );
}

// Build scripts and procedural macros.

#[test]
fn build_scripts_need_the_mark() {
    let file = problems_with(|files| {
        files.insert(
            "display/server/build.rs".to_string(),
            "fn main() {}\n".to_string(),
        );
    });
    let key = problems_with(|files| {
        edit(
            files,
            "display/server/Cargo.toml",
            "name = \"kosmo-server\"\n",
            "name = \"kosmo-server\"\nbuild = \"gen.rs\"\n",
        );
    });
    for found in [file, key] {
        assert_eq!(
            found,
            [
                "display/server/Cargo.toml: the crate kosmo-server has a build script, which the layout table does not allow"
            ]
        );
    }
}

#[test]
fn procedural_macros_need_the_mark_and_marks_need_their_target() {
    let proc_macro = problems_with(|files| {
        append(
            files,
            "framework/render/Cargo.toml",
            "\n[lib]\nproc-macro = true\n",
        );
    });
    assert_eq!(
        proc_macro,
        [
            "framework/render/Cargo.toml: the crate kosmo-render is a procedural-macro library, which the layout table does not allow"
        ]
    );
    // The crate type makes a procedural-macro library as well.
    let crate_type = problems_with(|files| {
        append(
            files,
            "framework/render/Cargo.toml",
            "\n[lib]\ncrate-type = [\"rlib\", \"proc-macro\"]\n",
        );
    });
    assert_eq!(crate_type, proc_macro);
    let underscore = problems_with(|files| {
        append(
            files,
            "framework/render/Cargo.toml",
            "\n[lib]\ncrate_type = [\"rlib\"]\n",
        );
    });
    assert_eq!(
        underscore,
        ["framework/render/Cargo.toml: [lib] crate_type is not allowed; write crate-type"]
    );
    let unused = problems_with(|files| {
        edit(
            files,
            TABLE,
            "[crates.kosmo-render]\npath = \"framework/render\"\n",
            "[crates.kosmo-render]\npath = \"framework/render\"\nbuild-script = true\n",
        );
    });
    assert_eq!(
        unused,
        [
            "framework/render/Cargo.toml: the crate kosmo-render is marked build-script but has no build script"
        ]
    );
}

#[test]
fn tool_workspaces_have_no_build_script_or_procedural_macro() {
    let found = problems_with(|files| {
        files.insert("xtask/build.rs".to_string(), "fn main() {}\n".to_string());
        append(files, "xtask/Cargo.toml", "\n[lib]\nproc-macro = true\n");
    });
    assert_problems(
        &found,
        &[
            "xtask/Cargo.toml: the tool workspace xtask may not have a build script",
            "xtask/Cargo.toml: the tool workspace xtask may not be a procedural-macro library",
        ],
    );
    let crate_type = problems_with(|files| {
        append(
            files,
            "xtask/Cargo.toml",
            "\n[lib]\ncrate-type = [\"proc-macro\"]\n",
        );
    });
    assert_eq!(
        crate_type,
        ["xtask/Cargo.toml: the tool workspace xtask may not be a procedural-macro library"]
    );
}

#[test]
fn tool_workspaces_check_the_dependencies_they_inherit_where_declared() {
    let found = problems_with(|files| {
        append(
            files,
            "xtask/Cargo.toml",
            "g.workspace = true\n\n[workspace.dependencies]\ng = { git = \"https://example.org/g\" }\n",
        );
    });
    assert_problems(
        &found,
        &[
            "xtask/Cargo.toml: [workspace.dependencies] g: may not use git",
            "xtask/Cargo.toml: [workspace.dependencies] g: the version must be exact (=x.y.z), not missing",
        ],
    );
}

// Versions and publishing.

#[test]
fn every_crate_has_the_version_of_the_program() {
    let other = problems_with(|files| {
        edit(
            files,
            "desktop/calc/Cargo.toml",
            "version.workspace = true",
            "version = \"0.1.0\"",
        );
    });
    assert_eq!(
        other,
        ["desktop/calc/Cargo.toml: the version 0.1.0 differs from 0.1.0-dev.1 of kosmo"]
    );
    let without_program = problems_with(|files| {
        edit(
            files,
            TABLE,
            "\n[crates.kosmo]\npath = \"desktop/kosmo\"\nlayer = \"S2\"\nexport = false\npure-core = false\npublish = false\n",
            "",
        );
        files.remove("desktop/kosmo/Cargo.toml");
        files.remove("desktop/kosmo/src/main.rs");
        edit(files, "desktop/Cargo.toml", ", \"kosmo\"]", "]");
    });
    assert_eq!(
        without_program,
        ["layout.toml: the program crate kosmo is not in the layout table"]
    );
}

#[test]
fn the_program_crate_has_a_version() {
    let expected = "desktop/kosmo/Cargo.toml: the program crate kosmo has no version; every crate has the version of kosmo";
    let removed = problems_with(|files| {
        edit(
            files,
            "desktop/kosmo/Cargo.toml",
            "version.workspace = true\n",
            "",
        );
    });
    assert_eq!(removed, [expected]);
    let number = problems_with(|files| {
        edit(
            files,
            "desktop/kosmo/Cargo.toml",
            "version.workspace = true",
            "version = 1",
        );
    });
    assert_eq!(number, [expected]);
    let not_inherited = problems_with(|files| {
        edit(
            files,
            "desktop/Cargo.toml",
            "version = \"0.1.0-dev.1\"\nedition",
            "edition",
        );
    });
    assert_eq!(not_inherited, [expected]);
}

#[test]
fn publish_agrees_with_the_table() {
    let unset = problems_with(|files| {
        edit(files, "desktop/shell/Cargo.toml", "publish = false\n", "");
    });
    assert_eq!(
        unset,
        ["desktop/shell/Cargo.toml: publish must be false, as the layout table marks the crate"]
    );
    let publishable = problems_with(|files| {
        edit(
            files,
            TABLE,
            "[crates.kosmo-types]\npath = \"display/types\"\nlayer = \"D0\"\npublish = false\n",
            "[crates.kosmo-types]\npath = \"display/types\"\nlayer = \"D0\"\npublish = true\n",
        );
    });
    assert_eq!(
        publishable,
        [
            "display/types/Cargo.toml: publish may not be false; the layout table marks the crate publishable"
        ]
    );
    let never = problems_with(|files| {
        edit(
            files,
            TABLE,
            "layer = \"D3\"\nexport = false\npure-core = false\npublish = false\n",
            "layer = \"D4\"\nexport = false\npure-core = false\npublish = true\n",
        );
    });
    assert_eq!(
        never,
        ["layout.toml: crates.kosmo-server: layer D4 is never published"]
    );
}

// Provisional crates.

#[test]
fn a_pure_core_crate_does_not_test_with_a_provisional_crate() {
    let found = problems_with(|files| {
        append(
            files,
            "display/scene/Cargo.toml",
            "\n[dev-dependencies]\nkosmo-proto.workspace = true\n",
        );
    });
    assert_eq!(
        found,
        [
            "display/scene/Cargo.toml: [dev-dependencies] kosmo-proto: the pure-core crate kosmo-scene may not depend on the provisional crate kosmo-proto"
        ]
    );
}

// The platform boundary.

/// The problem of a forbidden item at `line` of `file`, which `who` may not
/// use and the platform category `allowed_by` allows, if one does.
fn forbidden(file: &str, line: usize, who: &str, item: &str, allowed_by: Option<&str>) -> String {
    let allowed_by = match allowed_by {
        Some(category) => {
            format!("only a crate registered for the platform category {category} may")
        }
        None => "no platform category allows it".to_string(),
    };
    format!("{file}:{line}: {who} may not use {item}; {allowed_by}")
}

/// The library root of the core library `kosmo-render`, whose first line is
/// its documentation.
const RENDER_LIB: &str = "framework/render/src/lib.rs";

/// The problems of `kosmo-render` with `code` after the first line of its
/// library root, as (line, item, category) of the expected problems.
#[track_caller]
fn assert_render_uses(code: &str, expected: &[(usize, &str, Option<&str>)]) {
    let found = problems_with(|files| append(files, RENDER_LIB, code));
    let expected: Vec<String> = expected
        .iter()
        .map(|&(line, item, category)| {
            forbidden(
                RENDER_LIB,
                line,
                "the core library kosmo-render",
                item,
                category,
            )
        })
        .collect();
    assert_eq!(found, expected);
}

#[test]
fn use_trees_renames_and_paths_from_the_root_are_expanded() {
    assert_render_uses(
        "use std::{fs, io::Read};\n",
        &[
            (2, "std::fs", None),
            (2, "std::io", Some("standard-streams")),
        ],
    );
    assert_render_uses(
        "use std::fs as files;\npub fn f() {\n    let _ = files::read(\"x\");\n}\n",
        &[(2, "std::fs", None), (4, "std::fs", None)],
    );
    assert_render_uses(
        "pub fn f() -> bool {\n    ::std::path::Path::new(\"x\").is_absolute()\n}\n",
        &[(3, "std::path", None)],
    );
    assert_render_uses("use std::net::*;\n", &[(2, "std::net", None)]);
    assert_render_uses(
        "use std as s;\npub fn f() {\n    s::thread::spawn(|| {});\n}\n",
        &[(4, "std::thread", Some("threads"))],
    );
    assert_render_uses(
        "use std::os::unix::net::UnixStream;\n",
        &[(2, "std::os", None)],
    );
}

#[test]
fn unit_tests_and_inline_modules_count() {
    assert_render_uses(
        "#[cfg(test)]\nmod tests {\n    use std::time::Instant;\n}\n",
        &[(4, "std::time::Instant", Some("real-clock"))],
    );
    assert_render_uses(
        "mod inner {\n    use std::process::Command;\n}\n",
        &[(3, "std::process", None)],
    );
}

#[test]
fn forbidden_macros_types_and_functions_are_found() {
    assert_render_uses(
        "pub fn f() {\n    println!(\"a\");\n    eprint!(\"b\");\n    dbg!(1);\n}\n",
        &[
            (3, "println!", Some("standard-streams")),
            (4, "eprint!", Some("standard-streams")),
            (5, "dbg!", Some("standard-streams")),
        ],
    );
    assert_render_uses(
        "thread_local! {\n    static N: u8 = 0;\n}\n",
        &[(2, "thread_local!", Some("threads"))],
    );
    assert_render_uses(
        "pub fn f() -> bool {\n    std::env::var(\"X\").is_ok()\n}\n",
        &[(3, "std::env", None)],
    );
    assert_render_uses(
        "use std::ffi::{OsStr, OsString};\n",
        &[
            (2, "std::ffi::OsStr", Some("arguments")),
            (2, "std::ffi::OsString", Some("arguments")),
        ],
    );
    assert_render_uses(
        "pub fn f() {\n    let _ = std::time::SystemTime::now();\n    let _ = std::time::UNIX_EPOCH;\n}\n",
        &[
            (3, "std::time::SystemTime", Some("real-clock")),
            (4, "std::time::UNIX_EPOCH", Some("real-clock")),
        ],
    );
}

#[test]
fn kalyx_clocks_are_found() {
    let lib = "framework/math-kalyx/src/lib.rs";
    let found = problems_with(|files| {
        append(
            files,
            lib,
            "use kalyx::MonotonicClock;\npub fn f() {\n    let _ = kalyx::Context::new(limits, cancel);\n    let _ = kalyx::Context::with_observers(limits, cancel);\n}\n",
        );
    });
    let who = "the core library kosmo-math-kalyx";
    assert_eq!(
        found,
        [
            forbidden(lib, 2, who, "kalyx::MonotonicClock", Some("real-clock")),
            forbidden(lib, 4, who, "kalyx::Context::new", Some("real-clock")),
        ]
    );
}

#[test]
fn a_registered_crate_uses_only_what_its_categories_allow() {
    let lib = "framework/jobs/src/lib.rs";
    let found = problems_with(|files| {
        append(
            files,
            lib,
            "use std::thread::Builder;\npub fn f() {\n    let _ = std::fs::read(\"x\");\n    let _ = Builder::new();\n}\n",
        );
    });
    assert_eq!(
        found,
        [forbidden(
            lib,
            4,
            "the crate kosmo-jobs, registered for threads,",
            "std::fs",
            None
        )]
    );
    let lib = "desktop/kosmo/src/lib.rs";
    let found = problems_with(|files| {
        files.insert(
            lib.to_string(),
            "use std::io::{self, Write};\nuse std::process::ExitCode;\nuse std::time::Instant;\npub fn f() -> ExitCode {\n    let args: Vec<std::ffi::OsString> = std::env::args_os().collect();\n    let _ = (args, Instant::now(), io::stdout().flush());\n    std::process::exit(0)\n}\n".to_string(),
        );
    });
    assert_eq!(found, Vec::<String>::new());
    let found = problems_with(|files| {
        files.insert(
            lib.to_string(),
            "pub fn f() {\n    let _ = std::env::var(\"X\");\n    std::thread::yield_now();\n}\n"
                .to_string(),
        );
    });
    let who = "the crate kosmo, registered for standard-streams, arguments, exit-code, real-clock,";
    assert_eq!(
        found,
        [
            forbidden(lib, 2, who, "std::env", None),
            forbidden(lib, 3, who, "std::thread", Some("threads")),
        ]
    );
}

#[test]
fn compile_time_env_comments_and_strings_are_not_uses() {
    assert_render_uses(
        "//! Mentions std::fs and println! in a comment.\n/// ```compile_fail\n/// let _ = std::fs::read(\"x\");\n/// ```\npub const NAME: &str = env!(\"CARGO_PKG_VERSION\");\npub const PATH: &str = \"std::fs::read\";\npub const ALSO: Option<&str> = core::option_env!(\"X\");\n",
        &[],
    );
}

#[test]
fn core_alloc_and_allowed_std_items_are_not_uses() {
    assert_render_uses(
        "use core::cell::Cell;\nuse std::collections::BTreeMap;\nuse std::sync::Arc;\nextern crate alloc;\nuse alloc::vec::Vec;\npub struct Instant;\nmod thread {\n    pub fn spawn() {}\n}\npub fn f() {\n    let _ = Instant;\n    thread::spawn();\n    let _: Option<(Cell<u8>, BTreeMap<u8, u8>, Arc<u8>, Vec<u8>)> = None;\n}\n",
        &[],
    );
}

#[test]
fn a_crate_without_a_library_is_not_checked() {
    let found = problems_with(|files| {
        files.remove("desktop/calc/src/lib.rs");
        files.insert(
            "desktop/calc/src/main.rs".to_string(),
            "fn main() {\n    println!(\"{:?}\", std::fs::read(\"x\"));\n}\n".to_string(),
        );
    });
    assert_eq!(found, Vec::<String>::new());
}

#[test]
fn a_crate_without_a_library_registers_no_platform_category() {
    let registered = problems_with(|files| {
        files.remove("desktop/calc/src/lib.rs");
        files.insert(
            "desktop/calc/src/main.rs".to_string(),
            "fn main() {}\n".to_string(),
        );
        edit(
            files,
            TABLE,
            "[crates.kosmo-calc]\npath = \"desktop/calc\"\n",
            "[crates.kosmo-calc]\npath = \"desktop/calc\"\nplatform = [\"threads\"]\n",
        );
    });
    assert_eq!(
        registered,
        [
            "desktop/calc/Cargo.toml: the crate kosmo-calc lists platform categories but has no library target; only a library crate registers them"
        ]
    );
}

#[test]
fn the_platform_column_names_known_categories_for_crates_that_are_not_pure_core() {
    let unknown = problems_with(|files| {
        edit(
            files,
            TABLE,
            "platform = [\"threads\"]\n",
            "platform = [\"threads\", \"windows\", \"threads\"]\n",
        );
    });
    assert_problems(
        &unknown,
        &[
            "layout.toml: crates.kosmo-jobs: platform lists the unknown category windows; the categories are standard-streams, arguments, exit-code, real-clock, threads",
            "layout.toml: crates.kosmo-jobs: platform lists threads twice",
        ],
    );
    let pure = problems_with(|files| {
        edit(
            files,
            TABLE,
            "[crates.kosmo-types]\npath = \"display/types\"\n",
            "[crates.kosmo-types]\npath = \"display/types\"\nplatform = [\"threads\"]\n",
        );
    });
    assert_eq!(
        pure,
        [
            "layout.toml: crates.kosmo-types: a pure-core crate is a core library and registers no platform category"
        ]
    );
    let empty = problems_with(|files| {
        edit(
            files,
            TABLE,
            "platform = [\"threads\"]\n",
            "platform = []\n",
        );
    });
    assert_eq!(
        empty,
        [
            "layout.toml: crates.kosmo-jobs: platform lists no category; a core library leaves it out"
        ]
    );
}

// Coverage.

#[test]
fn the_steps_run_in_every_workspace_with_crates() {
    assert_eq!(
        check(&tree(&valid()), &["xtask", "desktop"]),
        [
            "xtask/src/check.rs: the fmt, clippy and test steps do not run in the workspace display",
            "xtask/src/check.rs: the fmt, clippy and test steps do not run in the workspace framework",
        ]
    );
}

// Reporting.

#[test]
fn reports_every_problem_sorted_by_path() {
    let found = problems_with(|files| {
        append(
            files,
            "framework/math-model/Cargo.toml",
            "\n[dependencies]\nkosmo-ui.workspace = true\n",
        );
        edit(
            files,
            "display/scene/src/lib.rs",
            "extern crate alloc;\n",
            "use core::cell::RefCell;\nextern crate alloc;\n",
        );
        files.insert(
            "desktop/extra/Cargo.toml".to_string(),
            crate_manifest("kosmo-extra", &[]),
        );
    });
    assert_eq!(
        found,
        [
            "desktop/extra/Cargo.toml: kosmo-extra is not a crate of the layout table",
            "display/scene/src/lib.rs:4: the pure-core crate kosmo-scene may not use cell, RefCell",
            "framework/math-model/Cargo.toml: [dependencies] kosmo-ui: a crate of layer F0 may not depend on kosmo-ui of the higher layer F1",
        ]
    );
}

#[test]
fn reports_are_ordered_by_path_then_line_number() {
    let mut problems = vec!["b.rs: x", "a.rs:10: y", "a.rs:9: z", "a.rs: w", "a.rs/c: v"];
    problems.sort_by(|a, b| report_order(a).cmp(&report_order(b)));
    assert_eq!(
        problems,
        ["a.rs: w", "a.rs:9: z", "a.rs:10: y", "a.rs/c: v", "b.rs: x"]
    );
}

// Facades.

#[test]
fn first_party_applications_reach_other_workspaces_only_through_facades() {
    let client = problems_with(|files| {
        append(
            files,
            "desktop/calc/Cargo.toml",
            "kosmo-client.workspace = true\n",
        );
    });
    assert_eq!(
        client,
        [
            "desktop/calc/Cargo.toml: [dependencies] kosmo-client: a crate of layer S1 depends on other workspaces only through facade crates; kosmo-client is not one"
        ]
    );
    let facade = problems_with(|files| {
        edit(
            files,
            TABLE,
            "[crates.kosmo-ui]\npath = \"framework/ui\"\n",
            "[crates.kosmo-ui]\npath = \"framework/ui\"\nfacade = true\n",
        );
    });
    assert_eq!(
        facade,
        ["layout.toml: crates.kosmo-ui: a facade must be an export crate"]
    );
}

// Export crates.

#[test]
fn an_export_crate_has_no_external_dependencies() {
    let found = problems_with(|files| {
        declare(files, "framework", "foo = \"=1.0.0\"");
        append(
            files,
            "framework/client/Cargo.toml",
            "foo.workspace = true\n",
        );
    });
    assert_eq!(
        found,
        [
            "framework/client/Cargo.toml: [dependencies] foo: the export crate kosmo-client may not have external dependencies"
        ]
    );
}

// kalyx.

/// The problem of a crate or tool other than the kalyx adapter whose
/// manifest declares kalyx in `table` under the key `key`.
fn declares_kalyx(manifest: &str, table: &str, key: &str) -> String {
    format!(
        "{manifest}: [{table}] {key}: declares the package kalyx, which only the kalyx adapter kosmo-math-kalyx may declare"
    )
}

#[test]
fn the_kalyx_adapter_may_declare_kalyx_in_any_table() {
    let found = problems_with(|files| {
        append(
            files,
            "framework/math-kalyx/Cargo.toml",
            "\n[dev-dependencies]\nkalyx.workspace = true\n\n[target.'cfg(unix)'.build-dependencies]\nkalyx.workspace = true\n",
        );
    });
    assert_eq!(found, Vec::<String>::new());
}

#[test]
fn no_other_crate_declares_kalyx_in_any_table() {
    // kosmo-ui is in layer F1, the layer of the kalyx adapter, and already
    // has a [dependencies] table.
    for manifest in ["framework/render/Cargo.toml", "framework/ui/Cargo.toml"] {
        for table in [
            "dependencies",
            "dev-dependencies",
            "build-dependencies",
            "target.'cfg(unix)'.dependencies",
            "target.'cfg(windows)'.dev-dependencies",
        ] {
            let found = problems_with(|files| {
                let header = format!("\n[{table}]\n");
                let declaration = format!("{header}kalyx.workspace = true\n");
                if files[manifest].contains(&header) {
                    edit(files, manifest, &header, &declaration);
                } else {
                    append(files, manifest, &declaration);
                }
            });
            assert_eq!(found, [declares_kalyx(manifest, table, "kalyx")]);
        }
    }
}

#[test]
fn an_export_crate_may_not_declare_kalyx_either() {
    // Two rules fail here; the kalyx rule must not lean on the rule for
    // external dependencies of export crates.
    let found = problems_with(|files| {
        append(
            files,
            "framework/client/Cargo.toml",
            "kalyx.workspace = true\n",
        );
    });
    assert_problems(
        &found,
        &[
            &declares_kalyx("framework/client/Cargo.toml", "dependencies", "kalyx"),
            "framework/client/Cargo.toml: [dependencies] kalyx: the export crate kosmo-client may not have external dependencies",
        ],
    );
}

#[test]
fn crates_of_other_workspaces_may_not_declare_kalyx() {
    let found = problems_with(|files| {
        declare(files, "desktop", "kalyx = \"=0.1.0\"");
        append(
            files,
            "desktop/shell/Cargo.toml",
            "kalyx.workspace = true\n",
        );
    });
    assert_eq!(
        found,
        [declares_kalyx(
            "desktop/shell/Cargo.toml",
            "dependencies",
            "kalyx"
        )]
    );
}

#[test]
fn kalyx_is_found_by_its_package_name_when_renamed() {
    let in_the_root = problems_with(|files| {
        declare(
            files,
            "framework",
            "calc = { version = \"=0.1.0\", package = \"kalyx\" }",
        );
        append(
            files,
            "framework/math-model/Cargo.toml",
            "\n[dependencies]\ncalc.workspace = true\n",
        );
    });
    assert_problems(
        &in_the_root,
        &[
            "framework/Cargo.toml: [workspace.dependencies] calc: may not rename a dependency",
            &declares_kalyx("framework/math-model/Cargo.toml", "dependencies", "calc"),
        ],
    );
    let in_the_crate = problems_with(|files| {
        append(
            files,
            "framework/math-model/Cargo.toml",
            "\n[dependencies]\ncalc = { version = \"=0.1.0\", package = \"kalyx\" }\n",
        );
    });
    assert_problems(
        &in_the_crate,
        &[
            "framework/math-model/Cargo.toml: [dependencies] calc: may not rename a dependency",
            "framework/math-model/Cargo.toml: [dependencies] calc: must be inherited with workspace = true from the workspace root",
            &declares_kalyx("framework/math-model/Cargo.toml", "dependencies", "calc"),
        ],
    );
}

#[test]
fn an_entry_that_is_not_inherited_is_not_named_by_the_root() {
    let found = problems_with(|files| {
        declare(
            files,
            "framework",
            "calc = { version = \"=0.1.0\", package = \"kalyx\" }",
        );
        append(
            files,
            "framework/math-model/Cargo.toml",
            "\n[dependencies]\ncalc = \"=1.0.0\"\n",
        );
    });
    assert_problems(
        &found,
        &[
            "framework/Cargo.toml: [workspace.dependencies] calc: may not rename a dependency",
            "framework/math-model/Cargo.toml: [dependencies] calc: must be inherited with workspace = true from the workspace root",
        ],
    );
}

#[test]
fn a_package_with_a_name_like_kalyx_is_not_kalyx() {
    let found = problems_with(|files| {
        declare(files, "framework", "kalyx-extra = \"=1.0.0\"");
        append(
            files,
            "framework/render/Cargo.toml",
            "\n[dependencies]\nkalyx-extra.workspace = true\n",
        );
    });
    assert_eq!(found, Vec::<String>::new());
}

#[test]
fn a_key_named_kalyx_that_renames_another_package_is_not_kalyx() {
    let found = problems_with(|files| {
        declare(
            files,
            "desktop",
            "kalyx = { version = \"=1.0.0\", package = \"other\" }",
        );
        append(
            files,
            "desktop/shell/Cargo.toml",
            "kalyx.workspace = true\n",
        );
    });
    assert_eq!(
        found,
        ["desktop/Cargo.toml: [workspace.dependencies] kalyx: may not rename a dependency"]
    );
}

#[test]
fn an_inherited_kalyx_is_kalyx_whatever_package_the_entry_names() {
    // Cargo takes the package of an inherited entry from the root and
    // ignores a `package` of the entry itself.
    let found = problems_with(|files| {
        append(
            files,
            "framework/math-model/Cargo.toml",
            "\n[dependencies]\nkalyx = { workspace = true, package = \"other\" }\n",
        );
    });
    assert_problems(
        &found,
        &[
            "framework/math-model/Cargo.toml: [dependencies] kalyx: may not rename a dependency",
            &declares_kalyx("framework/math-model/Cargo.toml", "dependencies", "kalyx"),
        ],
    );
}

#[test]
fn an_inherited_entry_naming_kalyx_is_not_kalyx_when_its_root_declares_another() {
    let found = problems_with(|files| {
        declare(files, "framework", "calc = \"=1.0.0\"");
        append(
            files,
            "framework/math-model/Cargo.toml",
            "\n[dependencies]\ncalc = { workspace = true, package = \"kalyx\" }\n",
        );
    });
    assert_eq!(
        found,
        ["framework/math-model/Cargo.toml: [dependencies] calc: may not rename a dependency"]
    );
}

#[test]
fn xtask_may_not_declare_kalyx() {
    let found = problems_with(|files| {
        append(
            files,
            "xtask/Cargo.toml",
            "kalyx = \"=0.1.0\"\nalgebra.workspace = true\n",
        );
        append(
            files,
            "xtask/Cargo.toml",
            "\n[target.'cfg(unix)'.dev-dependencies]\ncalc = { version = \"=0.1.0\", package = \"kalyx\" }\n\n[workspace.dependencies]\nkalyx = \"=0.1.0\"\nalgebra = { version = \"=0.1.0\", package = \"kalyx\" }\n",
        );
    });
    assert_problems(
        &found,
        &[
            &declares_kalyx("xtask/Cargo.toml", "dependencies", "kalyx"),
            &declares_kalyx(
                "xtask/Cargo.toml",
                "target.'cfg(unix)'.dev-dependencies",
                "calc",
            ),
            "xtask/Cargo.toml: [target.'cfg(unix)'.dev-dependencies] calc: may not rename a dependency",
            &declares_kalyx("xtask/Cargo.toml", "workspace.dependencies", "kalyx"),
            // `algebra`, inherited from the renaming declaration in the
            // `[workspace.dependencies]` of xtask itself.
            &declares_kalyx("xtask/Cargo.toml", "dependencies", "algebra"),
            &declares_kalyx("xtask/Cargo.toml", "workspace.dependencies", "algebra"),
            "xtask/Cargo.toml: [workspace.dependencies] algebra: may not rename a dependency",
        ],
    );
}

#[test]
fn an_optional_kalyx_is_still_kalyx() {
    let found = problems_with(|files| {
        // kosmo-ui already has a [dependencies] table.
        edit(
            files,
            "framework/ui/Cargo.toml",
            "\n[dependencies]\n",
            "\n[dependencies]\nkalyx = { workspace = true, optional = true }\n",
        );
    });
    assert_problems(
        &found,
        &[&declares_kalyx(
            "framework/ui/Cargo.toml",
            "dependencies",
            "kalyx",
        )],
    );
}

#[test]
fn an_inherited_kalyx_that_no_root_declares_is_still_kalyx() {
    // With no declaration to inherit from, the key names the package.
    let found = problems_with(|files| {
        append(files, "xtask/Cargo.toml", "kalyx.workspace = true\n");
    });
    assert_eq!(
        found,
        [declares_kalyx("xtask/Cargo.toml", "dependencies", "kalyx")]
    );
}

#[test]
fn an_inherited_kalyx_that_its_root_does_not_declare_is_still_kalyx() {
    // The root has a [workspace.dependencies] table, without kalyx in it.
    let found = problems_with(|files| {
        append(
            files,
            "desktop/shell/Cargo.toml",
            "kalyx.workspace = true\n",
        );
    });
    assert_problems(
        &found,
        &[
            &declares_kalyx("desktop/shell/Cargo.toml", "dependencies", "kalyx"),
            "desktop/shell/Cargo.toml: [dependencies] kalyx: desktop/Cargo.toml does not declare it",
        ],
    );
}

#[test]
fn kalyx_is_found_in_every_target_table_of_a_manifest() {
    // The first table, in the order in which the rule reads them, is
    // valid; the second declares kalyx.
    let found = problems_with(|files| {
        append(
            files,
            "framework/render/Cargo.toml",
            "\n[target.'cfg(unix)'.dependencies]\nkosmo-types.workspace = true\n\n[target.'cfg(windows)'.dependencies]\nkalyx.workspace = true\n",
        );
    });
    assert_eq!(
        found,
        [declares_kalyx(
            "framework/render/Cargo.toml",
            "target.'cfg(windows)'.dependencies",
            "kalyx"
        )]
    );
}

#[test]
fn a_crate_named_like_the_kalyx_adapter_may_not_declare_kalyx() {
    // Names that end with the adapter's name, or with which it ends. In
    // `framework/`, `math-kalyx` would have the adapter's directory, so it is
    // a crate of `display/`.
    for (name, workspace, layer) in [
        ("extra-kosmo-math-kalyx", "framework", "F1"),
        ("math-kalyx", "display", "D3"),
    ] {
        let mut crates = CRATES.to_vec();
        crates.push((name, workspace, layer, "", &["kalyx"]));
        let mut files = layout_of(&crates, &DECLARED);
        declare(&mut files, "framework", "kalyx = \"=0.1.0\"");
        if workspace != "framework" {
            declare(&mut files, workspace, "kalyx = \"=0.1.0\"");
        }
        assert_eq!(
            check(&tree(&files), &ALL),
            [declares_kalyx(
                &format!("{workspace}/{name}/Cargo.toml"),
                "dependencies",
                "kalyx"
            )]
        );
    }
}

#[test]
fn crates_of_the_display_workspace_may_not_declare_kalyx() {
    let found = problems_with(|files| {
        declare(files, "display", "kalyx = \"=0.1.0\"");
        append(
            files,
            "display/server/Cargo.toml",
            "kalyx.workspace = true\n",
        );
    });
    assert_eq!(
        found,
        [declares_kalyx(
            "display/server/Cargo.toml",
            "dependencies",
            "kalyx"
        )]
    );
}

#[test]
fn crates_may_reach_kalyx_through_the_adapter() {
    let found = problems_with(|files| {
        declare(
            files,
            "framework",
            "kosmo-math-kalyx = { path = \"math-kalyx\", version = \"=0.1.0-dev.1\" }",
        );
        append(
            files,
            "framework/sdk/Cargo.toml",
            "kosmo-math-kalyx.workspace = true\n",
        );
    });
    assert_eq!(found, Vec::<String>::new());
}
