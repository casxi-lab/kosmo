//! Call probes and test hooks stay out of ordinary builds. A crate turns on
//! its dependency on `kosmo-testkit` only through its own feature
//! `call-probes`; no dependency declaration turns on the `call-probes` of a
//! crate, and none but a dev-dependency, or the dependency of
//! `kosmo-testkit` on `kosmo-platform`, turns on the `test-hooks` of the
//! platform interface or of a backend; and no feature of a crate but
//! `call-probes` and `test-hooks`, its default features included, turns on
//! either.

use std::collections::{BTreeMap, BTreeSet};

use toml::{Table, Value};

use super::{Problems, dependency_tables};

/// The crate of the call probes.
const TESTKIT: &str = "kosmo-testkit";

/// The feature of a crate with probes, which turns them on.
const PROBES: &str = "call-probes";

/// The feature of the platform interface and of each backend that turns on
/// their test hooks.
const HOOKS: &str = "test-hooks";

/// The platform interface, whose test hooks `kosmo-testkit` turns on.
const PLATFORM: &str = "kosmo-platform";

/// What the default features may not turn on.
const OFF_BY_DEFAULT: &str = "call probes and test hooks stay off by default";

/// What the other features may not turn on.
const ONLY_THEIR_OWN: &str = "only call-probes and test-hooks turn them on";

/// Whether the crate `name` has test hooks: the platform interface or a
/// backend.
fn has_hooks(name: &str) -> bool {
    name == PLATFORM || name.starts_with("kosmo-backend-")
}

/// The dependency and the feature that a feature value names:
/// `<dependency>/<feature>` and `<dependency>?/<feature>`, or a feature of
/// `own` otherwise.
fn feature_of<'a>(value: &'a str, own: &'a str) -> (&'a str, &'a str) {
    match value.split_once('/') {
        Some((dependency, feature)) => (dependency.trim_end_matches('?'), feature),
        None => (own, value),
    }
}

fn strings(value: Option<&Value>) -> Vec<&str> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect()
}

/// The features of the dependency declaration `entry` of `dependency` that
/// turn on call probes or test hooks; `kind` names its table, and `dev`
/// tells whether it is a dev-dependency. `exempt` is the declaration of
/// `kosmo-testkit` on `kosmo-platform`.
fn check_declaration(
    manifest: &str,
    (kind, dev, exempt): (&str, bool, bool),
    dependency: &str,
    entry: &Value,
    problems: &mut Problems,
) {
    for value in strings(entry.get("features")) {
        let (owner, feature) = feature_of(value, dependency);
        if feature == PROBES {
            problems.insert(format!(
                "{manifest}: {kind} {dependency}: turns on {owner}/{PROBES}; only the command line turns on call probes"
            ));
        }
        if feature == HOOKS && has_hooks(owner) && !dev && !(exempt && value == HOOKS) {
            problems.insert(format!(
                "{manifest}: {kind} {dependency}: turns on {owner}/{HOOKS}; only dev-dependencies and the dependency of {TESTKIT} on {PLATFORM} turn on test hooks"
            ));
        }
    }
}

/// The `[workspace.dependencies]` of the root manifest `manifest` with the
/// `[workspace]` table `workspace`: every member that inherits one of them
/// gets its features.
pub fn check_root(manifest: &str, workspace: &Table, problems: &mut Problems) {
    let entries = workspace.get("dependencies").and_then(Value::as_table);
    for (dependency, entry) in entries.into_iter().flatten() {
        let kind = ("[workspace.dependencies]", false, false);
        check_declaration(manifest, kind, dependency, entry, problems);
    }
}

/// The dependencies and the features of the crate `name` with the manifest
/// `table` at `manifest`.
pub fn check_crate(manifest: &str, name: &str, table: &Table, problems: &mut Problems) {
    let features: BTreeMap<&str, Vec<&str>> = table
        .get("features")
        .and_then(Value::as_table)
        .into_iter()
        .flatten()
        .map(|(feature, values)| (feature.as_str(), strings(Some(values))))
        .collect();
    for (kind, entries) in dependency_tables(table) {
        let dev = kind.ends_with("dev-dependencies]");
        for (dependency, entry) in entries {
            let exempt = name == TESTKIT && dependency == PLATFORM;
            check_declaration(manifest, (&kind, dev, exempt), dependency, entry, problems);
            if dependency != TESTKIT || dev {
                continue;
            }
            if entry.get("optional").and_then(Value::as_bool) != Some(true) {
                problems.insert(format!(
                    "{manifest}: {kind} {TESTKIT}: is a normal dependency; only the feature {PROBES} may turn it on, as an optional dependency"
                ));
                continue;
            }
            let own = format!("dep:{TESTKIT}");
            if !features
                .get(PROBES)
                .is_some_and(|values| values.contains(&own.as_str()))
            {
                problems.insert(format!(
                    "{manifest}: {kind} {TESTKIT}: the feature {PROBES} must turn it on with \"{own}\", the only feature that does"
                ));
            }
            for (feature, values) in &features {
                let turns_on = values.iter().any(|value| {
                    *value == own || *value == TESTKIT || value.starts_with(&format!("{TESTKIT}/"))
                });
                if *feature != PROBES && turns_on {
                    problems.insert(format!(
                        "{manifest}: {kind} {TESTKIT}: the feature {feature} turns it on; only {PROBES} may"
                    ));
                }
            }
        }
    }
    for start in features.keys() {
        if *start != PROBES && *start != HOOKS {
            check_expansion(manifest, start, &features, problems);
        }
    }
}

/// The feature `start` of a crate, other than `call-probes` and
/// `test-hooks`, expanded through its feature table `features`: it turns on
/// neither its `call-probes` or `test-hooks` nor the `call-probes` or
/// `test-hooks` of a dependency, and the default features do not turn on
/// `kosmo-testkit` either. The dependency on `kosmo-testkit` reports the
/// other features that turn it on.
fn check_expansion(
    manifest: &str,
    start: &str,
    features: &BTreeMap<&str, Vec<&str>>,
    problems: &mut Problems,
) {
    let default = start == "default";
    let report = |value: &str, through: Option<&str>, problems: &mut Problems| {
        let through = through
            .map(|feature| format!(" through {feature}"))
            .unwrap_or_default();
        problems.insert(if default {
            format!("{manifest}: the default features turn on {value}{through}; {OFF_BY_DEFAULT}")
        } else {
            format!("{manifest}: the feature {start} turns on {value}{through}; {ONLY_THEIR_OWN}")
        });
    };
    let mut seen = BTreeSet::new();
    let mut work = vec![(start, None)];
    while let Some((feature, through)) = work.pop() {
        if !seen.insert(feature) {
            continue;
        }
        if feature == PROBES || feature == HOOKS || (default && feature == TESTKIT) {
            report(feature, through, problems);
            continue;
        }
        let through = through.or((feature != start).then_some(feature));
        for value in features.get(feature).into_iter().flatten() {
            if let Some(dependency) = value.strip_prefix("dep:") {
                if default && dependency == TESTKIT {
                    report(value, through, problems);
                }
            } else if let Some((dependency, feature)) = value.split_once('/') {
                let dependency = dependency.trim_end_matches('?');
                if (default && dependency == TESTKIT) || feature == PROBES || feature == HOOKS {
                    report(value, through, problems);
                }
            } else {
                work.push((value, through));
            }
        }
    }
}
