//! The platform boundary: the paths, macros and types that a core library
//! never uses, the platform categories that allow some of them to the
//! crates registered for them, and the scan that finds them in the tokens
//! of a Rust file.
//!
//! The scan reads tokens, so comments, doc comments and the contents of
//! string literals never match, while the bodies of macro calls are read
//! like any other tokens. It follows the names that `use` declarations,
//! `extern crate` items and type aliases bring into a module or block, with
//! their renames and glob imports, into the blocks and modules inside it,
//! paths from the crate root written with a leading `::`, paths that start
//! with `self`, `Self` in an `impl` for the type it implements, and
//! qualified paths such as `<kalyx::Context>::new`. An item hides a name
//! that a glob import or an outer module or block brings in, a module or
//! type for any path and a function, constant or static for a path of one
//! segment, and a `macro_rules!` macro hides the macro of the same name
//! after its definition. The scan counts a name imported in a module also
//! in the modules inside it, which Rust does not do, and reads names that
//! are not paths, such as variables, fields and enum variants, as paths; a
//! name of either kind that equals an import's is read as the import.

use std::borrow::Cow;
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
    /// Files and paths.
    Files,
}

impl Category {
    /// Every category, in the order of the table's comments.
    pub const ALL: [Category; 6] = [
        Category::StandardStreams,
        Category::Arguments,
        Category::ExitCode,
        Category::RealClock,
        Category::Threads,
        Category::Files,
    ];

    /// The name that the table uses.
    pub fn name(self) -> &'static str {
        match self {
            Category::StandardStreams => "standard-streams",
            Category::Arguments => "arguments",
            Category::ExitCode => "exit-code",
            Category::RealClock => "real-clock",
            Category::Threads => "threads",
            Category::Files => "files",
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
/// the rest of `std::env` is allowed by no category. The items are listed
/// under each path that the standard library of the pinned toolchain and
/// kalyx 0.1.0 make public, and kalyx's `MonotonicClock` also under the
/// crate root, as the platform boundary names it; the macros' paths
/// through `std::prelude` are read as their paths from the crate root.
const ITEMS: [Item; 31] = [
    path(&["std", "fs"], Some(Category::Files)),
    path(&["std", "path"], Some(Category::Files)),
    path(&["std", "ffi", "OsStr"], Some(Category::Arguments)),
    path(&["std", "ffi", "OsString"], Some(Category::Arguments)),
    path(
        &["std", "ffi", "os_str", "OsStr"],
        Some(Category::Arguments),
    ),
    path(
        &["std", "ffi", "os_str", "OsString"],
        Some(Category::Arguments),
    ),
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
const BEFORE_ROOTED: [&str; 23] = [
    "as", "box", "break", "const", "dyn", "else", "for", "if", "impl", "in", "let", "match",
    "move", "mut", "pub", "ref", "return", "static", "type", "unsafe", "where", "while", "yield",
];

/// The keywords that start an item which defines a name in the type or
/// value namespace.
const ITEM_KEYWORDS: [&str; 9] = [
    "const", "enum", "fn", "mod", "static", "struct", "trait", "type", "union",
];

/// Keywords that may come between the start of an item and its keyword.
const QUALIFIERS: [&str; 8] = [
    "async", "auto", "const", "default", "extern", "pub", "safe", "unsafe",
];

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
    let rules = Rules {
        items: &ITEMS,
        allowed,
    };
    let mut hits = BTreeSet::new();
    level(stream, &Scope::default(), false, &rules, &mut hits);
    hits
}

/// The functions through which a test would take time from the host
/// instead of an injected clock.
const CLOCK_READS: [Item; 2] = [
    path(&["std", "thread", "sleep"], None),
    path(&["std", "time", "SystemTime", "now"], None),
];

/// The uses of `std::thread::sleep` and `std::time::SystemTime::now` in
/// `stream`, found as the scan finds forbidden items; `SystemTime` used as
/// a type is not one of them.
pub fn clock_reads(stream: TokenStream) -> BTreeSet<Hit> {
    let rules = Rules {
        items: &CLOCK_READS,
        allowed: &[],
    };
    let mut hits = BTreeSet::new();
    level(stream, &Scope::default(), false, &rules, &mut hits);
    hits
}

/// What a scan looks for: its items, and the categories whose items it
/// lets pass.
#[derive(Debug)]
struct Rules<'a> {
    items: &'a [Item],
    allowed: &'a [Category],
}

/// The longest path item of `items` that `path`, from a crate root,
/// starts with.
fn item_of_path<'a>(items: &'a [Item], path: &[String]) -> Option<&'a Item> {
    items
        .iter()
        .filter(|item| {
            item.form == Form::Path
                && path.len() >= item.path.len()
                && item.path.iter().zip(path).all(|(a, b)| a == b)
        })
        .max_by_key(|item| item.path.len())
}

/// The macro item of `items` whose path is `path`, written from the crate
/// root or through one of the preludes in `std::prelude`.
fn item_of_macro<'a>(items: &'a [Item], path: &[String]) -> Option<&'a Item> {
    let path: Vec<&str> = match path {
        [krate, prelude, _, name] if krate == "std" && prelude == "prelude" => {
            vec![krate.as_str(), name.as_str()]
        }
        _ => path.iter().map(String::as_str).collect(),
    };
    items
        .iter()
        .find(|item| item.form == Form::Macro && item.path == path.as_slice())
}

/// What the names of a module or block stand for. Depths count the modules
/// and blocks around a name, from 1 for a file.
#[derive(Debug, Clone, Default)]
struct Scope {
    /// The depth of this module or block.
    depth: usize,
    /// The paths from a crate root that each imported name may stand for,
    /// with the depth of the module or block that imports it. Inside an
    /// `impl`, `Self` stands for the type that it implements.
    names: BTreeMap<String, (usize, BTreeSet<Vec<String>>)>,
    /// The names that items define in the type namespace, of modules,
    /// types and traits, with the depth of the innermost module or block
    /// that defines each.
    items: BTreeMap<String, usize>,
    /// The names that items define in the value namespace, of functions,
    /// constants, statics and the constructors of structs, with depths as
    /// for `items`.
    values: BTreeMap<String, usize>,
    /// The paths from a crate root whose items glob imports bring in,
    /// when those paths are not forbidden items themselves, with the depth
    /// of the innermost module or block that imports each.
    globs: BTreeMap<Vec<String>, usize>,
    /// The macros that `macro_rules!` defines before this point, here or
    /// in a module or block around it.
    macros: BTreeSet<String>,
}

/// Where a path is read, which decides what may hide its first segment
/// and which glob imports it goes through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Site {
    /// In code. A path of one segment may name a value, which a function,
    /// constant or static hides; a longer path starts with a module, a type
    /// or a crate.
    Code,
    /// In an import other than a glob import, whose path starts with a
    /// module, a type or a crate.
    Import,
    /// In an import other than a glob import, before the glob imports of
    /// its own module or block are known: as [`Site::Import`], but only
    /// through the glob imports of the modules and blocks around it.
    ImportOutside,
    /// In a glob import, which does not go through the glob imports of its
    /// own module or block: they would only extend one another.
    Glob,
}

impl Scope {
    /// The paths from a crate root that `segments`, read at `site`, may
    /// stand for. A path that starts with `crate`, `super` or `Self`
    /// outside an `impl` stands for none; `self::rest` stands for what
    /// `rest` does here. An import or an item that defines the first
    /// segment hides the glob imports of its own module or block and of
    /// those around it; a first segment that nothing hides goes through
    /// the glob imports that `site` reads, and also stands for itself when
    /// it names a crate that the scan follows.
    fn resolve(&self, segments: &[String], rooted: bool, site: Site) -> BTreeSet<Vec<String>> {
        if segments.is_empty() {
            return BTreeSet::new();
        }
        let joined = |prefix: &Vec<String>, rest: &[String]| {
            prefix.iter().chain(rest).cloned().collect::<Vec<String>>()
        };
        if rooted {
            return BTreeSet::from([segments.to_vec()]);
        }
        // `self::rest` names what `rest` does here; the scope of a block
        // stands in for that of its module.
        let selves = segments
            .iter()
            .take_while(|segment| *segment == "self")
            .count()
            .min(segments.len() - 1);
        let segments = &segments[selves..];
        let first = &segments[0];
        let imported = self.names.get(first);
        if imported.is_none() && LOCAL.contains(&first.as_str()) {
            return BTreeSet::new();
        }
        let mut defined = self.items.get(first).copied();
        if site == Site::Code && segments.len() == 1 {
            defined = defined.max(self.values.get(first).copied());
        }
        let hidden_to = imported.map(|(depth, _)| *depth).max(defined);
        let globs_below = if matches!(site, Site::Glob | Site::ImportOutside) {
            self.depth
        } else {
            usize::MAX
        };
        let mut targets: BTreeSet<Vec<String>> = self
            .globs
            .iter()
            .filter(|(_, depth)| {
                **depth < globs_below && hidden_to.is_none_or(|hidden| **depth > hidden)
            })
            .map(|(glob, _)| joined(glob, segments))
            .collect();
        match imported {
            Some((depth, paths)) if Some(*depth) >= defined => {
                targets.extend(paths.iter().map(|target| joined(target, &segments[1..])));
            }
            None if defined.is_none() && CRATES.contains(&first.as_str()) => {
                targets.insert(segments.to_vec());
            }
            _ => {}
        }
        targets
    }
}

/// An import of a `use` declaration, an `extern crate` item or a type
/// alias.
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

/// What the items of one module or block declare.
#[derive(Debug)]
struct Declarations {
    imports: Vec<Import>,
    /// Which tokens the scan skips: those of the imports, and the names
    /// that items define.
    taken: Vec<bool>,
    /// The names that items define in the type namespace.
    items: BTreeSet<String>,
    /// The names that items define in the value namespace.
    values: BTreeSet<String>,
    /// The macros that `macro_rules!` defines, with the index of the
    /// definition's first token.
    macros: Vec<(usize, String)>,
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

fn is_keyword(trees: &[TokenTree], at: usize, keywords: &[&str]) -> bool {
    matches!(ident_at(trees, at), Some(ident) if keywords.contains(&ident.to_string().as_str()))
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

/// Whether the keyword at `at` starts an item: only attributes, a
/// visibility and qualifiers come before it since the start of the module
/// or block, a `;` or a `{ ... }`.
fn starts_item(trees: &[TokenTree], at: usize) -> bool {
    let mut at = at;
    loop {
        let Some(before) = at.checked_sub(1) else {
            return true;
        };
        let earlier = |back: usize| before.checked_sub(back);
        match &trees[before] {
            TokenTree::Punct(punct) if punct.as_char() == ';' => return true,
            TokenTree::Group(group) if group.delimiter() == Delimiter::Brace => return true,
            TokenTree::Ident(_) if is_keyword(trees, before, &QUALIFIERS) => at = before,
            TokenTree::Group(group)
                if group.delimiter() == Delimiter::Parenthesis
                    && earlier(1).is_some_and(|previous| is_keyword(trees, previous, &["pub"])) =>
            {
                at = before;
            }
            TokenTree::Literal(_)
                if earlier(1).is_some_and(|previous| is_keyword(trees, previous, &["extern"])) =>
            {
                at = before;
            }
            TokenTree::Group(group) if group.delimiter() == Delimiter::Bracket => {
                // An outer attribute `#[...]` or an inner one `#![...]`.
                match (earlier(1), earlier(2)) {
                    (Some(hash), _) if is_punct(trees.get(hash), '#') => at = hash,
                    (Some(bang), Some(hash))
                        if is_punct(trees.get(bang), '!') && is_punct(trees.get(hash), '#') =>
                    {
                        at = hash;
                    }
                    _ => return false,
                }
            }
            _ => return false,
        }
    }
}

/// What a `{ ... }` is the body of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Body {
    /// A block, a module or an item whose names are in scope inside it.
    Block,
    /// A trait, whose associated items are not in scope inside it.
    Trait,
    /// An `impl`, whose associated items are not in scope inside it, with
    /// the index of its keyword `impl`.
    Impl(usize),
}

/// What the `{ ... }` at `at` is the body of.
fn body_of(trees: &[TokenTree], body: usize) -> Body {
    // The item starts after the `;` or the `{ ... }` before it; a `{ ... }`
    // inside angle brackets, as in `impl W<{ 1 }>`, is a generic argument.
    let mut angles = 0_usize;
    let mut start = 0;
    for before in (0..body).rev() {
        match &trees[before] {
            TokenTree::Punct(punct) if punct.as_char() == '>' && !is_arrow(trees, before) => {
                angles += 1;
            }
            TokenTree::Punct(punct) if punct.as_char() == '<' => {
                angles = angles.saturating_sub(1);
            }
            TokenTree::Punct(punct) if punct.as_char() == ';' => {
                start = before + 1;
                break;
            }
            TokenTree::Group(group) if group.delimiter() == Delimiter::Brace && angles == 0 => {
                start = before + 1;
                break;
            }
            _ => {}
        }
    }
    let mut at = start;
    loop {
        match trees.get(at) {
            Some(TokenTree::Punct(hash)) if hash.as_char() == '#' => {
                at += if is_punct(trees.get(at + 1), '!') {
                    3
                } else {
                    2
                };
            }
            Some(TokenTree::Ident(ident)) if ident == "pub" => {
                at += 1;
                if matches!(trees.get(at), Some(TokenTree::Group(group)) if group.delimiter() == Delimiter::Parenthesis)
                {
                    at += 1;
                }
            }
            Some(TokenTree::Ident(_)) if is_keyword(trees, at, &QUALIFIERS) => at += 1,
            _ if is_keyword(trees, at, &["impl"]) && header_reaches(trees, at, body) => {
                return Body::Impl(at);
            }
            _ if is_keyword(trees, at, &["trait"]) && header_reaches(trees, at, body) => {
                return Body::Trait;
            }
            _ => return Body::Block,
        }
    }
}

/// Whether the header of the item whose keyword is at `keyword` reaches
/// the `{ ... }` at `body`: no other `{ ... }` outside angle brackets comes
/// between them, as the body of a local `impl` before an `if n > 0 { ... }`
/// would.
fn header_reaches(trees: &[TokenTree], keyword: usize, body: usize) -> bool {
    let mut angles = 0_usize;
    for at in keyword..body {
        match &trees[at] {
            TokenTree::Punct(punct) if punct.as_char() == '<' => angles += 1,
            TokenTree::Punct(punct) if punct.as_char() == '>' && !is_arrow(trees, at) => {
                angles = angles.saturating_sub(1);
            }
            TokenTree::Group(group) if group.delimiter() == Delimiter::Brace && angles == 0 => {
                return false;
            }
            _ => {}
        }
    }
    true
}

/// The path of the type that the `impl` whose keyword is at `keyword` and
/// whose body is at `body` implements, and whether it starts with `::`:
/// the path after `for`, or after the generic parameters without a trait.
fn self_type(trees: &[TokenTree], keyword: usize, body: usize) -> Option<(Vec<String>, bool)> {
    let mut at = keyword + 1;
    if is_punct(trees.get(at), '<') {
        // The generic parameters, `impl<...>`.
        let mut angles = 0_usize;
        while at < body {
            match &trees[at] {
                TokenTree::Punct(punct) if punct.as_char() == '<' => angles += 1,
                TokenTree::Punct(punct) if punct.as_char() == '>' && !is_arrow(trees, at) => {
                    angles = angles.saturating_sub(1);
                    if angles == 0 {
                        at += 1;
                        break;
                    }
                }
                _ => {}
            }
            at += 1;
        }
    }
    let mut start = at;
    let mut angles = 0_usize;
    for index in at..body {
        match &trees[index] {
            TokenTree::Punct(punct) if punct.as_char() == '<' => angles += 1,
            TokenTree::Punct(punct) if punct.as_char() == '>' && !is_arrow(trees, index) => {
                angles = angles.saturating_sub(1);
            }
            TokenTree::Ident(ident) if angles == 0 && ident == "for" => start = index + 1,
            TokenTree::Ident(ident) if angles == 0 && ident == "where" => break,
            _ => {}
        }
    }
    let rooted = is_separator(trees, start);
    let (segments, _, _) = path_at(trees, if rooted { start + 2 } else { start })?;
    Some((segments, rooted))
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

/// The path at `at` when nothing but a `;` follows it, as in a type alias
/// `type Name = path;`: its segments, whether it starts with `::`, its
/// line and the index of the `;`.
fn plain_path(trees: &[TokenTree], at: usize) -> Option<(Vec<String>, bool, usize, usize)> {
    let rooted = is_separator(trees, at);
    let (segments, end, line) = path_at(trees, if rooted { at + 2 } else { at })?;
    is_punct(trees.get(end), ';').then_some((segments, rooted, line, end))
}

/// The declarations of the items `trees` of one module or block, or of the
/// associated items of an `impl` or a trait, which define no names in
/// scope. The names that items define are marked as taken, since they are
/// not paths.
fn declarations(trees: &[TokenTree], associated: bool) -> Declarations {
    let mut found = Declarations {
        imports: Vec::new(),
        taken: vec![false; trees.len()],
        items: BTreeSet::new(),
        values: BTreeSet::new(),
        macros: Vec::new(),
    };
    let mut at = 0;
    while at < trees.len() {
        let Some(keyword) = ident_at(trees, at).map(ToString::to_string) else {
            at += 1;
            continue;
        };
        let semicolon = || {
            trees[at..]
                .iter()
                .position(|tree| is_punct(Some(tree), ';'))
                .map(|offset| at + offset)
        };
        match keyword.as_str() {
            "macro_rules" if is_punct(trees.get(at + 1), '!') => {
                if let Some(name) = ident_at(trees, at + 2) {
                    found.macros.push((at, unraw(name)));
                }
            }
            "use" if starts_item(trees, at) => {
                if let Some(end) = semicolon() {
                    use_tree(&trees[at + 1..end], &[], false, &mut found.imports);
                    found.taken[at..=end].fill(true);
                    at = end + 1;
                    continue;
                }
            }
            "extern" if is_keyword(trees, at + 1, &["crate"]) => {
                if let Some(end) = semicolon() {
                    if let Some(krate) = ident_at(trees, at + 2) {
                        let alias = match (ident_at(trees, at + 3), ident_at(trees, at + 4)) {
                            (Some(keyword), Some(alias)) if keyword == "as" => unraw(alias),
                            _ => unraw(krate),
                        };
                        found.imports.push(Import {
                            segments: vec![unraw(krate)],
                            rooted: true,
                            name: (alias != "_").then_some(alias),
                            glob: false,
                            line: krate.span().start().line,
                        });
                    }
                    found.taken[at..=end].fill(true);
                    at = end + 1;
                    continue;
                }
            }
            _ if ITEM_KEYWORDS.contains(&keyword.as_str()) && starts_item(trees, at) => {
                let mut name_at = at + 1;
                if keyword == "static" && is_keyword(trees, name_at, &["mut"]) {
                    name_at += 1;
                }
                let name = ident_at(trees, name_at)
                    .map(unraw)
                    .filter(|name| name != "_")
                    .filter(|name| {
                        keyword != "const"
                            || !["async", "extern", "fn", "unsafe"].contains(&name.as_str())
                    });
                let Some(name) = name else {
                    at += 1;
                    continue;
                };
                // The name that an item defines is not a path.
                found.taken[name_at] = true;
                let alias =
                    (!associated && keyword == "type" && is_punct(trees.get(name_at + 1), '='))
                        .then(|| plain_path(trees, name_at + 2))
                        .flatten();
                if let Some((segments, rooted, line, end)) = alias {
                    found.imports.push(Import {
                        segments,
                        rooted,
                        name: Some(name),
                        glob: false,
                        line,
                    });
                    found.taken[at..=end].fill(true);
                    at = end + 1;
                    continue;
                }
                if !associated {
                    if matches!(keyword.as_str(), "fn" | "const" | "static" | "struct") {
                        found.values.insert(name.clone());
                    }
                    if !matches!(keyword.as_str(), "fn" | "const" | "static") {
                        found.items.insert(name);
                    }
                }
            }
            _ => {}
        }
        at += 1;
    }
    found
}

/// The scope of a module or block inside `outer` with the declarations
/// `found`, whose imports resolve through one another and through `outer`.
/// A glob import of an item of `items` is a use of the item, so it brings
/// in no names.
fn enter(outer: &Scope, found: &Declarations, items: &[Item]) -> Scope {
    let depth = outer.depth + 1;
    let mut base = outer.clone();
    base.depth = depth;
    for name in &found.items {
        base.items.insert(name.clone(), depth);
    }
    for name in &found.values {
        base.values.insert(name.clone(), depth);
    }
    for name in found
        .imports
        .iter()
        .filter_map(|import| import.name.as_ref())
    {
        base.names.remove(name);
    }
    let mut scope = base.clone();
    // First every import reads only the glob imports around its module or
    // block, so that this level's glob imports never grow from names that
    // came through them; then the other imports also read this level's
    // glob imports, which stay as they are. In each phase a pass resolves
    // one more import of a chain of imports through one another, and the
    // bound ends the cycles of imports, which only code that does not
    // compile has.
    for outside in [true, false] {
        for _ in 0..=found.imports.len() {
            let mut next = base.clone();
            if !outside {
                next.globs = scope.globs.clone();
            }
            for import in &found.imports {
                if import.glob && !outside {
                    continue;
                }
                let targets = import_targets(&scope, outer, &found.imports, import, outside);
                if import.glob {
                    for target in targets {
                        if item_of_path(items, &target).is_none() {
                            next.globs.insert(target, depth);
                        }
                    }
                } else if let Some(name) = &import.name {
                    next.names
                        .entry(name.clone())
                        .or_insert_with(|| (depth, BTreeSet::new()))
                        .1
                        .extend(targets);
                }
            }
            let settled = next.names == scope.names && next.globs == scope.globs;
            scope = next;
            if settled {
                break;
            }
        }
    }
    scope
}

/// The name that `import` brings in when its path starts with that name.
fn own_name(import: &Import) -> Option<&String> {
    import
        .name
        .as_ref()
        .filter(|name| !import.rooted && import.segments.first() == Some(*name))
}

/// The paths from a crate root that `import`, one of the imports `imports`
/// of a module or block with the scope `scope` inside `outer`, may stand
/// for; `outside` reads only the glob imports around the module or block.
/// An import whose path starts with the name it brings in, as
/// `use time::Instant as time;`, reads that name both as the other imports
/// of the module or block that bring it in do and as it is around it.
fn import_targets(
    scope: &Scope,
    outer: &Scope,
    imports: &[Import],
    import: &Import,
    outside: bool,
) -> BTreeSet<Vec<String>> {
    let site = match (import.glob, outside) {
        (true, _) => Site::Glob,
        (false, true) => Site::ImportOutside,
        (false, false) => Site::Import,
    };
    let Some(name) = own_name(import) else {
        return scope.resolve(&import.segments, import.rooted, site);
    };
    let others: Vec<&Import> = imports
        .iter()
        .filter(|other| {
            !other.glob && other.name.as_ref() == Some(name) && own_name(other).is_none()
        })
        .collect();
    let outside = outer.names.get(name);
    let mut around = scope.clone();
    match (others.is_empty(), outside) {
        (true, None) => {
            around.names.remove(name);
        }
        (true, Some(entry)) => {
            around.names.insert(name.clone(), entry.clone());
        }
        (false, _) => {
            let mut targets: BTreeSet<Vec<String>> = others
                .iter()
                .flat_map(|other| scope.resolve(&other.segments, other.rooted, site))
                .collect();
            if let Some((_, paths)) = outside {
                targets.extend(paths.iter().cloned());
            }
            around.names.insert(name.clone(), (scope.depth, targets));
        }
    }
    around.resolve(&import.segments, false, site)
}

/// Records `item`, used at `line`, unless the categories of `rules` allow
/// it.
fn record(item: &Item, line: usize, rules: &Rules, hits: &mut BTreeSet<Hit>) {
    if item
        .category
        .is_some_and(|category| rules.allowed.contains(&category))
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
/// continuing one, as after generic arguments: it does after a keyword, a
/// lifetime, `->`, `=>`, the generic parameters of an `impl` or a
/// `for<...>`, and a comparison or shift `>`.
fn starts_rooted(trees: &[TokenTree], at: usize) -> bool {
    let token = |back: usize| at.checked_sub(back).and_then(|before| trees.get(before));
    match token(1) {
        Some(TokenTree::Ident(ident)) => {
            BEFORE_ROOTED.contains(&ident.to_string().as_str()) || is_punct(token(2), '\'')
        }
        Some(TokenTree::Punct(punct)) if punct.as_char() == '>' => {
            is_arrow(trees, at - 1) || roots_after_angle(trees, at - 1)
        }
        _ => true,
    }
}

/// Whether the `>` at `at` is the end of `->` or `=>`.
fn is_arrow(trees: &[TokenTree], at: usize) -> bool {
    matches!(
        at.checked_sub(1).and_then(|before| trees.get(before)),
        Some(TokenTree::Punct(arrow))
            if matches!(arrow.as_char(), '-' | '=') && arrow.spacing() == Spacing::Joint
    )
}

/// Whether a `::` after the `>` at `close` starts a path from the crate
/// root: it does when the `>` ends the generic parameters of an `impl` or
/// a `for<...>`, or when no `<` before it in the statement matches it, as
/// after a comparison or a shift, and does not after generic arguments.
fn roots_after_angle(trees: &[TokenTree], close: usize) -> bool {
    let mut depth = 0_usize;
    for at in (0..=close).rev() {
        match &trees[at] {
            TokenTree::Punct(punct) if punct.as_char() == '>' && !is_arrow(trees, at) => {
                depth += 1;
            }
            TokenTree::Punct(punct) if punct.as_char() == '<' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return at
                        .checked_sub(1)
                        .is_some_and(|before| is_keyword(trees, before, &["impl", "for"]));
                }
            }
            TokenTree::Punct(punct) if punct.as_char() == ';' => return true,
            TokenTree::Group(group) if group.delimiter() == Delimiter::Brace => return true,
            _ => {}
        }
    }
    true
}

/// Whether the identifier at `at` starts a path: it is not one of the
/// keywords after which `::` starts a path from a crate root, and it does
/// not follow `::`, a `.` of a field or method, the `'` of a lifetime or
/// label, or the `$` of a macro variable.
fn starts_path(trees: &[TokenTree], at: usize) -> bool {
    let before = at.checked_sub(1).and_then(|before| trees.get(before));
    !is_keyword(trees, at, &BEFORE_ROOTED)
        && !after_separator(trees, at)
        && !is_punct(before, '.')
        && !is_punct(before, '\'')
        && !is_punct(before, '$')
}

/// Scans the tokens of one module or block, or of the associated items of
/// an `impl` or a trait, and the groups inside it, in the scope `outer`.
fn level(
    stream: TokenStream,
    outer: &Scope,
    associated: bool,
    rules: &Rules,
    hits: &mut BTreeSet<Hit>,
) {
    let trees: Vec<TokenTree> = stream.into_iter().collect();
    let found = declarations(&trees, associated);
    let scope = enter(outer, &found, rules.items);
    for import in &found.imports {
        for target in import_targets(&scope, outer, &found.imports, import, false) {
            let item = if import.glob {
                item_of_path(rules.items, &target)
            } else {
                item_of_macro(rules.items, &target).or_else(|| item_of_path(rules.items, &target))
            };
            if let Some(item) = item {
                record(item, import.line, rules, hits);
            }
        }
    }
    // A `macro_rules!` macro is defined for the tokens after it.
    let defined_before = |at: usize, name: &str| {
        scope.macros.contains(name)
            || found
                .macros
                .iter()
                .any(|(index, defined)| *index < at && defined == name)
    };
    let mut inner = Cow::Borrowed(&scope);
    let mut definitions = found.macros.iter().peekable();
    for (at, tree) in trees.iter().enumerate() {
        while let Some((_, name)) = definitions.next_if(|(index, _)| *index < at) {
            inner.to_mut().macros.insert(name.clone());
        }
        let (TokenTree::Group(group), false) = (tree, found.taken[at]) else {
            continue;
        };
        let body = if group.delimiter() == Delimiter::Brace {
            body_of(&trees, at)
        } else {
            Body::Block
        };
        match body {
            Body::Block => level(group.stream(), &inner, false, rules, hits),
            Body::Trait => level(group.stream(), &inner, true, rules, hits),
            Body::Impl(keyword) => {
                // Inside the `impl`, `Self` stands for the type it implements.
                let targets = self_type(&trees, keyword, at)
                    .map(|(segments, rooted)| inner.resolve(&segments, rooted, Site::Import))
                    .unwrap_or_default();
                let mut own = Scope::clone(&inner);
                own.names
                    .insert("Self".to_string(), (inner.depth + 1, targets));
                level(group.stream(), &own, true, rules, hits);
            }
        }
    }
    let mut at = 0;
    while at < trees.len() {
        if found.taken[at] {
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
        let Some((first, rooted)) = start else {
            at += 1;
            continue;
        };
        let Some((mut segments, mut end, line)) = path_at(&trees, first) else {
            at += 1;
            continue;
        };
        // A qualified path without `as`, `<path>::rest`, names what
        // `path::rest` names; after `::` the `<` starts generic arguments
        // instead.
        let open = at.checked_sub(1);
        if open.is_some_and(|open| is_punct(trees.get(open), '<') && !after_separator(&trees, open))
            && is_punct(trees.get(end), '>')
            && is_separator(&trees, end + 1)
            && let Some((rest, after, _)) = path_at(&trees, end + 3)
        {
            segments.extend(rest);
            end = after;
        }
        let called = is_punct(trees.get(end), '!')
            && matches!(trees.get(end + 1), Some(TokenTree::Group(_)));
        let mut targets = scope.resolve(&segments, rooted, Site::Code);
        if called
            && !rooted
            && segments.len() == 1
            && !scope.names.contains_key(&segments[0])
            && !defined_before(at, &segments[0])
        {
            // The macros of the standard library's prelude.
            targets.insert(vec!["std".to_string(), segments[0].clone()]);
        }
        for target in targets {
            let item = if called {
                item_of_macro(rules.items, &target)
            } else {
                item_of_path(rules.items, &target)
            };
            if let Some(item) = item {
                record(item, line, rules, hits);
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
        let shown = |text: &str| item_of_path(&ITEMS, &path(text)).map(Item::shown);
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
    fn precise_capturing_is_not_an_import() {
        let text = "fn f<'a>(x: &'a u8) -> impl Sized + use<'a> {\n    x\n}\npub fn g() {\n    let _ = std::fs::read(\"x\");\n}\nconst N: u8 = 1;\n";
        assert_eq!(hits(text, &[]), [at(5, "std::fs")]);
    }

    #[test]
    fn every_public_path_of_an_item_is_found() {
        let text = "use std::ffi::os_str::OsString;\nfn f(s: &std::ffi::os_str::OsStr) {\n    std::prelude::v1::println!(\"x\");\n}\nuse std::prelude::rust_2024::thread_local as local;\nlocal! {\n    static X: u8 = 0;\n}\n";
        assert_eq!(
            hits(text, &[]),
            [
                at(1, "std::ffi::os_str::OsString"),
                at(2, "std::ffi::os_str::OsStr"),
                at(3, "println!"),
                at(5, "thread_local!"),
                at(6, "thread_local!")
            ]
        );
        let allowed = [
            Category::Arguments,
            Category::StandardStreams,
            Category::Threads,
        ];
        assert_eq!(hits(text, &allowed), Vec::<(usize, String)>::new());
    }

    #[test]
    fn reads_paths_from_the_root_after_lifetimes_parameters_and_keywords() {
        let text = "fn f<'a>(x: &'a ::std::fs::File) {\n    while ::std::env::var(\"X\").is_ok() {}\n}\nstruct S(pub ::std::path::PathBuf);\nimpl<T: Fn() -> u8> ::std::io::Write for W<T> {}\ntype R = Box<dyn for<'a> ::std::io::Read>;\n";
        assert_eq!(
            hits(text, &[]),
            [
                at(1, "std::fs"),
                at(2, "std::env"),
                at(4, "std::path"),
                at(5, "std::io"),
                at(6, "std::io")
            ]
        );
    }

    #[test]
    fn qualified_paths_and_type_aliases_are_followed() {
        let text = "use kalyx::Context;\ntype Made = kalyx::Context;\nfn f() {\n    let _ = <kalyx::Context>::new(l, c);\n    let _ = <Context>::new(l, c);\n    let _ = Made::new(l, c);\n    let _ = Vec::<kalyx::Context>::new();\n    let _ = <Context as Clone>::clone(&x);\n}\n";
        assert_eq!(
            hits(text, &[]),
            [
                at(4, "kalyx::Context::new"),
                at(5, "kalyx::Context::new"),
                at(6, "kalyx::Context::new")
            ]
        );
    }

    #[test]
    fn items_hide_glob_imports_and_outer_names() {
        // An item hides the glob imports of its own module or block and
        // of those around it.
        let text = "use std::time::*;\nuse kalyx::*;\nstruct Instant;\nstruct Context;\nimpl Context {\n    fn new() -> Context {\n        Context\n    }\n}\nfn f() {\n    let _ = Instant;\n    let _ = Context::new();\n}\n";
        assert_eq!(hits(text, &[]), Vec::<(usize, String)>::new());
        // A glob import inside a block reaches past an item outside it.
        let text =
            "struct Instant;\nfn f() {\n    use std::time::*;\n    let _ = Instant::now();\n}\n";
        assert_eq!(hits(text, &[]), [at(4, "std::time::Instant")]);
        // A function of an inner module hides an outer import; a function
        // of an `impl` does not.
        let text = "use std::env::args;\nmod inner {\n    fn args() {}\n    fn f() {\n        args();\n    }\n}\nstruct S;\nimpl S {\n    fn args() {}\n    fn g() {\n        let _ = args();\n    }\n}\n";
        assert_eq!(
            hits(text, &[]),
            [at(1, "std::env::args"), at(12, "std::env::args")]
        );
    }

    #[test]
    fn a_local_macro_hides_a_forbidden_one_after_its_definition() {
        // Only `macro_rules!` hides a macro; an item of the same name does
        // not.
        let text = "fn early() {\n    dbg!(1);\n}\nmacro_rules! dbg {\n    ($value:expr) => {\n        $value\n    };\n}\nfn late() {\n    dbg!(1);\n}\nstruct print;\nfn f() {\n    print!(\"x\");\n}\n";
        assert_eq!(hits(text, &[]), [at(2, "dbg!"), at(14, "print!")]);
    }

    #[test]
    fn glob_imports_of_one_level_do_not_extend_one_another() {
        let mut text = String::from("use std::collections::*;\nuse std::vec::*;\nuse kalyx::*;\n");
        for index in 0..12 {
            text.push_str(&format!("use other::item{index};\n"));
        }
        text.push_str("fn f() {\n    let _ = Context::new(l, c);\n}\n");
        assert_eq!(hits(&text, &[]), [at(17, "kalyx::Context::new")]);
    }

    #[test]
    fn chains_of_imports_resolve_to_their_end_and_cycles_end() {
        let renames: Vec<String> = (0..12)
            .map(|index| match index {
                0 => "use std as s0;\n".to_string(),
                _ => format!("use s{} as s{index};\n", index - 1),
            })
            .collect();
        let call = "fn f() {\n    s11::fs::read(\"x\");\n}\n";
        let forward = format!("{}{call}", renames.concat());
        let backward: String = renames.iter().rev().cloned().collect();
        assert_eq!(hits(&forward, &[]), [at(14, "std::fs")]);
        assert_eq!(hits(&format!("{backward}{call}"), &[]), [at(14, "std::fs")]);
        let cycle = "use a as b;\nuse b as a;\nfn f() {\n    a::fs::read(\"x\");\n}\n";
        assert_eq!(hits(cycle, &[]), Vec::<(usize, String)>::new());
    }

    #[test]
    fn functions_constants_and_statics_hide_only_values() {
        let text = "use std::fs;\nfn std() {}\npub fn f() {\n    let _ = fs::read(\"x\");\n    let _ = std::env::var(\"X\");\n    let _ = std();\n}\nfn kalyx() {}\nfn g() {\n    let _ = kalyx::Context::new(l, c);\n}\n";
        assert_eq!(
            hits(text, &[]),
            [
                at(1, "std::fs"),
                at(4, "std::fs"),
                at(5, "std::env"),
                at(10, "kalyx::Context::new")
            ]
        );
    }

    #[test]
    fn an_impl_with_a_block_among_its_generic_arguments_has_associated_items() {
        let text = "use std::time::*;\nstruct W<const N: u8>;\nimpl W<{ 1 }> {\n    pub const UNIX_EPOCH: u8 = 0;\n    pub fn g() {\n        let _ = UNIX_EPOCH;\n    }\n}\n";
        assert_eq!(hits(text, &[]), [at(6, "std::time::UNIX_EPOCH")]);
    }

    #[test]
    fn paths_through_self_and_the_self_type_of_an_impl_are_followed() {
        let text = "use kalyx::context::CancelFlag;\nuse kalyx::{Context, Limits};\nuse std::time::*;\npub struct Settings(pub Limits, pub CancelFlag);\nimpl From<Settings> for Context {\n    fn from(settings: Settings) -> Self {\n        let _ = <Self>::new(settings.0, settings.1);\n        Self::new(settings.0, settings.1)\n    }\n}\nimpl Settings {\n    fn made() -> Self {\n        Self::new()\n    }\n}\npub fn f() {\n    let _ = self::Context::new(l, c);\n    let _ = self::Instant::now();\n}\n";
        assert_eq!(
            hits(text, &[]),
            [
                at(7, "kalyx::Context::new"),
                at(8, "kalyx::Context::new"),
                at(17, "kalyx::Context::new"),
                at(18, "std::time::Instant")
            ]
        );
    }

    #[test]
    fn a_path_from_the_root_after_a_comparison_or_a_shift_is_read() {
        let text = "pub fn f(n: usize) -> bool {\n    n > ::std::env::args().count()\n}\npub fn g(n: u32) -> u32 {\n    n >> ::std::process::id()\n}\n";
        assert_eq!(
            hits(text, &[]),
            [at(2, "std::env::args"), at(5, "std::process")]
        );
    }

    #[test]
    fn an_import_that_reuses_its_own_name_reads_the_name_around_it() {
        let text = "use std::time;\npub fn f() {\n    use time::Instant as time;\n    let _ = time::now();\n}\n";
        assert_eq!(
            hits(text, &[]),
            [at(3, "std::time::Instant"), at(4, "std::time::Instant")]
        );
    }

    #[test]
    fn glob_imports_through_imported_modules_settle() {
        let mut text = String::from(
            "use std::collections::btree_map;\nuse std::collections::hash_map;\nuse btree_map::*;\nuse hash_map::*;\nuse std::time;\nuse time::*;\n",
        );
        for index in 0..20 {
            text.push_str(&format!("use core::item{index};\n"));
        }
        text.push_str("fn f() {\n    let _ = Instant::now();\n}\n");
        assert_eq!(hits(&text, &[]), [at(28, "std::time::Instant")]);
    }

    #[test]
    fn a_block_after_a_local_impl_is_not_its_body() {
        let text = "use kalyx::Context;\nimpl From<u8> for Context {\n    fn from(n: u8) -> Self {\n        struct Floor;\n        impl Floor {\n            const VALUE: u8 = 0;\n        }\n        if n > Floor::VALUE {\n            type Made = kalyx::Context;\n            let _ = Made::new(l, c);\n            return Self::new(l, c);\n        }\n        Self::new(l, c)\n    }\n}\n";
        assert_eq!(
            hits(text, &[]),
            [
                at(10, "kalyx::Context::new"),
                at(11, "kalyx::Context::new"),
                at(13, "kalyx::Context::new")
            ]
        );
    }

    #[test]
    fn an_import_that_reuses_a_name_of_its_level_reads_the_other_import() {
        // The scan does not tell the module `time` from the constant
        // `time`, so it reads both in each path that starts with `time`.
        let text = "use std::time;\nuse time::UNIX_EPOCH as time;\npub fn f() -> time::Duration {\n    let _ = time;\n    time::Duration::ZERO\n}\n";
        let epoch = "std::time::UNIX_EPOCH";
        assert_eq!(
            hits(text, &[]),
            [at(2, epoch), at(3, epoch), at(4, epoch), at(5, epoch)]
        );
        // Through `super` the other import stands for nothing, so the name
        // is also read as it is around the module.
        let text = "use std::time;\nmod inner {\n    use super::time;\n    use time::Instant as time;\n}\n";
        assert_eq!(hits(text, &[]), [at(4, "std::time::Instant")]);
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
