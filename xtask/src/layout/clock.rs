//! The test clock: test code takes time only from an injected clock, so it
//! never calls `std::thread::sleep` or `std::time::SystemTime::now`.
//!
//! Test code is, in each crate of the table and each tool workspace: every
//! Rust file under `tests/`, `benches/` and `examples/`; under `src/`, the
//! items under `#[test]` or a `cfg` attribute whose condition names `test`
//! outside `not(...)`, such as `#[cfg(test)]`, and the files of the
//! modules that they declare or include, with the modules that those
//! declare in turn; the whole `src/` of `kosmo-testkit` and of the crates
//! of the conformance and integration-test layers; and the doc tests under
//! `src/`, the Rust code blocks of doc comments. The scan of the platform
//! boundary finds the uses, so imports, renames and glob imports are
//! followed, and comments and string literals never match. An item is test
//! code by its lines, from its first attribute to its end.

use std::collections::{BTreeMap, BTreeSet};

use proc_macro2::{Delimiter, Group, TokenStream, TokenTree};

use super::{Index, Problems, platform, resolve, source};
use crate::repo::{self, Tree};

/// The crate whose whole `src/` is test support.
const TESTKIT: &str = "kosmo-testkit";

/// The layers of conformance tests and integration tests.
const TEST_LAYERS: [&str; 2] = ["D4", "F4"];

/// The directories of a crate whose Rust files are tests.
const TEST_DIRS: [&str; 3] = ["tests", "benches", "examples"];

/// The keywords of the items that end with a `{ ... }` unless a `;` comes
/// first.
const BRACED: [&str; 9] = [
    "fn",
    "mod",
    "impl",
    "trait",
    "struct",
    "enum",
    "union",
    "extern",
    "macro_rules",
];

/// The tags of a fenced code block that rustdoc runs as Rust, besides
/// `edition...`, `ignore-<target>` and error codes such as `E0277`.
const RUST_TAGS: [&str; 6] = [
    "rust",
    "should_panic",
    "no_run",
    "compile_fail",
    "test_harness",
    "standalone_crate",
];

/// The lines of a document or a code block, each with its line in the
/// file.
type Lines = Vec<(usize, String)>;

/// A Rust file of a crate: its tokens, the lines of its test items and the
/// modules and Rust files that it declares or includes.
struct Read {
    stream: TokenStream,
    tests: Vec<(usize, usize)>,
    brings_in: Vec<source::SourcePath>,
}

impl Read {
    fn in_test(&self, line: usize) -> bool {
        self.tests
            .iter()
            .any(|(first, last)| (*first..=*last).contains(&line))
    }
}

/// The test clock of every crate of the table and every tool workspace.
pub fn check(tree: &Tree, index: &Index, problems: &mut Problems) {
    let crates = index.places().map(|place| {
        let all = place.name == TESTKIT || TEST_LAYERS.contains(&place.krate.layer.as_str());
        (place.krate.path.as_str(), all)
    });
    let tools = index
        .layout
        .tools
        .values()
        .map(|tool| (tool.path.as_str(), false));
    for (dir, all) in crates.chain(tools) {
        check_crate(tree, dir, all, problems);
    }
}

/// The test clock of the crate in `dir`, all of whose `src/` is test code
/// when `all` holds. A file whose tokens cannot be read is reported by the
/// repository boundary instead.
fn check_crate(tree: &Tree, dir: &str, all: bool, problems: &mut Problems) {
    let src = format!("{dir}/src");
    let mut reads = BTreeMap::new();
    let mut test_files = BTreeSet::new();
    for (path, text) in tree
        .texts()
        .filter(|(path, _)| super::is_rust_under(path, dir))
    {
        let Ok(stream) = source::tokens(text) else {
            continue;
        };
        let stem = super::stem(path);
        let mut brings_in = source::declared_modules(stream.clone(), stem);
        let mut paths = Vec::new();
        source::source_paths(stream.clone(), stem, &mut paths);
        brings_in.extend(
            paths
                .into_iter()
                .filter(|found| found.kind == source::Kind::Code),
        );
        let mut tests = Vec::new();
        test_lines(stream.clone(), &mut tests);
        let under_tests = TEST_DIRS
            .iter()
            .any(|tests| repo::is_within(path, &format!("{dir}/{tests}")));
        if under_tests || (all && repo::is_within(path, &src)) {
            test_files.insert(path.to_string());
        }
        reads.insert(
            path.to_string(),
            Read {
                stream,
                tests,
                brings_in,
            },
        );
    }
    // The files that test code declares or includes are test code, and so
    // are those that they declare or include in turn.
    let mut work: Vec<(String, bool)> =
        test_files.iter().map(|path| (path.clone(), true)).collect();
    work.extend(reads.keys().map(|path| (path.clone(), false)));
    while let Some((path, whole)) = work.pop() {
        let Some(read) = reads.get(&path) else {
            continue;
        };
        for found in &read.brings_in {
            if !whole && !read.in_test(found.line) {
                continue;
            }
            for candidate in &found.candidates {
                if let Ok(target) = resolve(tree, repo::parent(&path), candidate)
                    && reads.contains_key(&target)
                    && test_files.insert(target.clone())
                {
                    work.push((target, true));
                }
            }
        }
    }
    for (path, read) in &reads {
        let whole = test_files.contains(path);
        if whole || !read.tests.is_empty() {
            for hit in platform::clock_reads(read.stream.clone()) {
                if whole || read.in_test(hit.line) {
                    report(path, hit.line, &hit.item, problems);
                }
            }
        }
        if repo::is_within(path, &src) {
            check_doc_tests(path, read.stream.clone(), problems);
        }
    }
}

fn report(path: &str, line: usize, item: &str, problems: &mut Problems) {
    problems.insert(format!(
        "{path}:{line}: {item} in test code; tests take time from an injected clock"
    ));
}

fn is_punct(tree: Option<&TokenTree>, char: char) -> bool {
    source::is_punct(tree, char)
}

/// The attribute brackets at `at`, after `#` or `#!`, and the index after
/// them.
fn attribute_at(trees: &[TokenTree], at: usize) -> Option<(&Group, usize)> {
    if !is_punct(trees.get(at), '#') {
        return None;
    }
    let bang = usize::from(is_punct(trees.get(at + 1), '!'));
    match trees.get(at + 1 + bang) {
        Some(TokenTree::Group(group)) if group.delimiter() == Delimiter::Bracket => {
            Some((group, at + 2 + bang))
        }
        _ => None,
    }
}

/// Whether the attribute `attribute` makes what it stands on test code:
/// `test`, or `cfg` with a condition that names `test` outside `not(...)`.
fn is_test_attribute(attribute: &Group) -> bool {
    let inner: Vec<TokenTree> = attribute.stream().into_iter().collect();
    match inner.as_slice() {
        [TokenTree::Ident(test)] => test == "test",
        [TokenTree::Ident(cfg), TokenTree::Group(condition)] => {
            cfg == "cfg"
                && condition.delimiter() == Delimiter::Parenthesis
                && names_test(condition.stream())
        }
        _ => false,
    }
}

fn names_test(condition: TokenStream) -> bool {
    let trees: Vec<TokenTree> = condition.into_iter().collect();
    trees.iter().enumerate().any(|(at, tree)| match tree {
        TokenTree::Ident(ident) => ident == "test",
        TokenTree::Group(group) => {
            let negated = at.checked_sub(1).is_some_and(
                |before| matches!(&trees[before], TokenTree::Ident(not) if not == "not"),
            );
            !negated && names_test(group.stream())
        }
        _ => false,
    })
}

fn end_line(tree: &TokenTree) -> usize {
    tree.span().end().line
}

/// The lines of the test items in `stream`, at any depth, from the first
/// attribute of each to its end. An inner test attribute, as
/// `#![cfg(test)]`, makes all of `stream` test code.
fn test_lines(stream: TokenStream, found: &mut Vec<(usize, usize)>) {
    let trees: Vec<TokenTree> = stream.into_iter().collect();
    let mut at = 0;
    while at < trees.len() {
        if let Some((attribute, after)) = attribute_at(&trees, at)
            && is_test_attribute(attribute)
        {
            let first = trees[at].span().start().line;
            if is_punct(trees.get(at + 1), '!') {
                let last = trees.last().map_or(first, end_line);
                found.push((trees[0].span().start().line, last));
                return;
            }
            let end = item_end(&trees, after);
            found.push((first, end_line(&trees[end])));
            at = end + 1;
            continue;
        }
        if let TokenTree::Group(group) = &trees[at] {
            test_lines(group.stream(), found);
        }
        at += 1;
    }
}

/// The index of the last token of the item, field or statement whose
/// further attributes start at `at`: a `;`; for an item of [`BRACED`] or a
/// macro call, the first `{ ... }` outside angle brackets; otherwise a `,`
/// outside angle brackets; or the last token of `trees`.
fn item_end(trees: &[TokenTree], mut at: usize) -> usize {
    while let Some((_, after)) = attribute_at(trees, at) {
        at = after;
    }
    let mut braced = false;
    let mut angles = 0_usize;
    for (index, tree) in trees.iter().enumerate().skip(at) {
        match tree {
            TokenTree::Punct(punct) => match punct.as_char() {
                ';' => return index,
                ',' if angles == 0 && !braced => return index,
                '<' => angles += 1,
                '>' => angles = angles.saturating_sub(1),
                _ => {}
            },
            TokenTree::Ident(ident) if angles == 0 => {
                braced |= BRACED.contains(&ident.to_string().as_str());
            }
            TokenTree::Group(group)
                if group.delimiter() == Delimiter::Brace
                    && angles == 0
                    && (braced
                        || is_punct(index.checked_sub(1).and_then(|i| trees.get(i)), '!')) =>
            {
                return index;
            }
            _ => {}
        }
    }
    trees.len().saturating_sub(1)
}

/// The doc tests of the file `path` with the tokens `stream`: the uses in
/// each Rust code block of its doc comments, and a block whose tokens
/// cannot be read.
fn check_doc_tests(path: &str, stream: TokenStream, problems: &mut Problems) {
    let mut blocks = Vec::new();
    doc_blocks(stream, &mut blocks);
    for block in blocks {
        let Some(&(start, _)) = block.first() else {
            continue;
        };
        let at = |line: usize| {
            block
                .get(line.saturating_sub(1))
                .map_or(start, |(at, _)| *at)
        };
        let text: Vec<&str> = block.iter().map(|(_, text)| text.as_str()).collect();
        match source::tokens(&text.join("\n")) {
            Ok(tokens) => {
                for hit in platform::clock_reads(tokens) {
                    report(path, at(hit.line), &hit.item, problems);
                }
            }
            Err((line, error)) => {
                problems.insert(format!(
                    "{path}:{}: the doc test cannot be read as Rust tokens: {error}; the layout step reads doc tests for the test clock",
                    at(line)
                ));
            }
        }
    }
}

/// The Rust code blocks of the doc comments in `stream`, at any depth, as
/// their lines with the line of each in the file.
fn doc_blocks(stream: TokenStream, found: &mut Vec<Lines>) {
    let trees: Vec<TokenTree> = stream.into_iter().collect();
    // The lines of the doc attributes of one run of attributes, which
    // rustdoc reads as one document.
    let mut lines = Vec::new();
    let mut at = 0;
    while at < trees.len() {
        if let Some((attribute, after)) = attribute_at(&trees, at) {
            doc_lines(attribute, &mut lines);
            at = after;
            continue;
        }
        code_blocks(&std::mem::take(&mut lines), found);
        if let TokenTree::Group(group) = &trees[at] {
            doc_blocks(group.stream(), found);
        }
        at += 1;
    }
    code_blocks(&lines, found);
}

/// The lines of the doc attribute `attribute`, `doc = "..."`, with the
/// line of each. In a block doc comment whose later lines all start with
/// `*`, that `*` is not part of them.
fn doc_lines(attribute: &Group, lines: &mut Lines) {
    let inner: Vec<TokenTree> = attribute.stream().into_iter().collect();
    let [
        TokenTree::Ident(doc),
        TokenTree::Punct(equals),
        TokenTree::Literal(literal),
    ] = inner.as_slice()
    else {
        return;
    };
    let Some(text) = source::string(literal).filter(|_| doc == "doc" && equals.as_char() == '=')
    else {
        return;
    };
    // A literal on one line writes its line breaks as escapes.
    let (line, last) = (literal.span().start().line, literal.span().end().line);
    let parts: Vec<&str> = text.split('\n').collect();
    let starred = parts.len() > 1
        && parts[1..]
            .iter()
            .all(|part| part.trim().is_empty() || part.trim_start().starts_with('*'));
    for (offset, part) in parts.into_iter().enumerate() {
        let part = match part.trim_start().strip_prefix('*') {
            Some(rest) if starred && offset > 0 => rest,
            _ => part,
        };
        lines.push(((line + offset).min(last), part.to_string()));
    }
}

/// The Rust code blocks among the fenced code blocks of the document
/// `lines`, with rustdoc's hidden lines, `# ` and `#`, shown. A block that
/// is not closed ends with the document.
fn code_blocks(lines: &[(usize, String)], found: &mut Vec<Lines>) {
    let mut open: Option<(char, usize, bool, Lines)> = None;
    for (line, text) in lines {
        let trimmed = text.trim_start();
        let fence = |marker: char| trimmed.chars().take_while(|c| *c == marker).count();
        match &mut open {
            None => {
                for marker in ['`', '~'] {
                    let length = fence(marker);
                    let info = &trimmed[length..];
                    if length >= 3 && !(marker == '`' && info.contains('`')) {
                        open = Some((marker, length, is_rust(info), Vec::new()));
                    }
                }
            }
            Some((marker, length, rust, block)) => {
                let closing = fence(*marker);
                if closing >= *length && trimmed[closing..].trim().is_empty() {
                    if *rust {
                        found.push(std::mem::take(block));
                    }
                    open = None;
                } else {
                    let shown = match trimmed {
                        "#" => "",
                        _ => trimmed.strip_prefix("# ").unwrap_or(trimmed),
                    };
                    let shown = shown
                        .strip_prefix("##")
                        .map_or(shown.to_string(), |rest| format!("#{rest}"));
                    block.push((*line, shown));
                }
            }
        }
    }
    if let Some((_, _, true, block)) = open {
        found.push(block);
    }
}

/// Whether rustdoc runs a fenced code block with the info string `info` as
/// a Rust test: it is not marked `ignore`, and it has no tag, is marked
/// `rust` or has only the tags of Rust tests.
fn is_rust(info: &str) -> bool {
    let tags: Vec<&str> = info
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|tag| !tag.is_empty())
        .collect();
    let rust_tag = |tag: &&str| {
        RUST_TAGS.contains(tag)
            || tag.starts_with("edition")
            || tag.starts_with("ignore-")
            || (tag.len() == 5
                && tag.starts_with('E')
                && tag[1..].bytes().all(|b| b.is_ascii_digit()))
    };
    !tags.contains(&"ignore") && (tags.contains(&"rust") || tags.iter().all(rust_tag))
}
