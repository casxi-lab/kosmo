//! What the layout step reads from Rust source files: literal source paths,
//! names of interior mutability, `extern crate std` and the crate
//! attributes of a library root.
//!
//! The scans work on the token stream, so comments and the contents of
//! string literals never match, while the bodies of macro calls are read
//! like any other tokens. Doc comments become `#[doc = "..."]` attributes
//! whose text is a string literal.

use std::collections::BTreeMap;
use std::str::FromStr;

use proc_macro2::{Delimiter, Group, Literal, TokenStream, TokenTree};
use syn::punctuated::Punctuated;

/// Macros whose first argument is a path to a file.
const INCLUDE_MACROS: [&str; 3] = ["include", "include_str", "include_bytes"];

/// Names of types and modules of interior mutability that pure-core crates
/// do not use.
pub const INTERIOR_MUTABILITY: [&str; 6] = [
    "Cell",
    "RefCell",
    "UnsafeCell",
    "OnceCell",
    "cell",
    "atomic",
];

/// The tokens of a source file, or why they cannot be read and the line
/// where reading stopped, counted from 1.
pub fn tokens(text: &str) -> Result<TokenStream, (usize, String)> {
    TokenStream::from_str(text).map_err(|error| (error.span().start().line, error.to_string()))
}

/// The line of a literal, counted from 1.
fn line(literal: &Literal) -> usize {
    literal.span().start().line
}

/// The value of a string literal.
fn string(literal: &Literal) -> Option<String> {
    match syn::Lit::new(literal.clone()) {
        syn::Lit::Str(text) => Some(text.value()),
        _ => None,
    }
}

fn is_punct(tree: Option<&TokenTree>, char: char) -> bool {
    matches!(tree, Some(TokenTree::Punct(punct)) if punct.as_char() == char)
}

fn is_ident(tree: Option<&TokenTree>, name: &str) -> bool {
    matches!(tree, Some(TokenTree::Ident(ident)) if ident == name)
}

/// A literal path in a source file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourcePath {
    /// The line of the literal, counted from 1.
    pub line: usize,
    /// The literal as written.
    pub path: String,
    /// What the path names.
    pub kind: Kind,
    /// The paths, relative to the directory of the file, that the literal
    /// may name: the literal itself, or, for a `#[path]` inside inline
    /// modules, the literal under each directory that these modules may
    /// have.
    pub candidates: Vec<String>,
}

/// What a literal path names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A file of Rust code: `include!` and a `#[path]` on a module declared
    /// with `;` bring one in.
    Code,
    /// A file of data, for `include_str!` and `include_bytes!`.
    Data,
    /// The directory of the modules declared inside an inline module, for
    /// a `#[path]` on it or a `#![path]` at the start of its body.
    Directory,
}

/// A directory that an inline module may give the modules declared inside
/// it.
#[derive(Debug, Clone)]
enum Dir {
    /// Its name. In a file that is not a `mod.rs` file or a crate root,
    /// rustc puts the name of the file in front of the outermost one.
    Name(String),
    /// The literal of a `#[path]` on it or a `#![path]` at the start of
    /// its body.
    Path(String),
}

/// The directories that the `#[path]` literals of an inline module give
/// it, and whether one of them holds unconditionally.
#[derive(Debug, Default)]
struct Directories {
    dirs: Vec<Dir>,
    fixed: bool,
}

/// The literal paths in `stream`, the tokens of a file: the first argument
/// of `include!`, `include_str!` and `include_bytes!` and the value of
/// `#[path = "..."]`, also inside `cfg_attr` and in the inner attributes
/// `#![...]` at the start of an inline module, at any depth. A raw
/// identifier such as `r#path` counts without its `r#`. `stem` is the name
/// of the file without `.rs` unless it is a `mod.rs` file; the file may
/// then be a module file, whose inline modules lie in a directory of that
/// name, or a crate root, whose inline modules do not, so both are
/// candidates.
pub fn source_paths(stream: TokenStream, stem: Option<&str>, found: &mut Vec<SourcePath>) {
    scan(stream, &[], stem, found);
}

fn scan(
    stream: TokenStream,
    modules: &[Vec<Dir>],
    stem: Option<&str>,
    found: &mut Vec<SourcePath>,
) {
    let trees: Vec<TokenTree> = stream.into_iter().collect();
    // The directories that the outer attributes give the inline module
    // that follows.
    let mut pending = Directories::default();
    let mut body = None;
    for (at, tree) in trees.iter().enumerate() {
        match tree {
            TokenTree::Ident(ident)
                if INCLUDE_MACROS.contains(&unraw(ident).as_str())
                    && is_punct(trees.get(at + 1), '!') =>
            {
                if let Some(TokenTree::Group(group)) = trees.get(at + 2)
                    && let Some(TokenTree::Literal(literal)) = group.stream().into_iter().next()
                    && let Some(path) = string(&literal)
                {
                    found.push(SourcePath {
                        line: line(&literal),
                        candidates: vec![path.clone()],
                        path,
                        kind: if unraw(ident) == "include" {
                            Kind::Code
                        } else {
                            Kind::Data
                        },
                    });
                }
            }
            TokenTree::Ident(keyword) if keyword == "mod" => {
                if let Some(TokenTree::Ident(name)) = trees.get(at + 1)
                    && let Some(TokenTree::Group(group)) = trees.get(at + 2)
                    && group.delimiter() == Delimiter::Brace
                {
                    let mut directories = std::mem::take(&mut pending);
                    // rustc adds the inner attributes at the start of the
                    // body to the attributes of the module, so a #![path]
                    // there is resolved like a #[path] on the module.
                    let body_trees: Vec<TokenTree> = group.stream().into_iter().collect();
                    for attribute in inner_attributes(&body_trees) {
                        record_paths(attribute, modules, stem, Some(&mut directories), found);
                    }
                    let mut dirs = directories.dirs;
                    if !directories.fixed {
                        dirs.push(Dir::Name(unraw(name)));
                    }
                    let mut inner = modules.to_vec();
                    inner.push(dirs);
                    scan(group.stream(), &inner, stem, found);
                    body = Some(at + 2);
                }
            }
            TokenTree::Group(_) if body == Some(at) => {}
            TokenTree::Group(group) => {
                if group.delimiter() == Delimiter::Bracket
                    && is_punct(at.checked_sub(1).and_then(|before| trees.get(before)), '#')
                {
                    let module = inline_module_follows(&trees, at + 1).then_some(&mut pending);
                    record_paths(group, modules, stem, module, found);
                }
                scan(group.stream(), modules, stem, found);
            }
            _ => {}
        }
    }
}

/// Records the `path = "..."` literals of `attribute`, the brackets of an
/// attribute inside the inline modules `modules`. On an inline module,
/// `module` collects the directories that they give it; otherwise each
/// names a file of Rust code.
fn record_paths(
    attribute: &Group,
    modules: &[Vec<Dir>],
    stem: Option<&str>,
    mut module: Option<&mut Directories>,
    found: &mut Vec<SourcePath>,
) {
    let mut literals = Vec::new();
    attribute_paths(attribute.stream(), false, &mut literals);
    for (literal, conditional) in literals {
        let Some(path) = string(&literal) else {
            continue;
        };
        found.push(SourcePath {
            line: line(&literal),
            candidates: candidates(modules, stem, &path),
            path: path.clone(),
            kind: if module.is_some() {
                Kind::Directory
            } else {
                Kind::Code
            },
        });
        if let Some(module) = module.as_deref_mut() {
            module.dirs.push(Dir::Path(path));
            module.fixed |= !conditional;
        }
    }
}

/// The brackets of the inner attributes `#![...]` at the start of `trees`.
fn inner_attributes(trees: &[TokenTree]) -> Vec<&Group> {
    trees
        .chunks(3)
        .map_while(|chunk| match chunk {
            [
                TokenTree::Punct(hash),
                TokenTree::Punct(bang),
                TokenTree::Group(attribute),
            ] if hash.as_char() == '#'
                && bang.as_char() == '!'
                && attribute.delimiter() == Delimiter::Bracket =>
            {
                Some(attribute)
            }
            _ => None,
        })
        .collect()
}

/// The `path = "..."` literals of the attribute `stream`, the tokens inside
/// `#[...]` or `#![...]`, directly or inside `cfg_attr`, with whether each
/// is conditional. `r#path` and `r#cfg_attr` count as well.
fn attribute_paths(stream: TokenStream, conditional: bool, found: &mut Vec<(Literal, bool)>) {
    let inner: Vec<TokenTree> = stream.into_iter().collect();
    match inner.as_slice() {
        [
            TokenTree::Ident(name),
            TokenTree::Punct(equals),
            TokenTree::Literal(literal),
        ] if unraw(name) == "path" && equals.as_char() == '=' => {
            found.push((literal.clone(), conditional));
        }
        [TokenTree::Ident(name), TokenTree::Group(arguments)]
            if unraw(name) == "cfg_attr" && arguments.delimiter() == Delimiter::Parenthesis =>
        {
            let arguments: Vec<TokenTree> = arguments.stream().into_iter().collect();
            // The condition comes first; the attributes follow it.
            for attribute in arguments.split(|tree| is_punct(Some(tree), ',')).skip(1) {
                attribute_paths(attribute.iter().cloned().collect(), true, found);
            }
        }
        _ => {}
    }
}

/// Whether the item after the attribute that ends before `at`, past further
/// attributes and its visibility, is an inline module `mod name { ... }`.
fn inline_module_follows(trees: &[TokenTree], mut at: usize) -> bool {
    while is_punct(trees.get(at), '#')
        && matches!(trees.get(at + 1), Some(TokenTree::Group(group)) if group.delimiter() == Delimiter::Bracket)
    {
        at += 2;
    }
    if is_ident(trees.get(at), "pub") {
        at += 1;
        if matches!(trees.get(at), Some(TokenTree::Group(group)) if group.delimiter() == Delimiter::Parenthesis)
        {
            at += 1;
        }
    }
    is_ident(trees.get(at), "mod")
        && matches!(trees.get(at + 1), Some(TokenTree::Ident(_)))
        && matches!(trees.get(at + 2), Some(TokenTree::Group(group)) if group.delimiter() == Delimiter::Brace)
}

/// The paths, relative to the directory of the file, that `literal` may
/// name inside the inline modules `modules`, from the outermost.
fn candidates(modules: &[Vec<Dir>], stem: Option<&str>, literal: &str) -> Vec<String> {
    let mut prefixes: Vec<Vec<&Dir>> = vec![Vec::new()];
    for dirs in modules {
        prefixes = prefixes
            .iter()
            .flat_map(|prefix| {
                dirs.iter().map(move |dir| {
                    let mut longer = prefix.clone();
                    longer.push(dir);
                    longer
                })
            })
            .collect();
    }
    let mut found: Vec<String> = Vec::new();
    for prefix in prefixes {
        let mut starts = vec![""];
        if let (Some(stem), Some(Dir::Name(_))) = (stem, prefix.first()) {
            starts.push(stem);
        }
        for start in starts {
            let dir = prefix
                .iter()
                .fold(start.to_string(), |dir, part| match part {
                    Dir::Name(part) | Dir::Path(part) => within(&dir, part),
                });
            let path = within(&dir, literal);
            if !found.contains(&path) {
                found.push(path);
            }
        }
    }
    found
}

/// `relative` under the relative directory `dir`; an absolute `relative`
/// replaces it.
fn within(dir: &str, relative: &str) -> String {
    if dir.is_empty() || relative.starts_with('/') {
        relative.to_string()
    } else {
        format!("{dir}/{relative}")
    }
}

/// The text of an identifier without the `r#` of a raw identifier.
fn unraw(ident: &proc_macro2::Ident) -> String {
    let text = ident.to_string();
    match text.strip_prefix("r#") {
        Some(name) => name.to_string(),
        None => text,
    }
}

/// The names of [`INTERIOR_MUTABILITY`] that `stream` uses, by line, in the
/// order they first appear on the line. A raw identifier counts without its
/// `r#`.
pub fn interior_mutability(stream: TokenStream, found: &mut BTreeMap<usize, Vec<String>>) {
    for tree in stream {
        match tree {
            TokenTree::Ident(ident) => {
                let name = unraw(&ident);
                if INTERIOR_MUTABILITY.contains(&name.as_str()) {
                    let names = found.entry(ident.span().start().line).or_default();
                    if !names.contains(&name) {
                        names.push(name);
                    }
                }
            }
            TokenTree::Group(group) => interior_mutability(group.stream(), found),
            _ => {}
        }
    }
}

/// Whether `trees` ends with the attribute `#[cfg(test)]`.
fn ends_with_cfg_test(trees: &[TokenTree]) -> bool {
    let [.., TokenTree::Punct(hash), TokenTree::Group(attribute)] = trees else {
        return false;
    };
    let inner: Vec<TokenTree> = attribute.stream().into_iter().collect();
    hash.as_char() == '#'
        && attribute.delimiter() == Delimiter::Bracket
        && matches!(inner.as_slice(), [TokenTree::Ident(cfg), TokenTree::Group(condition)]
        if cfg == "cfg"
            && condition.delimiter() == Delimiter::Parenthesis
            && matches!(
                condition.stream().into_iter().collect::<Vec<_>>().as_slice(),
                [TokenTree::Ident(test)] if test == "test"
            ))
}

/// The lines of `extern crate std`, also with `r#std`, in `stream`, at any
/// depth, that do not directly follow `#[cfg(test)]`.
pub fn extern_std(stream: TokenStream, found: &mut Vec<usize>) {
    let trees: Vec<TokenTree> = stream.into_iter().collect();
    for (at, tree) in trees.iter().enumerate() {
        match tree {
            TokenTree::Ident(ident)
                if ident == "extern"
                    && is_ident(trees.get(at + 1), "crate")
                    && matches!(trees.get(at + 2), Some(TokenTree::Ident(name)) if unraw(name) == "std")
                    && !ends_with_cfg_test(&trees[..at]) =>
            {
                found.push(ident.span().start().line);
            }
            TokenTree::Group(group) => extern_std(group.stream(), found),
            _ => {}
        }
    }
}

/// The crate attributes of a library root that the layout step needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CrateAttributes {
    /// `#![no_std]`, not behind `cfg_attr`.
    pub no_std: bool,
    /// `#![forbid(unsafe_code)]`, possibly with other lints.
    pub forbids_unsafe_code: bool,
}

/// Reads the inner attributes of the library root `text`.
pub fn crate_attributes(text: &str) -> Result<CrateAttributes, String> {
    let file = syn::parse_file(text).map_err(|error| error.to_string())?;
    let inner = || {
        file.attrs
            .iter()
            .filter(|attr| matches!(attr.style, syn::AttrStyle::Inner(_)))
    };
    let lints = |attr: &syn::Attribute| {
        attr.parse_args_with(Punctuated::<syn::Path, syn::Token![,]>::parse_terminated)
            .map(|lints| lints.iter().any(|lint| lint.is_ident("unsafe_code")))
            .unwrap_or(false)
    };
    Ok(CrateAttributes {
        no_std: inner()
            .any(|attr| matches!(&attr.meta, syn::Meta::Path(path) if path.is_ident("no_std"))),
        forbids_unsafe_code: inner().any(|attr| attr.path().is_ident("forbid") && lints(attr)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(text: &str) -> Vec<(usize, String, Kind)> {
        let mut found = Vec::new();
        source_paths(tokens(text).unwrap(), None, &mut found);
        found
            .into_iter()
            .map(|found| (found.line, found.path, found.kind))
            .collect()
    }

    /// The candidates of each literal path in `text`, a file with `stem`.
    fn candidates_of(text: &str, stem: Option<&str>) -> Vec<Vec<String>> {
        let mut found = Vec::new();
        source_paths(tokens(text).unwrap(), stem, &mut found);
        found.into_iter().map(|found| found.candidates).collect()
    }

    #[test]
    fn finds_literal_paths_at_any_depth() {
        let text = "mod a {\n    #[path = \"x.rs\"]\n    mod b;\n}\nfn f() {\n    m!(include_bytes!(r\"y.bin\"));\n    include!(\"z.rs\");\n}\n";
        assert_eq!(
            paths(text),
            [
                (2, "x.rs".to_string(), Kind::Code),
                (6, "y.bin".to_string(), Kind::Data),
                (7, "z.rs".to_string(), Kind::Code),
            ]
        );
    }

    #[test]
    fn finds_paths_inside_cfg_attr() {
        let text = "#[cfg_attr(unix, path = \"u.rs\")]\n#[cfg_attr(all(), cfg_attr(windows, doc = \"d\", path = \"w.rs\"))]\nmod m;\n";
        assert_eq!(
            paths(text),
            [
                (1, "u.rs".to_string(), Kind::Code),
                (2, "w.rs".to_string(), Kind::Code)
            ]
        );
    }

    #[test]
    fn finds_paths_behind_raw_identifiers() {
        let text = "#[r#path = \"p.rs\"]\nmod p;\n#[r#cfg_attr(unix, r#path = \"u.rs\")]\nmod u;\nfn f() {\n    r#include!(\"i.rs\");\n    let _ = r#include_str!(\"s.txt\");\n    let _ = core::r#include_bytes!(\"b.bin\");\n}\n#[r#path = \"d\"]\nmod d {}\n";
        assert_eq!(
            paths(text),
            [
                (1, "p.rs".to_string(), Kind::Code),
                (3, "u.rs".to_string(), Kind::Code),
                (6, "i.rs".to_string(), Kind::Code),
                (7, "s.txt".to_string(), Kind::Data),
                (8, "b.bin".to_string(), Kind::Data),
                (10, "d".to_string(), Kind::Directory),
            ]
        );
    }

    #[test]
    fn finds_inner_paths_of_inline_modules() {
        let text = "mod a {\n    //! Docs.\n    #![path = \"d\"]\n    #[path = \"e.rs\"]\n    mod e;\n}\nmod f {\n    #![cfg_attr(unix, r#path = \"u\")]\n    mod g;\n}\n";
        assert_eq!(
            paths(text),
            [
                (3, "d".to_string(), Kind::Directory),
                (4, "e.rs".to_string(), Kind::Code),
                (8, "u".to_string(), Kind::Directory),
            ]
        );
    }

    #[test]
    fn inner_paths_give_inline_modules_their_directories() {
        let strings = |list: &[&str]| list.iter().map(|text| text.to_string()).collect::<Vec<_>>();
        // An unconditional #![path] replaces the module's name; a
        // conditional one adds a directory beside it.
        let text = "mod a {\n    #![path = \"d\"]\n    #[path = \"e.rs\"]\n    mod e;\n}\nmod f {\n    #![cfg_attr(unix, path = \"u\")]\n    #[path = \"g.rs\"]\n    mod g;\n}\n";
        assert_eq!(
            candidates_of(text, Some("x")),
            [
                strings(&["d"]),
                strings(&["d/e.rs"]),
                strings(&["u"]),
                strings(&["u/g.rs", "f/g.rs", "x/f/g.rs"]),
            ]
        );
        // A #![path] is resolved from the directory of the outer module,
        // like a #[path] on the module, and adds to one.
        let text = "mod k {\n    mod l {\n        #![path = \"m\"]\n        #[path = \"n.rs\"]\n        mod n;\n    }\n}\n#[cfg_attr(unix, path = \"o\")]\nmod p {\n    #![path = \"q\"]\n    include!(\"r.rs\");\n    #[path = \"s.rs\"]\n    mod s;\n}\n";
        assert_eq!(
            candidates_of(text, Some("x")),
            [
                strings(&["k/m", "x/k/m"]),
                strings(&["k/m/n.rs", "x/k/m/n.rs"]),
                strings(&["o"]),
                strings(&["q"]),
                strings(&["r.rs"]),
                strings(&["o/s.rs", "q/s.rs"]),
            ]
        );
    }

    #[test]
    fn paths_inside_inline_modules_are_relative_to_their_directories() {
        let text = "mod a {\n    #[path = \"../b.rs\"]\n    mod b;\n    #[path = \"d\"]\n    pub mod c {\n        #[path = \"e.rs\"]\n        mod e;\n    }\n    #[cfg_attr(unix, path = \"u\")]\n    mod f {\n        include!(\"g.rs\");\n        #[path = \"h.rs\"]\n        mod h;\n    }\n}\n";
        let strings = |list: &[&str]| list.iter().map(|text| text.to_string()).collect::<Vec<_>>();
        assert_eq!(
            candidates_of(text, None),
            [
                strings(&["a/../b.rs"]),
                strings(&["a/d"]),
                strings(&["a/d/e.rs"]),
                strings(&["a/u"]),
                strings(&["g.rs"]),
                strings(&["a/u/h.rs", "a/f/h.rs"]),
            ]
        );
        assert_eq!(
            candidates_of(text, Some("x"))[0],
            strings(&["a/../b.rs", "x/a/../b.rs"])
        );
        // An inline module with a #[path] does not start in the directory
        // of the file's name.
        assert_eq!(
            candidates_of(
                "#[path = \"p\"]\nmod a {\n    #[path = \"q.rs\"]\n    mod q;\n}\n",
                Some("x")
            ),
            [strings(&["p"]), strings(&["p/q.rs"])]
        );
        assert_eq!(
            paths("#[path = \"d\"]\nmod c {}\n#[path = \"f.rs\"]\nmod f;\n"),
            [
                (1, "d".to_string(), Kind::Directory),
                (3, "f.rs".to_string(), Kind::Code)
            ]
        );
    }

    #[test]
    fn reports_the_line_where_the_tokens_stop() {
        let (line, _) = tokens("fn f() {}\nconst S: &str = \"open;\n").unwrap_err();
        assert_eq!(line, 2);
    }

    #[test]
    fn ignores_paths_in_comments_strings_and_other_attributes() {
        let text = "// include_str!(\"a\")\n/// #[path = \"b\"]\nconst A: &str = \"include!(\\\"c\\\")\";\n#[doc = \"d\"]\nconst B: u8 = 0;\nfn g() { include!(concat!(\"e\", \".rs\")); }\n";
        assert_eq!(paths(text), []);
    }

    #[test]
    fn allows_extern_crate_std_only_for_tests() {
        let text = "#[cfg(test)]\nextern crate std;\nextern crate alloc;\nmod m {\n    extern crate std as s;\n}\nextern crate r#std;\n";
        let mut found = Vec::new();
        extern_std(tokens(text).unwrap(), &mut found);
        assert_eq!(found, [5, 7]);
    }

    #[test]
    fn reads_unconditional_crate_attributes() {
        let read = |text| crate_attributes(text).unwrap();
        let both = CrateAttributes {
            no_std: true,
            forbids_unsafe_code: true,
        };
        assert_eq!(
            read("#![no_std]\n#![forbid(missing_docs, unsafe_code)]\n"),
            both
        );
        assert_eq!(
            read("#![cfg_attr(not(test), no_std)]\n#![deny(unsafe_code)]\n"),
            CrateAttributes {
                no_std: false,
                forbids_unsafe_code: false,
            }
        );
        assert!(crate_attributes("fn (").is_err());
    }
}
