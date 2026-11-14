//! Licenses: the license of each member of a workspace, and the license
//! expression of each crates.io package, which the allowed set and the
//! license exceptions of the workspace's allowlist decide.

use std::collections::BTreeMap;
use std::collections::btree_map::Entry as Slot;

use spdx::{Expression, LicenseItem, LicenseReq, ParseMode};

use super::allowlist::{Allowlist, LicenseException};
use super::{Problems, Scope, inside};
use crate::layout;
use crate::repo::{self, Tree};

/// The licenses that the license expression of a crates.io package may use
/// without a license exception.
pub const ALLOWED: [&str; 7] = [
    "MIT",
    "Apache-2.0",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "ISC",
    "Zlib",
    "Unicode-3.0",
];

/// The license of a kosmo crate.
pub const KOSMO: &str = "MIT";

/// The license of a kosmo crate with font assets.
pub const WITH_FONTS: &str = "MIT AND OFL-1.1";

/// The extensions of font files, in any letter case.
pub const FONT_EXTENSIONS: [&str; 5] = ["ttf", "otf", "ttc", "woff", "woff2"];

/// How a license expression evaluates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// It is in the allowed set.
    Allowed,
    /// It is a valid SPDX expression outside the allowed set.
    NotAllowed,
    /// It is not a valid SPDX expression, for the reason that the parser
    /// gives.
    Invalid(String),
}

/// Evaluates the license expression `expression`. It is parsed in the
/// strict mode of the SPDX parser: identifiers are those of its license
/// list, in their letter case, deprecated identifiers are rejected, and the
/// operators are `AND`, `OR` and `WITH`, in upper or lower case. It is in
/// the allowed set when `OR` has an operand in the set and `AND` has all
/// its operands in it. A term is in the set when it is one of the allowed
/// identifiers, without `+` and without `WITH`.
pub fn evaluate(expression: &str) -> Verdict {
    match Expression::parse_mode(expression, ParseMode::STRICT) {
        Ok(parsed) if parsed.evaluate(is_allowed) => Verdict::Allowed,
        Ok(_) => Verdict::NotAllowed,
        Err(error) => Verdict::Invalid(error.reason.to_string()),
    }
}

fn is_allowed(term: &LicenseReq) -> bool {
    term.addition.is_none()
        && matches!(
            term.license,
            LicenseItem::Spdx { id, or_later: false } if ALLOWED.contains(&id.name)
        )
}

/// The expression `original` with `/` read as `OR`, when it is in the
/// legacy form of crates.io: not valid as written, valid with that reading,
/// and then in the allowed set. The reading of such an expression is this
/// one, compared after parsing; `None` for any other expression.
fn legacy_reading(original: &str) -> Option<Expression> {
    if Expression::parse_mode(original, ParseMode::STRICT).is_ok() {
        return None;
    }
    let slash = ParseMode {
        allow_slash_as_or_operator: true,
        ..ParseMode::STRICT
    };
    Expression::parse_mode(original, slash)
        .ok()
        .filter(|legacy| legacy.evaluate(is_allowed))
}

/// The tracked font files of the crate in the directory `dir`, leaving out
/// those of packages in directories below it, which `cargo package` leaves
/// out too.
pub fn font_assets<'t>(tree: &'t Tree, dir: &str) -> Vec<&'t str> {
    let nested: Vec<&str> = tree
        .entries()
        .map(|(path, _)| path)
        .filter(|path| layout::is_manifest(path))
        .map(repo::parent)
        .filter(|package| *package != dir && repo::is_within(package, dir))
        .collect();
    tree.entries()
        .map(|(path, _)| path)
        .filter(|path| *path != dir && repo::is_within(path, dir))
        .filter(|path| !nested.iter().any(|package| repo::is_within(path, package)))
        .filter(|path| {
            layout::file_name(path)
                .rsplit_once('.')
                .is_some_and(|(_, extension)| {
                    FONT_EXTENSIONS
                        .iter()
                        .any(|font| extension.eq_ignore_ascii_case(font))
                })
        })
        .collect()
}

/// Every member of the workspace has the license of a kosmo crate: `MIT`,
/// or `MIT AND OFL-1.1` when it has font assets, compared as written.
pub fn check_members(scope: &Scope, problems: &mut Problems) {
    for member in &scope.resolved.members {
        let Some(seen) = scope.resolved.packages.get(member) else {
            continue;
        };
        let package = &seen.package;
        let Some(manifest) = inside(scope.root, &package.manifest_path) else {
            continue;
        };
        let fonts = font_assets(scope.tree, repo::parent(manifest));
        let license = package.license.as_deref();
        let problem = match (fonts.first(), license) {
            (Some(_), Some(WITH_FONTS)) | (None, Some(KOSMO)) => continue,
            (Some(font), Some(license)) => format!(
                "the license of a crate with font assets, such as {font}, is {WITH_FONTS}, not {license}"
            ),
            (Some(font), None) => format!(
                "has no license; the license of a crate with font assets, such as {font}, is {WITH_FONTS}"
            ),
            (None, Some(WITH_FONTS)) => {
                format!("has no font asset, so its license is {KOSMO}, not {WITH_FONTS}")
            }
            (None, Some(license)) => {
                format!("the license of a kosmo crate is {KOSMO}, not {license}")
            }
            (None, None) => format!("has no license; the license of a kosmo crate is {KOSMO}"),
        };
        problems.insert(format!("{manifest}: {problem}"));
    }
}

/// The license expression of every crates.io package of the workspace is in
/// the allowed set, or the allowlist at `path` has a license exception for
/// it; and every license exception is needed and stands.
pub fn check_packages(scope: &Scope, allowlist: &Allowlist, problems: &mut Problems) {
    let path = scope.workspace.lock_allowlist;
    let lock = &scope.lock;
    let mut exceptions: BTreeMap<(&str, &str), &LicenseException> = BTreeMap::new();
    for exception in &allowlist.license_exception {
        match exceptions.entry((&exception.name, &exception.version)) {
            Slot::Vacant(slot) => {
                slot.insert(exception);
            }
            Slot::Occupied(_) => {
                problems.insert(format!(
                    "{path}: lists the license exception for {} {} twice",
                    exception.name, exception.version
                ));
            }
        }
    }
    for seen in scope.crates_io() {
        let package = &seen.package;
        let shown = format!("{} {}", package.name, package.version);
        let license = package.license.as_deref();
        let verdict = license.map(evaluate);
        let exception = exceptions.remove(&(package.name.as_str(), package.version.as_str()));
        match (verdict, exception) {
            (Some(Verdict::Allowed), None) => {}
            (Some(Verdict::Allowed), Some(_)) => {
                problems.insert(format!(
                    "{path}: {shown} needs no license exception: its license {} is in the allowed set",
                    license.unwrap_or_default()
                ));
            }
            (verdict, None) => {
                let problem = match verdict {
                    Some(Verdict::Invalid(reason)) => format!(
                        "{shown}: the license {} is not a valid SPDX expression ({reason})",
                        license.unwrap_or_default()
                    ),
                    Some(_) => format!(
                        "{shown}: the license {} is not in the allowed set",
                        license.unwrap_or_default()
                    ),
                    None => format!("{shown} has no license expression"),
                };
                problems.insert(format!(
                    "{lock}: {problem}, and {path} has no license exception for it"
                ));
            }
            (_, Some(exception)) => check_exception(path, &shown, license, exception, problems),
        }
    }
    for ((name, version), _) in exceptions {
        problems.insert(format!(
            "{path}: {name} {version} has a license exception but is not resolved for {}",
            scope.workspace.targets.join(" or ")
        ));
    }
}

/// The license exception `exception` of the allowlist at `path`, for the
/// package `shown` with the license expression `license`, stands.
fn check_exception(
    path: &str,
    shown: &str,
    license: Option<&str>,
    exception: &LicenseException,
    problems: &mut Problems,
) {
    let at = format!("{path}: the license exception for {shown}");
    let declared = license.unwrap_or("no license expression");
    match exception.license.as_deref() {
        Some(recorded) if license != Some(recorded) => {
            problems.insert(format!(
                "{at} records {recorded}, but the package declares {declared}"
            ));
        }
        None if license.is_some() => {
            problems.insert(format!(
                "{at} records no license, but the package declares {declared}"
            ));
        }
        _ => {}
    }
    if exception
        .reason
        .as_deref()
        .is_none_or(|reason| reason.trim().is_empty())
    {
        problems.insert(format!("{at} has no reason"));
    }
    let legacy = license.and_then(|license| Some((license, legacy_reading(license)?)));
    let Some(reading) = exception.reading.as_deref() else {
        if let Some((license, _)) = legacy {
            problems.insert(format!(
                "{at} has no reading, which for the legacy form {license} is the expression with `/` read as OR"
            ));
        }
        return;
    };
    match evaluate(reading) {
        Verdict::Allowed => {}
        Verdict::NotAllowed => {
            problems.insert(format!(
                "{at}: the reading {reading} is not in the allowed set"
            ));
            return;
        }
        Verdict::Invalid(reason) => {
            problems.insert(format!(
                "{at}: the reading {reading} is not a valid SPDX expression ({reason})"
            ));
            return;
        }
    }
    if let Some((license, legacy)) = legacy
        && Expression::parse_mode(reading, ParseMode::STRICT).is_ok_and(|read| read != legacy)
    {
        problems.insert(format!(
            "{at}: the reading {reading} is not {license} with `/` read as OR"
        ));
    }
}

#[cfg(test)]
mod tests;
