//! The allowlist of a workspace: the file that its `lock-allowlist` names
//! in the layout table. It has one `[[package]]` entry for every crates.io
//! package that Cargo resolves for the workspace's targets, members
//! excluded, and none for a package that only other platforms use, and the
//! `[[license-exception]]` entries of the workspace.

use std::collections::btree_map::Entry as Slot;
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use toml::Table;

use super::{Problems, is_false};
use crate::layout;

/// The source of every entry.
pub const CRATES_IO: &str = "crates.io";

/// An allowlist file.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Allowlist {
    #[serde(default)]
    pub package: Vec<Entry>,
    #[serde(default)]
    pub license_exception: Vec<LicenseException>,
}

/// A package whose license expression the workspace accepts although it is
/// not in the allowed set.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct LicenseException {
    pub name: String,
    pub version: String,
    /// The license expression of the package's manifest, as written; left
    /// out when it has none.
    #[serde(default)]
    pub license: Option<String>,
    /// How the workspace reads the license, as an expression in the allowed
    /// set; for the legacy form with `/`, the expression with `/` read as
    /// `OR` when that is in the set, compared after parsing.
    #[serde(default)]
    pub reading: Option<String>,
    /// Why the workspace accepts the license.
    #[serde(default)]
    pub reason: Option<String>,
}

/// An allowed package.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Entry {
    pub name: String,
    /// The version that `Cargo.lock` records, build metadata included.
    pub version: String,
    /// Always `crates.io`.
    pub source: String,
    /// The license expression of its manifest, as written.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    pub build_script: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub proc_macro: bool,
    /// The value of `links` in its manifest, which usually names the native
    /// library that its build script links.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub links: Option<String>,
    /// Its resolved features, for all targets together, sorted.
    pub features: Vec<String>,
    /// The crates of this repository in the resolution of the workspace,
    /// members or not, that reach it through crates.io packages alone,
    /// sorted.
    pub crates: Vec<String>,
    /// Why the workspace may resolve several versions of the package.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duplicate: Option<String>,
    /// Why its build script may link the native library of `links`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub links_exception: Option<String>,
    /// Why its build script may compile native code, or look for or bind to
    /// system libraries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_code_exception: Option<String>,
    /// Why its build script may download at build time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub download_exception: Option<String>,
}

impl Entry {
    /// Takes the reasons of `have`, the entry of the allowlist, which the
    /// resolution cannot give.
    fn keep_reasons(&mut self, have: &Entry) {
        self.duplicate.clone_from(&have.duplicate);
        self.links_exception.clone_from(&have.links_exception);
        self.native_code_exception
            .clone_from(&have.native_code_exception);
        self.download_exception.clone_from(&have.download_exception);
    }
}

/// Entries as an allowlist writes them.
#[derive(Serialize)]
struct Entries<'a> {
    package: Vec<&'a Entry>,
}

/// The entries as TOML text.
fn render(entries: Vec<&Entry>) -> String {
    toml::to_string(&Entries { package: entries })
        .unwrap_or_else(|error| format!("# the entries cannot be written: {error}\n"))
}

fn list(items: &[String]) -> String {
    format!("[{}]", items.join(", "))
}

fn or_none(value: Option<&str>) -> &str {
    value.unwrap_or("(none)")
}

/// Reads the allowlist at `path`, with the content `text`, or reports why
/// it cannot be read.
pub fn read(path: &str, text: &str, problems: &mut Problems) -> Option<Allowlist> {
    if let Err(error) = text.parse::<Table>() {
        problems.insert(format!("{path}: not valid TOML: {}", error.message()));
        return None;
    }
    match toml::from_str(text) {
        Ok(allowlist) => Some(allowlist),
        Err(error) => {
            problems.insert(match error.span() {
                Some(span) => format!(
                    "{path}:{}: {}",
                    layout::line_of(text, span.start),
                    error.message()
                ),
                None => format!("{path}: {}", error.message()),
            });
            None
        }
    }
}

/// Checks the packages of `allowlist`, the allowlist at `path`, against the
/// entries that the resolution gives, by name and version. `lock` is the
/// path of the workspace's `Cargo.lock`, and `targets` are its targets.
/// When they differ, a note shows the entries to add, replace and remove.
pub fn check(
    path: &str,
    lock: &str,
    allowlist: &Allowlist,
    mut expected: BTreeMap<(String, String), Entry>,
    targets: &[String],
    problems: &mut Problems,
    notes: &mut Vec<String>,
) {
    let mut entries: BTreeMap<(&str, &str), &Entry> = BTreeMap::new();
    // The packages of entries whose version is not a locked version: such an
    // entry is reported alone and stands for its package until it is fixed.
    let mut unreadable = BTreeSet::new();
    for entry in &allowlist.package {
        let shown = format!("{path}: {} {}", entry.name, entry.version);
        if !layout::is_exact(&format!("={}", entry.version)) {
            problems.insert(format!(
                "{path}: {}: the version {} is not a locked version",
                entry.name, entry.version
            ));
            unreadable.insert(entry.name.as_str());
            continue;
        }
        if entry
            .duplicate
            .as_deref()
            .is_some_and(|reason| reason.trim().is_empty())
        {
            problems.insert(format!("{shown}: the duplicate reason is empty"));
        }
        match entries.entry((&entry.name, &entry.version)) {
            Slot::Vacant(slot) => {
                slot.insert(entry);
            }
            Slot::Occupied(_) => {
                problems.insert(format!(
                    "{path}: lists {} {} twice",
                    entry.name, entry.version
                ));
            }
        }
    }
    check_versions(path, lock, &expected, &entries, problems);
    for ((name, version), want) in &mut expected {
        if let Some(have) = entries.get(&(name.as_str(), version.as_str())) {
            want.keep_reasons(have);
        }
    }
    let mut missing = Vec::new();
    let mut changed = Vec::new();
    for ((name, version), want) in &expected {
        match entries.get(&(name.as_str(), version.as_str())) {
            None if unreadable.contains(name.as_str()) => {}
            None => missing.push(want),
            Some(have) => {
                let found = compare(path, want, have);
                if !found.is_empty() {
                    problems.extend(found);
                    changed.push(want);
                }
            }
        }
    }
    let extra: Vec<&Entry> = entries
        .iter()
        .filter(|((name, version), _)| {
            !expected.contains_key(&(name.to_string(), version.to_string()))
        })
        .map(|(_, entry)| *entry)
        .collect();
    // A package with one version in Cargo.lock and one other version in the
    // allowlist has changed its version.
    let updated = |name: &str| {
        missing.iter().filter(|want| want.name == name).count() == 1
            && extra.iter().filter(|have| have.name == name).count() == 1
    };
    for want in &missing {
        match extra.iter().find(|have| have.name == want.name) {
            Some(have) if updated(&want.name) => {
                problems.insert(format!(
                    "{path}: {} is {} in {lock}, not {}",
                    want.name, want.version, have.version
                ));
            }
            _ => {
                problems.insert(format!(
                    "{lock}: {} {} is not in {path}",
                    want.name, want.version
                ));
            }
        }
    }
    for have in extra.iter().filter(|have| !updated(&have.name)) {
        problems.insert(format!(
            "{path}: {} {} is not resolved for {}",
            have.name,
            have.version,
            targets.join(" or ")
        ));
    }
    if missing.is_empty() && changed.is_empty() && extra.is_empty() {
        return;
    }
    let mut note = format!("deps: {path} matches what Cargo resolves after these changes:\n");
    let mut add: Vec<&Entry> = missing.into_iter().chain(changed).collect();
    add.sort_by(|a, b| (&a.name, &a.version).cmp(&(&b.name, &b.version)));
    if !add.is_empty() {
        note.push_str("\n# Add these entries, replacing those with the same name and version:\n\n");
        note.push_str(&render(add));
    }
    if !extra.is_empty() {
        note.push_str("\n# Remove these entries:\n\n");
        note.push_str(&render(extra));
    }
    notes.push(note);
}

/// A package that the workspace resolves in several versions has a
/// `duplicate` reason on the entry of each, and only such a package has
/// one.
fn check_versions(
    path: &str,
    lock: &str,
    expected: &BTreeMap<(String, String), Entry>,
    entries: &BTreeMap<(&str, &str), &Entry>,
    problems: &mut Problems,
) {
    let mut versions: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (name, version) in expected.keys() {
        versions.entry(name).or_default().push(version);
    }
    for (name, found) in versions {
        let reasons: Vec<(&str, Option<bool>)> = found
            .iter()
            .map(|version| {
                let entry = entries.get(&(name, *version));
                (*version, entry.map(|entry| entry.duplicate.is_some()))
            })
            .collect();
        if let [(version, Some(true))] = reasons.as_slice() {
            problems.insert(format!(
                "{path}: {name} {version} has a duplicate reason, but the workspace resolves only one version of {name}"
            ));
        }
        if found.len() < 2 {
            continue;
        }
        if reasons.iter().all(|(_, reason)| *reason != Some(true)) {
            problems.insert(format!(
                "{lock}: {name} has {} versions ({}), which {path} does not allow",
                found.len(),
                found.join(", ")
            ));
            continue;
        }
        for (version, reason) in reasons {
            if reason == Some(false) {
                problems.insert(format!(
                    "{path}: {name} {version} is one of {} versions of {name} and needs a duplicate reason",
                    found.len()
                ));
            }
        }
    }
}

/// How the entry `have` differs from `want`, the entry of the same name and
/// version that the resolution gives.
fn compare(path: &str, want: &Entry, have: &Entry) -> BTreeSet<String> {
    let shown = format!("{path}: {} {}", want.name, want.version);
    let mut found = BTreeSet::new();
    if have.source != want.source {
        found.insert(format!(
            "{shown}: the source is {}, not {}",
            want.source, have.source
        ));
    }
    if have.license != want.license {
        found.insert(format!(
            "{shown}: the license is {}, not {}",
            or_none(want.license.as_deref()),
            or_none(have.license.as_deref())
        ));
    }
    if have.build_script != want.build_script {
        let has = if want.build_script {
            "has a build script"
        } else {
            "has no build script"
        };
        found.insert(format!("{shown}: {has}"));
    }
    if have.proc_macro != want.proc_macro {
        let is = if want.proc_macro { "is" } else { "is not" };
        found.insert(format!("{shown}: {is} a procedural-macro library"));
    }
    if have.links != want.links {
        found.insert(format!(
            "{shown}: links {}, not {}",
            or_none(want.links.as_deref()),
            or_none(have.links.as_deref())
        ));
    }
    if have.features != want.features {
        found.insert(format!(
            "{shown}: the resolved features are {}, not {}",
            list(&want.features),
            list(&have.features)
        ));
    }
    if have.crates != want.crates {
        found.insert(format!(
            "{shown}: is reached from {}, not {}",
            list(&want.crates),
            list(&have.crates)
        ));
    }
    found
}
