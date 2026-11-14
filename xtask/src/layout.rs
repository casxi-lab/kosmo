//! The layout step: the Cargo workspaces, crates and dependency directions
//! that the layout table `layout.toml` describes, checked on the tracked
//! files without running Cargo.
//!
//! The step reads the table, validates it, and then checks the workspace
//! roots, the members, the form of the manifests, the direction of every
//! declared internal dependency, the repository boundary, the Cargo
//! configuration, pure-core crates, build scripts and procedural macros,
//! versions and publishing, provisional and export crates, the shape of the
//! workspace roots and the workspaces that the other steps cover. It
//! collects every problem and reports them one per line, as
//! `<path>[:<line>]: <message>`, sorted by path, then by line number and
//! then by message. When the table does not parse or is not consistent,
//! only the checks that need no table run: the repository root has no
//! manifest, there is no submodule, no symbolic link leaves the repository,
//! and the Cargo configuration.

mod source;

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use proc_macro2::TokenStream;
use serde::Deserialize;
use toml::{Table, Value};

use crate::check::{self, Facts};
use crate::deps::Allowed;
use crate::repo::{self, Entry, Tree};

/// The layout table, at the repository root.
pub const TABLE: &str = "layout.toml";
/// The program crate; every crate has its version.
pub const PROGRAM: &str = "kosmo";
/// The only Cargo configuration file.
pub const CARGO_CONFIG: &str = ".cargo/config.toml";
/// The only alias that the Cargo configuration defines.
pub const ALIAS: &str = "xtask";
/// Where the list of workspaces that the format, Clippy and test steps
/// cover is defined.
const COVERAGE_SOURCE: &str = "xtask/src/check.rs";
/// The Clippy lints that the root of every product workspace denies.
pub const DENIED_LINTS: [&str; 6] = [
    "unwrap_used",
    "expect_used",
    "panic",
    "todo",
    "unimplemented",
    "allow_attributes_without_reason",
];
/// Keys that no dependency entry has; `package` renames a dependency.
const FORBIDDEN_KEYS: [&str; 7] = [
    "git",
    "branch",
    "tag",
    "rev",
    "registry",
    "registry-index",
    "package",
];
/// The keys that a member may add to a dependency it inherits.
const INHERITED_KEYS: [&str; 3] = ["workspace", "features", "optional"];
/// The dependency tables of a manifest, also found under `[target.*]`.
const DEPENDENCY_KINDS: [&str; 3] = ["dependencies", "dev-dependencies", "build-dependencies"];
/// Target tables with a `path`.
const TARGET_KINDS: [&str; 4] = ["bin", "test", "bench", "example"];
/// The keys of `[package]` that name a file or directory.
const PACKAGE_PATHS: [&str; 3] = ["workspace", "readme", "license-file"];
/// The keys of `[workspace.package]` that name a file.
const WORKSPACE_PACKAGE_PATHS: [&str; 2] = ["readme", "license-file"];
/// The workspaces that each product workspace uses: the directions between
/// the workspaces that the source layout fixes.
const USES: [(&str, &[&str]); 3] = [
    ("display", &[]),
    ("framework", &["display"]),
    ("desktop", &["framework"]),
];
/// The layers of each product workspace, from the lowest, as the source
/// layout fixes them; the direction rules within a workspace follow this
/// order.
const LAYERS: [(&str, &[&str]); 3] = [
    ("display", &["D0", "D1", "D2", "D3", "D4"]),
    ("framework", &["F0", "F1", "F2", "F3", "F4"]),
    ("desktop", &["S0", "S1", "S2"]),
];
/// The layer of the program crate.
const PROGRAM_LAYER: &str = "S2";
/// The layer marks that the source layout fixes: conformance and
/// integration tests are never published, and first-party applications
/// reach other workspaces only through facades.
const FIXED_MARKS: [(&str, &str, Mark); 3] = [
    ("display", "D4", Mark::Unpublished),
    ("framework", "F4", Mark::Unpublished),
    ("desktop", "S1", Mark::FacadeOnly),
];

/// A mark of a layer.
#[derive(Debug, Clone, Copy)]
enum Mark {
    /// `publish = false`.
    Unpublished,
    /// `facade-only = true`.
    FacadeOnly,
}

impl Mark {
    fn holds(self, layer: &Layer) -> bool {
        match self {
            Mark::Unpublished => layer.publish == Some(false),
            Mark::FacadeOnly => layer.facade_only,
        }
    }

    fn text(self) -> &'static str {
        match self {
            Mark::Unpublished => "publish = false",
            Mark::FacadeOnly => "facade-only = true",
        }
    }
}

/// The layout table.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Layout {
    workspaces: Workspaces,
    tools: BTreeMap<String, Tool>,
    #[serde(default)]
    crates: BTreeMap<String, Crate>,
}

/// The three product workspaces.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Workspaces {
    display: Workspace,
    framework: Workspace,
    desktop: Workspace,
}

impl Workspaces {
    fn all(&self) -> [(&'static str, &Workspace); 3] {
        [
            ("display", &self.display),
            ("framework", &self.framework),
            ("desktop", &self.desktop),
        ]
    }
}

/// A product workspace.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Workspace {
    path: String,
    uses: Vec<String>,
    targets: Vec<String>,
    lock_allowlist: String,
    layers: Vec<Layer>,
}

/// A layer of a product workspace.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct Layer {
    id: String,
    #[allow(
        dead_code,
        reason = "the name describes the layer to readers of the table"
    )]
    name: String,
    #[serde(default)]
    publish: Option<bool>,
    #[serde(default)]
    facade_only: bool,
}

/// A workspace of repository tasks.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct Tool {
    path: String,
    targets: Vec<String>,
    lock_allowlist: String,
}

/// A crate and its marks.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Crate {
    pub path: String,
    layer: String,
    pub export: bool,
    pure_core: bool,
    publish: bool,
    #[serde(default)]
    provisional: Option<String>,
    #[serde(default)]
    facade: bool,
    #[serde(default)]
    same_layer: Vec<String>,
    #[serde(default)]
    build_script: bool,
    #[serde(default)]
    proc_macro: bool,
    /// The allowed direct external dependencies, which the deps step
    /// compares with what Cargo resolves.
    #[serde(default)]
    pub external: Vec<Allowed>,
}

/// The problems found so far, without repeats.
type Problems = BTreeSet<String>;

/// What the layout step found.
struct Report {
    /// Every problem, in the order of [`report_order`].
    problems: Vec<String>,
    /// The product workspaces with crates and the crates, when the table
    /// parses.
    summary: Option<(String, String)>,
}

/// The layout step.
pub fn run(root: &Path, facts: &mut Facts) -> Result<(), String> {
    let tree = Tree::load(root)?;
    facts.push(("table".to_string(), TABLE.to_string()));
    let report = inspect(&tree, &check::WORKSPACES);
    if let Some((workspaces, crates)) = report.summary {
        println!("layout: workspaces {workspaces}; crates {crates}");
        facts.push(("workspaces".to_string(), workspaces));
        facts.push(("crates".to_string(), crates));
    }
    if report.problems.is_empty() {
        Ok(())
    } else {
        Err(report.problems.join("\n"))
    }
}

/// Every layout problem of `tree`, as the tests compare them.
#[cfg(test)]
pub fn check(tree: &Tree, covered: &[&str]) -> Vec<String> {
    inspect(tree, covered).problems
}

/// What the layout step finds in `tree`. `covered` are the workspace
/// directories in which the format, Clippy and test steps run.
fn inspect(tree: &Tree, covered: &[&str]) -> Report {
    let mut problems = Problems::new();
    let layout = match read_table(tree) {
        Ok(layout) => Some(layout),
        Err(problem) => {
            problems.insert(problem);
            None
        }
    };
    let index = layout.as_ref().map(Index::new);
    let summary = index.as_ref().map(|index| {
        let workspaces: Vec<&str> = (0..index.workspaces.len())
            .filter(|&workspace| index.exists(workspace))
            .map(|workspace| index.workspaces[workspace].0)
            .collect();
        let crates: Vec<&str> = index.layout.crates.keys().map(String::as_str).collect();
        (workspaces.join(", "), crates.join(", "))
    });
    let index = index.filter(|index| {
        let found = index.validate();
        let valid = found.is_empty();
        problems.extend(found);
        valid
    });
    check_repository(tree, index.as_ref(), &mut problems);
    if let Some(index) = &index {
        check_crates(tree, index, covered, &mut problems);
    }
    let mut problems: Vec<String> = problems.into_iter().collect();
    problems.sort_by(|a, b| report_order(a).cmp(&report_order(b)));
    Report { problems, summary }
}

/// The order of reported problems: by path, then by line number, a
/// problem without a line first, then by the whole text.
pub fn report_order(problem: &str) -> (&str, Option<usize>, &str) {
    let location = problem
        .split_once(": ")
        .map_or(problem, |(location, _)| location);
    match location.rsplit_once(':') {
        Some((path, line)) if !line.is_empty() && line.bytes().all(|b| b.is_ascii_digit()) => {
            (path, line.parse().ok(), problem)
        }
        _ => (location, None, problem),
    }
}

/// The line of the byte at `offset`, counted from 1.
pub fn line_of(text: &str, offset: usize) -> usize {
    text.get(..offset)
        .map_or(1, |before| before.matches('\n').count() + 1)
}

fn read_table(tree: &Tree) -> Result<Layout, String> {
    let Some(text) = tree.text(TABLE) else {
        return Err(if tree.exists(TABLE) {
            format!("{TABLE}: not a UTF-8 regular file; the layout step reads it")
        } else {
            format!("{TABLE}: missing; the layout step reads it")
        });
    };
    toml::from_str(text).map_err(|error| match error.span() {
        Some(span) => format!("{TABLE}:{}: {}", line_of(text, span.start), error.message()),
        None => format!("{TABLE}: {}", error.message()),
    })
}

/// Where a crate of the table is: its workspace and layer, by index.
#[derive(Debug, Clone, Copy)]
pub struct Place<'a> {
    pub name: &'a str,
    pub krate: &'a Crate,
    pub workspace: usize,
    layer: usize,
}

/// A workspace that the fetch and deps steps cover: a tool workspace, or a
/// product workspace with crates in the table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checked<'a> {
    /// Its directory, relative to the repository root.
    pub dir: &'a str,
    /// Its target platforms, in the order of the table.
    pub targets: &'a [String],
    /// The path of its allowlist.
    pub lock_allowlist: &'a str,
    /// The index of the product workspace; `None` for a tool workspace.
    pub product: Option<usize>,
}

/// The layout table of `tree` for the steps after the layout step: it
/// parses and is consistent, or the layout step reports why not.
pub fn table(tree: &Tree) -> Result<Layout, String> {
    let layout = read_table(tree)?;
    if Index::new(&layout).validate().is_empty() {
        Ok(layout)
    } else {
        Err(format!(
            "{TABLE}: not consistent; the layout step reports why"
        ))
    }
}

/// The table with its layers looked up.
pub struct Index<'a> {
    layout: &'a Layout,
    workspaces: [(&'static str, &'a Workspace); 3],
    /// The workspace and position of each layer; the first one when an
    /// identifier repeats.
    layers: BTreeMap<&'a str, (usize, usize)>,
}

impl<'a> Index<'a> {
    pub fn new(layout: &'a Layout) -> Index<'a> {
        let workspaces = layout.workspaces.all();
        let mut layers = BTreeMap::new();
        for (workspace, (_, entry)) in workspaces.iter().enumerate() {
            for (position, layer) in entry.layers.iter().enumerate() {
                layers
                    .entry(layer.id.as_str())
                    .or_insert((workspace, position));
            }
        }
        Index {
            layout,
            workspaces,
            layers,
        }
    }

    pub fn place(&self, name: &str) -> Option<Place<'a>> {
        let (name, krate) = self.layout.crates.get_key_value(name)?;
        let &(workspace, layer) = self.layers.get(krate.layer.as_str())?;
        Some(Place {
            name,
            krate,
            workspace,
            layer,
        })
    }

    pub fn places(&self) -> impl Iterator<Item = Place<'a>> + '_ {
        self.layout
            .crates
            .keys()
            .filter_map(|name| self.place(name))
    }

    fn layer(&self, place: Place<'a>) -> &'a Layer {
        &self.workspaces[place.workspace].1.layers[place.layer]
    }

    /// Whether the table lists a crate in a layer of `workspace`.
    fn exists(&self, workspace: usize) -> bool {
        self.places().any(|place| place.workspace == workspace)
    }

    fn workspace_at(&self, dir: &str) -> Option<usize> {
        self.workspaces
            .iter()
            .position(|(_, workspace)| workspace.path == dir)
    }

    fn is_tool(&self, dir: &str) -> bool {
        self.layout.tools.values().any(|tool| tool.path == dir)
    }

    /// Whether the dependency `name`, or the package that its entry renames,
    /// is a crate of the table.
    fn names_a_crate(&self, name: &str, entry: &Value) -> bool {
        let package = entry.get("package").and_then(Value::as_str);
        self.layout.crates.contains_key(name)
            || package.is_some_and(|package| self.layout.crates.contains_key(package))
    }

    pub fn crate_at(&self, dir: &str) -> Option<&'a str> {
        self.layout
            .crates
            .iter()
            .find(|(_, krate)| krate.path == dir)
            .map(|(name, _)| name.as_str())
    }

    /// The workspaces that the fetch and deps steps cover: the tools, in
    /// the order of their names, and then the product workspaces with
    /// crates, from `display/` to `desktop/`.
    pub fn checked(&self) -> Vec<Checked<'a>> {
        let tools = self.layout.tools.values().map(|tool| Checked {
            dir: &tool.path,
            targets: &tool.targets,
            lock_allowlist: &tool.lock_allowlist,
            product: None,
        });
        let workspaces = (0..self.workspaces.len())
            .filter(|&workspace| self.exists(workspace))
            .map(|workspace| {
                let entry = self.workspaces[workspace].1;
                Checked {
                    dir: &entry.path,
                    targets: &entry.targets,
                    lock_allowlist: &entry.lock_allowlist,
                    product: Some(workspace),
                }
            });
        tools.chain(workspaces).collect()
    }

    /// The directories of the workspaces that exist and of the tools.
    fn roots(&self) -> Vec<&'a str> {
        let workspaces = (0..self.workspaces.len())
            .filter(|&workspace| self.exists(workspace))
            .map(|workspace| self.workspaces[workspace].1.path.as_str());
        let tools = self.layout.tools.values().map(|tool| tool.path.as_str());
        workspaces.chain(tools).collect()
    }

    /// The directory of the product or tool workspace that holds `path`, if
    /// any.
    fn boundary(&self, path: &str) -> Option<&'a str> {
        let workspaces = self.workspaces.iter().map(|(_, entry)| entry.path.as_str());
        let tools = self.layout.tools.values().map(|tool| tool.path.as_str());
        workspaces
            .chain(tools)
            .find(|dir| !dir.is_empty() && repo::is_within(path, dir))
    }

    /// The problems of the table itself.
    fn validate(&self) -> Vec<String> {
        let mut problems = Vec::new();
        let mut seen = BTreeSet::new();
        // The workspaces whose layers are those of the source layout.
        let mut fixed_layers = BTreeSet::new();
        for (name, workspace) in self.workspaces {
            let mut repeated = false;
            for layer in &workspace.layers {
                if !seen.insert(layer.id.as_str()) {
                    repeated = true;
                    problems.push(format!(
                        "{TABLE}: workspaces.{name}: layer {} is defined twice",
                        layer.id
                    ));
                }
            }
            let fixed = LAYERS
                .iter()
                .find(|(workspace, _)| *workspace == name)
                .map_or(&[][..], |(_, layers)| *layers);
            if workspace
                .layers
                .iter()
                .map(|layer| layer.id.as_str())
                .eq(fixed.iter().copied())
            {
                fixed_layers.insert(name);
            } else if !repeated {
                problems.push(format!(
                    "{TABLE}: workspaces.{name}: the layers must be {}, from the lowest; the source layout fixes the layers and their order",
                    fixed.join(", ")
                ));
            }
            let mut known = true;
            for used in &workspace.uses {
                if !self.workspaces.iter().any(|(other, _)| other == used) {
                    known = false;
                    problems.push(format!(
                        "{TABLE}: workspaces.{name}: uses names the unknown workspace {used}"
                    ));
                }
            }
            let fixed: BTreeSet<&str> = USES
                .iter()
                .filter(|(workspace, _)| *workspace == name)
                .flat_map(|(_, uses)| uses.iter().copied())
                .collect();
            let listed: BTreeSet<&str> = workspace.uses.iter().map(String::as_str).collect();
            if known && listed != fixed {
                let fixed: Vec<String> = fixed.iter().map(|used| format!("\"{used}\"")).collect();
                problems.push(format!(
                    "{TABLE}: workspaces.{name}: uses must be [{}]; the source layout fixes the directions between the workspaces",
                    fixed.join(", ")
                ));
            }
            validate_targets(
                &format!("workspaces.{name}"),
                &workspace.targets,
                &mut problems,
            );
        }
        for (workspace, id, mark) in FIXED_MARKS
            .into_iter()
            .filter(|(workspace, ..)| fixed_layers.contains(workspace))
        {
            let marked = self
                .workspaces
                .iter()
                .filter(|(name, _)| *name == workspace)
                .flat_map(|(_, entry)| &entry.layers)
                .any(|layer| layer.id == id && mark.holds(layer));
            if !marked {
                problems.push(format!(
                    "{TABLE}: workspaces.{workspace}: the source layout requires layer {id} with {}",
                    mark.text()
                ));
            }
        }
        for (name, tool) in &self.layout.tools {
            validate_targets(&format!("tools.{name}"), &tool.targets, &mut problems);
        }
        self.validate_roots(&mut problems);
        let mut paths: BTreeMap<&str, &str> = BTreeMap::new();
        for (name, krate) in &self.layout.crates {
            let at = format!("{TABLE}: crates.{name}");
            if let Some(first) = paths.insert(&krate.path, name) {
                problems.push(format!(
                    "{TABLE}: crates.{first} and crates.{name} share the path {}",
                    krate.path
                ));
            }
            let normalized = is_normalized(&krate.path);
            if !normalized {
                problems.push(format!(
                    "{at}: the path {} is not a normalized path inside the repository",
                    krate.path
                ));
            }
            if krate
                .provisional
                .as_ref()
                .is_some_and(|condition| condition.trim().is_empty())
            {
                problems.push(format!("{at}: the provisional condition is empty"));
            }
            if krate.facade && !krate.export {
                problems.push(format!("{at}: a facade must be an export crate"));
            }
            let Some(place) = self.place(name) else {
                problems.push(format!("{at}: unknown layer {}", krate.layer));
                continue;
            };
            let (workspace_name, workspace) = self.workspaces[place.workspace];
            if normalized
                && (krate.path == workspace.path || !repo::is_within(&krate.path, &workspace.path))
            {
                problems.push(format!(
                    "{at}: the path {} is not inside the workspace {workspace_name}",
                    krate.path
                ));
            }
            let layer = self.layer(place);
            if name == PROGRAM && layer.id != PROGRAM_LAYER {
                problems.push(format!(
                    "{at}: the program crate {PROGRAM} belongs in layer {PROGRAM_LAYER}, the layer of the program, not {}",
                    layer.id
                ));
            }
            if layer.publish == Some(false) && krate.publish {
                problems.push(format!("{at}: layer {} is never published", layer.id));
            }
            for other in &krate.same_layer {
                if self.place(other).is_none_or(|other| {
                    other.layer != place.layer || other.workspace != place.workspace
                }) {
                    problems.push(format!(
                        "{at}: same-layer lists {other}, which is not a crate of the layer {}",
                        layer.id
                    ));
                }
            }
        }
        problems
    }

    /// The paths of the workspaces and tools: normalized, not the repository
    /// root, not inside one another, each with its `lock-allowlist` inside
    /// it.
    fn validate_roots(&self, problems: &mut Vec<String>) {
        struct TableRoot<'r> {
            key: String,
            label: String,
            path: &'r str,
            lock_allowlist: &'r str,
        }
        let workspaces = self.workspaces.iter().map(|(name, workspace)| TableRoot {
            key: format!("workspaces.{name}"),
            label: format!("the workspace {name}"),
            path: &workspace.path,
            lock_allowlist: &workspace.lock_allowlist,
        });
        let tools = self.layout.tools.iter().map(|(name, tool)| TableRoot {
            key: format!("tools.{name}"),
            label: format!("{}/", tool.path),
            path: &tool.path,
            lock_allowlist: &tool.lock_allowlist,
        });
        let roots: Vec<TableRoot> = workspaces.chain(tools).collect();
        let usable = |root: &TableRoot| !root.path.is_empty() && is_normalized(root.path);
        for (at, root) in roots.iter().enumerate() {
            if root.path.is_empty() {
                problems.push(format!(
                    "{TABLE}: {}: the path of {} is the repository root",
                    root.key, root.label
                ));
                continue;
            }
            if !is_normalized(root.path) {
                problems.push(format!(
                    "{TABLE}: {}: the path {} is not a normalized path inside the repository",
                    root.key, root.path
                ));
                continue;
            }
            if !is_normalized(root.lock_allowlist) {
                problems.push(format!(
                    "{TABLE}: {}: the lock-allowlist {} is not a normalized path inside the repository",
                    root.key, root.lock_allowlist
                ));
            } else if root.lock_allowlist == root.path
                || !repo::is_within(root.lock_allowlist, root.path)
            {
                problems.push(format!(
                    "{TABLE}: {}: the lock-allowlist {} is not inside {}/",
                    root.key, root.lock_allowlist, root.path
                ));
            }
            for other in roots.iter().skip(at + 1).filter(|other| usable(other)) {
                if root.path == other.path {
                    problems.push(format!(
                        "{TABLE}: {}: {} and {} share the path {}",
                        root.key, root.label, other.label, root.path
                    ));
                } else if repo::is_within(root.path, other.path) {
                    problems.push(format!(
                        "{TABLE}: {}: {} lies inside {}",
                        root.key, root.label, other.label
                    ));
                } else if repo::is_within(other.path, root.path) {
                    problems.push(format!(
                        "{TABLE}: {}: {} lies inside {}",
                        other.key, other.label, root.label
                    ));
                }
            }
        }
    }
}

/// Whether `path` is written as the layout step compares paths: relative to
/// the repository root, with `/` between its parts, and without empty parts,
/// `.`, `..` or a trailing `/`. The empty path is the repository root.
fn is_normalized(path: &str) -> bool {
    repo::join("", path).as_deref() == Some(path)
}

/// `relative` joined to the directory `base`, or why the layout step does
/// not accept it: it leaves the repository, as an absolute path always
/// does, or it passes through a tracked symbolic link, which the step does
/// not follow.
fn resolve(tree: &Tree, base: &str, relative: &str) -> Result<String, String> {
    let leaves = || "leaves the repository".to_string();
    if relative.starts_with('/') {
        return Err(leaves());
    }
    if let Some(link) = tree.link_on(base, relative) {
        return Err(format!(
            "passes through the symbolic link {link}, which the layout step does not follow"
        ));
    }
    repo::join(base, relative).ok_or_else(leaves)
}

/// Why `relative`, joined to the directory `base`, does not stay inside
/// `boundary`, or inside the repository, if it does not.
fn escape(tree: &Tree, base: &str, relative: &str, boundary: Option<&str>) -> Option<String> {
    match resolve(tree, base, relative) {
        Err(problem) => Some(problem),
        Ok(resolved) => boundary
            .filter(|dir| !repo::is_within(&resolved, dir))
            .map(|dir| format!("leaves {dir}/")),
    }
}

/// `targets` of the table entry `key` lists at least one target platform,
/// each once.
fn validate_targets(key: &str, targets: &[String], problems: &mut Vec<String>) {
    if targets.is_empty() {
        problems.push(format!("{TABLE}: {key}: targets lists no target platform"));
    }
    let mut seen = BTreeSet::new();
    for target in targets {
        if !seen.insert(target) {
            problems.push(format!("{TABLE}: {key}: targets lists {target} twice"));
        }
    }
}

/// Whether the last part of `path` is `Cargo.toml` in any letter case, as a
/// case-insensitive file system reads it.
fn is_manifest(path: &str) -> bool {
    file_name(path).eq_ignore_ascii_case("Cargo.toml")
}

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// The checks that need no layout table, and the symbolic links: no
/// manifest at the repository root, no submodule, the Cargo configuration,
/// and symbolic links that stay inside their workspace, or inside the
/// repository when there is no valid table.
fn check_repository(tree: &Tree, index: Option<&Index>, problems: &mut Problems) {
    for (path, entry) in tree.entries() {
        if is_manifest(path) && !path.contains('/') {
            problems.insert(format!(
                "{path}: the repository root has no Cargo.toml; each workspace has its own root"
            ));
        }
        match entry {
            Entry::Submodule => {
                problems.insert(format!("{path}: submodules are not allowed"));
            }
            Entry::Symlink(target) => {
                let boundary = index.and_then(|index| index.boundary(path));
                if let Some(problem) = escape(tree, repo::parent(path), target, boundary) {
                    problems.insert(format!("{path}: the symbolic link to {target} {problem}"));
                }
            }
            Entry::File(_) => {}
        }
    }
    check_cargo_configuration(tree, problems);
}

/// Whether `path`, in any letter case, is a `.cargo` entry or a Cargo
/// configuration file in a `.cargo` directory.
fn is_cargo_configuration(path: &str) -> bool {
    let path = path.to_ascii_lowercase();
    let dir = path
        .strip_suffix("/config.toml")
        .or_else(|| path.strip_suffix("/config"))
        .unwrap_or(&path);
    file_name(dir) == ".cargo"
}

fn check_cargo_configuration(tree: &Tree, problems: &mut Problems) {
    for (path, entry) in tree
        .entries()
        .filter(|(path, _)| is_cargo_configuration(path))
    {
        if path != CARGO_CONFIG || !matches!(entry, Entry::File(_)) {
            problems.insert(format!(
                "{path}: the only Cargo configuration is {CARGO_CONFIG}, a regular file"
            ));
            continue;
        }
        let table = match tree.text(path).map(str::parse::<Table>) {
            Some(Ok(table)) => table,
            Some(Err(error)) => {
                problems.insert(format!("{path}: not valid TOML: {}", error.message()));
                continue;
            }
            None => {
                problems.insert(format!("{path}: not UTF-8"));
                continue;
            }
        };
        for key in table.keys().filter(|key| *key != "alias") {
            problems.insert(format!("{path}: may hold only [alias], not [{key}]"));
        }
        match table.get("alias") {
            Some(Value::Table(aliases)) => {
                for alias in aliases.keys().filter(|alias| *alias != ALIAS) {
                    problems.insert(format!(
                        "{path}: may define only the alias {ALIAS}, not {alias}"
                    ));
                }
            }
            Some(_) => {
                problems.insert(format!("{path}: [alias] must be a table"));
            }
            None => {}
        }
    }
}

/// How a root manifest declares a dependency in `[workspace.dependencies]`.
#[derive(Debug, Clone, Copy)]
enum Declared<'a> {
    /// A crate of the layout table.
    Internal(&'a str),
    /// A package from crates.io.
    External,
    /// A declaration with a problem of its own.
    Unusable,
}

/// The root manifest of a product workspace that exists.
struct Root<'a> {
    manifest: String,
    /// Its `[workspace]` table.
    workspace: &'a Table,
    declared: BTreeMap<&'a str, Declared<'a>>,
}

/// A declared dependency of one crate on another.
struct Edge<'a> {
    from: &'a str,
    to: &'a str,
    manifest: String,
    kind: String,
}

/// The parsed manifests, by path; `None` for one that is not valid TOML.
type Manifests<'a> = BTreeMap<&'a str, Option<Table>>;

fn read_manifests<'a>(tree: &'a Tree, problems: &mut Problems) -> Manifests<'a> {
    let mut manifests = Manifests::new();
    for (path, _) in tree.entries() {
        if !is_manifest(path) || !path.contains('/') {
            continue;
        }
        let table = match tree.text(path).map(str::parse::<Table>) {
            Some(Ok(table)) => Some(table),
            Some(Err(error)) => {
                problems.insert(format!("{path}: not valid TOML: {}", error.message()));
                None
            }
            None => {
                problems.insert(format!("{path}: not a UTF-8 regular file"));
                None
            }
        };
        manifests.insert(path, table);
    }
    manifests
}

/// The checks that need a valid table.
fn check_crates(tree: &Tree, index: &Index, covered: &[&str], problems: &mut Problems) {
    let manifests = read_manifests(tree, problems);
    check_manifests(index, &manifests, problems);
    check_locks(tree, index, problems);
    let usable: Vec<Option<UsableRoot>> = (0..index.workspaces.len())
        .map(|workspace| workspace_root(index, workspace, &manifests, problems))
        .collect();
    let version = program_version(index, &manifests, &usable, problems);
    let roots: Vec<Option<Root>> = usable
        .into_iter()
        .enumerate()
        .map(|(workspace, root)| {
            root.map(|root| {
                check_shape(&root.manifest, root.table, root.workspace, problems);
                check_listed_members(
                    tree,
                    index,
                    workspace,
                    &root.manifest,
                    root.workspace,
                    problems,
                );
                declarations(
                    tree,
                    index,
                    workspace,
                    root.manifest,
                    root.workspace,
                    version,
                    problems,
                )
            })
        })
        .collect();
    let mut edges = Vec::new();
    let mut read = BTreeSet::new();
    for place in index.places() {
        let manifest = format!("{}/Cargo.toml", place.krate.path);
        let (Some(Some(table)), Some(Some(root))) =
            (manifests.get(manifest.as_str()), roots.get(place.workspace))
        else {
            continue;
        };
        read.insert(place.name);
        check_crate(tree, place, table, root, version, &mut edges, problems);
    }
    check_edges(index, &edges, &read, problems);
    for (name, tool) in &index.layout.tools {
        check_tool(tree, index, name, tool, &manifests, problems);
    }
    for (path, table) in &manifests {
        if let Some(table) = table {
            check_form(tree, index, path, table, problems);
        }
    }
    check_source_paths(tree, index, problems);
    check_coverage(index, covered, problems);
}

/// Every tracked manifest is the root of a workspace that exists, the
/// manifest of a crate of the table or of a tool; every crate of the table
/// has a tracked manifest with its name.
fn check_manifests(index: &Index, manifests: &Manifests, problems: &mut Problems) {
    for (path, table) in manifests {
        let dir = repo::parent(path);
        if file_name(path) == "Cargo.toml" {
            if let Some(workspace) = index.workspace_at(dir) {
                if !index.exists(workspace) {
                    problems.insert(format!(
                        "{path}: the workspace {} has no crate in the layout table, so it has no root manifest yet",
                        index.workspaces[workspace].0
                    ));
                }
                continue;
            }
            if index.is_tool(dir) || index.crate_at(dir).is_some() {
                continue;
            }
        }
        let name = table
            .as_ref()
            .and_then(|table| package_name(table))
            .unwrap_or("this manifest");
        problems.insert(format!("{path}: {name} is not a crate of the layout table"));
    }
    for (name, krate) in &index.layout.crates {
        let manifest = format!("{}/Cargo.toml", krate.path);
        match manifests.get(manifest.as_str()) {
            None => {
                problems.insert(format!(
                    "{manifest}: is not tracked; the layout table lists the crate {name}"
                ));
            }
            Some(Some(table)) => {
                let found = package_name(table);
                if found != Some(name.as_str()) {
                    problems.insert(format!(
                        "{manifest}: the package is named {}, not {name}",
                        found.unwrap_or("nothing")
                    ));
                }
            }
            Some(None) => {}
        }
    }
}

fn package_name(table: &Table) -> Option<&str> {
    table
        .get("package")
        .and_then(|package| package.get("name"))
        .and_then(Value::as_str)
}

/// Each workspace that exists and each tool has a `Cargo.lock` at its root,
/// and no other directory has one.
fn check_locks(tree: &Tree, index: &Index, problems: &mut Problems) {
    let roots = index.roots();
    for (path, _) in tree.entries() {
        if file_name(path).eq_ignore_ascii_case("Cargo.lock")
            && (file_name(path) != "Cargo.lock" || !roots.contains(&repo::parent(path)))
        {
            problems.insert(format!("{path}: only workspace roots have a Cargo.lock"));
        }
    }
    for root in roots {
        let lock = format!("{root}/Cargo.lock");
        if !tree.exists(&lock) {
            problems.insert(format!("{lock}: missing; each workspace has its own"));
        }
    }
}

/// A root manifest that declares `[workspace]`.
struct UsableRoot<'m> {
    /// Its path.
    manifest: String,
    table: &'m Table,
    /// Its `[workspace]` table.
    workspace: &'m Table,
}

/// The root manifest of `workspace`, when the workspace exists and the root
/// declares `[workspace]`. The crates of a workspace without a usable root
/// are not checked further.
fn workspace_root<'m>(
    index: &Index,
    workspace: usize,
    manifests: &'m Manifests,
    problems: &mut Problems,
) -> Option<UsableRoot<'m>> {
    if !index.exists(workspace) {
        return None;
    }
    let (name, entry) = index.workspaces[workspace];
    let manifest = format!("{}/Cargo.toml", entry.path);
    let table = match manifests.get(manifest.as_str()) {
        None => {
            problems.insert(format!(
                "{manifest}: missing; the workspace {name} has crates in the layout table"
            ));
            return None;
        }
        Some(table) => table.as_ref()?,
    };
    if table.contains_key("package") {
        problems.insert(format!(
            "{manifest}: the workspace root must not declare [package]"
        ));
    }
    let Some(workspace_table) = table.get("workspace").and_then(Value::as_table) else {
        problems.insert(format!(
            "{manifest}: the workspace root must declare [workspace]"
        ));
        return None;
    };
    Some(UsableRoot {
        manifest,
        table,
        workspace: workspace_table,
    })
}

/// The value of `key` in the `[package]` of `table`, read from the
/// `[workspace.package]` of the root when the manifest inherits it.
fn package_field<'t>(table: &'t Table, key: &str, workspace: &'t Table) -> Option<&'t Value> {
    let value = table.get("package")?.get(key)?;
    if value.get("workspace").and_then(Value::as_bool) == Some(true) {
        workspace.get("package")?.get(key)
    } else {
        Some(value)
    }
}

/// The version of the program crate, if the table lists it and its
/// manifest and root can be read. A program crate whose manifest and root
/// can be read but give no version string is a problem: Cargo would take
/// 0.0.0, and no crate could be compared with it.
fn program_version<'m>(
    index: &Index,
    manifests: &'m Manifests,
    roots: &[Option<UsableRoot<'m>>],
    problems: &mut Problems,
) -> Option<&'m str> {
    let Some(place) = index.place(PROGRAM) else {
        problems.insert(format!(
            "{TABLE}: the program crate {PROGRAM} is not in the layout table"
        ));
        return None;
    };
    let manifest = format!("{}/Cargo.toml", place.krate.path);
    let table = manifests.get(manifest.as_str())?.as_ref()?;
    let root = roots.get(place.workspace)?.as_ref()?;
    let version = package_field(table, "version", root.workspace).and_then(Value::as_str);
    if version.is_none() {
        problems.insert(format!(
            "{manifest}: the program crate {PROGRAM} has no version; every crate has the version of {PROGRAM}"
        ));
    }
    version
}

/// Whether `version` is `=` followed by an exact version: three numbers,
/// optionally with a pre-release and build metadata.
pub fn is_exact(version: &str) -> bool {
    let Some(version) = version.strip_prefix('=') else {
        return false;
    };
    let (version, build) = match version.split_once('+') {
        Some((version, build)) => (version, Some(build)),
        None => (version, None),
    };
    let (core, pre) = match version.split_once('-') {
        Some((core, pre)) => (core, Some(pre)),
        None => (version, None),
    };
    let label = |text: Option<&str>| {
        text.is_none_or(|text| {
            !text.is_empty()
                && text
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
        })
    };
    let numbers: Vec<&str> = core.split('.').collect();
    numbers.len() == 3
        && numbers
            .iter()
            .all(|number| !number.is_empty() && number.chars().all(|c| c.is_ascii_digit()))
        && label(pre)
        && label(build)
}

fn version_of(entry: &Value) -> Option<&str> {
    match entry {
        Value::String(version) => Some(version),
        _ => entry.get("version").and_then(Value::as_str),
    }
}

fn entry_path(entry: &Value) -> Option<&str> {
    entry.get("path").and_then(Value::as_str)
}

pub fn is_krepi(name: &str) -> bool {
    name == "krepi" || name.starts_with("krepi-") || name.starts_with("krepi_")
}

pub const KREPI: &str =
    "kosmo does not depend on krepi: no package is named krepi or starts with krepi- or krepi_";

/// The checks of a dependency entry that apply wherever it is written.
fn check_entry(manifest: &str, kind: &str, name: &str, entry: &Value, problems: &mut Problems) {
    let Some(entry) = entry.as_table() else {
        return;
    };
    for key in FORBIDDEN_KEYS
        .iter()
        .filter(|key| entry.contains_key(**key))
    {
        let rule = match *key {
            "package" => "may not rename a dependency".to_string(),
            key => format!("may not use {key}"),
        };
        problems.insert(format!("{manifest}: {kind} {name}: {rule}"));
    }
    if entry.contains_key("default_features") {
        problems.insert(format!(
            "{manifest}: {kind} {name}: default_features is not allowed; write default-features"
        ));
    }
}

/// The `[workspace.dependencies]` of a root manifest: external packages
/// with an exact version, and paths to crates of the table with the version
/// of the program crate.
fn declarations<'a>(
    tree: &Tree,
    index: &Index<'a>,
    workspace: usize,
    manifest: String,
    workspace_table: &'a Table,
    version: Option<&str>,
    problems: &mut Problems,
) -> Root<'a> {
    let dir = &index.workspaces[workspace].1.path;
    let kind = "[workspace.dependencies]";
    let mut declared = BTreeMap::new();
    let entries = workspace_table
        .get("dependencies")
        .and_then(Value::as_table)
        .into_iter()
        .flatten();
    for (name, entry) in entries {
        check_entry(&manifest, kind, name, entry, problems);
        if is_krepi(name) {
            problems.insert(format!("{manifest}: {kind} {name}: {KREPI}"));
        }
        let dependency = match entry_path(entry) {
            Some(path) => match resolve(tree, dir, path) {
                Err(problem) => {
                    problems.insert(format!(
                        "{manifest}: {kind} {name}: the path {path} {problem}"
                    ));
                    Declared::Unusable
                }
                Ok(target) => match index.crate_at(&target) {
                    None => {
                        problems.insert(format!(
                            "{manifest}: {kind} {name}: the path {path} is not a crate of the layout table"
                        ));
                        Declared::Unusable
                    }
                    Some(krate) => {
                        if let Some(version) = version
                            && version_of(entry) != Some(format!("={version}").as_str())
                        {
                            problems.insert(format!(
                                "{manifest}: {kind} {name}: an internal dependency must have version = \"={version}\""
                            ));
                        }
                        Declared::Internal(krate)
                    }
                },
            },
            None if index.names_a_crate(name, entry) => {
                problems.insert(format!(
                    "{manifest}: {kind} {name}: a crate of the layout table is declared with its path, not from a registry"
                ));
                Declared::Unusable
            }
            None => {
                check_exact(&manifest, kind, name, entry, problems);
                Declared::External
            }
        };
        declared.insert(name.as_str(), dependency);
    }
    Root {
        manifest,
        workspace: workspace_table,
        declared,
    }
}

fn check_exact(manifest: &str, kind: &str, name: &str, entry: &Value, problems: &mut Problems) {
    let version = version_of(entry);
    if !version.is_some_and(is_exact) {
        problems.insert(format!(
            "{manifest}: {kind} {name}: the version must be exact (=x.y.z), not {}",
            version.unwrap_or("missing")
        ));
    }
}

/// The level of a lint as a manifest writes it: a string or a table with a
/// `level`.
fn lint_level(value: &Value) -> Option<&str> {
    match value {
        Value::String(level) => Some(level),
        _ => value.get("level").and_then(Value::as_str),
    }
}

/// The shape of a product workspace root.
fn check_shape(manifest: &str, table: &Table, workspace: &Table, problems: &mut Problems) {
    if workspace.get("resolver").and_then(Value::as_str) != Some("3") {
        problems.insert(format!("{manifest}: [workspace] resolver must be \"3\""));
    }
    for key in ["exclude", "default-members"] {
        if workspace.contains_key(key) {
            problems.insert(format!(
                "{manifest}: the workspace root may not have workspace.{key}"
            ));
        }
    }
    let clippy = workspace.get("lints").and_then(|lints| lints.get("clippy"));
    let level = |lint: &str| {
        clippy
            .and_then(|clippy| clippy.get(lint))
            .and_then(lint_level)
    };
    for lint in DENIED_LINTS {
        if !matches!(level(lint), Some("deny" | "forbid")) {
            problems.insert(format!(
                "{manifest}: [workspace.lints.clippy] {lint} must be \"deny\" or \"forbid\""
            ));
        }
    }
    // The group holds the denied lints; at a higher priority, a lower level
    // would replace theirs.
    if let Some(level @ ("allow" | "warn")) = level("restriction") {
        problems.insert(format!(
            "{manifest}: [workspace.lints.clippy] restriction may not be \"{level}\"; the group holds the denied lints"
        ));
    }
    let profiles = table.get("profile").and_then(Value::as_table);
    let overflow_checks =
        |settings: &Value| settings.get("overflow-checks").and_then(Value::as_bool);
    let release = profiles.and_then(|profiles| profiles.get("release"));
    if release.and_then(overflow_checks) != Some(true) {
        problems.insert(format!(
            "{manifest}: [profile.release] must set overflow-checks = true"
        ));
    }
    for (profile, settings) in profiles.into_iter().flatten() {
        let header = format!("profile.{}", toml_key(profile));
        let mut sections = Vec::new();
        if profile != "release" {
            sections.push((header.clone(), settings));
        }
        let packages = settings.get("package").and_then(Value::as_table);
        for (package, entry) in packages.into_iter().flatten() {
            sections.push((format!("{header}.package.{}", toml_key(package)), entry));
        }
        if let Some(entry) = settings.get("build-override") {
            sections.push((format!("{header}.build-override"), entry));
        }
        for (section, entry) in sections {
            if overflow_checks(entry) == Some(false) {
                problems.insert(format!(
                    "{manifest}: [{section}] may not set overflow-checks = false"
                ));
            }
        }
    }
}

/// `key` as a TOML key in a table header: bare when it may be, quoted
/// otherwise.
fn toml_key(key: &str) -> String {
    if !key.is_empty()
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        key.to_string()
    } else {
        format!("{key:?}")
    }
}

/// `members` lists exactly the crates of the workspace, without patterns.
fn check_listed_members(
    tree: &Tree,
    index: &Index,
    workspace: usize,
    manifest: &str,
    table: &Table,
    problems: &mut Problems,
) {
    let dir = &index.workspaces[workspace].1.path;
    let expected: BTreeSet<&str> = index
        .places()
        .filter(|place| place.workspace == workspace)
        .map(|place| place.krate.path.as_str())
        .collect();
    let listed: Option<Vec<&str>> = table
        .get("members")
        .and_then(Value::as_array)
        .and_then(|members| members.iter().map(Value::as_str).collect());
    if listed
        .iter()
        .flatten()
        .any(|member| member.contains(['*', '?', '[']))
    {
        problems.insert(format!(
            "{manifest}: members may not use patterns; list each crate"
        ));
        return;
    }
    let resolved: Option<BTreeSet<String>> = listed.as_ref().and_then(|listed| {
        listed
            .iter()
            .map(|member| resolve(tree, dir, member).ok())
            .collect()
    });
    let matches = listed.as_ref().map(Vec::len) == Some(expected.len())
        && resolved.is_some_and(|resolved| {
            resolved
                .iter()
                .map(String::as_str)
                .eq(expected.iter().copied())
        });
    if !matches {
        let relative: Vec<&str> = expected
            .iter()
            .map(|path| {
                path.strip_prefix(dir.as_str())
                    .and_then(|rest| rest.strip_prefix('/'))
                    .unwrap_or(path)
            })
            .collect();
        problems.insert(format!(
            "{manifest}: members must be exactly the crates of the workspace in the layout table: {}",
            relative.join(", ")
        ));
    }
}

/// The dependency tables of a manifest with the names that messages give
/// them, `[target.'<cfg>'.<kind>]` included.
fn dependency_tables(table: &Table) -> Vec<(String, &Table)> {
    let mut tables = Vec::new();
    for kind in DEPENDENCY_KINDS {
        if let Some(entries) = table.get(kind).and_then(Value::as_table) {
            tables.push((format!("[{kind}]"), entries));
        }
    }
    let targets = table.get("target").and_then(Value::as_table);
    for (target, entry) in targets.into_iter().flatten() {
        for kind in DEPENDENCY_KINDS {
            if let Some(entries) = entry.get(kind).and_then(Value::as_table) {
                tables.push((format!("[target.'{target}'.{kind}]"), entries));
            }
        }
    }
    tables
}

/// The checks of one crate whose manifest and root are usable: its
/// dependencies, which add edges, and what its marks require.
fn check_crate<'a>(
    tree: &Tree,
    place: Place<'a>,
    table: &Table,
    root: &Root<'a>,
    version: Option<&str>,
    edges: &mut Vec<Edge<'a>>,
    problems: &mut Problems,
) {
    let (name, krate) = (place.name, place.krate);
    let manifest = format!("{}/Cargo.toml", krate.path);
    let manifest = manifest.as_str();
    if table
        .get("lints")
        .and_then(|lints| lints.get("workspace"))
        .and_then(Value::as_bool)
        != Some(true)
    {
        problems.insert(format!("{manifest}: must set [lints] workspace = true"));
    }
    let mut externals = Vec::new();
    for (kind, entries) in dependency_tables(table) {
        for (dependency, entry) in entries {
            check_entry(manifest, &kind, dependency, entry, problems);
            if entry.get("workspace").and_then(Value::as_bool) != Some(true) {
                problems.insert(format!(
                    "{manifest}: {kind} {dependency}: must be inherited with workspace = true from the workspace root"
                ));
                if is_krepi(dependency) {
                    problems.insert(format!("{manifest}: {kind} {dependency}: {KREPI}"));
                }
                if let Some(path) = entry_path(entry)
                    && let Err(problem) = resolve(tree, &krate.path, path)
                {
                    problems.insert(format!(
                        "{manifest}: {kind} {dependency}: the path {path} {problem}"
                    ));
                }
                continue;
            }
            let extra = entry
                .as_table()
                .into_iter()
                .flat_map(Table::keys)
                .filter(|key| {
                    !INHERITED_KEYS.contains(&key.as_str())
                        && !FORBIDDEN_KEYS.contains(&key.as_str())
                        && *key != "default_features"
                });
            for key in extra {
                problems.insert(format!(
                    "{manifest}: {kind} {dependency}: an inherited dependency may add only features and optional, not {key}"
                ));
            }
            match root.declared.get(dependency.as_str()) {
                None => {
                    problems.insert(format!(
                        "{manifest}: {kind} {dependency}: {} does not declare it",
                        root.manifest
                    ));
                }
                Some(Declared::Internal(target)) => edges.push(Edge {
                    from: name,
                    to: target,
                    manifest: manifest.to_string(),
                    kind: kind.clone(),
                }),
                Some(Declared::External) => externals.push((kind.clone(), dependency.as_str())),
                Some(Declared::Unusable) => {}
            }
        }
    }
    for (kind, dependency) in &externals {
        if krate.pure_core {
            problems.insert(format!(
                "{manifest}: {kind} {dependency}: the pure-core crate {name} may not have external dependencies"
            ));
        } else if krate.export {
            problems.insert(format!(
                "{manifest}: {kind} {dependency}: the export crate {name} may not have external dependencies"
            ));
        }
    }
    check_build_scripts(tree, place, manifest, table, problems);
    if krate.pure_core {
        check_pure_sources(tree, place, repo::parent(&root.manifest), table, problems);
    }
    if let Some(version) = version {
        match package_field(table, "version", root.workspace).and_then(Value::as_str) {
            Some(own) if own == version => {}
            Some(own) => {
                problems.insert(format!(
                    "{manifest}: the version {own} differs from {version} of {PROGRAM}"
                ));
            }
            None => {
                problems.insert(format!(
                    "{manifest}: has no version; it must be {version}, the version of {PROGRAM}"
                ));
            }
        }
    }
    let publishes = match package_field(table, "publish", root.workspace) {
        Some(Value::Boolean(publish)) => *publish,
        Some(Value::Array(registries)) => !registries.is_empty(),
        _ => true,
    };
    if !krate.publish && publishes {
        problems.insert(format!(
            "{manifest}: publish must be false, as the layout table marks the crate"
        ));
    }
    if krate.publish && !publishes {
        problems.insert(format!(
            "{manifest}: publish may not be false; the layout table marks the crate publishable"
        ));
    }
}

/// Build scripts and procedural-macro libraries only where the table
/// allows them; pure-core crates have no build script.
fn check_build_scripts(
    tree: &Tree,
    place: Place,
    manifest: &str,
    table: &Table,
    problems: &mut Problems,
) {
    let (name, krate) = (place.name, place.krate);
    let has_build_script = has_build_script(tree, &krate.path, table);
    if has_build_script && krate.pure_core {
        problems.insert(format!(
            "{manifest}: the pure-core crate {name} may not have a build script"
        ));
    } else if has_build_script && !krate.build_script {
        problems.insert(format!(
            "{manifest}: the crate {name} has a build script, which the layout table does not allow"
        ));
    }
    if krate.build_script && !has_build_script {
        problems.insert(format!(
            "{manifest}: the crate {name} is marked build-script but has no build script"
        ));
    }
    let proc_macro = is_proc_macro(table);
    if proc_macro && !krate.proc_macro {
        problems.insert(format!(
            "{manifest}: the crate {name} is a procedural-macro library, which the layout table does not allow"
        ));
    }
    if krate.proc_macro && !proc_macro {
        problems.insert(format!(
            "{manifest}: the crate {name} is marked proc-macro but is not a procedural-macro library"
        ));
    }
}

/// Whether the package in `dir` with the manifest `table` has a build
/// script: `package.build` names a file, or `build.rs` is tracked and
/// `build` is not `false`.
fn has_build_script(tree: &Tree, dir: &str, table: &Table) -> bool {
    match table
        .get("package")
        .and_then(|package| package.get("build"))
    {
        Some(Value::String(_)) => true,
        Some(Value::Boolean(false)) => false,
        _ => tree.exists(&format!("{dir}/build.rs")),
    }
}

/// Whether the manifest `table` makes its library a procedural-macro
/// library, with `proc-macro = true` or with `proc-macro` in `crate-type`.
fn is_proc_macro(table: &Table) -> bool {
    let lib = table.get("lib");
    let flag = lib
        .and_then(|lib| lib.get("proc-macro"))
        .and_then(Value::as_bool);
    let crate_types = lib
        .and_then(|lib| lib.get("crate-type"))
        .and_then(Value::as_array);
    flag == Some(true)
        || crate_types
            .into_iter()
            .flatten()
            .any(|kind| kind.as_str() == Some("proc-macro"))
}

/// Whether `path` is a `.rs` file under `dir`.
fn is_rust_under(path: &str, dir: &str) -> bool {
    path != dir && repo::is_within(path, dir) && path.ends_with(".rs")
}

/// The sources of a pure-core crate: everything under `src/` is a regular
/// file; its library root and every file that `include!` or `#[path]`
/// brings in are `.rs` files under `src/`; the crate attributes of the
/// library root; and no interior mutability or `extern crate std` outside
/// tests in any file under `src/`. A path that leaves the workspace or
/// passes through a symbolic link is reported by the repository boundary
/// instead.
fn check_pure_sources(
    tree: &Tree,
    place: Place,
    workspace: &str,
    table: &Table,
    problems: &mut Problems,
) {
    let (name, krate) = (place.name, place.krate);
    let src = format!("{}/src", krate.path);
    for (path, entry) in tree.entries() {
        if matches!(entry, Entry::Symlink(_)) && repo::is_within(path, &src) {
            problems.insert(format!(
                "{path}: the pure-core crate {name} keeps its sources under {src}/ as regular files, not symbolic links"
            ));
        }
    }
    let root = table
        .get("lib")
        .and_then(|lib| lib.get("path"))
        .and_then(Value::as_str)
        .unwrap_or("src/lib.rs");
    if let Ok(lib) = resolve(tree, &krate.path, root)
        && repo::is_within(&lib, workspace)
    {
        if !is_rust_under(&lib, &src) {
            problems.insert(format!(
                "{}/Cargo.toml: the library root {lib} of the pure-core crate {name} is not a .rs file under {src}/",
                krate.path
            ));
        }
        match (tree.entry(&lib), tree.text(&lib)) {
            // Reported as a symbolic link under `src/`, or as a root
            // outside it.
            (Some(Entry::Symlink(_)), _) => {}
            (None, _) => {
                problems.insert(format!(
                    "{lib}: missing; the pure-core crate {name} is a library with this root"
                ));
            }
            (Some(_), None) => {
                problems.insert(format!(
                    "{lib}: not a UTF-8 regular file; it is the library root of the pure-core crate {name}"
                ));
            }
            (_, Some(text)) => match source::crate_attributes(text) {
                Err(error) => {
                    problems.insert(format!("{lib}: does not parse: {error}"));
                }
                Ok(attributes) => {
                    if !attributes.no_std {
                        problems.insert(format!(
                            "{lib}: the pure-core crate {name} must have an unconditional #![no_std]"
                        ));
                    }
                    if !attributes.forbids_unsafe_code {
                        problems.insert(format!(
                            "{lib}: the pure-core crate {name} must have #![forbid(unsafe_code)]"
                        ));
                    }
                }
            },
        }
    }
    for (path, text) in tree.texts().filter(|(path, _)| is_rust_under(path, &src)) {
        let (stream, paths) = match read_rust(path, text) {
            Ok(read) => read,
            Err(problem) => {
                problems.insert(problem);
                continue;
            }
        };
        let mut found = BTreeMap::new();
        source::interior_mutability(stream.clone(), &mut found);
        for (line, names) in found {
            problems.insert(format!(
                "{path}:{line}: the pure-core crate {name} may not use {}",
                names.join(", ")
            ));
        }
        let mut lines = Vec::new();
        source::extern_std(stream, &mut lines);
        for line in lines {
            problems.insert(format!(
                "{path}:{line}: the pure-core crate {name} may not use extern crate std outside #[cfg(test)]; the attribute stands directly on the extern crate item"
            ));
        }
        let dir = repo::parent(path);
        for found in &paths {
            // The modules declared inside an inline module with a
            // directory are read from that directory.
            let allowed = |resolved: &str| match found.kind {
                source::Kind::Code => is_rust_under(resolved, &src),
                source::Kind::Directory => repo::is_within(resolved, &src),
                source::Kind::Data => true,
            };
            let outside = found.candidates.iter().any(|candidate| {
                resolve(tree, dir, candidate).is_ok_and(|resolved| {
                    repo::is_within(&resolved, workspace) && !allowed(&resolved)
                })
            });
            if outside {
                problems.insert(format!(
                    "{path}:{}: the pure-core crate {name} brings in Rust code only from .rs files under {src}/, not {}",
                    found.line, found.path
                ));
            }
        }
    }
}

/// The problem of a Rust file whose tokens cannot be read.
fn untokenized(path: &str, (line, error): (usize, String)) -> String {
    if line == 0 {
        format!("{path}: cannot be read as Rust tokens: {error}")
    } else {
        format!("{path}:{line}: cannot be read as Rust tokens: {error}")
    }
}

/// The rules of the layout table for the edge `from` -> `to`: the first one
/// it breaks.
pub fn direction(index: &Index, from: Place, to: Place) -> Option<String> {
    let (from_workspace, from_entry) = index.workspaces[from.workspace];
    let (to_workspace, _) = index.workspaces[to.workspace];
    let from_layer = index.layer(from);
    let to_layer = index.layer(to);
    if from.workspace != to.workspace {
        if !from_entry.uses.iter().any(|used| used == to_workspace) {
            return Some(format!(
                "crates of {from_workspace} may not depend on crates of {to_workspace}"
            ));
        }
        if !to.krate.export {
            return Some(format!(
                "{} is not an export crate of the workspace {to_workspace}",
                to.name
            ));
        }
        if from_layer.facade_only && !to.krate.facade {
            return Some(format!(
                "a crate of layer {} depends on other workspaces only through facade crates; {} is not one",
                from_layer.id, to.name
            ));
        }
    } else if to.layer > from.layer {
        return Some(format!(
            "a crate of layer {} may not depend on {} of the higher layer {}",
            from_layer.id, to.name, to_layer.id
        ));
    } else if to.layer == from.layer && !from.krate.same_layer.iter().any(|name| name == to.name) {
        return Some(format!(
            "{} is in the same layer {} and is not listed in same-layer of {}",
            to.name, to_layer.id, from.name
        ));
    }
    if from.krate.pure_core && !to.krate.pure_core {
        return Some(format!(
            "the pure-core crate {} may depend only on pure-core crates; {} is not one",
            from.name, to.name
        ));
    }
    if from.krate.pure_core && to.krate.provisional.is_some() {
        return Some(format!(
            "the pure-core crate {} may not depend on the provisional crate {}",
            from.name, to.name
        ));
    }
    None
}

/// The direction of every declared edge, the `same-layer` lists of the
/// crates that were read, and cycles.
fn check_edges(index: &Index, edges: &[Edge], read: &BTreeSet<&str>, problems: &mut Problems) {
    let mut graph: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for edge in edges {
        graph.entry(edge.from).or_default().insert(edge.to);
        let (Some(from), Some(to)) = (index.place(edge.from), index.place(edge.to)) else {
            continue;
        };
        if let Some(rule) = direction(index, from, to) {
            problems.insert(format!(
                "{}: {} {}: {rule}",
                edge.manifest, edge.kind, edge.to
            ));
        }
    }
    for name in read {
        let Some(place) = index.place(name) else {
            continue;
        };
        for listed in &place.krate.same_layer {
            if !graph
                .get(name)
                .is_some_and(|targets| targets.contains(listed.as_str()))
            {
                problems.insert(format!(
                    "{TABLE}: crates.{name}: lists {listed} in same-layer but does not depend on it"
                ));
            }
        }
    }
    for cycle in cycles(&graph) {
        let Some(place) = cycle.first().and_then(|name| index.place(name)) else {
            continue;
        };
        problems.insert(format!(
            "{}/Cargo.toml: the internal dependency graph has a cycle: {}",
            place.krate.path,
            cycle.join(" -> ")
        ));
    }
}

/// The cycles that a depth-first search of `graph` meets, each starting and
/// ending at its smallest crate.
pub fn cycles<'a>(graph: &BTreeMap<&'a str, BTreeSet<&'a str>>) -> BTreeSet<Vec<&'a str>> {
    fn visit<'a>(
        node: &'a str,
        graph: &BTreeMap<&'a str, BTreeSet<&'a str>>,
        path: &mut Vec<&'a str>,
        done: &mut BTreeSet<&'a str>,
        found: &mut BTreeSet<Vec<&'a str>>,
    ) {
        if let Some(start) = path.iter().position(|seen| *seen == node) {
            let mut cycle = path[start..].to_vec();
            let smallest = (0..cycle.len())
                .min_by_key(|&at| cycle[at])
                .unwrap_or_default();
            cycle.rotate_left(smallest);
            if let Some(&first) = cycle.first() {
                cycle.push(first);
            }
            found.insert(cycle);
            return;
        }
        if done.contains(node) {
            return;
        }
        path.push(node);
        for next in graph.get(node).into_iter().flatten() {
            visit(next, graph, path, done, found);
        }
        path.pop();
        done.insert(node);
    }
    let mut found = BTreeSet::new();
    let mut done = BTreeSet::new();
    for node in graph.keys() {
        visit(node, graph, &mut Vec::new(), &mut done, &mut found);
    }
    found
}

/// A tool workspace: no build script, no procedural-macro library, and
/// external dependencies with exact versions, without a path dependency or
/// a crate of the table, in its dependency tables and its
/// `[workspace.dependencies]`. An inherited dependency is checked where
/// `[workspace.dependencies]` declares it.
fn check_tool(
    tree: &Tree,
    index: &Index,
    name: &str,
    tool: &Tool,
    manifests: &Manifests,
    problems: &mut Problems,
) {
    let manifest = format!("{}/Cargo.toml", tool.path);
    let table = match manifests.get(manifest.as_str()) {
        None => {
            problems.insert(format!(
                "{manifest}: missing; the layout table lists the tool workspace {name}"
            ));
            return;
        }
        Some(None) => return,
        Some(Some(table)) => table,
    };
    if has_build_script(tree, &tool.path, table) {
        problems.insert(format!(
            "{manifest}: the tool workspace {name} may not have a build script"
        ));
    }
    if is_proc_macro(table) {
        problems.insert(format!(
            "{manifest}: the tool workspace {name} may not be a procedural-macro library"
        ));
    }
    let declared = table
        .get("workspace")
        .and_then(|workspace| workspace.get("dependencies"))
        .and_then(Value::as_table)
        .map(|entries| ("[workspace.dependencies]".to_string(), entries));
    for (kind, entries) in dependency_tables(table).into_iter().chain(declared) {
        for (dependency, entry) in entries {
            check_entry(&manifest, &kind, dependency, entry, problems);
            if entry.get("workspace").and_then(Value::as_bool) == Some(true) {
                continue;
            }
            if is_krepi(dependency) {
                problems.insert(format!("{manifest}: {kind} {dependency}: {KREPI}"));
            }
            match entry_path(entry) {
                Some(path) => {
                    let rule = match resolve(tree, &tool.path, path) {
                        Err(problem) => format!("the path {path} {problem}"),
                        Ok(_) => format!("{name} may not have path dependencies"),
                    };
                    problems.insert(format!("{manifest}: {kind} {dependency}: {rule}"));
                }
                None if index.names_a_crate(dependency, entry) => {
                    problems.insert(format!(
                        "{manifest}: {kind} {dependency}: {name} may not depend on crates of the layout table"
                    ));
                }
                None => check_exact(&manifest, &kind, dependency, entry, problems),
            }
        }
    }
}

/// The checks of every manifest: no `[patch]` or `[replace]`, no
/// underscore spellings of Cargo keys, no krepi package, and target paths
/// that stay inside the workspace.
fn check_form(tree: &Tree, index: &Index, path: &str, table: &Table, problems: &mut Problems) {
    for key in ["patch", "replace"] {
        if table.contains_key(key) {
            problems.insert(format!("{path}: [{key}] is not allowed"));
        }
    }
    let targets = table.get("target").and_then(Value::as_table);
    let sections = std::iter::once(("".to_string(), table)).chain(
        targets.into_iter().flatten().filter_map(|(target, entry)| {
            entry
                .as_table()
                .map(|entry| (format!("target.'{target}'."), entry))
        }),
    );
    for (prefix, section) in sections {
        for key in ["dev_dependencies", "build_dependencies"] {
            if section.contains_key(key) {
                problems.insert(format!(
                    "{path}: [{prefix}{key}] is not allowed; write {}",
                    key.replace('_', "-")
                ));
            }
        }
    }
    for key in ["proc_macro", "crate_type"] {
        if table.get("lib").is_some_and(|lib| lib.get(key).is_some()) {
            problems.insert(format!(
                "{path}: [lib] {key} is not allowed; write {}",
                key.replace('_', "-")
            ));
        }
    }
    if let Some(name) = package_name(table)
        && is_krepi(name)
    {
        problems.insert(format!("{path}: the package {name}: {KREPI}"));
    }
    let dir = repo::parent(path);
    let mut target_paths: Vec<&str> = Vec::new();
    if let Some(Value::String(build)) = table
        .get("package")
        .and_then(|package| package.get("build"))
    {
        target_paths.push(build);
    }
    if let Some(lib) = table
        .get("lib")
        .and_then(|lib| lib.get("path"))
        .and_then(Value::as_str)
    {
        target_paths.push(lib);
    }
    for kind in TARGET_KINDS {
        let entries = table.get(kind).and_then(Value::as_array);
        target_paths.extend(
            entries
                .into_iter()
                .flatten()
                .filter_map(|entry| entry.get("path").and_then(Value::as_str)),
        );
    }
    let package = table.get("package");
    let workspace_package = table
        .get("workspace")
        .and_then(|workspace| workspace.get("package"));
    let named = PACKAGE_PATHS
        .iter()
        .map(|key| {
            (
                format!("package.{key}"),
                package.and_then(|table| table.get(key)),
            )
        })
        .chain(WORKSPACE_PACKAGE_PATHS.iter().map(|key| {
            (
                format!("workspace.package.{key}"),
                workspace_package.and_then(|table| table.get(key)),
            )
        }));
    for (key, value) in named {
        if let Some(Value::String(value)) = value
            && let Err(problem) = resolve(tree, dir, value)
        {
            problems.insert(format!("{path}: the path {value} of {key} {problem}"));
        }
    }
    let boundary = index.boundary(path);
    for target in target_paths {
        if let Some(problem) = escape(tree, dir, target, boundary) {
            problems.insert(format!("{path}: the target path {target} {problem}"));
        }
    }
}

/// The literal paths of every tracked Rust file stay inside its workspace,
/// or inside the repository for a file outside the workspaces, for every
/// candidate path that each may name.
fn check_source_paths(tree: &Tree, index: &Index, problems: &mut Problems) {
    for (path, text) in tree.texts().filter(|(path, _)| path.ends_with(".rs")) {
        let (_, found) = match read_rust(path, text) {
            Ok(read) => read,
            Err(problem) => {
                problems.insert(problem);
                continue;
            }
        };
        let dir = repo::parent(path);
        let boundary = index.boundary(path);
        for found in found {
            let problem = found
                .candidates
                .iter()
                .find_map(|candidate| escape(tree, dir, candidate, boundary));
            if let Some(problem) = problem {
                problems.insert(format!(
                    "{path}:{}: the source path {} {problem}",
                    found.line, found.path
                ));
            }
        }
    }
}

/// The tokens of the Rust file `path` and its literal paths, or the problem
/// of a file whose tokens cannot be read.
fn read_rust(path: &str, text: &str) -> Result<(TokenStream, Vec<source::SourcePath>), String> {
    let stream = source::tokens(text).map_err(|error| untokenized(path, error))?;
    // Only a `mod.rs` file is never a module file named after its module.
    let name = file_name(path);
    let stem = (name != "mod.rs").then(|| name.strip_suffix(".rs").unwrap_or(name));
    let mut found = Vec::new();
    source::source_paths(stream.clone(), stem, &mut found);
    Ok((stream, found))
}

/// The format, Clippy and test steps run in exactly the tools and the
/// product workspaces that exist.
fn check_coverage(index: &Index, covered: &[&str], problems: &mut Problems) {
    let expected: BTreeSet<&str> = index.roots().into_iter().collect();
    let covered: BTreeSet<&str> = covered.iter().copied().collect();
    for missing in expected.difference(&covered) {
        problems.insert(format!(
            "{COVERAGE_SOURCE}: the fmt, clippy and test steps do not run in the workspace {missing}"
        ));
    }
    for extra in covered.difference(&expected) {
        problems.insert(format!(
            "{COVERAGE_SOURCE}: the fmt, clippy and test steps run in {extra}, which is not a workspace of the layout table with crates"
        ));
    }
}

#[cfg(test)]
mod tests;
