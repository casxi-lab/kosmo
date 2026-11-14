//! A model of a repository's workspaces for the tests of the fetch and deps
//! steps: its layout table, allowlists and lock files, and the output of
//! `cargo metadata` for each workspace and target, which the model derives
//! the way Cargo filters by platform.
//!
//! The valid model has crates in all three product workspaces. The
//! framework's `kosmo-render` depends on `foo 1.0.0` from crates.io, which
//! depends on `bar 2.1.0`, and `kosmo-ui` has the dev dependency
//! `qa 0.3.0`; xtask depends on `yaml-rust2 0.13.0`.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::{CRATES_IO, Report, inspect};
use crate::check::{Cmd, Facts};
use crate::layout::{self, Index};
use crate::repo::Tree;

/// The absolute path of the repository.
pub const ROOT: &str = "/work/kosmo";
pub const LINUX: &str = "aarch64-unknown-linux-gnu";
pub const DARWIN: &str = "aarch64-apple-darwin";
/// The checksum of every crates.io package in the lock files of the model,
/// which nothing verifies.
const CHECKSUM: &str = "8f2c0a5d6b1e4f3a9c7d2e0b5a8f1c6d3e9b4a7f0c2d5e8b1a4f7c0d3e6b9a2f";

/// The workspaces and tools of the layout table.
const WORKSPACES: &str = r#"[workspaces.display]
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

/// The crates of the valid layout table, with the allow entries of
/// `kosmo-render` and `kosmo-ui`.
const CRATES: &str = r#"
[crates.kosmo-types]
path = "display/kosmo-types"
layer = "D0"
export = true
pure-core = true
publish = false

[crates.kosmo-server]
path = "display/kosmo-server"
layer = "D3"
export = false
pure-core = false
publish = false

[crates.kosmo-math-model]
path = "framework/kosmo-math-model"
layer = "F0"
export = false
pure-core = false
publish = false

[crates.kosmo-jobs]
path = "framework/kosmo-jobs"
layer = "F0"
export = false
pure-core = false
publish = false
same-layer = ["kosmo-math-model"]

[crates.kosmo-render]
path = "framework/kosmo-render"
layer = "F0"
export = false
pure-core = false
publish = false

[[crates.kosmo-render.external]]
name = "foo"
version = "=1.0.0"
default-features = false
features = ["x"]
resolved-features = ["x"]

[crates.kosmo-ui]
path = "framework/kosmo-ui"
layer = "F1"
export = false
pure-core = false
publish = false

[[crates.kosmo-ui.external]]
name = "qa"
version = "=0.3.0"
kind = "dev"
default-features = true
features = []
resolved-features = ["default"]

[crates.kosmo-calc]
path = "desktop/kosmo-calc"
layer = "S1"
export = false
pure-core = false
publish = false

[crates.kosmo]
path = "desktop/kosmo"
layer = "S2"
export = false
pure-core = false
publish = false
"#;

const XTASK_ALLOWLIST: &str = r#"# The allowed crates.io packages of xtask.

[[package]]
name = "yaml-rust2"
version = "0.13.0"
source = "crates.io"
license = "MIT OR Apache-2.0"
build-script = false
features = []
crates = ["xtask"]
"#;

const FRAMEWORK_ALLOWLIST: &str = r#"# The allowed crates.io packages of the framework.

[[package]]
name = "bar"
version = "2.1.0"
source = "crates.io"
license = "MIT OR Apache-2.0"
build-script = false
features = ["default"]
crates = ["kosmo-render"]

[[package]]
name = "foo"
version = "1.0.0"
source = "crates.io"
license = "MIT"
build-script = false
features = ["x"]
crates = ["kosmo-render"]

[[package]]
name = "qa"
version = "0.3.0"
source = "crates.io"
license = "Apache-2.0"
build-script = false
features = ["default"]
crates = ["kosmo-ui"]
"#;

const EMPTY_ALLOWLIST: &str = "# No crates.io package.\n";

/// Where a package comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// A member of the workspace, in this directory of the repository.
    Member(String),
    /// A path package that is not a member, in this absolute directory.
    Path(String),
    /// A registry or Git package with this source.
    Registry(String),
}

/// A dependency of a package of the model.
#[derive(Debug, Clone)]
pub struct Dep {
    /// The package depended on, as `<name>@<version>`.
    pub to: String,
    pub req: String,
    /// `None` for a normal dependency, else `"build"` or `"dev"`.
    pub kind: Option<&'static str>,
    /// The condition of a `[target.<condition>.*]` table.
    pub cfg: Option<String>,
    pub optional: bool,
    pub default_features: bool,
    pub features: Vec<String>,
    /// The targets whose resolution has the dependency; `None` for all.
    pub on: Option<Vec<&'static str>>,
}

impl Dep {
    pub fn new(to: &str, req: &str) -> Dep {
        Dep {
            to: to.to_string(),
            req: req.to_string(),
            kind: None,
            cfg: None,
            optional: false,
            default_features: true,
            features: Vec::new(),
            on: None,
        }
    }

    pub fn kind(mut self, kind: &'static str) -> Dep {
        self.kind = Some(kind);
        self
    }

    /// Declared under the condition `cfg` and resolved only for `on`.
    pub fn cfg(mut self, cfg: &str, on: &[&'static str]) -> Dep {
        self.cfg = Some(cfg.to_string());
        self.on = Some(on.to_vec());
        self
    }

    pub fn features(mut self, default_features: bool, features: &[&str]) -> Dep {
        self.default_features = default_features;
        self.features = features.iter().map(|feature| feature.to_string()).collect();
        self
    }
}

/// A package of the model.
#[derive(Debug, Clone)]
pub struct Pkg {
    pub name: String,
    pub version: String,
    pub source: Source,
    pub license: Option<String>,
    pub license_file: Option<String>,
    pub build_script: bool,
    /// Whether it is a procedural-macro library.
    pub proc_macro: bool,
    pub links: Option<String>,
    /// Its resolved features on every target.
    pub features: Vec<String>,
    /// Further resolved features on some targets.
    pub features_on: BTreeMap<&'static str, Vec<String>>,
    pub deps: Vec<Dep>,
    /// Whether `Cargo.lock` records a checksum, for a registry package.
    pub checksum: bool,
}

impl Pkg {
    fn new(name: &str, version: &str, source: Source) -> Pkg {
        Pkg {
            name: name.to_string(),
            version: version.to_string(),
            source,
            license: Some("MIT".to_string()),
            license_file: None,
            build_script: false,
            proc_macro: false,
            links: None,
            features: Vec::new(),
            features_on: BTreeMap::new(),
            deps: Vec::new(),
            checksum: true,
        }
    }

    /// A member of the workspace in `dir`, at version `0.1.0-dev.1`.
    pub fn member(dir: &str, name: &str) -> Pkg {
        Pkg::new(name, "0.1.0-dev.1", Source::Member(dir.to_string()))
    }

    /// A package from crates.io, with the license `MIT`.
    pub fn crates_io(name: &str, version: &str) -> Pkg {
        Pkg::new(name, version, Source::Registry(CRATES_IO.to_string()))
    }

    /// A path package that is not a member, in the absolute directory
    /// `dir`.
    pub fn path(dir: &str, name: &str, version: &str) -> Pkg {
        Pkg::new(name, version, Source::Path(dir.to_string()))
    }

    pub fn dep(mut self, dep: Dep) -> Pkg {
        self.deps.push(dep);
        self
    }

    pub fn license(mut self, license: &str) -> Pkg {
        self.license = Some(license.to_string());
        self
    }

    /// With a build script.
    pub fn build_script(mut self) -> Pkg {
        self.build_script = true;
        self
    }

    /// With `links = "<library>"` and a build script.
    pub fn links(mut self, library: &str) -> Pkg {
        self.links = Some(library.to_string());
        self.build_script()
    }

    pub fn resolved(mut self, features: &[&str]) -> Pkg {
        self.features = features.iter().map(|feature| feature.to_string()).collect();
        self
    }

    pub fn key(&self) -> String {
        format!("{}@{}", self.name, self.version)
    }

    pub fn id(&self) -> String {
        match &self.source {
            Source::Member(dir) => format!("path+file://{ROOT}/{dir}#{}", self.key()),
            Source::Path(dir) => format!("path+file://{dir}#{}", self.key()),
            Source::Registry(source) => format!("{source}#{}", self.key()),
        }
    }

    fn manifest_path(&self) -> String {
        match &self.source {
            Source::Member(dir) => format!("{ROOT}/{dir}/Cargo.toml"),
            Source::Path(dir) => format!("{dir}/Cargo.toml"),
            Source::Registry(_) => format!(
                "/home/user/.cargo/registry/src/index/{}-{}/Cargo.toml",
                self.name, self.version
            ),
        }
    }

    fn source_value(&self) -> Value {
        match &self.source {
            Source::Registry(source) => json!(source),
            Source::Member(_) | Source::Path(_) => Value::Null,
        }
    }
}

/// A workspace of the model.
#[derive(Debug, Clone)]
pub struct Ws {
    pub dir: String,
    pub packages: Vec<Pkg>,
}

impl Ws {
    fn members(&self) -> impl Iterator<Item = &Pkg> {
        self.packages
            .iter()
            .filter(|pkg| matches!(pkg.source, Source::Member(_)))
    }

    fn find(&self, key: &str) -> Option<&Pkg> {
        self.packages.iter().find(|pkg| pkg.key() == key)
    }

    /// The packages that the resolution for `target` reaches, by key, with
    /// the dependencies it has.
    fn reached(&self, target: &str) -> BTreeMap<String, (&Pkg, Vec<&Dep>)> {
        let mut reached = BTreeMap::new();
        let mut stack: Vec<&Pkg> = self.members().collect();
        while let Some(pkg) = stack.pop() {
            if reached.contains_key(&pkg.key()) {
                continue;
            }
            let deps: Vec<&Dep> = pkg
                .deps
                .iter()
                .filter(|dep| dep.on.as_ref().is_none_or(|on| on.contains(&target)))
                .filter(|dep| self.find(&dep.to).is_some())
                .collect();
            for dep in &deps {
                if let Some(next) = self.find(&dep.to) {
                    stack.push(next);
                }
            }
            reached.insert(pkg.key(), (pkg, deps));
        }
        reached
    }

    /// The output of `cargo metadata --filter-platform <target>`.
    pub fn metadata(&self, target: &str) -> String {
        let reached = self.reached(target);
        let packages: Vec<Value> = reached
            .values()
            .map(|(pkg, _)| {
                let library = if pkg.proc_macro { "proc-macro" } else { "lib" };
                let mut targets = vec![json!({ "kind": [library] })];
                if pkg.build_script {
                    targets.push(json!({ "kind": ["custom-build"] }));
                }
                let dependencies: Vec<Value> = pkg
                    .deps
                    .iter()
                    .map(|dep| {
                        let to = self.find(&dep.to);
                        json!({
                            "name": to.map_or(dep.to.as_str(), |to| to.name.as_str()),
                            "source": to.map_or(Value::Null, Pkg::source_value),
                            "req": dep.req,
                            "kind": dep.kind,
                            "rename": null,
                            "optional": dep.optional,
                            "uses_default_features": dep.default_features,
                            "features": dep.features,
                            "target": dep.cfg,
                            "registry": null,
                        })
                    })
                    .collect();
                json!({
                    "name": pkg.name,
                    "version": pkg.version,
                    "id": pkg.id(),
                    "license": pkg.license,
                    "license_file": pkg.license_file,
                    "source": pkg.source_value(),
                    "dependencies": dependencies,
                    "targets": targets,
                    "manifest_path": pkg.manifest_path(),
                    "links": pkg.links,
                })
            })
            .collect();
        let nodes: Vec<Value> = reached
            .values()
            .map(|(pkg, deps)| {
                let mut by_target: BTreeMap<String, Vec<&Dep>> = BTreeMap::new();
                for dep in deps {
                    by_target.entry(dep.to.clone()).or_default().push(dep);
                }
                let edges: Vec<Value> = by_target
                    .iter()
                    .filter_map(|(key, deps)| {
                        let to = self.find(key)?;
                        let kinds: Vec<Value> = deps
                            .iter()
                            .map(|dep| json!({ "kind": dep.kind, "target": dep.cfg }))
                            .collect();
                        Some(json!({
                            "name": to.name.replace('-', "_"),
                            "pkg": to.id(),
                            "dep_kinds": kinds,
                        }))
                    })
                    .collect();
                let mut features: BTreeSet<String> = pkg.features.iter().cloned().collect();
                if let Some(more) = pkg.features_on.get(target) {
                    features.extend(more.iter().cloned());
                }
                json!({
                    "id": pkg.id(),
                    "dependencies": edges.iter().map(|edge| edge["pkg"].clone()).collect::<Vec<_>>(),
                    "deps": edges,
                    "features": features,
                })
            })
            .collect();
        let members: Vec<String> = self.members().map(Pkg::id).collect();
        json!({
            "packages": packages,
            "workspace_members": members,
            "workspace_default_members": members,
            "resolve": { "nodes": nodes, "root": null },
            "target_directory": format!("{ROOT}/{}/target", self.dir),
            "version": 1,
            "workspace_root": format!("{ROOT}/{}", self.dir),
            "metadata": null,
        })
        .to_string()
    }

    /// The `Cargo.lock` of the workspace: every package, for every
    /// platform.
    pub fn lock(&self) -> String {
        let mut packages: Vec<&Pkg> = self.packages.iter().collect();
        packages.sort_by_key(|pkg| (pkg.name.clone(), pkg.version.clone()));
        let mut text = "version = 4\n".to_string();
        for pkg in packages {
            text.push_str(&format!(
                "\n[[package]]\nname = \"{}\"\nversion = \"{}\"\n",
                pkg.name, pkg.version
            ));
            if let Source::Registry(source) = &pkg.source {
                text.push_str(&format!("source = \"{source}\"\n"));
                if pkg.checksum && source == CRATES_IO {
                    text.push_str(&format!("checksum = \"{CHECKSUM}\"\n"));
                }
            }
        }
        text
    }
}

/// A model of a repository.
#[derive(Debug, Clone)]
pub struct Model {
    /// The files besides the lock files, which the workspaces give.
    pub files: BTreeMap<String, String>,
    pub workspaces: Vec<Ws>,
    /// The directories of the workspaces whose `Cargo.lock` is not
    /// tracked.
    pub untracked_locks: Vec<String>,
}

/// The valid model, with crates in all three product workspaces.
pub fn valid() -> Model {
    let render = Pkg::member("framework/kosmo-render", "kosmo-render")
        .dep(Dep::new("foo@1.0.0", "=1.0.0").features(false, &["x"]));
    let ui = Pkg::member("framework/kosmo-ui", "kosmo-ui")
        .dep(Dep::new("kosmo-render@0.1.0-dev.1", "=0.1.0-dev.1"))
        .dep(Dep::new("kosmo-types@0.1.0-dev.1", "=0.1.0-dev.1"))
        .dep(Dep::new("qa@0.3.0", "=0.3.0").kind("dev"));
    let types_dir = format!("{ROOT}/display/kosmo-types");
    let workspaces = vec![
        Ws {
            dir: "xtask".to_string(),
            packages: vec![
                Pkg::member("xtask", "xtask")
                    .dep(Dep::new("yaml-rust2@0.13.0", "=0.13.0").features(false, &[])),
                Pkg::crates_io("yaml-rust2", "0.13.0").license("MIT OR Apache-2.0"),
            ],
        },
        Ws {
            dir: "display".to_string(),
            packages: vec![
                Pkg::member("display/kosmo-types", "kosmo-types"),
                Pkg::member("display/kosmo-server", "kosmo-server")
                    .dep(Dep::new("kosmo-types@0.1.0-dev.1", "=0.1.0-dev.1")),
            ],
        },
        Ws {
            dir: "framework".to_string(),
            packages: vec![
                Pkg::member("framework/kosmo-math-model", "kosmo-math-model"),
                Pkg::member("framework/kosmo-jobs", "kosmo-jobs")
                    .dep(Dep::new("kosmo-math-model@0.1.0-dev.1", "=0.1.0-dev.1")),
                render,
                ui,
                Pkg::path(&types_dir, "kosmo-types", "0.1.0-dev.1"),
                Pkg::crates_io("foo", "1.0.0")
                    .resolved(&["x"])
                    .dep(Dep::new("bar@2.1.0", "^2.1")),
                Pkg::crates_io("bar", "2.1.0")
                    .license("MIT OR Apache-2.0")
                    .resolved(&["default"]),
                Pkg::crates_io("qa", "0.3.0")
                    .license("Apache-2.0")
                    .resolved(&["default"]),
            ],
        },
        Ws {
            dir: "desktop".to_string(),
            packages: vec![
                Pkg::member("desktop/kosmo-calc", "kosmo-calc"),
                Pkg::member("desktop/kosmo", "kosmo")
                    .dep(Dep::new("kosmo-calc@0.1.0-dev.1", "=0.1.0-dev.1")),
            ],
        },
    ];
    let files = BTreeMap::from(
        [
            ("layout.toml", format!("{WORKSPACES}{CRATES}")),
            ("xtask/allowlist.toml", XTASK_ALLOWLIST.to_string()),
            ("display/allowlist.toml", EMPTY_ALLOWLIST.to_string()),
            ("framework/allowlist.toml", FRAMEWORK_ALLOWLIST.to_string()),
            ("desktop/allowlist.toml", EMPTY_ALLOWLIST.to_string()),
        ]
        .map(|(path, text)| (path.to_string(), text)),
    );
    Model {
        files,
        workspaces,
        untracked_locks: Vec::new(),
    }
}

/// The valid model in which only `desktop/` has a crate, the program
/// `kosmo`, as in this repository so far.
pub fn desktop_only() -> Model {
    let mut model = valid();
    model
        .workspaces
        .retain(|ws| ws.dir == "xtask" || ws.dir == "desktop");
    model
        .workspaces
        .iter_mut()
        .filter(|ws| ws.dir == "desktop")
        .for_each(|ws| ws.packages = vec![Pkg::member("desktop/kosmo", "kosmo")]);
    let crates = "\n[crates.kosmo]\npath = \"desktop/kosmo\"\nlayer = \"S2\"\nexport = false\npure-core = false\npublish = false\n";
    model
        .files
        .insert("layout.toml".to_string(), format!("{WORKSPACES}{crates}"));
    model.files.remove("display/allowlist.toml");
    model.files.remove("framework/allowlist.toml");
    model
}

impl Model {
    /// The workspace in `dir`.
    #[track_caller]
    pub fn ws(&mut self, dir: &str) -> &mut Ws {
        self.workspaces
            .iter_mut()
            .find(|ws| ws.dir == dir)
            .expect("the workspace exists")
    }

    /// The package `key`, as `<name>@<version>`, of the workspace in `dir`.
    #[track_caller]
    pub fn pkg(&mut self, dir: &str, key: &str) -> &mut Pkg {
        self.ws(dir)
            .packages
            .iter_mut()
            .find(|pkg| pkg.key() == key)
            .expect("the package exists")
    }

    /// Adds `pkg` to the workspace in `dir`.
    pub fn add(&mut self, dir: &str, pkg: Pkg) {
        self.ws(dir).packages.push(pkg);
    }

    /// Replaces the one occurrence of `from` in the file at `path`.
    #[track_caller]
    pub fn edit(&mut self, path: &str, from: &str, to: &str) {
        let text = self.files.get_mut(path).expect("the file exists");
        assert_eq!(text.matches(from).count(), 1, "{from}");
        *text = text.replacen(from, to, 1);
    }

    /// Appends `text` to the file at `path`.
    #[track_caller]
    pub fn append(&mut self, path: &str, text: &str) {
        self.files
            .get_mut(path)
            .expect("the file exists")
            .push_str(text);
    }

    /// The tracked files: those of the model and the lock files.
    pub fn tree(&self) -> Tree {
        let mut files = self.files.clone();
        for ws in self
            .workspaces
            .iter()
            .filter(|ws| !self.untracked_locks.contains(&ws.dir))
        {
            files
                .entry(format!("{}/Cargo.lock", ws.dir))
                .or_insert_with(|| ws.lock());
        }
        let borrowed: Vec<(&str, &str)> = files
            .iter()
            .map(|(path, text)| (path.as_str(), text.as_str()))
            .collect();
        Tree::from_files(&borrowed)
    }

    /// What the deps step finds in the model, and the facts it records.
    pub fn inspect(&self) -> (Report, Facts) {
        self.inspect_tree(&self.tree())
    }

    /// What the deps step finds in `tree`, the files of the model as a test
    /// has changed them, with the metadata of the model.
    pub fn inspect_tree(&self, tree: &Tree) -> (Report, Facts) {
        let table = layout::table(tree).expect("the layout table is valid");
        let index = Index::new(&table);
        let mut facts = Facts::new();
        let report = inspect(
            tree,
            &index,
            ROOT,
            &mut facts,
            &mut |workspace, target, _| {
                let ws = self
                    .workspaces
                    .iter()
                    .find(|ws| ws.dir == workspace.dir)
                    .ok_or_else(|| format!("no workspace {}", workspace.dir))?;
                Ok(ws.metadata(target))
            },
        );
        (report, facts)
    }

    /// The problems that the deps step finds in the model.
    pub fn problems(&self) -> Vec<String> {
        self.inspect().0.problems
    }
}

/// A scratch directory in this crate's ignored build directory, with
/// `files`, and an empty Cargo home in it.
pub fn scratch(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join(format!("deps-test-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    for (path, text) in files {
        let path = dir.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    fs::create_dir_all(dir.join("cargo-home")).unwrap();
    dir
}

/// `cmd` with the Cargo home of the scratch directory `dir`.
pub fn in_scratch(dir: &Path, mut cmd: Cmd) -> Cmd {
    cmd.env.push((
        "CARGO_HOME".to_string(),
        dir.join("cargo-home").to_string_lossy().into_owned(),
    ));
    cmd
}

/// A workspace of two path packages whose `Cargo.lock` lacks the second.
pub const STALE_LOCK: [(&str, &str); 6] = [
    (
        "ws/Cargo.toml",
        "[workspace]\nresolver = \"3\"\nmembers = [\"a\", \"b\"]\n",
    ),
    (
        "ws/a/Cargo.toml",
        "[package]\nname = \"a\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    ),
    ("ws/a/src/lib.rs", ""),
    (
        "ws/b/Cargo.toml",
        "[package]\nname = \"b\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    ),
    ("ws/b/src/lib.rs", ""),
    (
        "ws/Cargo.lock",
        "# This file is automatically @generated by Cargo.\n# It is not intended for manual editing.\nversion = 4\n\n[[package]]\nname = \"a\"\nversion = \"0.1.0\"\n",
    ),
];
