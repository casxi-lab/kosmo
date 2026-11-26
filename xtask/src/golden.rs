//! Pixel goldens: the goldens step of `cargo xtask check`.
//!
//! The goldens of a crate are in its directory `tests/golden/`: one file
//! `<name>.px` for each golden, holding a drawn buffer as its crate's tests
//! encode it, and `MANIFEST`, with one line `<digest>  <name>` for each
//! golden: the SHA-256 of the file in 64 lowercase hexadecimal digits, two
//! spaces and the name, sorted by name. Names are lowercase letters, digits
//! and `-`. A test of the crate refers to a golden by calling
//! `matches_golden` with its name as a string literal.
//!
//! The goldens step reads only tracked files and does not run Cargo. It
//! reports every problem it finds, one per line as `<path>[:<line>]:
//! <message>`, sorted by path and then by line number.

#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::str::FromStr;

use proc_macro2::{Delimiter, TokenStream, TokenTree};
use sha2::{Digest, Sha256};

use crate::check::Facts;
use crate::repo::{self, Entry, Tree};

/// A crate whose tests compare drawings with goldens.
#[derive(Debug, Clone, Copy)]
pub struct Goldens {
    /// The directory of the crate.
    pub dir: &'static str,
}

/// The crates with goldens. A golden directory elsewhere fails the step.
pub const SETS: [Goldens; 1] = [Goldens {
    dir: "framework/render",
}];

/// The function through which a test refers to a golden.
const REFERENCE: &str = "matches_golden";

impl Goldens {
    /// The golden directory.
    pub fn golden_dir(&self) -> String {
        format!("{}/tests/golden", self.dir)
    }
}

/// The SHA-256 digest of `bytes` in lowercase hexadecimal digits.
pub fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Whether `name` can name a golden: lowercase letters, digits and `-`.
fn is_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

/// Whether `digest` is written as `MANIFEST` writes digests.
fn is_digest(digest: &str) -> bool {
    digest.len() == 64
        && digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Problems by path and line; line 0 stands for the whole file.
#[derive(Debug, Default)]
pub struct Problems(BTreeSet<(String, usize, String)>);

impl Problems {
    fn add(&mut self, path: &str, line: usize, message: String) {
        self.0.insert((path.to_string(), line, message));
    }

    /// Each problem as `<path>[:<line>]: <message>`, in order.
    pub fn lines(&self) -> Vec<String> {
        self.0
            .iter()
            .map(|(path, line, message)| match line {
                0 => format!("{path}: {message}"),
                _ => format!("{path}:{line}: {message}"),
            })
            .collect()
    }
}

/// The goldens that a manifest lists, by name, with their lines and
/// digests. Bad lines are reported and left out.
pub fn parse_manifest(
    path: &str,
    text: &str,
    problems: &mut Problems,
) -> BTreeMap<String, (usize, String)> {
    let mut listed: BTreeMap<String, (usize, String)> = BTreeMap::new();
    let mut last: Option<&str> = None;
    for (index, line) in text.split_terminator('\n').enumerate() {
        let number = index + 1;
        let fields = line
            .split_once("  ")
            .filter(|(digest, name)| !digest.contains(' ') && !name.contains(' '));
        let Some((digest, name)) = fields else {
            problems.add(
                path,
                number,
                "expected `<digest>  <name>`: the SHA-256 digest, two spaces and the name"
                    .to_string(),
            );
            continue;
        };
        if !is_digest(digest) {
            let message = format!("`{digest}` is not 64 lowercase hexadecimal digits");
            problems.add(path, number, message);
            continue;
        }
        if !is_name(name) {
            let message =
                format!("`{name}` is not a golden name of lowercase letters, digits and `-`");
            problems.add(path, number, message);
            continue;
        }
        if let Some((first, _)) = listed.get(name) {
            let message = format!("golden {name} is listed twice, first on line {first}");
            problems.add(path, number, message);
            continue;
        }
        if let Some(before) = last.filter(|before| *before > name) {
            let message = format!("golden {name} follows {before}; lines are sorted by name");
            problems.add(path, number, message);
        }
        last = Some(name);
        listed.insert(name.to_string(), (number, digest.to_string()));
    }
    listed
}

/// The goldens that the Rust files of the crate at `dir` refer to: the
/// path, line and name of each call of [`REFERENCE`] with a string
/// literal as its first argument. Files that are not Rust tokens are left
/// to the layout step.
fn references(tree: &Tree, dir: &str) -> Vec<(String, usize, String)> {
    let mut found = Vec::new();
    for (path, text) in tree.texts() {
        if !(repo::is_within(path, dir) && path.ends_with(".rs")) {
            continue;
        }
        if let Ok(stream) = TokenStream::from_str(text) {
            scan(stream, &mut |line, name| {
                found.push((path.to_string(), line, name));
            });
        }
    }
    found
}

fn scan(stream: TokenStream, found: &mut dyn FnMut(usize, String)) {
    let trees: Vec<TokenTree> = stream.into_iter().collect();
    for (at, tree) in trees.iter().enumerate() {
        match tree {
            TokenTree::Group(group) => scan(group.stream(), found),
            TokenTree::Ident(ident) if ident == REFERENCE => {
                let Some(TokenTree::Group(args)) = trees.get(at + 1) else {
                    continue;
                };
                if args.delimiter() != Delimiter::Parenthesis {
                    continue;
                }
                if let Some(TokenTree::Literal(literal)) = args.stream().into_iter().next()
                    && let syn::Lit::Str(name) = syn::Lit::new(literal.clone())
                {
                    found(literal.span().start().line, name.value());
                }
            }
            _ => {}
        }
    }
}

/// Whether `path` lies in a directory `tests/golden`.
fn in_golden_dir(path: &str) -> bool {
    path.starts_with("tests/golden/") || path.contains("/tests/golden/")
}

/// Checks the goldens of `sets` in `tree`, and that no golden directory
/// lies elsewhere. Returns how many goldens the manifests list.
pub fn check(tree: &Tree, sets: &[Goldens], problems: &mut Problems) -> usize {
    let dirs: Vec<String> = sets.iter().map(Goldens::golden_dir).collect();
    for (path, _) in tree.entries() {
        if in_golden_dir(path) && !dirs.iter().any(|dir| repo::is_within(path, dir)) {
            let message = "is in a golden directory that `cargo xtask golden` does not write";
            problems.add(path, 0, message.to_string());
        }
    }
    sets.iter().map(|set| check_set(tree, set, problems)).sum()
}

fn check_set(tree: &Tree, set: &Goldens, problems: &mut Problems) -> usize {
    let dir = set.golden_dir();
    let manifest = format!("{dir}/MANIFEST");
    let mut files: BTreeMap<&str, (&str, &[u8])> = BTreeMap::new();
    for (path, entry) in tree.entries() {
        let Some(file) = path
            .strip_prefix(&dir)
            .and_then(|rest| rest.strip_prefix('/'))
        else {
            continue;
        };
        let name = file.strip_suffix(".px").filter(|name| is_name(name));
        match (name, entry) {
            _ if path == manifest => {}
            (Some(name), Entry::File(bytes)) => {
                files.insert(name, (path, bytes.as_slice()));
            }
            _ => {
                let message = "is not a golden: a golden directory holds MANIFEST and a regular file `<name>.px` for each golden";
                problems.add(path, 0, message.to_string());
            }
        }
    }
    let listed = match tree.entry(&manifest) {
        None => BTreeMap::new(),
        Some(Entry::File(bytes)) => match std::str::from_utf8(bytes) {
            Ok(text) => parse_manifest(&manifest, text, problems),
            Err(_) => {
                problems.add(&manifest, 0, "is not UTF-8".to_string());
                BTreeMap::new()
            }
        },
        Some(_) => {
            problems.add(&manifest, 0, "is not a regular file".to_string());
            BTreeMap::new()
        }
    };
    let references = references(tree, set.dir);
    let referenced: BTreeSet<&str> = references
        .iter()
        .map(|(_, _, name)| name.as_str())
        .collect();
    for (name, (line, listed_digest)) in &listed {
        match files.get(name.as_str()) {
            None => problems.add(&manifest, *line, format!("golden {name} has no file")),
            Some((path, bytes)) => {
                let actual = digest(bytes);
                if actual != *listed_digest {
                    let message = format!(
                        "SHA-256 {actual} differs from MANIFEST; goldens change only through cargo xtask golden"
                    );
                    problems.add(path, 0, message);
                }
            }
        }
        if !referenced.contains(name.as_str()) {
            let message = format!("golden {name} is not referenced by any test");
            problems.add(&manifest, *line, message);
        }
    }
    for (name, (path, _)) in &files {
        if !listed.contains_key(*name) {
            problems.add(path, 0, format!("golden {name} is not in MANIFEST"));
        }
    }
    for (path, line, name) in &references {
        if !listed.contains_key(name) {
            problems.add(path, *line, format!("golden {name} is not in MANIFEST"));
        }
    }
    listed.len()
}

/// The goldens step on the tracked files.
pub fn run(root: &Path, facts: &mut Facts) -> Result<(), String> {
    let tree = Tree::load(root)?;
    let mut problems = Problems::default();
    let count = check(&tree, &SETS, &mut problems);
    let dirs: Vec<String> = SETS.iter().map(Goldens::golden_dir).collect();
    println!("goldens: {count} in {}", dirs.join(", "));
    facts.push(("goldens".to_string(), count.to_string()));
    let lines = problems.lines();
    if lines.is_empty() {
        Ok(())
    } else {
        Err(lines.join("\n"))
    }
}
