//! Tests of the determinism lints themselves: Clippy, with the root
//! `clippy.toml` of the repository, on a fixture crate on the deterministic
//! path.
//!
//! The fixture crate is written into a scratch directory of this crate's
//! ignored build directory, with its own build directory, and checked once
//! with the Clippy of the pinned toolchain, offline. It depends on the
//! `libm` that the root of `framework/` declares, which the fetch step of
//! `cargo xtask check` downloads. It denies `disallowed_methods` and
//! `disallowed_types` as the product workspaces do, and each file of it
//! holds one case; the tests compare what Clippy reports in each file.

use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;

use serde_json::Value;
use toml::Table;

use crate::check;
use crate::repo;

/// A diagnostic that Clippy or the compiler reported for the fixture: the
/// file of its primary span, relative to the fixture crate, its line, its
/// lint or error code, empty for none, and its message.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Diagnostic {
    file: String,
    line: u64,
    code: String,
    message: String,
}

/// The cases of the fixture crate: each file under `src/` and its code.
const CASES: [(&str, &str); 10] = [
    (
        "method_call",
        "pub fn sine(x: f32) -> f32 {\n    x.sin()\n}\n",
    ),
    (
        "path_call",
        "pub fn power(a: f64, b: f64) -> f64 {\n    f64::powf(a, b)\n}\n",
    ),
    (
        "function_pointer",
        "pub fn exponential(x: f32) -> f32 {\n    let f: fn(f32) -> f32 = f32::exp;\n    f(x)\n}\n",
    ),
    (
        "algebraic",
        "pub fn add(a: f32, b: f32) -> f32 {\n    a.algebraic_add(b)\n}\n",
    ),
    (
        "maximum",
        "pub fn larger(a: f32, b: f32) -> f32 {\n    a.max(b)\n}\n",
    ),
    (
        "outside_the_wrapper",
        "pub fn sine(x: f32) -> f32 {\n    libm::sinf(x)\n}\n",
    ),
    (
        "det_math",
        "#![expect(\n    clippy::disallowed_methods,\n    reason = \"the wrapper of the transcendental functions calls libm\"\n)]\n\npub fn sinf(x: f32) -> f32 {\n    libm::sinf(x)\n}\n",
    ),
    (
        "own_type",
        "pub struct Angle(pub f32);\n\nimpl Angle {\n    pub fn sin(&self) -> f32 {\n        self.0\n    }\n}\n\npub fn sine(angle: &Angle) -> f32 {\n    angle.sin()\n}\n",
    ),
    (
        "hash_map",
        "pub fn entries() -> usize {\n    std::collections::HashMap::<u8, u8>::new().len()\n}\n",
    ),
    (
        "re_exported",
        "pub fn entries() -> usize {\n    std::collections::hash_map::HashMap::<u8, u8>::new().len()\n}\n",
    ),
];

/// The file in which the fixture names every method and function of the
/// root configuration, one on each line, and every type.
const EVERY_ENTRY: &str = "every_entry";

/// The methods of the configuration that are not stable in the pinned
/// toolchain, so stable code cannot name them; they are listed for when
/// they become stable.
const UNSTABLE: [&str; 4] = ["gamma", "ln_gamma", "erf", "erfc"];

/// The types of the configuration, each as the fixture names it.
const TYPES: [(&str, &str); 4] = [
    (
        "std::collections::HashMap",
        "std::collections::HashMap<u8, u8>",
    ),
    ("std::collections::HashSet", "std::collections::HashSet<u8>"),
    ("std::hash::RandomState", "std::hash::RandomState"),
    ("libm::Libm", "libm::Libm<f32>"),
];

/// The paths of the entries of `key` in the root configuration `config`.
fn listed(config: &Table, key: &str) -> Vec<String> {
    config
        .get(key)
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| match entry {
            toml::Value::String(path) => Some(path.clone()),
            _ => entry.get("path")?.as_str().map(str::to_string),
        })
        .collect()
}

/// Whether the method `path`, `f32::<name>` or `f64::<name>`, is unstable.
fn is_unstable(path: &str) -> bool {
    UNSTABLE.iter().any(|name| {
        path.strip_suffix(name)
            .is_some_and(|ty| ty == "f32::" || ty == "f64::")
    })
}

/// The root configuration of the repository.
fn config() -> Table {
    let path = repo::root().join("clippy.toml");
    fs::read_to_string(&path)
        .unwrap()
        .parse()
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// The line of [`EVERY_ENTRY`] that names the first method.
const FIRST_METHOD: u64 = 4;

/// The code of [`EVERY_ENTRY`]: the function `methods`, which names one
/// method or function on each line from [`FIRST_METHOD`], and the function
/// `types`, which takes a parameter of each type on each line from the
/// third after the last method.
fn every_entry(methods: &[String]) -> String {
    let mut text = "#![allow(deprecated)]\n\npub fn methods() {\n".to_string();
    for path in methods {
        text.push_str(&format!("    let _ = {path};\n"));
    }
    text.push_str("}\n\npub fn types(\n");
    for (_, named) in TYPES {
        text.push_str(&format!("    _: &{named},\n"));
    }
    text.push_str(") {\n}\n");
    text
}

/// The requirement of `libm` that the root of `framework/` declares.
fn libm_requirement() -> String {
    let path = repo::root().join("framework/Cargo.toml");
    let manifest: Table = fs::read_to_string(path).unwrap().parse().unwrap();
    manifest["workspace"]["dependencies"]["libm"]["version"]
        .as_str()
        .unwrap()
        .to_string()
}

/// Writes the fixture crate into `dir`.
fn write_fixture(dir: &Path, methods: &[String]) {
    let manifest = format!(
        "[package]\nname = \"determinism-fixture\"\nversion = \"0.0.0\"\nedition = \"2024\"\npublish = false\n\n[workspace]\n\n[dependencies]\nlibm = {{ version = \"{}\", default-features = false }}\n\n[lints.clippy]\ndisallowed_methods = \"deny\"\ndisallowed_types = \"deny\"\n",
        libm_requirement()
    );
    let mut lib = String::new();
    let mut files = vec![("Cargo.toml".to_string(), manifest)];
    for (name, code) in CASES {
        lib.push_str(&format!("pub mod {name};\n"));
        files.push((format!("src/{name}.rs"), code.to_string()));
    }
    lib.push_str(&format!("pub mod {EVERY_ENTRY};\n"));
    files.push((format!("src/{EVERY_ENTRY}.rs"), every_entry(methods)));
    files.push(("src/lib.rs".to_string(), lib));
    for (path, text) in files {
        let path = dir.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
}

/// The diagnostics of the fixture crate in the messages that
/// `cargo clippy --message-format=json` printed.
fn diagnostics(stdout: &str, manifest: &str) -> Vec<Diagnostic> {
    stdout
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|message| {
            message["reason"] == "compiler-message" && message["manifest_path"] == manifest
        })
        .filter_map(|message| {
            let message = &message["message"];
            let text = message["message"].as_str().unwrap_or_default();
            // The compiler's counts of its errors and warnings.
            if text.starts_with("aborting due to") || text.ends_with("emitted") {
                return None;
            }
            let primary = message["spans"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|span| span["is_primary"] == true);
            Some(Diagnostic {
                file: primary
                    .and_then(|span| span["file_name"].as_str())
                    .unwrap_or_default()
                    .to_string(),
                line: primary
                    .and_then(|span| span["line_start"].as_u64())
                    .unwrap_or_default(),
                code: message["code"]["code"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                message: text.to_string(),
            })
        })
        .collect()
}

/// Runs Clippy once on the fixture crate and returns its diagnostics, or
/// why Clippy did not run.
fn clippy() -> Result<Vec<Diagnostic>, String> {
    let root = repo::root();
    let dir = root
        .join("xtask/target")
        .join(format!("clippy-fixture-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let methods: Vec<String> = listed(&config(), "disallowed-methods")
        .into_iter()
        .filter(|path| !is_unstable(path))
        .collect();
    write_fixture(&dir, &methods);
    let manifest = dir.join("Cargo.toml");
    let output = Command::new(check::cargo())
        .args([
            "clippy",
            "--offline",
            "--message-format=json",
            "--manifest-path",
        ])
        .arg(&manifest)
        .env(check::OFFLINE.0, check::OFFLINE.1)
        .env("CARGO_TARGET_DIR", dir.join("target"))
        .env("CLIPPY_CONF_DIR", &root)
        .current_dir(&dir)
        .output()
        .map_err(|error| format!("cannot run cargo clippy: {error}"))?;
    let _ = fs::remove_dir_all(&dir);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let found = diagnostics(&stdout, &manifest.to_string_lossy());
    if found.is_empty() {
        return Err(format!(
            "cargo clippy on the fixture reported nothing, with {}; the fetch step of cargo xtask check downloads libm:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(found)
}

/// The diagnostics of the single run of Clippy on the fixture.
fn reported() -> &'static [Diagnostic] {
    static RUN: OnceLock<Result<Vec<Diagnostic>, String>> = OnceLock::new();
    match RUN.get_or_init(clippy) {
        Ok(found) => found,
        Err(why) => panic!("{why}"),
    }
}

/// What Clippy reported in the case `name`, as (line, code, message).
fn reported_in(name: &str) -> Vec<(u64, &str, &str)> {
    let file = format!("src/{name}.rs");
    reported()
        .iter()
        .filter(|found| found.file == file)
        .map(|found| (found.line, found.code.as_str(), found.message.as_str()))
        .collect()
}

const METHODS: &str = "clippy::disallowed_methods";
const TYPES_LINT: &str = "clippy::disallowed_types";

#[test]
fn clippy_reports_a_method_call_of_f32_sin() {
    assert_eq!(
        reported_in("method_call"),
        [(2, METHODS, "use of a disallowed method `f32::sin`")]
    );
}

#[test]
fn clippy_reports_a_path_call_of_f64_powf() {
    assert_eq!(
        reported_in("path_call"),
        [(2, METHODS, "use of a disallowed method `f64::powf`")]
    );
}

#[test]
fn clippy_reports_f32_exp_as_a_function_pointer() {
    assert_eq!(
        reported_in("function_pointer"),
        [(2, METHODS, "use of a disallowed method `f32::exp`")]
    );
}

#[test]
fn clippy_reports_algebraic_add() {
    assert_eq!(
        reported_in("algebraic"),
        [(
            2,
            METHODS,
            "use of a disallowed method `f32::algebraic_add`"
        )]
    );
}

#[test]
fn clippy_reports_the_maximum_of_two_f32() {
    assert_eq!(
        reported_in("maximum"),
        [(2, METHODS, "use of a disallowed method `f32::max`")]
    );
}

#[test]
fn clippy_reports_libm_outside_the_wrapper_module() {
    assert_eq!(
        reported_in("outside_the_wrapper"),
        [(2, METHODS, "use of a disallowed method `libm::sinf`")]
    );
    // The wrapper's expectation is met, so nothing is reported there.
    assert_eq!(reported_in("det_math"), []);
}

#[test]
fn clippy_does_not_report_the_sin_method_of_an_own_type() {
    assert_eq!(reported_in("own_type"), []);
}

#[test]
fn clippy_reports_a_std_hash_map() {
    assert_eq!(
        reported_in("hash_map"),
        [(
            2,
            TYPES_LINT,
            "use of a disallowed type `std::collections::HashMap`"
        )]
    );
}

#[test]
fn clippy_reports_a_type_through_its_re_export() {
    assert_eq!(
        reported_in("re_exported"),
        [(
            2,
            TYPES_LINT,
            "use of a disallowed type `std::collections::HashMap`"
        )]
    );
}

#[test]
fn clippy_reports_every_listed_method_function_and_type() {
    let config = config();
    let types: Vec<String> = listed(&config, "disallowed-types");
    assert_eq!(types, TYPES.map(|(path, _)| path));
    let methods: Vec<String> = listed(&config, "disallowed-methods")
        .into_iter()
        .filter(|path| !is_unstable(path))
        .collect();
    let mut expected: Vec<(u64, &str, String)> = Vec::new();
    for (line, path) in (FIRST_METHOD..).zip(&methods) {
        expected.push((
            line,
            METHODS,
            format!("use of a disallowed method `{path}`"),
        ));
    }
    let first = FIRST_METHOD + methods.len() as u64 + 3;
    for (line, (path, _)) in (first..).zip(TYPES) {
        expected.push((
            line,
            TYPES_LINT,
            format!("use of a disallowed type `{path}`"),
        ));
    }
    let mut found: Vec<(u64, &str, String)> = reported_in(EVERY_ENTRY)
        .into_iter()
        .map(|(line, code, message)| (line, code, message.to_string()))
        .collect();
    found.sort();
    assert_eq!(found, expected);
}

#[test]
fn clippy_reports_nothing_but_the_cases_in_the_fixture() {
    let cases: Vec<String> = CASES
        .iter()
        .map(|(name, _)| *name)
        .chain([EVERY_ENTRY])
        .map(|name| format!("src/{name}.rs"))
        .collect();
    let other: Vec<&Diagnostic> = reported()
        .iter()
        .filter(|found| {
            !cases.contains(&found.file) || (found.code != METHODS && found.code != TYPES_LINT)
        })
        .collect();
    assert_eq!(other, Vec::<&Diagnostic>::new());
}
