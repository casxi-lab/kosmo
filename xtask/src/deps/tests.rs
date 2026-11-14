//! Tests of the deps step.
//!
//! Most tests change one thing in the valid model of the fixture, which has
//! crates in all three product workspaces, and expect exactly the problems
//! that the change causes. A few run Cargo itself in a scratch directory,
//! offline, with an empty Cargo home.

use std::fs;

use super::fixture::{self, DARWIN, Dep, LINUX, Model, Pkg, ROOT, STALE_LOCK, in_scratch, scratch};
use super::*;
use crate::check::OFFLINE;

/// Asserts that `found` has one problem for each of `expected`, each
/// containing its fragment.
#[track_caller]
pub fn assert_problems(found: &[String], expected: &[&str]) {
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

/// The problems of the valid model after `change`.
pub fn problems_with(change: impl FnOnce(&mut Model)) -> Vec<String> {
    let mut model = fixture::valid();
    change(&mut model);
    model.problems()
}

/// An allowlist entry of a crates.io package without a build script.
pub fn entry(
    name: &str,
    version: &str,
    license: &str,
    features: &[&str],
    crates: &[&str],
) -> String {
    format!(
        "\n[[package]]\nname = \"{name}\"\nversion = \"{version}\"\nsource = \"crates.io\"\nlicense = \"{license}\"\nbuild-script = false\nfeatures = {features:?}\ncrates = {crates:?}\n"
    )
}

/// An allow entry of `krate` for `name` with the requirement `version`,
/// followed by `more`, the remaining fields.
fn allow(krate: &str, name: &str, version: &str, more: &str) -> String {
    format!("\n[[crates.{krate}.external]]\nname = \"{name}\"\nversion = \"{version}\"\n{more}")
}

/// The fields of an allow entry for a declaration with the default features
/// and no other feature, resolved with `default`.
const PLAIN: &str = "default-features = true\nfeatures = []\nresolved-features = [\"default\"]\n";

/// The allow entry of `foo` in the valid model.
const FOO_ALLOWED: &str = "\n[[crates.kosmo-render.external]]\nname = \"foo\"\nversion = \"=1.0.0\"\ndefault-features = false\nfeatures = [\"x\"]\nresolved-features = [\"x\"]\n";

/// The allow entry of `qa` in the valid model.
const QA_ALLOWED: &str = "\n[[crates.kosmo-ui.external]]\nname = \"qa\"\nversion = \"=0.3.0\"\nkind = \"dev\"\ndefault-features = true\nfeatures = []\nresolved-features = [\"default\"]\n";

#[test]
fn the_valid_model_passes() {
    let (report, _) = fixture::valid().inspect();
    assert_eq!(report.problems, Vec::<String>::new());
}

#[test]
fn the_valid_model_with_only_the_desktop_passes() {
    let (report, facts) = fixture::desktop_only().inspect();
    assert_eq!(report.problems, Vec::<String>::new());
    let keys: Vec<&str> = facts.iter().map(|(key, _)| key.as_str()).collect();
    assert!(keys.contains(&"desktop_members"), "{keys:?}");
    assert!(
        !keys.iter().any(|key| key.starts_with("framework")),
        "{keys:?}"
    );
}

#[test]
fn this_repository_passes() {
    let root = crate::repo::root();
    let mut facts = Facts::new();
    let result = run(&root, &mut facts);
    assert_eq!(result, Ok(()), "{facts:?}");
    let fact = |key: &str| {
        facts
            .iter()
            .find(|(found, _)| found == key)
            .map(|(_, value)| value.as_str())
    };
    let listed = fs::read_to_string(root.join("xtask/allowlist.toml"))
        .unwrap()
        .matches("[[package]]")
        .count()
        .to_string();
    assert_eq!(fact("xtask_members"), Some("1"));
    assert_eq!(fact("xtask_exports"), Some("0"));
    assert_eq!(fact("xtask_crates_io"), Some(listed.as_str()));
    assert_eq!(fact("xtask_internal_edges"), Some("0"));
    for (key, value) in [
        ("desktop_members", "1"),
        ("desktop_exports", "0"),
        ("desktop_crates_io", "0"),
        ("desktop_internal_edges", "0"),
    ] {
        assert_eq!(fact(key), Some(value), "{key}");
    }
}

// The eight cases below and the platform case after them are the main
// failures and the main non-failure that the deps step exists for: a Git
// source, a path outside the repository, a requirement that is not exact,
// an allowlist that no longer matches Cargo.lock, a package in two
// versions, a krepi package, a license outside the allowed set, a build
// script that compiles C, and a package that only other platforms use.

#[test]
fn rejects_a_git_source() {
    let found = problems_with(|model| {
        let bar = model.pkg("framework", "bar@2.1.0");
        bar.source = fixture::Source::Registry(
            "git+https://example.com/bar.git?rev=0123abc#0123abc0123abc0123abc0123abc0123abc0123a"
                .to_string(),
        );
        model.edit(
            "framework/allowlist.toml",
            &entry(
                "bar",
                "2.1.0",
                "MIT OR Apache-2.0",
                &["default"],
                &["kosmo-render"],
            ),
            "",
        );
    });
    assert_eq!(
        found,
        [
            "framework/Cargo.lock: bar 2.1.0 comes from git+https://example.com/bar.git?rev=0123abc#0123abc0123abc0123abc0123abc0123abc0123a; packages come only from crates.io and from crates of the layout table"
        ]
    );
}

#[test]
fn rejects_a_path_outside_the_repository() {
    let found = problems_with(|model| {
        model.add("framework", Pkg::path("/work/outside", "outside", "0.1.0"));
        model
            .pkg("framework", "kosmo-render@0.1.0-dev.1")
            .deps
            .push(Dep::new("outside@0.1.0", "*"));
    });
    assert_eq!(
        found,
        [
            "framework/Cargo.lock: outside 0.1.0 is a path package outside the repository (../outside/Cargo.toml)"
        ]
    );
}

#[test]
fn rejects_a_requirement_that_is_not_exact() {
    let found = problems_with(|model| {
        model.pkg("framework", "kosmo-render@0.1.0-dev.1").deps[0].req = "^1.0.0".to_string();
    });
    assert_eq!(
        found,
        [
            "framework/kosmo-render/Cargo.toml: [dependencies] foo: the requirement ^1.0.0 is not exact (=x.y.z)"
        ]
    );
}

#[test]
fn rejects_an_allowlist_behind_the_lock_file() {
    let found = problems_with(|model| {
        model.pkg("framework", "bar@2.1.0").version = "2.1.1".to_string();
        model.pkg("framework", "foo@1.0.0").deps[0].to = "bar@2.1.1".to_string();
    });
    assert_eq!(
        found,
        ["framework/allowlist.toml: bar is 2.1.1 in framework/Cargo.lock, not 2.1.0"]
    );
}

/// The valid model in which `kosmo-ui` also depends on `baz 0.3.0`, which
/// brings in `bar 1.4.0` beside the `bar 2.1.0` of `foo`; the allow entries
/// and the allowlist list both, the latter without a `duplicate` reason.
fn two_versions_of_bar() -> Model {
    let mut model = fixture::valid();
    model.add(
        "framework",
        Pkg::crates_io("baz", "0.3.0").dep(Dep::new("bar@1.4.0", "^1")),
    );
    model.add("framework", Pkg::crates_io("bar", "1.4.0"));
    model
        .pkg("framework", "kosmo-ui@0.1.0-dev.1")
        .deps
        .push(Dep::new("baz@0.3.0", "=0.3.0").features(false, &[]));
    model.append(
        "layout.toml",
        &allow(
            "kosmo-ui",
            "baz",
            "=0.3.0",
            "default-features = false\nfeatures = []\nresolved-features = []\n",
        ),
    );
    model.append(
        "framework/allowlist.toml",
        &entry("baz", "0.3.0", "MIT", &[], &["kosmo-ui"]),
    );
    model.append(
        "framework/allowlist.toml",
        &entry("bar", "1.4.0", "MIT", &[], &["kosmo-ui"]),
    );
    model
}

#[test]
fn rejects_two_versions_of_a_package() {
    assert_eq!(
        two_versions_of_bar().problems(),
        [
            "framework/Cargo.lock: bar has 2 versions (1.4.0, 2.1.0), which framework/allowlist.toml does not allow"
        ]
    );
}

#[test]
fn rejects_a_krepi_package() {
    let found = problems_with(|model| {
        model.add("framework", Pkg::crates_io("krepi-abi", "0.1.0"));
        model
            .pkg("framework", "foo@1.0.0")
            .deps
            .push(Dep::new("krepi-abi@0.1.0", "^0.1"));
        model.append(
            "framework/allowlist.toml",
            &entry("krepi-abi", "0.1.0", "MIT", &[], &["kosmo-render"]),
        );
    });
    assert_eq!(
        found,
        [
            "framework/Cargo.lock: krepi-abi 0.1.0: kosmo does not depend on krepi: no package is named krepi or starts with krepi- or krepi_"
        ]
    );
}

/// Makes `foo` in `model` depend on `qux 1.0.0`, whose license
/// `GPL-3.0-only` is not in the allowed set, and lists `qux` in the
/// allowlist without a license exception.
pub fn add_qux_under_the_gpl(model: &mut Model) {
    model.add(
        "framework",
        Pkg::crates_io("qux", "1.0.0").license("GPL-3.0-only"),
    );
    model
        .pkg("framework", "foo@1.0.0")
        .deps
        .push(Dep::new("qux@1.0.0", "^1"));
    model.append(
        "framework/allowlist.toml",
        &entry("qux", "1.0.0", "GPL-3.0-only", &[], &["kosmo-render"]),
    );
}

/// The problem of the package that [`add_qux_under_the_gpl`] adds.
pub const QUX_UNDER_THE_GPL: &str = "framework/Cargo.lock: qux 1.0.0: the license GPL-3.0-only is not in the allowed set, and framework/allowlist.toml has no license exception for it";

#[test]
fn rejects_a_license_outside_the_allowed_set() {
    assert_eq!(problems_with(add_qux_under_the_gpl), [QUX_UNDER_THE_GPL]);
}

/// `entry`, an allowlist entry, for a package with a build script.
pub fn with_build_script(entry: &str) -> String {
    entry.replace("build-script = false", "build-script = true")
}

/// The valid model in which `foo` also depends on `zlib-sys 1.0.0`, which
/// has a build script and the license `MIT`, and whose build dependencies
/// are the packages `build`, each at version `1.2.0`. The allowlist lists
/// them all, without exceptions.
pub fn zlib_sys_with(build: &[&str]) -> Model {
    let mut model = fixture::valid();
    let mut zlib_sys = Pkg::crates_io("zlib-sys", "1.0.0").build_script();
    for name in build {
        zlib_sys = zlib_sys.dep(Dep::new(&format!("{name}@1.2.0"), "^1.2").kind("build"));
        model.add("framework", Pkg::crates_io(name, "1.2.0"));
        model.append(
            "framework/allowlist.toml",
            &entry(name, "1.2.0", "MIT", &[], &["kosmo-render"]),
        );
    }
    model.add("framework", zlib_sys);
    model
        .pkg("framework", "foo@1.0.0")
        .deps
        .push(Dep::new("zlib-sys@1.0.0", "^1"));
    model.append("framework/allowlist.toml", &zlib_sys_entry());
    model
}

/// The allowlist entry of `zlib-sys` in [`zlib_sys_with`].
pub fn zlib_sys_entry() -> String {
    with_build_script(&entry("zlib-sys", "1.0.0", "MIT", &[], &["kosmo-render"]))
}

#[test]
fn rejects_a_build_script_that_compiles_c() {
    assert_eq!(
        zlib_sys_with(&["cc"]).problems(),
        [
            "framework/Cargo.lock: zlib-sys 1.0.0: its build script compiles C or C++ (build dependency cc)"
        ]
    );
}

/// The valid model in which `kosmo-render` depends on `win-only 1.0.0` only
/// under `cfg(windows)`: Cargo.lock records it with its checksum, but the
/// resolution of neither target has it. Its license is not allowed, and its
/// build script compiles C with `cc`.
fn windows_only() -> Model {
    let mut model = fixture::valid();
    let mut win_only = Pkg::crates_io("win-only", "1.0.0")
        .license("GPL-3.0-only")
        .dep(Dep::new("cc@1.2.0", "^1.2").kind("build"));
    win_only.build_script = true;
    model.add("framework", win_only);
    model.add("framework", Pkg::crates_io("cc", "1.2.0"));
    model
        .pkg("framework", "kosmo-render@0.1.0-dev.1")
        .deps
        .push(Dep::new("win-only@1.0.0", "=1.0.0").cfg("cfg(windows)", &[]));
    model
}

#[test]
fn a_package_only_other_platforms_use_is_not_listed_and_passes() {
    let model = windows_only();
    assert!(
        model
            .tree()
            .text("framework/Cargo.lock")
            .unwrap()
            .contains("name = \"win-only\"")
    );
    assert_eq!(model.problems(), Vec::<String>::new());
}

#[test]
fn accepts_only_the_crates_io_source() {
    for source in [
        "registry+https://example.com/index",
        "sparse+https://index.crates.io/",
    ] {
        let found = problems_with(|model| {
            model.pkg("framework", "bar@2.1.0").source =
                fixture::Source::Registry(source.to_string());
            model.edit(
                "framework/allowlist.toml",
                &entry(
                    "bar",
                    "2.1.0",
                    "MIT OR Apache-2.0",
                    &["default"],
                    &["kosmo-render"],
                ),
                "",
            );
        });
        assert_eq!(
            found,
            [format!(
                "framework/Cargo.lock: bar 2.1.0 comes from {source}; packages come only from crates.io and from crates of the layout table"
            )]
        );
    }
}

#[test]
fn rejects_path_packages_that_are_not_crates_of_the_table() {
    for (dir, name) in [
        ("framework/tools/helper", "helper"),
        ("docs/snippets", "snippets"),
    ] {
        let found = problems_with(|model| {
            model.add(
                "framework",
                Pkg::path(&format!("{ROOT}/{dir}"), name, "0.1.0"),
            );
            model
                .pkg("framework", "kosmo-render@0.1.0-dev.1")
                .deps
                .push(Dep::new(&format!("{name}@0.1.0"), "=0.1.0"));
        });
        assert_eq!(
            found,
            [format!(
                "framework/Cargo.lock: {name} 0.1.0 is a path package that is not a crate of the layout table ({dir}/Cargo.toml)"
            )]
        );
    }
}

#[test]
fn the_tool_workspace_resolves_no_crate_of_the_table() {
    let found = problems_with(|model| {
        model.add(
            "xtask",
            Pkg::path(&format!("{ROOT}/desktop/kosmo"), "kosmo", "0.1.0-dev.1"),
        );
        model
            .pkg("xtask", "xtask@0.1.0-dev.1")
            .deps
            .push(Dep::new("kosmo@0.1.0-dev.1", "=0.1.0-dev.1"));
    });
    assert_eq!(
        found,
        [
            "xtask/Cargo.lock: kosmo 0.1.0-dev.1: the tool workspace xtask may not depend on crates of the layout table"
        ]
    );
}

#[test]
fn rejects_resolved_edges_against_the_workspace_directions() {
    let found = problems_with(|model| {
        model.add(
            "framework",
            Pkg::path(
                &format!("{ROOT}/display/kosmo-server"),
                "kosmo-server",
                "0.1.0-dev.1",
            ),
        );
        model
            .pkg("framework", "kosmo-ui@0.1.0-dev.1")
            .deps
            .push(Dep::new("kosmo-server@0.1.0-dev.1", "=0.1.0-dev.1"));
    });
    assert_eq!(
        found,
        [
            "framework/kosmo-ui/Cargo.toml: kosmo-server: kosmo-server is not an export crate of the workspace display"
        ]
    );
    let found = problems_with(|model| {
        model.add(
            "desktop",
            Pkg::path(
                &format!("{ROOT}/display/kosmo-types"),
                "kosmo-types",
                "0.1.0-dev.1",
            ),
        );
        model
            .pkg("desktop", "kosmo-calc@0.1.0-dev.1")
            .deps
            .push(Dep::new("kosmo-types@0.1.0-dev.1", "=0.1.0-dev.1"));
    });
    assert_eq!(
        found,
        [
            "desktop/kosmo-calc/Cargo.toml: kosmo-types: crates of desktop may not depend on crates of display"
        ]
    );
}

/// The valid model in which the framework has the export facade
/// `kosmo-sdk`, which depends on `kosmo-ui`, and `kosmo-calc` depends on it:
/// the resolution of the desktop has the crates of the other workspaces
/// that `kosmo-sdk` reaches, and `foo` and `bar` through `kosmo-render`.
pub fn desktop_through_a_facade() -> Model {
    let mut model = fixture::valid();
    model.append(
        "layout.toml",
        "\n[crates.kosmo-sdk]\npath = \"framework/kosmo-sdk\"\nlayer = \"F3\"\nexport = true\nfacade = true\npure-core = false\npublish = false\n",
    );
    let on_ui = Dep::new("kosmo-ui@0.1.0-dev.1", "=0.1.0-dev.1");
    model.add(
        "framework",
        Pkg::member("framework/kosmo-sdk", "kosmo-sdk").dep(on_ui.clone()),
    );
    let at = |dir: &str| format!("{ROOT}/{dir}");
    for pkg in [
        Pkg::path(&at("framework/kosmo-sdk"), "kosmo-sdk", "0.1.0-dev.1").dep(on_ui),
        Pkg::path(&at("framework/kosmo-ui"), "kosmo-ui", "0.1.0-dev.1")
            .dep(Dep::new("kosmo-render@0.1.0-dev.1", "=0.1.0-dev.1"))
            .dep(Dep::new("kosmo-types@0.1.0-dev.1", "=0.1.0-dev.1")),
        Pkg::path(&at("framework/kosmo-render"), "kosmo-render", "0.1.0-dev.1")
            .dep(Dep::new("foo@1.0.0", "=1.0.0").features(false, &["x"])),
        Pkg::path(&at("display/kosmo-types"), "kosmo-types", "0.1.0-dev.1"),
        Pkg::crates_io("foo", "1.0.0")
            .resolved(&["x"])
            .dep(Dep::new("bar@2.1.0", "^2.1")),
        Pkg::crates_io("bar", "2.1.0")
            .license("MIT OR Apache-2.0")
            .resolved(&["default"]),
    ] {
        model.add("desktop", pkg);
    }
    model
        .pkg("desktop", "kosmo-calc@0.1.0-dev.1")
        .deps
        .push(Dep::new("kosmo-sdk@0.1.0-dev.1", "=0.1.0-dev.1"));
    model.append(
        "desktop/allowlist.toml",
        &entry(
            "bar",
            "2.1.0",
            "MIT OR Apache-2.0",
            &["default"],
            &["kosmo-render"],
        ),
    );
    model.append(
        "desktop/allowlist.toml",
        &entry("foo", "1.0.0", "MIT", &["x"], &["kosmo-render"]),
    );
    model
}

#[test]
fn crates_of_another_workspace_are_reached_through_its_export_crates() {
    // `kosmo-ui` and `kosmo-render` are not export crates of the framework,
    // and the desktop reaches them through `kosmo-sdk`, which is one.
    let model = desktop_through_a_facade();
    assert_eq!(model.problems(), Vec::<String>::new());
    // The allowlist names the crate that brings a package in, also when it
    // is a crate of another workspace.
    let mut unnamed = model.clone();
    unnamed.edit(
        "desktop/allowlist.toml",
        "features = [\"x\"]\ncrates = [\"kosmo-render\"]",
        "features = [\"x\"]\ncrates = []",
    );
    assert_eq!(
        unnamed.problems(),
        ["desktop/allowlist.toml: foo 1.0.0: is reached from [kosmo-render], not []"]
    );
    // A crate of the desktop may not depend on them directly.
    let mut direct = model;
    direct
        .pkg("desktop", "kosmo-calc@0.1.0-dev.1")
        .deps
        .push(Dep::new("kosmo-ui@0.1.0-dev.1", "=0.1.0-dev.1"));
    assert_eq!(
        direct.problems(),
        [
            "desktop/kosmo-calc/Cargo.toml: kosmo-ui: kosmo-ui is not an export crate of the workspace framework"
        ]
    );
}

#[test]
fn accepts_only_exact_requirements_in_every_form() {
    for req in ["=1.0", "~1.0.0", ">=1.0.0, <1.0.1", "*"] {
        let found = problems_with(|model| {
            model.pkg("framework", "kosmo-render@0.1.0-dev.1").deps[0].req = req.to_string();
        });
        assert_eq!(
            found,
            [format!(
                "framework/kosmo-render/Cargo.toml: [dependencies] foo: the requirement {req} is not exact (=x.y.z)"
            )]
        );
    }
}

#[test]
fn requirements_of_dev_build_and_tool_dependencies_are_exact_too() {
    let found = problems_with(|model| {
        model.pkg("framework", "kosmo-ui@0.1.0-dev.1").deps[2].req = "^0.3.0".to_string();
    });
    assert_eq!(
        found,
        [
            "framework/kosmo-ui/Cargo.toml: [dev-dependencies] qa: the requirement ^0.3.0 is not exact (=x.y.z)"
        ]
    );
    let found = problems_with(|model| {
        let qa = &mut model.pkg("framework", "kosmo-ui@0.1.0-dev.1").deps[2];
        qa.req = "^0.3.0".to_string();
        qa.kind = Some("build");
        model.edit("layout.toml", "kind = \"dev\"", "kind = \"build\"");
    });
    assert_eq!(
        found,
        [
            "framework/kosmo-ui/Cargo.toml: [build-dependencies] qa: the requirement ^0.3.0 is not exact (=x.y.z)"
        ]
    );
    let found = problems_with(|model| {
        model.pkg("xtask", "xtask@0.1.0-dev.1").deps[0].req = "^0.13".to_string();
    });
    assert_eq!(
        found,
        [
            "xtask/Cargo.toml: [dependencies] yaml-rust2: the requirement ^0.13 is not exact (=x.y.z)"
        ]
    );
}

#[test]
fn allowlist_versions_keep_build_metadata() {
    let with_toml = |listed: &str| {
        problems_with(|model| {
            model.add("xtask", Pkg::crates_io("toml", "1.2.0+spec-1.1.0"));
            model
                .pkg("xtask", "xtask@0.1.0-dev.1")
                .deps
                .push(Dep::new("toml@1.2.0+spec-1.1.0", "=1.2.0"));
            model.append(
                "xtask/allowlist.toml",
                &entry("toml", listed, "MIT", &[], &["xtask"]),
            );
        })
    };
    assert_eq!(with_toml("1.2.0+spec-1.1.0"), Vec::<String>::new());
    assert_eq!(
        with_toml("1.2.0"),
        ["xtask/allowlist.toml: toml is 1.2.0+spec-1.1.0 in xtask/Cargo.lock, not 1.2.0"]
    );
}

#[test]
fn rejects_a_package_without_a_checksum() {
    let found = problems_with(|model| model.pkg("framework", "bar@2.1.0").checksum = false);
    assert_eq!(found, ["framework/Cargo.lock: bar 2.1.0 has no checksum"]);
}

#[test]
fn checks_the_directions_of_the_resolved_graph_for_each_target() {
    for kind in [None, Some("build")] {
        let found = problems_with(|model| {
            let mut dep = Dep::new("kosmo-ui@0.1.0-dev.1", "=0.1.0-dev.1")
                .cfg("cfg(target_os = \"macos\")", &[DARWIN]);
            dep.kind = kind;
            model
                .pkg("framework", "kosmo-math-model@0.1.0-dev.1")
                .deps
                .push(dep);
        });
        assert_eq!(
            found,
            [
                "framework/kosmo-math-model/Cargo.toml: kosmo-ui (aarch64-apple-darwin): a crate of layer F0 may not depend on kosmo-ui of the higher layer F1"
            ]
        );
    }
}

#[test]
fn rejects_a_cycle_in_the_resolved_graph() {
    let found = problems_with(|model| {
        model
            .pkg("framework", "kosmo-math-model@0.1.0-dev.1")
            .deps
            .push(Dep::new("kosmo-jobs@0.1.0-dev.1", "=0.1.0-dev.1").kind("dev"));
        model.edit(
            "layout.toml",
            "path = \"framework/kosmo-math-model\"\nlayer = \"F0\"\nexport = false\npure-core = false\npublish = false\n",
            "path = \"framework/kosmo-math-model\"\nlayer = \"F0\"\nexport = false\npure-core = false\npublish = false\nsame-layer = [\"kosmo-jobs\"]\n",
        );
    });
    assert_eq!(
        found,
        [
            "framework/kosmo-jobs/Cargo.toml: the resolved internal dependency graph has a cycle: kosmo-jobs -> kosmo-math-model -> kosmo-jobs"
        ]
    );
}

#[test]
fn rejects_krepi_packages_in_every_workspace_by_name() {
    let found = problems_with(|model| {
        model.add("xtask", Pkg::crates_io("krepi", "0.1.0"));
        model
            .pkg("xtask", "xtask@0.1.0-dev.1")
            .deps
            .push(Dep::new("krepi@0.1.0", "=0.1.0"));
        model.append(
            "xtask/allowlist.toml",
            &entry("krepi", "0.1.0", "MIT", &[], &["xtask"]),
        );
        model.add("desktop", Pkg::crates_io("krepi_sys", "0.1.0"));
        model
            .pkg("desktop", "kosmo@0.1.0-dev.1")
            .deps
            .push(Dep::new("krepi_sys@0.1.0", "=0.1.0"));
        model.append(
            "layout.toml",
            &allow(
                "kosmo",
                "krepi_sys",
                "=0.1.0",
                "default-features = true\nfeatures = []\nresolved-features = []\n",
            ),
        );
        model.append(
            "desktop/allowlist.toml",
            &entry("krepi_sys", "0.1.0", "MIT", &[], &["kosmo"]),
        );
        for name in ["krepit", "kosmo-krepi"] {
            model.add("framework", Pkg::crates_io(name, "1.0.0"));
            model
                .pkg("framework", "foo@1.0.0")
                .deps
                .push(Dep::new(&format!("{name}@1.0.0"), "^1"));
            model.append(
                "framework/allowlist.toml",
                &entry(name, "1.0.0", "MIT", &[], &["kosmo-render"]),
            );
        }
    });
    assert_problems(
        &found,
        &[
            "xtask/Cargo.lock: krepi 0.1.0: kosmo does not depend on krepi",
            "desktop/Cargo.lock: krepi_sys 0.1.0: kosmo does not depend on krepi",
        ],
    );
}

#[test]
fn several_versions_need_a_duplicate_reason_on_each_entry() {
    let mut model = two_versions_of_bar();
    for version in ["1.4.0", "2.1.0"] {
        model.edit(
            "framework/allowlist.toml",
            &format!("name = \"bar\"\nversion = \"{version}\"\n"),
            &format!(
                "name = \"bar\"\nversion = \"{version}\"\nduplicate = \"baz has not moved to bar 2 yet\"\n"
            ),
        );
    }
    assert_eq!(model.problems(), Vec::<String>::new());
    let mut model = two_versions_of_bar();
    model.edit(
        "framework/allowlist.toml",
        "name = \"bar\"\nversion = \"1.4.0\"\n",
        "name = \"bar\"\nversion = \"1.4.0\"\nduplicate = \"baz has not moved to bar 2 yet\"\n",
    );
    assert_eq!(
        model.problems(),
        [
            "framework/allowlist.toml: bar 2.1.0 is one of 2 versions of bar and needs a duplicate reason"
        ]
    );
    let found = problems_with(|model| {
        model.edit(
            "framework/allowlist.toml",
            "name = \"bar\"\nversion = \"2.1.0\"\n",
            "name = \"bar\"\nversion = \"2.1.0\"\nduplicate = \"no longer needed\"\n",
        );
    });
    assert_eq!(
        found,
        [
            "framework/allowlist.toml: bar 2.1.0 has a duplicate reason, but the workspace resolves only one version of bar"
        ]
    );
}

#[test]
fn versions_are_counted_over_the_union_of_the_targets() {
    let found = problems_with(|model| {
        model.add("framework", Pkg::crates_io("bar", "1.4.0"));
        let foo = model.pkg("framework", "foo@1.0.0");
        foo.deps[0] = Dep::new("bar@2.1.0", "^2.1").cfg("cfg(target_os = \"macos\")", &[DARWIN]);
        foo.deps
            .push(Dep::new("bar@1.4.0", "^1").cfg("cfg(target_os = \"linux\")", &[LINUX]));
        model.append(
            "framework/allowlist.toml",
            &entry("bar", "1.4.0", "MIT", &[], &["kosmo-render"]),
        );
    });
    assert_eq!(
        found,
        [
            "framework/Cargo.lock: bar has 2 versions (1.4.0, 2.1.0), which framework/allowlist.toml does not allow"
        ]
    );
}

#[test]
fn every_direct_dependency_has_an_allow_entry() {
    let found = problems_with(|model| model.edit("layout.toml", FOO_ALLOWED, ""));
    assert_eq!(
        found,
        [
            "framework/kosmo-render/Cargo.toml: [dependencies] foo =1.0.0 has no allow entry in layout.toml"
        ]
    );
    let found = problems_with(|model| model.edit("layout.toml", QA_ALLOWED, ""));
    assert_eq!(
        found,
        [
            "framework/kosmo-ui/Cargo.toml: [dev-dependencies] qa =0.3.0 has no allow entry in layout.toml"
        ]
    );
}

#[test]
fn every_allow_entry_is_a_direct_dependency_for_some_target() {
    let found = problems_with(|model| {
        model.append(
            "layout.toml",
            &allow(
                "kosmo-render",
                "baz",
                "=0.3.0",
                "default-features = true\nfeatures = []\nresolved-features = []\n",
            ),
        );
    });
    assert_eq!(
        found,
        [
            "layout.toml: crates.kosmo-render: allows baz =0.3.0, which kosmo-render does not depend on for aarch64-unknown-linux-gnu or aarch64-apple-darwin"
        ]
    );
    let mut model = windows_only();
    model.append(
        "layout.toml",
        &allow(
            "kosmo-render",
            "win-only",
            "=1.0.0",
            "target = \"cfg(windows)\"\ndefault-features = true\nfeatures = []\nresolved-features = []\n",
        ),
    );
    assert_eq!(
        model.problems(),
        [
            "layout.toml: crates.kosmo-render: allows win-only =1.0.0 in [target.'cfg(windows)'.dependencies], which kosmo-render does not depend on for aarch64-unknown-linux-gnu or aarch64-apple-darwin"
        ]
    );
}

#[test]
fn allow_entries_match_the_requirement_and_the_declared_features() {
    let found = problems_with(|model| {
        model.edit(
            "layout.toml",
            "version = \"=1.0.0\"",
            "version = \"=1.0.1\"",
        )
    });
    assert_eq!(
        found,
        ["layout.toml: crates.kosmo-render: foo: the allow entry has =1.0.1, the manifest =1.0.0"]
    );
    let found = problems_with(|model| {
        model.pkg("framework", "kosmo-render@0.1.0-dev.1").deps[0] =
            Dep::new("foo@1.0.0", "=1.0.0").features(false, &["y", "x"]);
        model.pkg("framework", "foo@1.0.0").features = vec!["x".to_string(), "y".to_string()];
        model.edit(
            "layout.toml",
            "resolved-features = [\"x\"]",
            "resolved-features = [\"x\", \"y\"]",
        );
        model.edit(
            "framework/allowlist.toml",
            "features = [\"x\"]",
            "features = [\"x\", \"y\"]",
        );
    });
    assert_eq!(
        found,
        [
            "layout.toml: crates.kosmo-render: foo: the features [x, y] differ from the allow entry [x]"
        ]
    );
    let found = problems_with(|model| {
        model.pkg("framework", "kosmo-render@0.1.0-dev.1").deps[0].default_features = true;
    });
    assert_eq!(
        found,
        [
            "layout.toml: crates.kosmo-render: foo: the default features are on, the allow entry turns them off"
        ]
    );
}

#[test]
fn allow_entries_cover_the_dependencies_of_every_target() {
    let found = problems_with(|model| {
        model.add("framework", Pkg::crates_io("mac-only", "1.0.0"));
        model
            .pkg("framework", "kosmo-render@0.1.0-dev.1")
            .deps
            .push(
                Dep::new("mac-only@1.0.0", "=1.0.0").cfg("cfg(target_os = \"macos\")", &[DARWIN]),
            );
        model.append(
            "framework/allowlist.toml",
            &entry("mac-only", "1.0.0", "MIT", &[], &["kosmo-render"]),
        );
    });
    assert_eq!(
        found,
        [
            "framework/kosmo-render/Cargo.toml: [target.'cfg(target_os = \"macos\")'.dependencies] mac-only =1.0.0 (aarch64-apple-darwin) has no allow entry in layout.toml"
        ]
    );
}

/// The valid model in which `kosmo-render` has the optional dependency
/// `opt 1.0.0`, which only a feature that is off by default turns on.
fn optional_dependency() -> Model {
    let mut model = fixture::valid();
    model.add("framework", Pkg::crates_io("opt", "1.0.0"));
    let mut dep = Dep::new("opt@1.0.0", "=1.0.0");
    dep.optional = true;
    model
        .pkg("framework", "kosmo-render@0.1.0-dev.1")
        .deps
        .push(dep);
    model
}

#[test]
fn optional_dependencies_count_with_all_features() {
    assert_problems(
        &optional_dependency().problems(),
        &[
            "framework/kosmo-render/Cargo.toml: [dependencies] opt =1.0.0 has no allow entry in layout.toml",
            "framework/Cargo.lock: opt 1.0.0 is not in framework/allowlist.toml",
        ],
    );
}

#[test]
fn allow_entries_state_the_kind_the_target_and_whether_optional() {
    let found = problems_with(|model| model.edit("layout.toml", "kind = \"dev\"\n", ""));
    assert_eq!(
        found,
        [
            "layout.toml: crates.kosmo-ui: qa: the allow entry is for a normal dependency, the manifest declares a dev dependency"
        ]
    );
    let found = problems_with(|model| {
        model.add("framework", Pkg::crates_io("mac-only", "1.0.0"));
        model
            .pkg("framework", "kosmo-render@0.1.0-dev.1")
            .deps
            .push(
                Dep::new("mac-only@1.0.0", "=1.0.0").cfg("cfg(target_os = \"macos\")", &[DARWIN]),
            );
        model.append(
            "framework/allowlist.toml",
            &entry("mac-only", "1.0.0", "MIT", &[], &["kosmo-render"]),
        );
        model.append(
            "layout.toml",
            &allow(
                "kosmo-render",
                "mac-only",
                "=1.0.0",
                "default-features = true\nfeatures = []\nresolved-features = []\n",
            ),
        );
    });
    assert_eq!(
        found,
        [
            "layout.toml: crates.kosmo-render: mac-only: the allow entry has the target (none), the manifest cfg(target_os = \"macos\")"
        ]
    );
    let mut model = optional_dependency();
    model.append(
        "framework/allowlist.toml",
        &entry("opt", "1.0.0", "MIT", &[], &["kosmo-render"]),
    );
    model.append(
        "layout.toml",
        &allow(
            "kosmo-render",
            "opt",
            "=1.0.0",
            "default-features = true\nfeatures = []\nresolved-features = []\n",
        ),
    );
    assert_eq!(
        model.problems(),
        [
            "layout.toml: crates.kosmo-render: opt: the dependency is optional, the allow entry is not"
        ]
    );
    let found = problems_with(|model| {
        model.pkg("framework", "kosmo-render@0.1.0-dev.1").deps[0] =
            Dep::new("foo@1.0.0", "=1.0.0").features(false, &["x", "y"]);
        model.pkg("framework", "foo@1.0.0").features = vec!["x".to_string(), "y".to_string()];
        model.edit(
            "layout.toml",
            "features = [\"x\"]\nresolved-features = [\"x\"]",
            "features = [\"y\", \"x\"]\nresolved-features = [\"x\", \"y\"]",
        );
        model.edit(
            "framework/allowlist.toml",
            "features = [\"x\"]",
            "features = [\"x\", \"y\"]",
        );
    });
    assert_eq!(
        found,
        [
            "layout.toml: crates.kosmo-render: foo: the features of the allow entry are not sorted or repeat"
        ]
    );
}

#[test]
fn an_allow_entry_states_its_default_features() {
    let mut model = fixture::valid();
    model.edit(
        "layout.toml",
        "version = \"=1.0.0\"\ndefault-features = false\n",
        "version = \"=1.0.0\"\n",
    );
    let error = layout::table(&model.tree()).unwrap_err();
    assert!(
        error.contains("missing field `default-features`"),
        "{error}"
    );
}

#[test]
fn allow_entries_record_the_resolved_features_of_all_targets() {
    // A dev dependency of another crate turns on a feature of `foo` for one
    // target; the allowlist already lists the new features.
    let with_std = |model: &mut Model| {
        model.pkg("framework", "qa@0.3.0").deps.push(
            Dep::new("foo@1.0.0", "^1")
                .features(false, &["std"])
                .cfg("cfg(target_os = \"macos\")", &[DARWIN]),
        );
        model
            .pkg("framework", "foo@1.0.0")
            .features_on
            .insert(DARWIN, vec!["std".to_string()]);
        model.edit(
            "framework/allowlist.toml",
            "features = [\"x\"]\ncrates = [\"kosmo-render\"]",
            "features = [\"std\", \"x\"]\ncrates = [\"kosmo-render\", \"kosmo-ui\"]",
        );
        // `bar`, the dependency of `foo`, is now reached through `qa` too.
        model.edit(
            "framework/allowlist.toml",
            "features = [\"default\"]\ncrates = [\"kosmo-render\"]",
            "features = [\"default\"]\ncrates = [\"kosmo-render\", \"kosmo-ui\"]",
        );
    };
    let found = problems_with(with_std);
    assert_eq!(
        found,
        [
            "layout.toml: crates.kosmo-render: foo: the resolved features [std, x] differ from the allow entry [x]"
        ]
    );
    let found = problems_with(|model| {
        model.edit(
            "layout.toml",
            "resolved-features = [\"x\"]",
            "resolved-features = [\"x\", \"y\"]",
        );
    });
    assert_eq!(
        found,
        [
            "layout.toml: crates.kosmo-render: foo: the resolved features [x] differ from the allow entry [x, y]"
        ]
    );
}

#[test]
fn every_resolved_package_is_in_the_allowlist() {
    let mut model = fixture::valid();
    model.add("framework", Pkg::crates_io("baz", "0.3.0"));
    model
        .pkg("framework", "foo@1.0.0")
        .deps
        .push(Dep::new("baz@0.3.0", "^0.3"));
    let (report, _) = model.inspect();
    assert_eq!(
        report.problems,
        ["framework/Cargo.lock: baz 0.3.0 is not in framework/allowlist.toml"]
    );
    // The printed entry, added to the allowlist, makes it match.
    let note = report
        .notes
        .iter()
        .find(|note| note.contains("framework/allowlist.toml"))
        .expect("the step prints the entry to add");
    let added = note
        .split_once("same name and version:\n")
        .map(|(_, entries)| entries)
        .unwrap();
    assert!(added.contains("name = \"baz\""), "{note}");
    model.append("framework/allowlist.toml", &format!("\n{added}"));
    assert_eq!(model.problems(), Vec::<String>::new());
}

#[test]
fn every_allowlist_entry_is_resolved_for_some_target() {
    let found = problems_with(|model| {
        model.append(
            "framework/allowlist.toml",
            &entry("old", "1.0.0", "MIT", &[], &["kosmo-render"]),
        );
    });
    assert_eq!(
        found,
        [
            "framework/allowlist.toml: old 1.0.0 is not resolved for aarch64-unknown-linux-gnu or aarch64-apple-darwin"
        ]
    );
    let mut model = windows_only();
    model.append(
        "framework/allowlist.toml",
        &entry("win-only", "1.0.0", "GPL-3.0-only", &[], &["kosmo-render"]),
    );
    let (report, _) = model.inspect();
    assert_eq!(
        report.problems,
        [
            "framework/allowlist.toml: win-only 1.0.0 is not resolved for aarch64-unknown-linux-gnu or aarch64-apple-darwin"
        ]
    );
    assert!(
        report.notes.iter().any(
            |note| note.contains("# Remove these entries:\n\n[[package]]\nname = \"win-only\"")
        ),
        "{:?}",
        report.notes
    );
}

#[test]
fn allowlist_entries_match_the_resolved_package() {
    let bar = entry(
        "bar",
        "2.1.0",
        "MIT OR Apache-2.0",
        &["default"],
        &["kosmo-render"],
    );
    let cases: [(&str, &str, &str); 4] = [
        (
            "license = \"MIT OR Apache-2.0\"",
            "license = \"MIT\"",
            "bar 2.1.0: the license is MIT OR Apache-2.0, not MIT",
        ),
        (
            "build-script = false",
            "build-script = true",
            "bar 2.1.0: has no build script",
        ),
        (
            "features = [\"default\"]",
            "features = []",
            "bar 2.1.0: the resolved features are [default], not []",
        ),
        (
            "source = \"crates.io\"",
            "source = \"github\"",
            "bar 2.1.0: the source is crates.io, not github",
        ),
    ];
    for (from, to, expected) in cases {
        let found = problems_with(|model| {
            model.edit("framework/allowlist.toml", &bar, &bar.replace(from, to));
        });
        assert_eq!(found, [format!("framework/allowlist.toml: {expected}")]);
    }
    let found = problems_with(|model| model.pkg("framework", "bar@2.1.0").build_script = true);
    assert_eq!(
        found,
        ["framework/allowlist.toml: bar 2.1.0: has a build script"]
    );
    let found = problems_with(|model| {
        model
            .pkg("framework", "qa@0.3.0")
            .deps
            .push(Dep::new("bar@2.1.0", "^2"));
    });
    assert_eq!(
        found,
        [
            "framework/allowlist.toml: bar 2.1.0: is reached from [kosmo-render, kosmo-ui], not [kosmo-render]"
        ]
    );
}

#[test]
fn every_workspace_of_the_check_has_a_readable_allowlist() {
    let found = problems_with(|model| {
        model.files.remove("desktop/allowlist.toml");
    });
    assert_eq!(
        found,
        [
            "desktop/allowlist.toml: missing; every workspace of the check has an allowlist, without entries when it has no crates.io package"
        ]
    );
    let found = problems_with(|model| model.append("desktop/allowlist.toml", "[[package]\n"));
    assert_problems(&found, &["desktop/allowlist.toml: not valid TOML"]);
    let found = problems_with(|model| model.append("desktop/allowlist.toml", "packages = []\n"));
    assert_problems(
        &found,
        &["desktop/allowlist.toml:2: unknown field `packages`"],
    );
}

#[test]
fn allowlist_entries_have_locked_versions_once() {
    let bar = entry(
        "bar",
        "2.1.0",
        "MIT OR Apache-2.0",
        &["default"],
        &["kosmo-render"],
    );
    let found = problems_with(|model| {
        model.edit(
            "framework/allowlist.toml",
            &bar,
            &bar.replace("\"2.1.0\"", "\"^2.1.0\""),
        );
    });
    assert_eq!(
        found,
        ["framework/allowlist.toml: bar: the version ^2.1.0 is not a locked version"]
    );
    let found = problems_with(|model| {
        model.edit(
            "framework/allowlist.toml",
            &bar,
            &bar.replace("version = \"2.1.0\"\n", ""),
        );
    });
    assert_problems(
        &found,
        &["framework/allowlist.toml:3: missing field `version`"],
    );
    let found = problems_with(|model| model.append("framework/allowlist.toml", &bar));
    assert_eq!(found, ["framework/allowlist.toml: lists bar 2.1.0 twice"]);
}

#[test]
fn each_workspace_has_its_own_allowlist() {
    let found = problems_with(|model| {
        model.add(
            "desktop",
            Pkg::crates_io("bar", "2.1.0")
                .license("MIT OR Apache-2.0")
                .resolved(&["default"]),
        );
        model
            .pkg("desktop", "kosmo@0.1.0-dev.1")
            .deps
            .push(Dep::new("bar@2.1.0", "=2.1.0"));
        model.append("layout.toml", &allow("kosmo", "bar", "=2.1.0", PLAIN));
    });
    assert_eq!(
        found,
        ["desktop/Cargo.lock: bar 2.1.0 is not in desktop/allowlist.toml"]
    );
}

#[test]
fn the_lock_file_of_xtask_is_checked_too() {
    let found = problems_with(|model| {
        model.add("xtask", Pkg::crates_io("extra", "1.0.0"));
        model
            .pkg("xtask", "xtask@0.1.0-dev.1")
            .deps
            .push(Dep::new("extra@1.0.0", "=1.0.0"));
    });
    assert_eq!(
        found,
        ["xtask/Cargo.lock: extra 1.0.0 is not in xtask/allowlist.toml"]
    );
    let found = problems_with(|model| {
        model.add("xtask", Pkg::crates_io("hashbrown", "0.15.5"));
        model.add("xtask", Pkg::crates_io("hashbrown", "0.16.1"));
        model
            .pkg("xtask", "yaml-rust2@0.13.0")
            .deps
            .push(Dep::new("hashbrown@0.15.5", "^0.15"));
        model
            .pkg("xtask", "xtask@0.1.0-dev.1")
            .deps
            .push(Dep::new("hashbrown@0.16.1", "=0.16.1"));
        model.append(
            "xtask/allowlist.toml",
            &entry("hashbrown", "0.15.5", "MIT", &[], &["xtask"]),
        );
        model.append(
            "xtask/allowlist.toml",
            &entry("hashbrown", "0.16.1", "MIT", &[], &["xtask"]),
        );
    });
    assert_eq!(
        found,
        [
            "xtask/Cargo.lock: hashbrown has 2 versions (0.15.5, 0.16.1), which xtask/allowlist.toml does not allow"
        ]
    );
}

#[test]
fn each_workspace_is_resolved_for_its_own_targets() {
    let found = problems_with(|model| {
        model.edit(
            "layout.toml",
            "[tools.xtask]\npath = \"xtask\"\ntargets = [\"aarch64-unknown-linux-gnu\", \"aarch64-apple-darwin\"]",
            "[tools.xtask]\npath = \"xtask\"\ntargets = [\"aarch64-apple-darwin\"]",
        );
        let linux_only =
            || Dep::new("linux-only@1.0.0", "=1.0.0").cfg("cfg(target_os = \"linux\")", &[LINUX]);
        model.add("xtask", Pkg::crates_io("linux-only", "1.0.0"));
        model
            .pkg("xtask", "xtask@0.1.0-dev.1")
            .deps
            .push(linux_only());
        model.add("desktop", Pkg::crates_io("linux-only", "1.0.0"));
        model
            .pkg("desktop", "kosmo@0.1.0-dev.1")
            .deps
            .push(linux_only());
        model.append(
            "layout.toml",
            &allow(
                "kosmo",
                "linux-only",
                "=1.0.0",
                "target = \"cfg(target_os = \\\"linux\\\")\"\ndefault-features = true\nfeatures = []\nresolved-features = []\n",
            ),
        );
        model.append(
            "desktop/allowlist.toml",
            &entry("linux-only", "1.0.0", "MIT", &[], &["kosmo"]),
        );
    });
    assert_eq!(found, Vec::<String>::new());
}

#[test]
fn reports_versions_that_differ_between_the_workspaces() {
    let mut model = fixture::valid();
    model.add("desktop", Pkg::crates_io("bar", "1.4.0"));
    model
        .pkg("desktop", "kosmo@0.1.0-dev.1")
        .deps
        .push(Dep::new("bar@1.4.0", "=1.4.0"));
    model.append(
        "layout.toml",
        &allow(
            "kosmo",
            "bar",
            "=1.4.0",
            "default-features = true\nfeatures = []\nresolved-features = []\n",
        ),
    );
    model.append(
        "desktop/allowlist.toml",
        &entry("bar", "1.4.0", "MIT", &[], &["kosmo"]),
    );
    let (report, facts) = model.inspect();
    assert_eq!(report.problems, Vec::<String>::new());
    assert!(
        facts.contains(&(
            "drift".to_string(),
            "bar: framework 2.1.0, desktop 1.4.0".to_string()
        )),
        "{facts:?}"
    );
    let (_, facts) = fixture::valid().inspect();
    assert!(
        facts.contains(&("drift".to_string(), "none".to_string())),
        "{facts:?}"
    );
}

#[test]
fn reports_the_numbers_of_each_workspace() {
    let (report, facts) = windows_only().inspect();
    assert_eq!(report.problems, Vec::<String>::new());
    let numbers: Vec<(&str, &str)> = facts
        .iter()
        .filter(|(key, _)| key != "drift")
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect();
    assert_eq!(
        numbers,
        [
            ("xtask_members", "1"),
            ("xtask_exports", "0"),
            ("xtask_crates_io", "1"),
            ("xtask_internal_edges", "0"),
            ("display_members", "2"),
            ("display_exports", "1"),
            ("display_crates_io", "0"),
            ("display_internal_edges", "1"),
            // win-only and cc only resolve for other platforms.
            ("framework_members", "4"),
            ("framework_exports", "0"),
            ("framework_crates_io", "3"),
            ("framework_internal_edges", "3"),
            ("desktop_members", "2"),
            ("desktop_exports", "0"),
            ("desktop_crates_io", "0"),
            ("desktop_internal_edges", "1"),
        ]
    );
}

#[test]
fn reports_every_problem_sorted_by_path() {
    let mut model = two_versions_of_bar();
    model.add("framework", Pkg::path("/work/outside", "outside", "0.1.0"));
    model
        .pkg("framework", "kosmo-render@0.1.0-dev.1")
        .deps
        .push(Dep::new("outside@0.1.0", "*"));
    add_qux_under_the_gpl(&mut model);
    assert_eq!(
        model.problems(),
        [
            "framework/Cargo.lock: bar has 2 versions (1.4.0, 2.1.0), which framework/allowlist.toml does not allow",
            "framework/Cargo.lock: outside 0.1.0 is a path package outside the repository (../outside/Cargo.toml)",
            QUX_UNDER_THE_GPL,
        ]
    );
    model.pkg("framework", "qa@0.3.0").checksum = false;
    model.edit(
        "framework/allowlist.toml",
        "license = \"Apache-2.0\"",
        "license = \"MIT\"",
    );
    assert_eq!(
        model.problems(),
        [
            "framework/Cargo.lock: bar has 2 versions (1.4.0, 2.1.0), which framework/allowlist.toml does not allow",
            "framework/Cargo.lock: outside 0.1.0 is a path package outside the repository (../outside/Cargo.toml)",
            "framework/Cargo.lock: qa 0.3.0 has no checksum",
            QUX_UNDER_THE_GPL,
            "framework/allowlist.toml: qa 0.3.0: the license is Apache-2.0, not MIT",
        ]
    );
}

#[test]
fn a_failing_step_keeps_its_observations() {
    let mut model = fixture::valid();
    add_qux_under_the_gpl(&mut model);
    let (report, facts) = model.inspect();
    assert_eq!(report.problems, [QUX_UNDER_THE_GPL]);
    let keys: Vec<&str> = facts.iter().map(|(key, _)| key.as_str()).collect();
    assert!(keys.contains(&"framework_crates_io"), "{keys:?}");
    assert!(keys.contains(&"drift"), "{keys:?}");
}

#[test]
fn the_printed_allow_entries_match_what_cargo_resolves() {
    let mut model = fixture::valid();
    model.edit("layout.toml", FOO_ALLOWED, "");
    let (report, _) = model.inspect();
    assert_eq!(report.problems.len(), 1, "{:?}", report.problems);
    let note = report
        .notes
        .iter()
        .find(|note| note.contains("allow entries of kosmo-render"))
        .expect("the step prints the allow entries");
    let (_, entries) = note.split_once("\n\n").unwrap();
    assert!(
        entries.starts_with("[[crates.kosmo-render.external]]\n"),
        "{entries}"
    );
    model.append("layout.toml", &format!("\n{entries}"));
    assert_eq!(model.problems(), Vec::<String>::new());
}

#[test]
fn the_printed_note_says_when_every_allow_entry_of_a_crate_goes() {
    let mut model = fixture::valid();
    model.append("layout.toml", &allow("kosmo", "foo", "=1.0.0", PLAIN));
    let (report, _) = model.inspect();
    assert_eq!(
        report.problems,
        [
            "layout.toml: crates.kosmo: allows foo =1.0.0, which kosmo does not depend on for aarch64-unknown-linux-gnu or aarch64-apple-darwin"
        ]
    );
    let notes: Vec<&String> = report
        .notes
        .iter()
        .filter(|note| note.contains("allow entries of kosmo "))
        .collect();
    assert_eq!(
        notes,
        [
            "deps: the allow entries of kosmo in layout.toml match what Cargo resolves once every `[[crates.kosmo.external]]` table is removed"
        ]
    );
}

#[test]
fn reads_only_the_metadata_that_cargo_prints() {
    let read = |text: &str| metadata::parse(text).map(|_| ()).unwrap_err();
    assert!(read("not json").starts_with("printed no valid JSON"));
    let valid = fixture::valid().ws("xtask").metadata(LINUX);
    let mut value: serde_json::Value = serde_json::from_str(&valid).unwrap();
    assert!(metadata::parse(&valid).is_ok());
    value["resolve"] = serde_json::Value::Null;
    assert_eq!(read(&value.to_string()), "reports no resolve");
    value["version"] = serde_json::json!(2);
    assert_eq!(
        read(&value.to_string()),
        "printed the metadata format version 2, which is not supported"
    );
}

#[test]
fn a_workspace_whose_metadata_cannot_be_read_is_reported_alone() {
    let model = fixture::valid();
    let tree = model.tree();
    let table = layout::table(&tree).unwrap();
    let index = Index::new(&table);
    let mut facts = Facts::new();
    let report = inspect(
        &tree,
        &index,
        ROOT,
        &mut facts,
        &mut |workspace, target, _| {
            if workspace.dir == "framework" && target == DARWIN {
                Ok("not json".to_string())
            } else {
                Ok(model
                    .workspaces
                    .iter()
                    .find(|ws| ws.dir == workspace.dir)
                    .unwrap()
                    .metadata(target))
            }
        },
    );
    assert_eq!(report.problems.len(), 1, "{:?}", report.problems);
    assert!(
        report.problems[0].starts_with(
            "framework/Cargo.toml: cargo metadata for aarch64-apple-darwin printed no valid JSON"
        ),
        "{:?}",
        report.problems
    );
    let keys: Vec<&str> = facts.iter().map(|(key, _)| key.as_str()).collect();
    assert!(!keys.contains(&"framework_members"), "{keys:?}");
    assert!(keys.contains(&"desktop_members"), "{keys:?}");
}

#[test]
fn metadata_runs_offline_for_one_target() {
    let cmd = metadata_command("desktop", LINUX);
    assert_eq!(
        cmd.args,
        [
            "metadata",
            "--locked",
            "--offline",
            "--all-features",
            "--filter-platform",
            LINUX,
            "--format-version",
            "1",
            "--manifest-path",
            "desktop/Cargo.toml",
        ]
    );
    assert_eq!(cmd.env, [(OFFLINE.0.to_string(), OFFLINE.1.to_string())]);
    assert_eq!(cmd.timing, format!("desktop_{LINUX}_ms"));
}

#[test]
fn metadata_fails_on_a_stale_lock_file_without_changing_it() {
    let dir = scratch("stale", &STALE_LOCK);
    let before = fs::read(dir.join("ws/Cargo.lock")).unwrap();
    let cmd = in_scratch(&dir, metadata_command("ws", LINUX));
    let error = crate::check::capture(&dir, &cmd).unwrap_err();
    assert!(error.contains("exited with exit status: 101"), "{error}");
    assert!(error.contains("--locked was passed"), "{error}");
    assert_eq!(fs::read(dir.join("ws/Cargo.lock")).unwrap(), before);
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn metadata_does_not_go_online_for_a_missing_package() {
    let dir = scratch(
        "offline",
        &[
            (
                "ws/Cargo.toml",
                "[package]\nname = \"a\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[workspace]\n\n[dependencies]\nwinnow = { version = \"=1.0.4\", default-features = false }\n",
            ),
            ("ws/src/lib.rs", ""),
            (
                "ws/Cargo.lock",
                "version = 4\n\n[[package]]\nname = \"a\"\nversion = \"0.1.0\"\ndependencies = [\n \"winnow\",\n]\n\n[[package]]\nname = \"winnow\"\nversion = \"1.0.4\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\nchecksum = \"23b97319f7b8343df12cc98938e5c3eb436064524c8d2b4e30a1d3a36eecdf81\"\n",
            ),
        ],
    );
    let cmd = in_scratch(&dir, metadata_command("ws", LINUX));
    let error = crate::check::capture(&dir, &cmd).unwrap_err();
    assert!(error.contains("exited with exit status: 101"), "{error}");
    assert!(
        error.contains("no matching package named `winnow` found"),
        "{error}"
    );
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn reads_the_root_without_symbolic_links_as_cargo_reports_the_members() {
    let dir = scratch(
        "link",
        &[
            (
                "repo/ws/Cargo.toml",
                "[package]\nname = \"a\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[workspace]\n",
            ),
            ("repo/ws/src/lib.rs", ""),
            (
                "repo/ws/Cargo.lock",
                "version = 4\n\n[[package]]\nname = \"a\"\nversion = \"0.1.0\"\n",
            ),
        ],
    );
    let link = dir.join("link");
    std::os::unix::fs::symlink(dir.join("repo"), &link).unwrap();
    let cmd = in_scratch(&dir, metadata_command("ws", LINUX));
    let text = String::from_utf8(crate::check::capture(&link, &cmd).unwrap()).unwrap();
    let read = metadata::parse(&text).unwrap();
    let manifest = &read.packages[0].manifest_path;
    // Cargo reports the member without the link, so only the root read
    // without it holds the member.
    assert_eq!(inside(link.to_str().unwrap(), manifest), None);
    let root = real_root(&link).unwrap();
    assert_eq!(inside(&root, manifest), Some("ws/Cargo.toml"));
    fs::remove_dir_all(&dir).unwrap();
}
