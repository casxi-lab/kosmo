//! Build scripts: a crates.io package with a build script does not declare
//! `links`, and its build script does not compile native code, look for or
//! bind to system libraries, or download at build time, unless the package's
//! entry in the workspace's allowlist states the exception with its reason.
//! The check reads `links` and the names of the packages that the build
//! dependencies bring in; it does not read the build scripts.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use super::allowlist::{Allowlist, Entry};
use super::{Kind, Problems, Scope};

/// The build dependencies that make a build script compile native code or
/// look for or bind to system libraries, by name, with what they do.
pub const NATIVE_CODE: [(&str, &str); 9] = [
    ("autotools", "compiles C or C++"),
    ("bindgen", "generates bindings to C headers"),
    ("cc", "compiles C or C++"),
    ("cmake", "compiles C or C++"),
    ("cxx-build", "compiles C++"),
    ("nasm-rs", "assembles native code"),
    ("pkg-config", "looks for system libraries"),
    ("system-deps", "looks for system libraries"),
    ("vcpkg", "looks for system libraries"),
];

/// The build dependencies that make a build script able to download at
/// build time, by name: HTTP clients and Git.
pub const DOWNLOAD: [&str; 9] = [
    "attohttpc",
    "curl",
    "git2",
    "http_req",
    "hyper",
    "isahc",
    "minreq",
    "reqwest",
    "ureq",
];

/// What a build script may do only with an exception.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Class {
    /// It declares `links`, which usually names the native library that its
    /// build script links.
    Links,
    /// It compiles native code, or looks for or binds to system libraries.
    NativeCode,
    /// It may download at build time.
    Download,
}

impl Class {
    const ALL: [Class; 3] = [Class::Links, Class::NativeCode, Class::Download];

    /// The field of an allowlist entry that states the exception.
    fn field(self) -> &'static str {
        match self {
            Class::Links => "links-exception",
            Class::NativeCode => "native-code-exception",
            Class::Download => "download-exception",
        }
    }

    /// What a package without such a build script lacks.
    fn lacks(self) -> &'static str {
        match self {
            Class::Links => "it has no build script with `links`",
            Class::NativeCode => {
                "it has no build script with a build dependency that compiles native code or looks for or binds to system libraries"
            }
            Class::Download => "it has no build script with a build dependency that can download",
        }
    }

    /// The reason of the exception of `entry`, if it states one.
    fn reason(self, entry: &Entry) -> Option<&str> {
        match self {
            Class::Links => entry.links_exception.as_deref(),
            Class::NativeCode => entry.native_code_exception.as_deref(),
            Class::Download => entry.download_exception.as_deref(),
        }
    }
}

/// What the build script of the package `id` does that needs an exception,
/// with the message that says so.
fn findings(scope: &Scope, id: &str) -> Vec<(Class, String)> {
    let mut found = Vec::new();
    let Some(seen) = scope.resolved.packages.get(id) else {
        return found;
    };
    if let Some(links) = seen.package.links.as_deref() {
        found.push((Class::Links, format!("it declares `links = \"{links}\"`")));
    }
    // The packages that the build dependencies bring in, each with the
    // shortest chain of names that leads to it.
    let mut chains: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    let mut queue = VecDeque::new();
    for (to, links) in scope.resolved.edges_from(id) {
        if links.iter().any(|link| link.kind == Kind::Build) && !chains.contains_key(to) {
            chains.insert(to, vec![name(scope, to)]);
            queue.push_back(to);
        }
    }
    while let Some(from) = queue.pop_front() {
        let chain = chains.get(from).cloned().unwrap_or_default();
        for (to, _) in scope.resolved.edges_from(from) {
            if to != id && !chains.contains_key(to) {
                let mut longer = chain.clone();
                longer.push(name(scope, to));
                chains.insert(to, longer);
                queue.push_back(to);
            }
        }
    }
    for (to, chain) in &chains {
        let shown = chain.join(" -> ");
        let found_name = name(scope, to);
        if let Some((_, what)) = NATIVE_CODE.iter().find(|(name, _)| *name == found_name) {
            found.push((
                Class::NativeCode,
                format!("its build script {what} (build dependency {shown})"),
            ));
        }
        if DOWNLOAD.contains(&found_name) {
            found.push((
                Class::Download,
                format!("its build script may download at build time (build dependency {shown})"),
            ));
        }
    }
    found
}

/// The name of the package `id`.
fn name<'s>(scope: &Scope<'s>, id: &'s str) -> &'s str {
    scope
        .resolved
        .packages
        .get(id)
        .map_or(id, |seen| seen.package.name.as_str())
}

/// The build script of every crates.io package of the workspace does
/// nothing that needs an exception, or the package's entry in `allowlist`
/// states the exception with its reason; and every exception is needed.
pub fn check(scope: &Scope, allowlist: &Allowlist, problems: &mut Problems) {
    let path = scope.workspace.lock_allowlist;
    let mut entries: BTreeMap<(&str, &str), &Entry> = BTreeMap::new();
    for entry in &allowlist.package {
        entries
            .entry((&entry.name, &entry.version))
            .or_insert(entry);
    }
    for seen in scope.crates_io() {
        let package = &seen.package;
        let shown = format!("{} {}", package.name, package.version);
        let found = if package.has_build_script() {
            findings(scope, &package.id)
        } else {
            Vec::new()
        };
        let entry = entries.get(&(package.name.as_str(), package.version.as_str()));
        let classes: BTreeSet<Class> = found.iter().map(|(class, _)| *class).collect();
        for (class, message) in found {
            if entry.and_then(|entry| class.reason(entry)).is_none() {
                problems.insert(format!("{}: {shown}: {message}", scope.lock));
            }
        }
        let Some(entry) = entry else {
            continue;
        };
        for class in Class::ALL {
            let Some(reason) = class.reason(entry) else {
                continue;
            };
            if !classes.contains(&class) {
                problems.insert(format!(
                    "{path}: {shown} needs no {}: {}",
                    class.field(),
                    class.lacks()
                ));
            } else if reason.trim().is_empty() {
                problems.insert(format!(
                    "{path}: {shown}: the {} has no reason",
                    class.field()
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests;
