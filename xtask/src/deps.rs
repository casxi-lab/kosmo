//! The deps step: what Cargo resolves for every workspace of the check,
//! read offline, against the layout table and the allowlists.
//!
//! For each workspace that the fetch step covers and each of its targets,
//! the step runs
//! `cargo metadata --locked --offline --all-features --filter-platform <target>`
//! with `CARGO_NET_OFFLINE=true` and takes the union of the resolutions by
//! package id. Normal, build and dev dependencies all count, optional
//! dependencies of members count as if their features were on, and a package
//! that only other platforms use is not part of the union. On the union it
//! checks:
//!
//! - sources: every package is a member, a crate of the layout table reached
//!   through a `path` dependency from a product workspace, or a package from
//!   crates.io; no package is named `krepi` or starts with `krepi-` or
//!   `krepi_`; every crates.io package has a checksum in the workspace's
//!   `Cargo.lock`;
//! - exact requirements: every dependency of a member on a crates.io package
//!   has the requirement `=x.y.z`;
//! - directions: every resolved edge between crates of the layout table
//!   follows the directions of the table, and the resolved graph has no
//!   cycle;
//! - allow entries: the direct crates.io dependencies of each crate of the
//!   table are exactly the `external` entries of the crate in the table;
//! - allowlists: the crates.io packages are exactly the entries of the
//!   workspace's allowlist, the file that `lock-allowlist` names, and a
//!   package with several versions has a `duplicate` reason on each;
//! - licenses: every member has the license of a kosmo crate, and the
//!   license expression of every crates.io package is in the allowed set or
//!   has a license exception in the allowlist;
//! - build scripts: a crates.io package with a build script declares no
//!   `links`, and the build script has no build dependency that compiles
//!   native code, looks for or binds to system libraries or can download,
//!   unless its allowlist entry states the exception.
//!
//! It also reports, without failing, the versions of a package that differ
//! between workspaces and four numbers of each workspace: its members, its
//! export crates, its crates.io packages and its internal edges. It reports
//! every problem it finds, one per line as `<path>[:<line>]: <message>`,
//! sorted by path and then by line number, and prints the entries that would
//! bring the allow entries and the allowlists up to date.

mod allowlist;
mod build_scripts;
mod direct;
#[cfg(test)]
pub mod fixture;
mod license;
mod metadata;
#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::Path;
use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::check::{self, Cmd, Facts};
use crate::layout::{self, Checked, Index};
use crate::repo::{self, Tree};

use metadata::Resolved;

/// The source that Cargo reports for every package from crates.io, with the
/// sparse protocol too.
pub const CRATES_IO: &str = "registry+https://github.com/rust-lang/crates.io-index";

/// The kind of a dependency.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    #[default]
    Normal,
    Build,
    Dev,
}

impl Kind {
    pub fn is_normal(&self) -> bool {
        *self == Kind::Normal
    }

    /// The manifest table that declares a dependency of this kind, under
    /// the target condition `cfg` if there is one.
    pub fn table(self, cfg: Option<&str>) -> String {
        let kind = match self {
            Kind::Normal => "dependencies",
            Kind::Build => "build-dependencies",
            Kind::Dev => "dev-dependencies",
        };
        match cfg {
            Some(cfg) => format!("[target.'{cfg}'.{kind}]"),
            None => format!("[{kind}]"),
        }
    }

    fn noun(self) -> &'static str {
        match self {
            Kind::Normal => "a normal dependency",
            Kind::Build => "a build dependency",
            Kind::Dev => "a dev dependency",
        }
    }
}

fn is_false(value: &bool) -> bool {
    !value
}

/// An allowed direct external dependency of a crate: an entry of its
/// `external` list in the layout table.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Allowed {
    /// The name of the package.
    pub name: String,
    /// The requirement as Cargo reports it, such as `=1.2.0`.
    pub version: String,
    #[serde(default, skip_serializing_if = "Kind::is_normal")]
    pub kind: Kind,
    /// The condition of a `[target.<condition>.*]` table.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub optional: bool,
    pub default_features: bool,
    /// The features that the declaration turns on, sorted.
    pub features: Vec<String>,
    /// The features of the package after resolution, for all targets of the
    /// workspace together, sorted.
    pub resolved_features: Vec<String>,
}

/// The problems found so far, without repeats.
type Problems = BTreeSet<String>;

/// What the deps step found.
#[derive(Debug, Default)]
pub struct Report {
    /// Every problem, sorted by path, then by message.
    pub problems: Vec<String>,
    /// What the step prints: its observations and the entries that would
    /// bring the allow entries and the allowlists up to date.
    pub notes: Vec<String>,
}

/// The source of the output of `cargo metadata` for a workspace and a
/// target: the command, or a fixture in tests. It may add facts, such as how
/// long the command ran.
pub type MetadataSource<'s> = dyn FnMut(&Checked, &str, &mut Facts) -> Result<String, String> + 's;

/// The absolute path of the repository at `root`, without symbolic links:
/// Cargo runs in that directory and reports the manifest paths of the
/// members in this form.
fn real_root(root: &Path) -> Result<String, String> {
    let canonical = std::fs::canonicalize(root)
        .map_err(|error| format!("cannot resolve the path {}: {error}", root.display()))?;
    canonical
        .to_str()
        .map(str::to_string)
        .ok_or_else(|| format!("the path {} is not UTF-8", canonical.display()))
}

/// The deps step, which prints its notes on the standard output.
pub fn run(root: &Path, facts: &mut Facts) -> Result<(), String> {
    run_to(root, facts, &mut std::io::stdout().lock())
}

/// The deps step, which prints its notes to `out`.
fn run_to(root: &Path, facts: &mut Facts, out: &mut impl Write) -> Result<(), String> {
    let tree = Tree::load(root)?;
    let table = layout::table(&tree)?;
    let index = Index::new(&table);
    let base = real_root(root)?;
    let mut source = |workspace: &Checked, target: &str, facts: &mut Facts| {
        let cmd = metadata_command(workspace.dir, target);
        let started = Instant::now();
        let output = check::capture(root, &cmd);
        facts.push((
            cmd.timing.clone(),
            started.elapsed().as_millis().to_string(),
        ));
        String::from_utf8(output?)
            .map_err(|_| format!("`{}` printed text that is not UTF-8", cmd.shown()))
    };
    let report = inspect(&tree, &index, &base, facts, &mut source);
    report.finish(out)
}

impl Report {
    /// How the step ends: it prints every note to `out`, one per line, and
    /// then passes or fails as [`Report::verdict`] says.
    pub fn finish(&self, out: &mut impl Write) -> Result<(), String> {
        for note in &self.notes {
            writeln!(out, "{note}").map_err(|error| format!("cannot print a note: {error}"))?;
        }
        self.verdict()
    }

    /// Whether the step passes: it does without problems and fails with
    /// every problem, one per line.
    pub fn verdict(&self) -> Result<(), String> {
        if self.problems.is_empty() {
            Ok(())
        } else {
            Err(self.problems.join("\n"))
        }
    }
}

/// The offline `cargo metadata` command for the workspace in `dir` and the
/// target `target`, with its time recorded as `<workspace>_<target>_ms`.
pub fn metadata_command(dir: &str, target: &str) -> Cmd {
    Cmd {
        program: check::cargo(),
        args: [
            "metadata",
            "--locked",
            "--offline",
            "--all-features",
            "--filter-platform",
            target,
            "--format-version",
            "1",
            "--manifest-path",
            &format!("{dir}/Cargo.toml"),
        ]
        .map(str::to_string)
        .to_vec(),
        env: check::offline(),
        timing: format!("{dir}_{target}_ms"),
    }
}

/// What the deps step finds in `tree`, for the repository at the absolute
/// path `root`, with the output of `cargo metadata` from `source`. The
/// observations become facts.
pub fn inspect(
    tree: &Tree,
    index: &Index,
    root: &str,
    facts: &mut Facts,
    source: &mut MetadataSource,
) -> Report {
    let mut problems = Problems::new();
    let mut notes = Vec::new();
    let mut observed = Vec::new();
    for workspace in index.checked() {
        let manifest = format!("{}/Cargo.toml", workspace.dir);
        let mut resolved = Resolved::default();
        let mut complete = true;
        for (at, target) in workspace.targets.iter().enumerate() {
            let read = source(&workspace, target, facts).and_then(|text| {
                metadata::parse(&text)
                    .map_err(|problem| format!("cargo metadata for {target} {problem}"))
            });
            match read {
                Ok(metadata) => resolved.add(at, metadata),
                Err(problem) => {
                    complete = false;
                    problems.insert(format!("{manifest}: {problem}"));
                }
            }
        }
        // Without every target, the union would be wrong in both
        // directions.
        if !complete {
            continue;
        }
        let scope = Scope::new(tree, index, root, &workspace, &resolved, &mut problems);
        scope.check(&mut problems, &mut notes);
        observed.push(scope.observe());
    }
    for observation in &observed {
        let dir = &observation.dir;
        notes.push(format!(
            "deps: {dir}: {} members, {} export crates, {} crates.io packages, {} internal edges",
            observation.members, observation.exports, observation.crates_io, observation.edges
        ));
        facts.push((format!("{dir}_members"), observation.members.to_string()));
        facts.push((format!("{dir}_exports"), observation.exports.to_string()));
        facts.push((
            format!("{dir}_crates_io"),
            observation.crates_io.to_string(),
        ));
        facts.push((
            format!("{dir}_internal_edges"),
            observation.edges.to_string(),
        ));
    }
    let drift = drift(&observed);
    if !drift.is_empty() {
        notes.push(format!(
            "deps: versions that differ between the workspaces: {drift}"
        ));
    }
    facts.push((
        "drift".to_string(),
        if drift.is_empty() {
            "none".to_string()
        } else {
            drift
        },
    ));
    let mut problems: Vec<String> = problems.into_iter().collect();
    problems.sort_by(|a, b| layout::report_order(a).cmp(&layout::report_order(b)));
    Report { problems, notes }
}

/// The packages of crates.io that resolve in more than one workspace with
/// different versions, as `<package>: <workspace> <versions>, ...` joined
/// with `; `, or an empty string.
fn drift(observed: &[Observation]) -> String {
    let mut by_name: BTreeMap<&str, Vec<(&str, &BTreeSet<String>)>> = BTreeMap::new();
    for observation in observed {
        for (name, versions) in &observation.versions {
            by_name
                .entry(name)
                .or_default()
                .push((&observation.dir, versions));
        }
    }
    let differing: Vec<String> = by_name
        .into_iter()
        .filter(|(_, found)| found.len() > 1 && found.iter().any(|(_, v)| *v != found[0].1))
        .map(|(name, found)| {
            let found: Vec<String> = found
                .iter()
                .map(|(dir, versions)| {
                    let versions: Vec<&str> = versions.iter().map(String::as_str).collect();
                    format!("{dir} {}", versions.join(" and "))
                })
                .collect();
            format!("{name}: {}", found.join(", "))
        })
        .collect();
    differing.join("; ")
}

/// What the step observes in a workspace, without judging it.
struct Observation {
    dir: String,
    members: usize,
    exports: usize,
    crates_io: usize,
    edges: usize,
    /// The versions of each crates.io package.
    versions: BTreeMap<String, BTreeSet<String>>,
}

/// Where a package of a workspace's resolution comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Origin<'a> {
    /// A member of the workspace, with its name in the layout table when it
    /// is a crate of the table.
    Member(Option<&'a str>),
    /// A crate of the layout table that is not a member, reached through a
    /// `path` dependency from a product workspace; the directions decide
    /// whether it may be.
    Crate(&'a str),
    /// A package from crates.io.
    CratesIo,
    /// Any other package, which the source check reports.
    Rejected,
}

impl<'a> Origin<'a> {
    /// The crate of the layout table, if the package is one.
    fn table_crate(self) -> Option<&'a str> {
        match self {
            Origin::Member(name) => name,
            Origin::Crate(name) => Some(name),
            Origin::CratesIo | Origin::Rejected => None,
        }
    }

    /// Whether the package is a crate of this repository.
    fn is_internal(self) -> bool {
        matches!(self, Origin::Member(_) | Origin::Crate(_))
    }
}

/// `path`, an absolute path, relative to the repository root `root` when it
/// lies inside it.
fn inside<'p>(root: &str, path: &'p str) -> Option<&'p str> {
    path.strip_prefix(root)?.strip_prefix('/')
}

/// The absolute path `path` relative to the absolute path `root`, with `..`
/// for each directory of `root` that it leaves.
fn relative(root: &str, path: &str) -> String {
    let root: Vec<&str> = root.split('/').filter(|part| !part.is_empty()).collect();
    let path: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
    let common = root.iter().zip(&path).take_while(|(a, b)| a == b).count();
    let mut parts = vec![".."; root.len() - common];
    parts.extend(&path[common..]);
    parts.join("/")
}

/// The targets of `targets`, by index, among `all`, in parentheses, unless
/// they are all of them.
fn only_for(targets: &BTreeSet<usize>, all: &[String]) -> String {
    if targets.len() == all.len() {
        return String::new();
    }
    let names: Vec<&str> = targets
        .iter()
        .filter_map(|&at| all.get(at))
        .map(String::as_str)
        .collect();
    format!(" ({})", names.join(", "))
}

/// The resolution of one workspace, with the origin of each package.
struct Scope<'s> {
    tree: &'s Tree,
    index: &'s Index<'s>,
    root: &'s str,
    workspace: &'s Checked<'s>,
    resolved: &'s Resolved,
    origins: BTreeMap<&'s str, Origin<'s>>,
    /// The path of the workspace's `Cargo.lock`.
    lock: String,
}

impl<'s> Scope<'s> {
    /// The scope of `resolved`, with the problems of the sources.
    fn new(
        tree: &'s Tree,
        index: &'s Index<'s>,
        root: &'s str,
        workspace: &'s Checked<'s>,
        resolved: &'s Resolved,
        problems: &mut Problems,
    ) -> Scope<'s> {
        let lock = format!("{}/Cargo.lock", workspace.dir);
        let mut origins = BTreeMap::new();
        for (id, seen) in &resolved.packages {
            let package = &seen.package;
            let shown = format!("{lock}: {} {}", package.name, package.version);
            let origin = if resolved.members.contains(id) {
                let dir = inside(root, &package.manifest_path).map(repo::parent);
                Origin::Member(dir.and_then(|dir| index.crate_at(dir)))
            } else {
                match package.source.as_deref() {
                    Some(CRATES_IO) => Origin::CratesIo,
                    Some(source) => {
                        problems.insert(format!(
                            "{shown} comes from {source}; packages come only from crates.io and from crates of the layout table"
                        ));
                        Origin::Rejected
                    }
                    None => match inside(root, &package.manifest_path) {
                        None => {
                            problems.insert(format!(
                                "{shown} is a path package outside the repository ({})",
                                relative(root, &package.manifest_path)
                            ));
                            Origin::Rejected
                        }
                        Some(path) => match index.crate_at(repo::parent(path)) {
                            Some(_) if workspace.product.is_none() => {
                                problems.insert(format!(
                                    "{shown}: the tool workspace {} may not depend on crates of the layout table",
                                    workspace.dir
                                ));
                                Origin::Rejected
                            }
                            Some(name) => Origin::Crate(name),
                            None => {
                                problems.insert(format!(
                                    "{shown} is a path package that is not a crate of the layout table ({path})"
                                ));
                                Origin::Rejected
                            }
                        },
                    },
                }
            };
            origins.insert(id.as_str(), origin);
        }
        Scope {
            tree,
            index,
            root,
            workspace,
            resolved,
            origins,
            lock,
        }
    }

    fn origin(&self, id: &str) -> Origin<'s> {
        self.origins.get(id).copied().unwrap_or(Origin::Rejected)
    }

    /// The packages of crates.io, by id.
    fn crates_io(&self) -> impl Iterator<Item = &'s metadata::Seen> + '_ {
        self.resolved
            .packages
            .iter()
            .filter(|(id, _)| self.origin(id) == Origin::CratesIo)
            .map(|(_, seen)| seen)
    }

    /// Every check of the workspace but the sources.
    fn check(&self, problems: &mut Problems, notes: &mut Vec<String>) {
        for seen in self.resolved.packages.values() {
            let package = &seen.package;
            let checked = matches!(
                self.origin(&package.id),
                Origin::CratesIo | Origin::Crate(_)
            );
            if checked && layout::is_krepi(&package.name) {
                problems.insert(format!(
                    "{}: {} {}: {}",
                    self.lock,
                    package.name,
                    package.version,
                    layout::KREPI
                ));
            }
        }
        self.check_checksums(problems);
        self.check_directions(problems);
        direct::check(self, problems, notes);
        license::check_members(self, problems);
        let path = self.workspace.lock_allowlist;
        let Some(allowlist) = self.allowlist(problems) else {
            return;
        };
        allowlist::check(
            path,
            &self.lock,
            &allowlist,
            self.expected_entries(),
            self.workspace.targets,
            problems,
            notes,
        );
        license::check_packages(self, &allowlist, problems);
        build_scripts::check(self, &allowlist, problems);
    }

    /// The allowlist of the workspace, or why it cannot be read.
    fn allowlist(&self, problems: &mut Problems) -> Option<allowlist::Allowlist> {
        let path = self.workspace.lock_allowlist;
        if self.tree.entry(path).is_none() {
            problems.insert(format!(
                "{path}: missing; every workspace of the check has an allowlist, without entries when it has no crates.io package"
            ));
            return None;
        }
        let Some(text) = self.tree.text(path) else {
            problems.insert(format!("{path}: not a UTF-8 regular file"));
            return None;
        };
        allowlist::read(path, text, problems)
    }

    /// Every crates.io package has a checksum in `Cargo.lock`.
    fn check_checksums(&self, problems: &mut Problems) {
        #[derive(Deserialize)]
        struct Lock {
            #[serde(default)]
            package: Vec<Locked>,
        }
        #[derive(Deserialize)]
        struct Locked {
            name: String,
            version: String,
            #[serde(default)]
            source: Option<String>,
            #[serde(default)]
            checksum: Option<String>,
        }
        let Some(text) = self.tree.text(&self.lock) else {
            problems.insert(format!(
                "{}: missing; the deps step reads the checksums from it",
                self.lock
            ));
            return;
        };
        let lock: Lock = match toml::from_str(text) {
            Ok(lock) => lock,
            Err(error) => {
                problems.insert(format!(
                    "{}: not valid TOML: {}",
                    self.lock,
                    error.message()
                ));
                return;
            }
        };
        for seen in self.crates_io() {
            let package = &seen.package;
            let checksum = lock.package.iter().any(|locked| {
                locked.name == package.name
                    && locked.version == package.version
                    && locked.source.as_deref() == Some(CRATES_IO)
                    && locked.checksum.is_some()
            });
            if !checksum {
                problems.insert(format!(
                    "{}: {} {} has no checksum",
                    self.lock, package.name, package.version
                ));
            }
        }
    }

    /// Every resolved edge between crates of the layout table follows the
    /// directions of the table, and the resolved graph has no cycle.
    fn check_directions(&self, problems: &mut Problems) {
        let mut edges: BTreeMap<(&str, &str), BTreeSet<usize>> = BTreeMap::new();
        for (from, to_all) in &self.resolved.edges {
            let Some(from) = self.origin(from).table_crate() else {
                continue;
            };
            for (to, links) in to_all {
                let Some(to) = self.origin(to).table_crate() else {
                    continue;
                };
                edges
                    .entry((from, to))
                    .or_default()
                    .extend(links.iter().map(|link| link.target));
            }
        }
        let mut graph: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
        for (&(from, to), targets) in &edges {
            graph.entry(from).or_default().insert(to);
            let (Some(from), Some(to)) = (self.index.place(from), self.index.place(to)) else {
                continue;
            };
            if let Some(rule) = layout::direction(self.index, from, to) {
                problems.insert(format!(
                    "{}/Cargo.toml: {}{}: {rule}",
                    from.krate.path,
                    to.name,
                    only_for(targets, self.workspace.targets)
                ));
            }
        }
        for cycle in layout::cycles(&graph) {
            let Some(place) = cycle.first().and_then(|name| self.index.place(name)) else {
                continue;
            };
            problems.insert(format!(
                "{}/Cargo.toml: the resolved internal dependency graph has a cycle: {}",
                place.krate.path,
                cycle.join(" -> ")
            ));
        }
    }

    /// The allowlist entry of each crates.io package, by name and version,
    /// as the resolution gives it.
    fn expected_entries(&self) -> BTreeMap<(String, String), allowlist::Entry> {
        // The crates of this repository in the resolution, members or not,
        // from which each package is reached through crates.io packages
        // alone: those whose external dependencies bring it in.
        let mut reached: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
        for (from, seen) in &self.resolved.packages {
            if !self.origin(from).is_internal() {
                continue;
            }
            let mut stack = vec![from.as_str()];
            let mut visited = BTreeSet::new();
            while let Some(id) = stack.pop() {
                for (to, _) in self.resolved.edges_from(id) {
                    if self.origin(to) == Origin::CratesIo && visited.insert(to) {
                        stack.push(to);
                    }
                }
            }
            for id in visited {
                reached
                    .entry(id)
                    .or_default()
                    .insert(seen.package.name.as_str());
            }
        }
        self.crates_io()
            .map(|seen| {
                let package = &seen.package;
                let crates = reached
                    .get(package.id.as_str())
                    .into_iter()
                    .flatten()
                    .map(|name| name.to_string())
                    .collect();
                let entry = allowlist::Entry {
                    name: package.name.clone(),
                    version: package.version.clone(),
                    source: allowlist::CRATES_IO.to_string(),
                    license: package.license.clone(),
                    build_script: package.has_build_script(),
                    proc_macro: package.is_proc_macro(),
                    links: package.links.clone(),
                    features: seen.features.iter().cloned().collect(),
                    crates,
                    duplicate: None,
                    links_exception: None,
                    native_code_exception: None,
                    download_exception: None,
                };
                ((package.name.clone(), package.version.clone()), entry)
            })
            .collect()
    }

    /// The observations of the workspace.
    fn observe(&self) -> Observation {
        let exports = match self.workspace.product {
            Some(workspace) => self
                .index
                .places()
                .filter(|place| place.workspace == workspace && place.krate.export)
                .count(),
            None => 0,
        };
        let mut edges = BTreeSet::new();
        for (from, to_all) in &self.resolved.edges {
            if !self.origin(from).is_internal() {
                continue;
            }
            for (to, links) in to_all {
                if self.origin(to).is_internal() {
                    edges.extend(links.iter().map(|link| (from, to, link.kind)));
                }
            }
        }
        let mut versions: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for seen in self.crates_io() {
            versions
                .entry(seen.package.name.clone())
                .or_default()
                .insert(seen.package.version.clone());
        }
        Observation {
            dir: self.workspace.dir.to_string(),
            members: self.resolved.members.len(),
            exports,
            crates_io: self.crates_io().count(),
            edges: edges.len(),
            versions,
        }
    }
}
