//! Tests of the build-script check.

use super::super::fixture::{self, Dep, Model, Pkg};
use super::super::tests::{
    desktop_through_a_facade, entry, problems_with, with_build_script, zlib_sys_entry,
    zlib_sys_with,
};
use super::*;

/// Makes `foo` in `model` depend on `pkg`, at version `1.0.0`, with the
/// license `MIT`, and lists it in the allowlist with the entry `listed`.
fn add_to_foo(model: &mut Model, pkg: Pkg, listed: &str) {
    let to = pkg.key();
    model.add("framework", pkg);
    model
        .pkg("framework", "foo@1.0.0")
        .deps
        .push(Dep::new(&to, "^1"));
    model.append("framework/allowlist.toml", listed);
}

/// The allowlist entry of a package with a build script at version
/// `1.0.0`, reached from `kosmo-render`, followed by `more`.
fn built(name: &str, more: &str) -> String {
    format!(
        "{}{more}",
        with_build_script(&entry(name, "1.0.0", "MIT", &[], &["kosmo-render"]))
    )
}

/// The valid model with `foo-sys 1.0.0`, whose build script links the
/// native library `z` and has the build dependencies `build`, each at
/// version `1.2.0`, listed in the allowlist, `foo-sys` followed by
/// `more`.
fn foo_sys_with(build: &[&str], more: &str) -> Model {
    let mut model = fixture::valid();
    let mut foo_sys = Pkg::crates_io("foo-sys", "1.0.0").links("z");
    for name in build {
        foo_sys = foo_sys.dep(Dep::new(&format!("{name}@1.2.0"), "^1.2").kind("build"));
        model.add("framework", Pkg::crates_io(name, "1.2.0"));
        model.append(
            "framework/allowlist.toml",
            &entry(name, "1.2.0", "MIT", &[], &["kosmo-render"]),
        );
    }
    add_to_foo(
        &mut model,
        foo_sys,
        &built("foo-sys", &format!("links = \"z\"\n{more}")),
    );
    model
}

/// The valid model of [`zlib_sys_with`] in which the allowlist entry of
/// `zlib-sys` is followed by `more`.
fn zlib_sys_listed_with(build: &[&str], more: &str) -> Model {
    let mut model = zlib_sys_with(build);
    let listed = zlib_sys_entry();
    model.edit(
        "framework/allowlist.toml",
        &listed,
        &format!("{listed}{more}"),
    );
    model
}

#[test]
fn rejects_a_package_that_declares_links() {
    assert_eq!(
        foo_sys_with(&[], "").problems(),
        ["framework/Cargo.lock: foo-sys 1.0.0: it declares `links = \"z\"`"]
    );
}

#[test]
fn rejects_build_dependencies_that_compile_or_bind_native_code() {
    // The list that CONTRIBUTING.md gives, with what each package does.
    let listed = [
        ("cc", "compiles C or C++"),
        ("cmake", "compiles C or C++"),
        ("cxx-build", "compiles C++"),
        ("autotools", "compiles C or C++"),
        ("nasm-rs", "assembles native code"),
        ("bindgen", "generates bindings to C headers"),
        ("pkg-config", "looks for system libraries"),
        ("vcpkg", "looks for system libraries"),
        ("system-deps", "looks for system libraries"),
    ];
    assert_eq!(NATIVE_CODE.len(), listed.len());
    for (name, what) in listed {
        assert_eq!(
            zlib_sys_with(&[name]).problems(),
            [format!(
                "framework/Cargo.lock: zlib-sys 1.0.0: its build script {what} (build dependency {name})"
            )]
        );
    }
}

#[test]
fn rejects_build_dependencies_that_can_download() {
    // The list that CONTRIBUTING.md gives.
    let listed = [
        "reqwest",
        "ureq",
        "curl",
        "attohttpc",
        "minreq",
        "http_req",
        "hyper",
        "isahc",
        "git2",
    ];
    assert_eq!(DOWNLOAD.len(), listed.len());
    for name in listed {
        assert_eq!(
            zlib_sys_with(&[name]).problems(),
            [format!(
                "framework/Cargo.lock: zlib-sys 1.0.0: its build script may download at build time (build dependency {name})"
            )]
        );
    }
}

#[test]
fn follows_the_build_dependencies_to_what_they_bring_in() {
    /// Makes the package `from` of `model`, at version `1.2.0`, depend on
    /// `to` at the same version, as a normal dependency, and lists `to`.
    fn chain(model: &mut Model, from: &str, to: &str) {
        model.add("framework", Pkg::crates_io(to, "1.2.0"));
        model
            .pkg("framework", &format!("{from}@1.2.0"))
            .deps
            .push(Dep::new(&format!("{to}@1.2.0"), "^1.2"));
        model.append(
            "framework/allowlist.toml",
            &entry(to, "1.2.0", "MIT", &[], &["kosmo-render"]),
        );
    }
    let mut model = zlib_sys_with(&["helper"]);
    chain(&mut model, "helper", "cc");
    assert_eq!(
        model.problems(),
        [
            "framework/Cargo.lock: zlib-sys 1.0.0: its build script compiles C or C++ (build dependency helper -> cc)"
        ]
    );
    let mut model = zlib_sys_with(&["helper"]);
    chain(&mut model, "helper", "inner");
    chain(&mut model, "inner", "ureq");
    assert_eq!(
        model.problems(),
        [
            "framework/Cargo.lock: zlib-sys 1.0.0: its build script may download at build time (build dependency helper -> inner -> ureq)"
        ]
    );
    // Only the build dependencies of a package with a build script count.
    let found = problems_with(|model| {
        model.add("framework", Pkg::crates_io("cc", "1.2.0"));
        model.append(
            "framework/allowlist.toml",
            &entry("cc", "1.2.0", "MIT", &[], &["kosmo-render"]),
        );
        add_to_foo(
            model,
            Pkg::crates_io("tool", "1.0.0").dep(Dep::new("cc@1.2.0", "^1.2")),
            &entry("tool", "1.0.0", "MIT", &[], &["kosmo-render"]),
        );
        add_to_foo(
            model,
            Pkg::crates_io("built", "1.0.0")
                .build_script()
                .dep(Dep::new("cc@1.2.0", "^1.2")),
            &built("built", ""),
        );
    });
    assert_eq!(found, Vec::<String>::new());
}

#[test]
fn reports_every_reason_of_a_build_script() {
    assert_eq!(
        foo_sys_with(&["cc", "ureq"], "").problems(),
        [
            "framework/Cargo.lock: foo-sys 1.0.0: it declares `links = \"z\"`",
            "framework/Cargo.lock: foo-sys 1.0.0: its build script compiles C or C++ (build dependency cc)",
            "framework/Cargo.lock: foo-sys 1.0.0: its build script may download at build time (build dependency ureq)",
        ]
    );
}

#[test]
fn build_dependencies_of_other_platforms_do_not_count() {
    let mut model = zlib_sys_with(&[]);
    model.add("framework", Pkg::crates_io("cc", "1.2.0"));
    model.pkg("framework", "zlib-sys@1.0.0").deps.push(
        Dep::new("cc@1.2.0", "^1.2")
            .kind("build")
            .cfg("cfg(windows)", &[]),
    );
    assert_eq!(model.problems(), Vec::<String>::new());
}

#[test]
fn other_build_scripts_are_left_to_review() {
    let found = problems_with(|model| {
        for name in ["proc-macro2", "quote", "serde", "serde_core"] {
            add_to_foo(
                model,
                Pkg::crates_io(name, "1.0.0").build_script(),
                &built(name, ""),
            );
        }
    });
    assert_eq!(found, Vec::<String>::new());
    assert_eq!(zlib_sys_with(&["autocfg"]).problems(), Vec::<String>::new());
}

#[test]
fn an_exception_with_its_reason_allows_a_build_script() {
    const REASON: &str = "native-code-exception = \"Reviewed: it compiles the bundled zlib.\"\n";
    assert_eq!(
        zlib_sys_listed_with(&["cc"], REASON).problems(),
        Vec::<String>::new()
    );
    for blank in ["\"\"", "\" \""] {
        assert_eq!(
            zlib_sys_listed_with(&["cc"], &format!("native-code-exception = {blank}\n")).problems(),
            ["framework/allowlist.toml: zlib-sys 1.0.0: the native-code-exception has no reason"]
        );
    }
    let found = problems_with(|model| {
        add_to_foo(
            model,
            Pkg::crates_io("serde", "1.0.229").build_script(),
            &format!(
                "{}{REASON}",
                with_build_script(&entry("serde", "1.0.229", "MIT", &[], &["kosmo-render"]))
            ),
        );
    });
    assert_eq!(
        found,
        [
            "framework/allowlist.toml: serde 1.0.229 needs no native-code-exception: it has no build script with a build dependency that compiles native code or looks for or binds to system libraries"
        ]
    );
    // The desktop resolves `zlib-sys` too, through the facade, and its
    // exception does not hold for the framework.
    let mut model = desktop_through_a_facade();
    for dir in ["framework", "desktop"] {
        model.add(dir, Pkg::crates_io("cc", "1.2.0"));
        model.add(
            dir,
            Pkg::crates_io("zlib-sys", "1.0.0")
                .build_script()
                .dep(Dep::new("cc@1.2.0", "^1.2").kind("build")),
        );
        model
            .pkg(dir, "foo@1.0.0")
            .deps
            .push(Dep::new("zlib-sys@1.0.0", "^1"));
        model.append(
            &format!("{dir}/allowlist.toml"),
            &entry("cc", "1.2.0", "MIT", &[], &["kosmo-render"]),
        );
    }
    model.append("framework/allowlist.toml", &zlib_sys_entry());
    model.append(
        "desktop/allowlist.toml",
        &format!("{}{REASON}", zlib_sys_entry()),
    );
    assert_eq!(
        model.problems(),
        [
            "framework/Cargo.lock: zlib-sys 1.0.0: its build script compiles C or C++ (build dependency cc)"
        ]
    );
}

#[test]
fn each_exception_covers_its_own_kind() {
    let links = "links-exception = \"Reviewed: it links the system zlib.\"\n";
    assert_eq!(
        foo_sys_with(&["cc", "ureq"], links).problems(),
        [
            "framework/Cargo.lock: foo-sys 1.0.0: its build script compiles C or C++ (build dependency cc)",
            "framework/Cargo.lock: foo-sys 1.0.0: its build script may download at build time (build dependency ureq)",
        ]
    );
    let all = format!(
        "{links}native-code-exception = \"Reviewed.\"\ndownload-exception = \"Reviewed.\"\n"
    );
    assert_eq!(
        foo_sys_with(&["cc", "ureq"], &all).problems(),
        Vec::<String>::new()
    );
    assert_eq!(
        zlib_sys_listed_with(&["cc"], links).problems(),
        [
            "framework/Cargo.lock: zlib-sys 1.0.0: its build script compiles C or C++ (build dependency cc)",
            "framework/allowlist.toml: zlib-sys 1.0.0 needs no links-exception: it has no build script with `links`",
        ]
    );
    assert_eq!(
        zlib_sys_listed_with(&["cc"], "download-exception = \"Reviewed.\"\n").problems(),
        [
            "framework/Cargo.lock: zlib-sys 1.0.0: its build script compiles C or C++ (build dependency cc)",
            "framework/allowlist.toml: zlib-sys 1.0.0 needs no download-exception: it has no build script with a build dependency that can download",
        ]
    );
}

#[test]
fn the_build_scripts_of_the_packages_of_xtask_are_checked_too() {
    let found = problems_with(|model| {
        model.pkg("xtask", "yaml-rust2@0.13.0").links = Some("yaml".to_string());
        model.pkg("xtask", "yaml-rust2@0.13.0").build_script = true;
        model.edit(
            "xtask/allowlist.toml",
            "build-script = false\n",
            "build-script = true\nlinks = \"yaml\"\n",
        );
    });
    assert_eq!(
        found,
        ["xtask/Cargo.lock: yaml-rust2 0.13.0: it declares `links = \"yaml\"`"]
    );
}
