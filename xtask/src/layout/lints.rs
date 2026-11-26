//! What the layout step reads for the determinism lints: where Clippy may
//! find its configuration, and the lint attributes of Rust source files
//! that would turn off `clippy::disallowed_methods` or
//! `clippy::disallowed_types`.
//!
//! The scan works on the token stream, as the other scans of the layout
//! step do: comments and string literals never match, the bodies of macro
//! calls and macro definitions are read like any other tokens, and a raw
//! identifier such as `r#allow` counts without its `r#`, as the compiler
//! reads it.

use proc_macro2::{Delimiter, Group, TokenStream, TokenTree};

use super::source::{is_punct, unraw};

/// The only Clippy configuration, at the repository root.
pub const CLIPPY_CONFIG: &str = "clippy.toml";

/// The lints of the determinism rules, which every product workspace
/// denies with the lists of the root Clippy configuration.
pub const LINTS: [&str; 2] = ["disallowed_methods", "disallowed_types"];

/// The names under which an attribute also reaches the lints of
/// [`LINTS`]: their former names, which Clippy still accepts, and the
/// groups that hold them.
const OTHER_NAMES: [&str; 4] = ["disallowed_method", "disallowed_type", "style", "all"];

/// The lint levels that turn a denied lint off or lower it.
const LOWERING: [&str; 3] = ["allow", "expect", "warn"];

/// The crates that the architecture puts on the deterministic path by
/// name; each of them that the table lists is marked `deterministic`.
pub const DETERMINISTIC: [&str; 7] = [
    "kosmo-types",
    "kosmo-ui",
    "kosmo-render",
    "kosmo-text",
    "kosmo-controls",
    "kosmo-math-controls",
    "kosmo-compositor",
];

/// The wrapper module of the transcendental functions: the crate and its
/// file, the one place that may allow or expect `clippy::disallowed_methods`,
/// once, at the start of the file, to call libm.
pub const WRAPPER: (&str, &str) = ("kosmo-render", "src/det_math.rs");

/// Whether Clippy may read the file at `path` as a configuration: its name
/// is `clippy.toml` or `.clippy.toml`, in any letter case, since a
/// file system that ignores the case finds it under either spelling.
pub fn is_clippy_configuration(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.eq_ignore_ascii_case("clippy.toml") || name.eq_ignore_ascii_case(".clippy.toml")
}

/// A lint attribute that lowers a lint of the determinism rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lowering {
    /// The line of the attribute's `#`, counted from 1.
    pub line: usize,
    /// `allow`, `expect` or `warn`.
    pub level: String,
    /// The names that reach the lints of [`LINTS`], as `clippy::<name>`, in
    /// the order the attribute lists them.
    pub lints: Vec<String>,
    /// Whether the attribute stands inside `cfg_attr`.
    pub conditional: bool,
    /// Whether it is an inner attribute `#![...]` of the file itself,
    /// outside every bracket, brace and parenthesis, which stands at the
    /// start of the file and applies to the whole module.
    pub of_the_file: bool,
}

impl Lowering {
    /// Whether this is the one attribute that the wrapper module may have:
    /// an unconditional `#![allow(...)]` or `#![expect(...)]` of
    /// `clippy::disallowed_methods` alone at the start of the file.
    pub fn is_the_wrappers(&self) -> bool {
        self.of_the_file
            && !self.conditional
            && (self.level == "allow" || self.level == "expect")
            && self.lints == ["clippy::disallowed_methods"]
    }
}

/// The attributes in `stream`, the tokens of a file, at any depth, that
/// lower a lint of the determinism rules, in the order they appear.
pub fn lowerings(stream: TokenStream, found: &mut Vec<Lowering>) {
    scan(stream, true, found);
}

/// The lowerings in `stream`, the tokens of the whole file when `top` is
/// true and of a group inside it otherwise.
fn scan(stream: TokenStream, top: bool, found: &mut Vec<Lowering>) {
    let trees: Vec<TokenTree> = stream.into_iter().collect();
    for (at, tree) in trees.iter().enumerate() {
        match tree {
            TokenTree::Punct(hash) if hash.as_char() == '#' => {
                let inner = is_punct(trees.get(at + 1), '!');
                if let Some(TokenTree::Group(attribute)) = trees.get(at + 1 + usize::from(inner))
                    && attribute.delimiter() == Delimiter::Bracket
                {
                    let line = hash.span().start().line;
                    attribute_lowerings(attribute.stream(), line, false, top && inner, found);
                }
            }
            TokenTree::Group(group) => scan(group.stream(), false, found),
            _ => {}
        }
    }
}

/// The lowerings of the attribute `meta`, the contents of `#[...]` or one
/// attribute of a `cfg_attr`, at `line`; `of_the_file` as in [`Lowering`].
fn attribute_lowerings(
    meta: TokenStream,
    line: usize,
    conditional: bool,
    of_the_file: bool,
    found: &mut Vec<Lowering>,
) {
    let trees: Vec<TokenTree> = meta.into_iter().collect();
    let [TokenTree::Ident(name), TokenTree::Group(arguments)] = trees.as_slice() else {
        return;
    };
    if arguments.delimiter() != Delimiter::Parenthesis {
        return;
    }
    let name = unraw(name);
    if name == "cfg_attr" {
        // The condition, then the attributes.
        for attribute in split(arguments).into_iter().skip(1) {
            attribute_lowerings(
                attribute.into_iter().collect(),
                line,
                true,
                of_the_file,
                found,
            );
        }
        return;
    }
    if !LOWERING.contains(&name.as_str()) {
        return;
    }
    let lints: Vec<String> = split(arguments)
        .iter()
        .filter_map(|lint| match lint.as_slice() {
            [
                TokenTree::Ident(tool),
                TokenTree::Punct(first),
                TokenTree::Punct(second),
                TokenTree::Ident(lint),
            ] if unraw(tool) == "clippy" && first.as_char() == ':' && second.as_char() == ':' => {
                let lint = unraw(lint);
                (LINTS.contains(&lint.as_str()) || OTHER_NAMES.contains(&lint.as_str()))
                    .then(|| format!("clippy::{lint}"))
            }
            _ => None,
        })
        .collect();
    if !lints.is_empty() {
        found.push(Lowering {
            line,
            level: name,
            lints,
            conditional,
            of_the_file,
        });
    }
}

/// The lines, in `stream`, at any depth, of the module declarations that
/// end with `;`, whose module is in another file, and of the calls of
/// `include!`, in the order they appear.
pub fn other_files(stream: TokenStream, found: &mut Vec<usize>) {
    let trees: Vec<TokenTree> = stream.into_iter().collect();
    for (at, tree) in trees.iter().enumerate() {
        match tree {
            TokenTree::Ident(keyword)
                if keyword == "mod"
                    && matches!(trees.get(at + 1), Some(TokenTree::Ident(_)))
                    && is_punct(trees.get(at + 2), ';') =>
            {
                found.push(keyword.span().start().line);
            }
            TokenTree::Ident(name)
                if unraw(name) == "include" && is_punct(trees.get(at + 1), '!') =>
            {
                found.push(name.span().start().line);
            }
            TokenTree::Group(group) => other_files(group.stream(), found),
            _ => {}
        }
    }
}

/// The parts of the contents of `group` between its top-level commas.
fn split(group: &Group) -> Vec<Vec<TokenTree>> {
    let mut parts = vec![Vec::new()];
    for tree in group.stream() {
        match &tree {
            TokenTree::Punct(comma) if comma.as_char() == ',' => parts.push(Vec::new()),
            _ => {
                if let Some(part) = parts.last_mut() {
                    part.push(tree);
                }
            }
        }
    }
    parts.retain(|part| !part.is_empty());
    parts
}

#[cfg(test)]
mod fixture;

#[cfg(test)]
mod tests {
    use super::*;

    fn found(text: &str) -> Vec<Lowering> {
        let mut found = Vec::new();
        lowerings(crate::layout::source::tokens(text).unwrap(), &mut found);
        found
    }

    fn lowering(line: usize, level: &str, lints: &[&str], conditional: bool) -> Lowering {
        Lowering {
            line,
            level: level.to_string(),
            lints: lints.iter().map(|lint| lint.to_string()).collect(),
            conditional,
            of_the_file: false,
        }
    }

    /// `lowering` as an inner attribute of the file itself.
    fn of_the_file(lowering: Lowering) -> Lowering {
        Lowering {
            of_the_file: true,
            ..lowering
        }
    }

    fn other_files_in(text: &str) -> Vec<usize> {
        let mut found = Vec::new();
        other_files(crate::layout::source::tokens(text).unwrap(), &mut found);
        found
    }

    #[test]
    fn finds_every_level_that_lowers_the_lints() {
        let text = "#[allow(clippy::disallowed_methods, reason = \"x\")]\nfn a() {}\n#[expect(clippy::disallowed_types)]\nfn b() {}\n#[warn(clippy :: disallowed_methods)]\nfn c() {}\n#[deny(clippy::disallowed_methods)]\nfn d() {}\n";
        assert_eq!(
            found(text),
            [
                lowering(1, "allow", &["clippy::disallowed_methods"], false),
                lowering(3, "expect", &["clippy::disallowed_types"], false),
                lowering(5, "warn", &["clippy::disallowed_methods"], false),
            ]
        );
    }

    #[test]
    fn finds_inner_attributes_former_names_and_groups() {
        let text = "#![allow(clippy::disallowed_method, clippy::unwrap_used)]\nmod m {\n    #![allow(clippy::style)]\n}\n#[allow(clippy::all, clippy::disallowed_type)]\nfn f() {}\n";
        assert_eq!(
            found(text),
            [
                of_the_file(lowering(1, "allow", &["clippy::disallowed_method"], false)),
                lowering(3, "allow", &["clippy::style"], false),
                lowering(
                    5,
                    "allow",
                    &["clippy::all", "clippy::disallowed_type"],
                    false
                ),
            ]
        );
    }

    #[test]
    fn finds_attributes_inside_cfg_attr_bodies_and_macros() {
        let text = "#[cfg_attr(test, allow(clippy::disallowed_methods), cfg_attr(all(), expect(clippy::disallowed_types)))]\nfn f() {\n    #[allow(clippy::disallowed_methods)]\n    let x = 1;\n}\nmacro_rules! m { () => { #[allow(clippy::disallowed_types)] fn g() {} } }\n";
        assert_eq!(
            found(text),
            [
                lowering(1, "allow", &["clippy::disallowed_methods"], true),
                lowering(1, "expect", &["clippy::disallowed_types"], true),
                lowering(3, "allow", &["clippy::disallowed_methods"], false),
                lowering(6, "allow", &["clippy::disallowed_types"], false),
            ]
        );
    }

    #[test]
    fn raw_identifiers_are_read_as_their_names() {
        let text = "#[r#allow(r#clippy::r#disallowed_methods)]\nfn a() {}\n#[r#cfg_attr(all(), r#expect(clippy::r#style))]\nfn b() {}\n";
        assert_eq!(
            found(text),
            [
                lowering(1, "allow", &["clippy::disallowed_methods"], false),
                lowering(3, "expect", &["clippy::style"], true),
            ]
        );
    }

    #[test]
    fn other_lints_comments_and_strings_do_not_count() {
        let text = "// #[allow(clippy::disallowed_methods)]\n/// #[allow(clippy::disallowed_methods)]\nconst S: &str = \"#[allow(clippy::disallowed_methods)]\";\n#[allow(clippy::unwrap_used, disallowed_methods)]\nfn f() {}\n";
        assert_eq!(found(text), []);
    }

    #[test]
    fn only_the_inner_attributes_of_the_file_itself_are_of_the_file() {
        let text = "#![expect(clippy::disallowed_methods)]\n#[expect(clippy::disallowed_methods)]\nfn f() {}\nmod m {\n    #![expect(clippy::disallowed_methods)]\n}\nmacro_rules! m { () => { #![expect(clippy::disallowed_methods)] } }\n";
        let methods = ["clippy::disallowed_methods"];
        assert_eq!(
            found(text),
            [
                of_the_file(lowering(1, "expect", &methods, false)),
                lowering(2, "expect", &methods, false),
                lowering(5, "expect", &methods, false),
                lowering(7, "expect", &methods, false),
            ]
        );
    }

    #[test]
    fn only_one_unconditional_allowance_of_the_methods_lint_is_the_wrappers() {
        let methods = ["clippy::disallowed_methods"];
        assert!(of_the_file(lowering(1, "expect", &methods, false)).is_the_wrappers());
        assert!(of_the_file(lowering(1, "allow", &methods, false)).is_the_wrappers());
        assert!(!lowering(1, "expect", &methods, false).is_the_wrappers());
        assert!(!of_the_file(lowering(1, "warn", &methods, false)).is_the_wrappers());
        assert!(!of_the_file(lowering(1, "allow", &methods, true)).is_the_wrappers());
        assert!(
            !of_the_file(lowering(1, "allow", &["clippy::disallowed_types"], false))
                .is_the_wrappers()
        );
        assert!(
            !of_the_file(lowering(
                1,
                "allow",
                &["clippy::disallowed_methods", "clippy::style"],
                false
            ))
            .is_the_wrappers()
        );
    }

    #[test]
    fn finds_modules_in_other_files_and_included_files() {
        let text = "mod inline {\n    mod nested;\n}\npub mod extra;\n#[path = \"x.rs\"]\nmod x;\nr#include!(\"y.rs\");\nconst S: &str = include_str!(\"z.txt\");\nmacro_rules! m { () => { include!(\"w.rs\"); } }\n// mod comment;\n";
        assert_eq!(other_files_in(text), [2, 4, 6, 7, 9]);
    }

    #[test]
    fn a_clippy_configuration_is_found_in_any_letter_case() {
        for path in [
            "clippy.toml",
            ".clippy.toml",
            "framework/render/clippy.toml",
            "display/Clippy.TOML",
            "x/.CLIPPY.toml",
        ] {
            assert!(is_clippy_configuration(path), "{path}");
        }
        for path in ["clippy.toml.orig", "docs/clippy.md", "my-clippy.toml"] {
            assert!(!is_clippy_configuration(path), "{path}");
        }
    }
}
