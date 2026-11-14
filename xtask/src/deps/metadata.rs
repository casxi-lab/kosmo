//! The output of `cargo metadata --format-version 1`, as far as the deps
//! step reads it, and the union of the resolutions of a workspace for its
//! targets.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use serde_json::Value;

use super::Kind;

/// The output of `cargo metadata` for one target.
#[derive(Debug, Deserialize)]
pub struct Metadata {
    /// The packages that the resolution for the target reaches.
    pub packages: Vec<Package>,
    /// The ids of the workspace members.
    pub workspace_members: Vec<String>,
    pub resolve: Option<Resolve>,
}

/// A package.
#[derive(Debug, Clone, Deserialize)]
pub struct Package {
    /// The package id, which the step never takes apart.
    pub id: String,
    pub name: String,
    pub version: String,
    /// `None` for a path package.
    pub source: Option<String>,
    pub license: Option<String>,
    pub links: Option<String>,
    /// The absolute path of its manifest.
    pub manifest_path: String,
    pub targets: Vec<Target>,
    /// The dependencies that its manifest declares, for every platform.
    pub dependencies: Vec<Dependency>,
}

impl Package {
    /// Whether the package has a build script.
    pub fn has_build_script(&self) -> bool {
        self.has_target("custom-build")
    }

    /// Whether the package is a procedural-macro library.
    pub fn is_proc_macro(&self) -> bool {
        self.has_target("proc-macro")
    }

    fn has_target(&self, kind: &str) -> bool {
        self.targets
            .iter()
            .any(|target| target.kind.iter().any(|found| found == kind))
    }
}

/// A target of a package: its library, binaries, tests or build script.
#[derive(Debug, Clone, Deserialize)]
pub struct Target {
    pub kind: Vec<String>,
}

/// A dependency as a manifest declares it.
#[derive(Debug, Clone, Deserialize)]
pub struct Dependency {
    /// The name of the package, not the name it is renamed to.
    pub name: String,
    /// The version requirement, as Cargo writes it: `"0.13"` becomes
    /// `^0.13`.
    pub req: String,
    /// `None` for a normal dependency.
    pub kind: Option<Kind>,
    /// The condition of a `[target.<condition>.*]` table.
    pub target: Option<String>,
    pub optional: bool,
    pub uses_default_features: bool,
    pub features: Vec<String>,
}

/// The resolve graph.
#[derive(Debug, Deserialize)]
pub struct Resolve {
    pub nodes: Vec<Node>,
}

/// A package in the resolve graph, with its resolved features.
#[derive(Debug, Deserialize)]
pub struct Node {
    pub id: String,
    pub deps: Vec<NodeDep>,
    pub features: Vec<String>,
}

/// An edge of the resolve graph.
#[derive(Debug, Deserialize)]
pub struct NodeDep {
    /// The id of the package depended on.
    pub pkg: String,
    /// The kinds and target conditions of the declarations behind the edge.
    pub dep_kinds: Vec<DepKind>,
}

/// One declaration behind an edge.
#[derive(Debug, Deserialize)]
pub struct DepKind {
    pub kind: Option<Kind>,
    pub target: Option<String>,
}

/// Reads the output of `cargo metadata --format-version 1`, or says what is
/// wrong with it, as the end of a sentence about the command.
pub fn parse(text: &str) -> Result<Metadata, String> {
    let value: Value =
        serde_json::from_str(text).map_err(|error| format!("printed no valid JSON: {error}"))?;
    match value.get("version").and_then(Value::as_u64) {
        Some(1) => {}
        Some(version) => {
            return Err(format!(
                "printed the metadata format version {version}, which is not supported"
            ));
        }
        None => return Err("printed no metadata format version".to_string()),
    }
    let metadata: Metadata = serde_json::from_value(value)
        .map_err(|error| format!("printed metadata that the deps step cannot read: {error}"))?;
    if metadata.resolve.is_none() {
        return Err("reports no resolve".to_string());
    }
    Ok(metadata)
}

/// A package that the resolution for some target reaches.
#[derive(Debug)]
pub struct Seen {
    pub package: Package,
    /// Its resolved features, for all the targets whose resolution reaches
    /// it together.
    pub features: BTreeSet<String>,
}

/// One declaration behind an edge, for one target.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Link {
    pub kind: Kind,
    /// The condition of a `[target.<condition>.*]` table.
    pub cfg: Option<String>,
    /// The target, by index.
    pub target: usize,
}

/// The union of the resolutions of a workspace for its targets, by package
/// id.
#[derive(Debug, Default)]
pub struct Resolved {
    pub members: BTreeSet<String>,
    pub packages: BTreeMap<String, Seen>,
    /// The edges, from the id of the dependent to the id of the package it
    /// depends on.
    pub edges: BTreeMap<String, BTreeMap<String, BTreeSet<Link>>>,
}

impl Resolved {
    /// Adds the resolution for the target with index `target`.
    pub fn add(&mut self, target: usize, metadata: Metadata) {
        self.members.extend(metadata.workspace_members);
        for package in metadata.packages {
            self.packages
                .entry(package.id.clone())
                .or_insert_with(|| Seen {
                    package,
                    features: BTreeSet::new(),
                });
        }
        let nodes = metadata.resolve.map(|resolve| resolve.nodes);
        for node in nodes.into_iter().flatten() {
            if let Some(seen) = self.packages.get_mut(&node.id) {
                seen.features.extend(node.features);
            }
            let edges = self.edges.entry(node.id).or_default();
            for dep in node.deps {
                let links = edges.entry(dep.pkg).or_default();
                for declared in dep.dep_kinds {
                    links.insert(Link {
                        kind: declared.kind.unwrap_or_default(),
                        cfg: declared.target,
                        target,
                    });
                }
            }
        }
    }

    /// The edges from the package `id`.
    pub fn edges_from(&self, id: &str) -> impl Iterator<Item = (&str, &BTreeSet<Link>)> {
        self.edges
            .get(id)
            .into_iter()
            .flatten()
            .map(|(to, links)| (to.as_str(), links))
    }
}
