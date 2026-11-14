//! The direct dependencies of the members on crates.io packages: their
//! requirements are exact, and those of each crate of the layout table are
//! exactly the `external` entries of the crate.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use super::{Allowed, Kind, Origin, Problems, Scope, inside, only_for};
use crate::layout::{self, TABLE};

/// The key of a declaration: the package, the kind and the target
/// condition.
type Key = (String, Kind, Option<String>);

/// A direct dependency of a member on a crates.io package, as the manifest
/// declares it, for the targets whose resolution has it.
struct Direct {
    req: String,
    optional: bool,
    default_features: bool,
    /// The declared features, sorted.
    features: Vec<String>,
    /// The resolved features of the package.
    resolved: BTreeSet<String>,
    targets: BTreeSet<usize>,
}

impl Direct {
    fn allowed(&self, (name, kind, target): &Key) -> Allowed {
        Allowed {
            name: name.clone(),
            version: self.req.clone(),
            kind: *kind,
            target: target.clone(),
            optional: self.optional,
            default_features: self.default_features,
            features: self.features.clone(),
            resolved_features: self.resolved.iter().cloned().collect(),
        }
    }
}

fn key(allowed: &Allowed) -> Key {
    (allowed.name.clone(), allowed.kind, allowed.target.clone())
}

/// `name`, and the table that declares it unless it is `[dependencies]`.
fn label((name, kind, target): &Key) -> String {
    if kind.is_normal() && target.is_none() {
        name.clone()
    } else {
        format!("{name} in {}", kind.table(target.as_deref()))
    }
}

fn list(items: &[String]) -> String {
    format!("[{}]", items.join(", "))
}

fn is_sorted_set(items: &[String]) -> bool {
    items.windows(2).all(|pair| pair[0] < pair[1])
}

/// The allow entries of crates, as the layout table writes them.
#[derive(Serialize)]
struct Crates<'a> {
    crates: BTreeMap<&'a str, Externals>,
}

#[derive(Serialize)]
struct Externals {
    external: Vec<Allowed>,
}

/// Checks the direct dependencies of every member of `scope`.
pub fn check(scope: &Scope, problems: &mut Problems, notes: &mut Vec<String>) {
    for member in &scope.resolved.members {
        let Some(seen) = scope.resolved.packages.get(member) else {
            continue;
        };
        let package = &seen.package;
        let manifest = inside(scope.root, &package.manifest_path)
            .map_or_else(|| package.manifest_path.clone(), str::to_string);
        let mut directs: BTreeMap<Key, Direct> = BTreeMap::new();
        for (to, links) in scope.resolved.edges_from(member) {
            let (Origin::CratesIo, Some(dependency)) =
                (scope.origin(to), scope.resolved.packages.get(to))
            else {
                continue;
            };
            for link in links {
                // The declaration in the table of the link. The layout step,
                // which runs first, rejects renamed dependencies, so a table
                // declares a package once; two versions of a package declared
                // under other names in one table would both take the first
                // declaration here.
                let declared = package.dependencies.iter().find(|declared| {
                    declared.name == dependency.package.name
                        && declared.kind.unwrap_or_default() == link.kind
                        && declared.target == link.cfg
                });
                let Some(declared) = declared else {
                    continue;
                };
                let direct = directs
                    .entry((declared.name.clone(), link.kind, link.cfg.clone()))
                    .or_insert_with(|| {
                        let mut features = declared.features.clone();
                        features.sort();
                        features.dedup();
                        Direct {
                            req: declared.req.clone(),
                            optional: declared.optional,
                            default_features: declared.uses_default_features,
                            features,
                            resolved: BTreeSet::new(),
                            targets: BTreeSet::new(),
                        }
                    });
                direct.targets.insert(link.target);
                direct.resolved.extend(dependency.features.iter().cloned());
            }
        }
        let mut inexact = BTreeSet::new();
        for (key, direct) in &directs {
            if !layout::is_exact(&direct.req) {
                problems.insert(format!(
                    "{manifest}: {} {}: the requirement {} is not exact (=x.y.z)",
                    key.1.table(key.2.as_deref()),
                    key.0,
                    direct.req
                ));
                inexact.insert(key.clone());
            }
        }
        directs.retain(|key, _| !inexact.contains(key));
        if let Origin::Member(Some(name)) = scope.origin(member)
            && let Some(place) = scope.index.place(name)
        {
            let found = compare(
                scope,
                name,
                &manifest,
                &place.krate.external,
                &directs,
                &inexact,
            );
            if !found.is_empty() {
                problems.extend(found);
                notes.push(note(name, &directs));
            }
        }
    }
}

/// What the step prints for the crate `name` whose allow entries differ from
/// its direct dependencies `directs`: the entries that replace all its
/// `external` entries, or, when there are none, that they all go.
fn note(name: &str, directs: &BTreeMap<Key, Direct>) -> String {
    if directs.is_empty() {
        return format!(
            "deps: the allow entries of {name} in {TABLE} match what Cargo resolves once every `[[crates.{name}.external]]` table is removed"
        );
    }
    let external = directs
        .iter()
        .map(|(key, direct)| direct.allowed(key))
        .collect();
    let crates = Crates {
        crates: BTreeMap::from([(name, Externals { external })]),
    };
    let entries = toml::to_string(&crates)
        .unwrap_or_else(|error| format!("# the entries cannot be written: {error}\n"));
    format!(
        "deps: the allow entries of {name} in {TABLE} that match what Cargo resolves, to replace all its `[[crates.{name}.external]]` tables:\n\n{entries}"
    )
}

/// How the allow entries `allowed` of the crate `name` differ from its
/// direct dependencies `directs`, leaving out the entries of `inexact`
/// declarations.
fn compare(
    scope: &Scope,
    name: &str,
    manifest: &str,
    allowed: &[Allowed],
    directs: &BTreeMap<Key, Direct>,
    inexact: &BTreeSet<Key>,
) -> BTreeSet<String> {
    let at = format!("{TABLE}: crates.{name}");
    let targets = scope.workspace.targets;
    let mut found = BTreeSet::new();
    let mut entries: BTreeMap<Key, &Allowed> = BTreeMap::new();
    for entry in allowed {
        let key = key(entry);
        let shown = label(&key);
        if !is_sorted_set(&entry.features) {
            found.insert(format!(
                "{at}: {shown}: the features of the allow entry are not sorted or repeat"
            ));
        }
        if !is_sorted_set(&entry.resolved_features) {
            found.insert(format!(
                "{at}: {shown}: the resolved features of the allow entry are not sorted or repeat"
            ));
        }
        if entries.insert(key, entry).is_some() {
            found.insert(format!("{at}: lists {shown} twice"));
        }
    }
    entries.retain(|key, _| !inexact.contains(key));
    let mut unmatched: Vec<(&Key, &Direct)> = Vec::new();
    for (key, direct) in directs {
        match entries.remove(key) {
            Some(entry) => found.extend(differences(&at, &label(key), entry, direct)),
            None => unmatched.push((key, direct)),
        }
    }
    for &(key, direct) in &unmatched {
        // One dependency without an entry and one entry without a
        // dependency, both of this name, differ in kind or target.
        let others: Vec<Key> = entries
            .keys()
            .filter(|other| other.0 == key.0)
            .cloned()
            .collect();
        let alone = unmatched
            .iter()
            .filter(|(other, _)| other.0 == key.0)
            .count()
            == 1;
        let paired = match others.as_slice() {
            [other] if alone => entries.remove(other),
            _ => None,
        };
        let Some(entry) = paired else {
            found.insert(format!(
                "{manifest}: {} {} {}{} has no allow entry in {TABLE}",
                key.1.table(key.2.as_deref()),
                key.0,
                direct.req,
                only_for(&direct.targets, targets)
            ));
            continue;
        };
        let shown = &key.0;
        if entry.kind != key.1 {
            found.insert(format!(
                "{at}: {shown}: the allow entry is for {}, the manifest declares {}",
                entry.kind.noun(),
                key.1.noun()
            ));
        }
        if entry.target != key.2 {
            found.insert(format!(
                "{at}: {shown}: the allow entry has the target {}, the manifest {}",
                entry.target.as_deref().unwrap_or("(none)"),
                key.2.as_deref().unwrap_or("(none)")
            ));
        }
        found.extend(differences(&at, shown, entry, direct));
    }
    for (key, entry) in entries {
        found.insert(format!(
            "{at}: allows {} {}{}, which {name} does not depend on for {}",
            key.0,
            entry.version,
            if key.1.is_normal() && key.2.is_none() {
                String::new()
            } else {
                format!(" in {}", key.1.table(key.2.as_deref()))
            },
            targets.join(" or ")
        ));
    }
    found
}

/// How the allow entry `entry` differs from the dependency `direct` that it
/// is matched with, `shown` naming the dependency.
fn differences(at: &str, shown: &str, entry: &Allowed, direct: &Direct) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    if entry.version != direct.req {
        found.insert(format!(
            "{at}: {shown}: the allow entry has {}, the manifest {}",
            entry.version, direct.req
        ));
    }
    if entry.optional != direct.optional {
        let (is, entry_is) = if direct.optional {
            ("is", "is not")
        } else {
            ("is not", "is")
        };
        found.insert(format!(
            "{at}: {shown}: the dependency {is} optional, the allow entry {entry_is}"
        ));
    }
    if entry.default_features != direct.default_features {
        let (on, turns) = if direct.default_features {
            ("on", "off")
        } else {
            ("off", "on")
        };
        found.insert(format!(
            "{at}: {shown}: the default features are {on}, the allow entry turns them {turns}"
        ));
    }
    let mut features = entry.features.clone();
    features.sort();
    features.dedup();
    if features != direct.features {
        found.insert(format!(
            "{at}: {shown}: the features {} differ from the allow entry {}",
            list(&direct.features),
            list(&entry.features)
        ));
    }
    let resolved: Vec<String> = direct.resolved.iter().cloned().collect();
    let mut listed = entry.resolved_features.clone();
    listed.sort();
    listed.dedup();
    if listed != resolved {
        found.insert(format!(
            "{at}: {shown}: the resolved features {} differ from the allow entry {}",
            list(&resolved),
            list(&entry.resolved_features)
        ));
    }
    found
}
