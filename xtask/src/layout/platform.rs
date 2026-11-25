//! The platform boundary: the paths, macros and types that a core library
//! never uses, the platform categories that allow some of them to the
//! crates registered for them, and the scan that finds them in the tokens
//! of a Rust file.
//!
//! The scan reads tokens, so comments, doc comments and the contents of
//! string literals never match, while the bodies of macro calls are read
//! like any other tokens. It follows the names that `use` declarations and
//! `extern crate` items bring into a module or block, with their renames and
//! glob imports, into the blocks and modules inside it, and paths from the
//! crate root written with a leading `::`. A name imported in a module
//! also counts in the modules inside it, which Rust does not do, so a local
//! item there with the name of such an import is read as the import.

use std::collections::{BTreeMap, BTreeSet};

use proc_macro2::{Delimiter, Spacing, TokenStream, TokenTree};

/// A platform category of the layout table's `platform` column: the
/// forbidden items that a crate registered for it may use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Category {
    /// All of `std::io` and the macros that print.
    StandardStreams,
    /// The program's arguments.
    Arguments,
    /// The program's exit code.
    ExitCode,
    /// The real clock.
    RealClock,
    /// Threads.
    Threads,
}

impl Category {
    /// Every category, in the order of the table's comments.
    pub const ALL: [Category; 5] = [
        Category::StandardStreams,
        Category::Arguments,
        Category::ExitCode,
        Category::RealClock,
        Category::Threads,
    ];

    /// The name that the table uses.
    pub fn name(self) -> &'static str {
        match self {
            Category::StandardStreams => "standard-streams",
            Category::Arguments => "arguments",
            Category::ExitCode => "exit-code",
            Category::RealClock => "real-clock",
            Category::Threads => "threads",
        }
    }

    /// The category with the name `name`.
    pub fn named(name: &str) -> Option<Category> {
        Category::ALL
            .into_iter()
            .find(|category| category.name() == name)
    }
}

/// How a forbidden item is used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Form {
    /// Through its path, as a module, type, function or constant: the item
    /// and everything inside it.
    Path,
    /// As a macro called by its name.
    Macro,
}

/// A forbidden item: its path from a crate root, how it is used, and the
/// category that allows it, if one does.
#[derive(Debug)]
struct Item {
    path: &'static [&'static str],
    form: Form,
    category: Option<Category>,
}

impl Item {
    /// The item as reports name it: its path, and a macro with `!`.
    fn shown(&self) -> String {
        match self.form {
            Form::Path => self.path.join("::"),
            Form::Macro => format!("{}!", self.path.last().copied().unwrap_or_default()),
        }
    }
}

const fn path(path: &'static [&'static str], category: Option<Category>) -> Item {
    Item {
        path,
        form: Form::Path,
        category,
    }
}

const fn called(path: &'static [&'static str], category: Category) -> Item {
    Item {
        path,
        form: Form::Macro,
        category: Some(category),
    }
}

/// The forbidden items of a core library, as the platform boundary in
/// `docs/architecture.md` lists them. For a path, the longest item that it
/// starts with decides, so `std::env::args` is allowed by `arguments` while
/// the rest of `std::env` is allowed by no category. kalyx's types are
/// listed under each path that kalyx 0.1.0 makes public.
const ITEMS: [Item; 29] = [
    path(&["std", "fs"], None),
    path(&["std", "path"], None),
    path(&["std", "ffi", "OsStr"], Some(Category::Arguments)),
    path(&["std", "ffi", "OsString"], Some(Category::Arguments)),
    path(&["std", "net"], None),
    path(&["std", "os"], None),
    path(&["std", "process"], None),
    path(&["std", "process", "ExitCode"], Some(Category::ExitCode)),
    path(&["std", "process", "exit"], Some(Category::ExitCode)),
    path(&["std", "env"], None),
    path(&["std", "env", "args"], Some(Category::Arguments)),
    path(&["std", "env", "args_os"], Some(Category::Arguments)),
    path(&["std", "env", "Args"], Some(Category::Arguments)),
    path(&["std", "env", "ArgsOs"], Some(Category::Arguments)),
    path(&["std", "thread"], Some(Category::Threads)),
    path(&["std", "time", "Instant"], Some(Category::RealClock)),
    path(&["std", "time", "SystemTime"], Some(Category::RealClock)),
    path(&["std", "time", "UNIX_EPOCH"], Some(Category::RealClock)),
    path(&["std", "io"], Some(Category::StandardStreams)),
    path(&["kalyx", "MonotonicClock"], Some(Category::RealClock)),
    path(
        &["kalyx", "context", "MonotonicClock"],
        Some(Category::RealClock),
    ),
    path(&["kalyx", "Context", "new"], Some(Category::RealClock)),
    path(
        &["kalyx", "context", "Context", "new"],
        Some(Category::RealClock),
    ),
    called(&["std", "print"], Category::StandardStreams),
    called(&["std", "println"], Category::StandardStreams),
    called(&["std", "eprint"], Category::StandardStreams),
    called(&["std", "eprintln"], Category::StandardStreams),
    called(&["std", "dbg"], Category::StandardStreams),
    called(&["std", "thread_local"], Category::Threads),
];

/// The crates whose paths the scan follows when a path starts with their
/// name.
const CRATES: [&str; 2] = ["std", "kalyx"];

/// The first segments of paths to items of the crate itself.
const LOCAL: [&str; 4] = ["crate", "self", "super", "Self"];

/// Keywords after which `::` starts a path from a crate root.
const BEFORE_ROOTED: [&str; 21] = [
    "as", "box", "break", "const", "dyn", "else", "for", "if", "impl", "in", "let", "match",
    "move", "mut", "ref", "return", "static", "type", "unsafe", "where", "yield",
];

/// How many times a scope resolves its imports through one another.
const PASSES: usize = 8;

/// A forbidden item that a file uses, at a line counted from 1.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Hit {
    pub line: usize,
    /// The item, as [`Item::shown`] names it.
    pub item: String,
    /// The category that allows it, if one does.
    pub category: Option<Category>,
}

/// The forbidden items in `stream`, the tokens of a file, that the
/// categories `allowed` do not allow.
pub fn scan(stream: TokenStream, allowed: &[Category]) -> BTreeSet<Hit> {
    let mut hits = BTreeSet::new();
    level(stream, &Scope::default(), allowed, &mut hits);
    hits
}

/// The longest path item that `path`, from a crate root, starts with.
fn item_of_path(path: &[String]) -> Option<&'static Item> {
    ITEMS
        .iter()
        .filter(|item| {
            item.form == Form::Path
                && path.len() >= item.path.len()
                && item.path.iter().zip(path).all(|(a, b)| a == b)
        })
        .max_by_key(|item| item.path.len())
}

/// The macro item whose path is `path`.
fn item_of_macro(path: &[String]) -> Option<&'static Item> {
    ITEMS
        .iter()
        .find(|item| item.form == Form::Macro && item.path.iter().eq(path.iter()))
}

/// What the names of a module or block stand for.
#[derive(Debug, Clone, Default)]
struct Scope {
    /// The paths from a crate root that each imported name may stand for.
    names: BTreeMap<String, BTreeSet<Vec<String>>>,
    /// The paths from a crate root whose items glob imports bring in,
    /// when those paths are not forbidden items themselves.
    globs: BTreeSet<Vec<String>>,
}

impl Scope {
    /// The paths from a crate root that `segments` may stand for: none for
    /// a path to the crate's own items or to a crate the scan does not
    /// follow.
    fn resolve(&self, segments: &[String], rooted: bool) -> BTreeSet<Vec<String>> {
        let Some(first) = segments.first() else {
            return BTreeSet::new();
        };
        let joined = |prefix: &Vec<String>, rest: &[String]| {
            prefix.iter().chain(rest).cloned().collect::<Vec<String>>()
        };
        if rooted {
            return BTreeSet::from([segments.to_vec()]);
        }
        if LOCAL.contains(&first.as_str()) {
            return BTreeSet::new();
        }
        if let Some(targets) = self.names.get(first) {
            return targets
                .iter()
                .map(|target| joined(target, &segments[1..]))
                .collect();
        }
        if CRATES.contains(&first.as_str()) {
            return BTreeSet::from([segments.to_vec()]);
        }
        self.globs
            .iter()
            .map(|glob| joined(glob, segments))
            .collect()
    }
}

/// An import of a `use` declaration or an `extern crate` item.
#[derive(Debug)]
struct Import {
    segments: Vec<String>,
    /// Written from the crate root, with a leading `::` or as a crate.
    rooted: bool,
    /// The name it brings in; `None` for `_` and for a glob import.
    name: Option<String>,
    glob: bool,
    line: usize,
}

fn is_punct(tree: Option<&TokenTree>, char: char) -> bool {
    matches!(tree, Some(TokenTree::Punct(punct)) if punct.as_char() == char)
}

/// The text of an identifier without the `r#` of a raw identifier.
fn unraw(ident: &proc_macro2::Ident) -> String {
    let text = ident.to_string();
    match text.strip_prefix("r#") {
        Some(name) => name.to_string(),
        None => text,
    }
}

fn ident_at(trees: &[TokenTree], at: usize) -> Option<&proc_macro2::Ident> {
    match trees.get(at) {
        Some(TokenTree::Ident(ident)) => Some(ident),
        _ => None,
    }
}

/// Whether `::` starts at `at`.
fn is_separator(trees: &[TokenTree], at: usize) -> bool {
    matches!(trees.get(at), Some(TokenTree::Punct(punct)) if punct.as_char() == ':' && punct.spacing() == Spacing::Joint)
        && is_punct(trees.get(at + 1), ':')
}

/// Whether `::` ends just before `at`.
fn after_separator(trees: &[TokenTree], at: usize) -> bool {
    at >= 2 && is_separator(trees, at - 2)
}

/// The imports of the `use` tree `trees`, under `prefix`.
fn use_tree(trees: &[TokenTree], prefix: &[String], rooted: bool, found: &mut Vec<Import>) {
    let mut segments = prefix.to_vec();
    let mut rooted = rooted;
    let mut at = 0;
    if segments.is_empty() && is_separator(trees, 0) {
        rooted = true;
        at = 2;
    }
    loop {
        match trees.get(at) {
            Some(TokenTree::Ident(ident)) => {
                let name = unraw(ident);
                if is_separator(trees, at + 1) {
                    segments.push(name);
                    at += 3;
                    continue;
                }
                let line = ident.span().start().line;
                let bound = if name == "self" {
                    segments.last().cloned()
                } else {
                    segments.push(name.clone());
                    Some(name)
                };
                let renamed = match (ident_at(trees, at + 1), ident_at(trees, at + 2)) {
                    (Some(keyword), Some(alias)) if keyword == "as" => Some(unraw(alias)),
                    _ => None,
                };
                let name = match renamed {
                    Some(alias) if alias == "_" => None,
                    Some(alias) => Some(alias),
                    None => bound,
                };
                found.push(Import {
                    segments,
                    rooted,
                    name,
                    glob: false,
                    line,
                });
                return;
            }
            Some(TokenTree::Punct(star)) if star.as_char() == '*' => {
                found.push(Import {
                    segments,
                    rooted,
                    name: None,
                    glob: true,
                    line: star.span().start().line,
                });
                return;
            }
            Some(TokenTree::Group(group)) if group.delimiter() == Delimiter::Brace => {
                let inner: Vec<TokenTree> = group.stream().into_iter().collect();
                for part in inner.split(|tree| is_punct(Some(tree), ',')) {
                    use_tree(part, &segments, rooted, found);
                }
                return;
            }
            _ => return,
        }
    }
}

/// The imports of the items `trees` of one module or block, and which
/// tokens belong to them.
fn imports(trees: &[TokenTree]) -> (Vec<Import>, Vec<bool>) {
    let mut found = Vec::new();
    let mut taken = vec![false; trees.len()];
    let mut at = 0;
    while at < trees.len() {
        let starts_use = matches!(&trees[at], TokenTree::Ident(ident) if ident == "use");
        let starts_extern = matches!(&trees[at], TokenTree::Ident(ident) if ident == "extern")
            && matches!(ident_at(trees, at + 1), Some(ident) if ident == "crate");
        let end = (starts_use || starts_extern)
            .then(|| {
                trees[at..]
                    .iter()
                    .position(|tree| is_punct(Some(tree), ';'))
            })
            .flatten()
            .map(|offset| at + offset);
        match end {
            Some(end) if starts_use => {
                use_tree(&trees[at + 1..end], &[], false, &mut found);
                taken[at..=end].fill(true);
                at = end + 1;
            }
            Some(end) if starts_extern => {
                if let Some(krate) = ident_at(trees, at + 2) {
                    let alias = match (ident_at(trees, at + 3), ident_at(trees, at + 4)) {
                        (Some(keyword), Some(alias)) if keyword == "as" => unraw(alias),
                        _ => unraw(krate),
                    };
                    found.push(Import {
                        segments: vec![unraw(krate)],
                        rooted: true,
                        name: (alias != "_").then_some(alias),
                        glob: false,
                        line: krate.span().start().line,
                    });
                }
                taken[at..=end].fill(true);
                at = end + 1;
            }
            _ => at += 1,
        }
    }
    (found, taken)
}

/// The scope of a module or block inside `outer` with the imports
/// `imports`, which resolve through one another and through `outer`.
fn enter(outer: &Scope, imports: &[Import]) -> Scope {
    let mut base = outer.clone();
    for name in imports.iter().filter_map(|import| import.name.as_ref()) {
        base.names.remove(name);
    }
    let mut scope = base.clone();
    for _ in 0..PASSES {
        let mut next = base.clone();
        for import in imports {
            let targets = scope.resolve(&import.segments, import.rooted);
            if import.glob {
                next.globs.extend(
                    targets
                        .into_iter()
                        .filter(|target| item_of_path(target).is_none()),
                );
            } else if let Some(name) = &import.name {
                next.names.entry(name.clone()).or_default().extend(targets);
            }
        }
        let settled = next.names == scope.names && next.globs == scope.globs;
        scope = next;
        if settled {
            break;
        }
    }
    scope
}

/// Records `item`, used at `line`, unless `allowed` allows it.
fn record(item: &Item, line: usize, allowed: &[Category], hits: &mut BTreeSet<Hit>) {
    if item
        .category
        .is_some_and(|category| allowed.contains(&category))
    {
        return;
    }
    hits.insert(Hit {
        line,
        item: item.shown(),
        category: item.category,
    });
}

/// The path that starts at the identifier `at`: its segments, the index
/// after it and its line.
fn path_at(trees: &[TokenTree], at: usize) -> Option<(Vec<String>, usize, usize)> {
    let first = ident_at(trees, at)?;
    let mut segments = vec![unraw(first)];
    let mut end = at + 1;
    while is_separator(trees, end) {
        let Some(next) = ident_at(trees, end + 2) else {
            break;
        };
        segments.push(unraw(next));
        end += 3;
    }
    Some((segments, end, first.span().start().line))
}

/// Whether the `::` at `at` starts a path from a crate root rather than
/// continuing one, as after a generic argument list; after `->` and `=>` it
/// starts one.
fn starts_rooted(trees: &[TokenTree], at: usize) -> bool {
    let token = |back: usize| at.checked_sub(back).and_then(|before| trees.get(before));
    match token(1) {
        Some(TokenTree::Ident(ident)) => BEFORE_ROOTED.contains(&ident.to_string().as_str()),
        Some(TokenTree::Punct(punct)) if punct.as_char() == '>' => matches!(
            token(2),
            Some(TokenTree::Punct(arrow))
                if matches!(arrow.as_char(), '-' | '=') && arrow.spacing() == Spacing::Joint
        ),
        _ => true,
    }
}

/// Whether the identifier at `at` starts a path: it is not one of the
/// keywords after which `::` starts a path from a crate root, and it does
/// not follow `::`, a `.` of a field or method, the `'` of a lifetime or
/// label, or the `$` of a macro variable.
fn starts_path(trees: &[TokenTree], at: usize) -> bool {
    let before = at.checked_sub(1).and_then(|before| trees.get(before));
    !matches!(ident_at(trees, at), Some(ident) if BEFORE_ROOTED.contains(&ident.to_string().as_str()))
        && !after_separator(trees, at)
        && !is_punct(before, '.')
        && !is_punct(before, '\'')
        && !is_punct(before, '$')
}

/// Scans the tokens of one module or block, and the groups inside it, in
/// the scope `outer`.
fn level(stream: TokenStream, outer: &Scope, allowed: &[Category], hits: &mut BTreeSet<Hit>) {
    let trees: Vec<TokenTree> = stream.into_iter().collect();
    let (found, taken) = imports(&trees);
    let scope = enter(outer, &found);
    for import in &found {
        for target in scope.resolve(&import.segments, import.rooted) {
            let item = if import.glob {
                item_of_path(&target)
            } else {
                item_of_macro(&target).or_else(|| item_of_path(&target))
            };
            if let Some(item) = item {
                record(item, import.line, allowed, hits);
            }
        }
    }
    for (at, tree) in trees.iter().enumerate() {
        if let (TokenTree::Group(group), false) = (tree, taken[at]) {
            level(group.stream(), &scope, allowed, hits);
        }
    }
    let mut at = 0;
    while at < trees.len() {
        if taken[at] {
            at += 1;
            continue;
        }
        let start = match &trees[at] {
            TokenTree::Punct(punct)
                if punct.as_char() == ':'
                    && is_separator(&trees, at)
                    && starts_rooted(&trees, at) =>
            {
                Some((at + 2, true))
            }
            TokenTree::Ident(_) if starts_path(&trees, at) => Some((at, false)),
            _ => None,
        };
        let Some((segments, end, line, rooted)) = start.and_then(|(first, rooted)| {
            path_at(&trees, first).map(|(segments, end, line)| (segments, end, line, rooted))
        }) else {
            at += 1;
            continue;
        };
        let called = is_punct(trees.get(end), '!')
            && matches!(trees.get(end + 1), Some(TokenTree::Group(_)));
        let mut targets = scope.resolve(&segments, rooted);
        if called && !rooted && segments.len() == 1 && !scope.names.contains_key(&segments[0]) {
            // The macros of the standard library's prelude.
            targets.insert(vec!["std".to_string(), segments[0].clone()]);
        }
        for target in targets {
            let item = if called {
                item_of_macro(&target)
            } else {
                item_of_path(&target)
            };
            if let Some(item) = item {
                record(item, line, allowed, hits);
            }
        }
        at = end;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::source;

    /// The hits in `text` for a crate with `allowed`, as (line, item).
    fn hits(text: &str, allowed: &[Category]) -> Vec<(usize, String)> {
        scan(source::tokens(text).unwrap(), allowed)
            .into_iter()
            .map(|hit| (hit.line, hit.item))
            .collect()
    }

    fn at(line: usize, item: &str) -> (usize, String) {
        (line, item.to_string())
    }

    #[test]
    fn every_category_has_a_name_and_allows_an_item() {
        for category in Category::ALL {
            assert_eq!(Category::named(category.name()), Some(category));
            assert!(ITEMS.iter().any(|item| item.category == Some(category)));
        }
        assert_eq!(Category::named("windows"), None);
    }

    #[test]
    fn the_longest_item_decides() {
        let path = |text: &str| text.split("::").map(str::to_string).collect::<Vec<_>>();
        let shown = |text: &str| item_of_path(&path(text)).map(Item::shown);
        assert_eq!(
            shown("std::env::args_os"),
            Some("std::env::args_os".to_string())
        );
        assert_eq!(shown("std::env::vars"), Some("std::env".to_string()));
        assert_eq!(
            shown("std::process::ExitCode::SUCCESS"),
            Some("std::process::ExitCode".to_string())
        );
        assert_eq!(shown("std::time::Duration"), None);
        assert_eq!(shown("std::envy"), None);
    }

    #[test]
    fn follows_imports_through_one_another_in_any_order() {
        let text = "use io::Write;\nuse std::io;\nfn f(w: &mut impl Write) {}\n";
        assert_eq!(
            hits(text, &[]),
            [at(1, "std::io"), at(2, "std::io"), at(3, "std::io")]
        );
    }

    #[test]
    fn reads_paths_after_generic_arguments_and_keywords() {
        let text = "fn f() -> ::std::io::Result<()> {\n    let v = Vec::<u8>::new();\n    let p = <::std::fs::File as Clone>::clone;\n    return ::std::process::exit(1);\n}\n";
        assert_eq!(
            hits(text, &[]),
            [
                at(1, "std::io"),
                at(3, "std::fs"),
                at(4, "std::process::exit")
            ]
        );
        assert_eq!(
            hits(text, &[Category::ExitCode, Category::StandardStreams]),
            [at(3, "std::fs")]
        );
    }

    #[test]
    fn methods_fields_lifetimes_and_macro_variables_are_not_paths() {
        let text = "use std::env::args;\nfn f<'args>(x: &'args S) {\n    x.args();\n    let _ = S { args: 1 };\n    m!($args);\n}\n";
        // Only the import, and the field name in the struct expression,
        // which the scan reads as the imported name.
        assert_eq!(
            hits(text, &[]),
            [at(1, "std::env::args"), at(4, "std::env::args")]
        );
    }

    #[test]
    fn an_inner_import_hides_an_outer_one() {
        let text = "use std::fs as files;\nmod local {\n    use crate::store as files;\n    fn f() {\n        files::read();\n    }\n}\n";
        assert_eq!(hits(text, &[]), [at(1, "std::fs")]);
    }

    #[test]
    fn glob_imports_bring_in_the_items_of_their_module() {
        let text = "use std::time::*;\nuse kalyx::*;\nfn f() {\n    let _ = Instant::now();\n    let _ = Duration::ZERO;\n    let _ = Context::new(l, c);\n}\n";
        assert_eq!(
            hits(text, &[]),
            [at(4, "std::time::Instant"), at(6, "kalyx::Context::new")]
        );
    }

    #[test]
    fn extern_crate_renames_are_followed() {
        let text = "extern crate std as s;\nfn f() {\n    s::thread::yield_now();\n}\n";
        assert_eq!(hits(text, &[]), [at(3, "std::thread")]);
    }

    #[test]
    fn paths_that_macros_assemble_and_reexported_items_are_not_found() {
        // The limits of the scan: a path built from a macro variable, and a
        // forbidden item that another crate re-exports.
        let text = "macro_rules! read {\n    ($krate:ident) => {\n        $krate::fs::read(\"x\")\n    };\n}\nfn f() {\n    let _ = read!(std);\n    let _ = other::Instant::now();\n}\n";
        assert_eq!(hits(text, &[]), Vec::<(usize, String)>::new());
    }
}
