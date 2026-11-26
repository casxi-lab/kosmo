//! What the layout step reads for the determinism lints: where Clippy may
//! find its configuration, and the lint attributes of Rust source files
//! that would turn off `clippy::disallowed_methods` or
//! `clippy::disallowed_types`.
//!
//! The scan works on the token stream, as the other scans of the layout
//! step do: comments and string literals never match, and the bodies of
//! macro calls and macro definitions are read like any other tokens.

use proc_macro2::{Delimiter, Group, TokenStream, TokenTree};

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

/// The wrapper module of the transcendental functions: the crate and its
/// file, the one place that may allow or expect `clippy::disallowed_methods`,
/// once, to call libm.
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
}

impl Lowering {
    /// Whether this is the one attribute that the wrapper module may have:
    /// an unconditional `allow` or `expect` of `clippy::disallowed_methods`
    /// alone.
    pub fn is_the_wrappers(&self) -> bool {
        !self.conditional
            && (self.level == "allow" || self.level == "expect")
            && self.lints == ["clippy::disallowed_methods"]
    }
}

/// The attributes in `stream`, at any depth, that lower a lint of the
/// determinism rules, in the order they appear.
pub fn lowerings(stream: TokenStream, found: &mut Vec<Lowering>) {
    let trees: Vec<TokenTree> = stream.into_iter().collect();
    for (at, tree) in trees.iter().enumerate() {
        match tree {
            TokenTree::Punct(hash) if hash.as_char() == '#' => {
                let mut next = at + 1;
                if matches!(trees.get(next), Some(TokenTree::Punct(bang)) if bang.as_char() == '!')
                {
                    next += 1;
                }
                if let Some(TokenTree::Group(attribute)) = trees.get(next)
                    && attribute.delimiter() == Delimiter::Bracket
                {
                    let line = hash.span().start().line;
                    attribute_lowerings(attribute.stream(), line, false, found);
                }
            }
            TokenTree::Group(group) => lowerings(group.stream(), found),
            _ => {}
        }
    }
}

/// The lowerings of the attribute `meta`, the contents of `#[...]` or one
/// attribute of a `cfg_attr`, at `line`.
fn attribute_lowerings(
    meta: TokenStream,
    line: usize,
    conditional: bool,
    found: &mut Vec<Lowering>,
) {
    let trees: Vec<TokenTree> = meta.into_iter().collect();
    let [TokenTree::Ident(name), TokenTree::Group(arguments)] = trees.as_slice() else {
        return;
    };
    if arguments.delimiter() != Delimiter::Parenthesis {
        return;
    }
    let name = name.to_string();
    if name == "cfg_attr" {
        // The condition, then the attributes.
        for attribute in split(arguments).into_iter().skip(1) {
            attribute_lowerings(attribute.into_iter().collect(), line, true, found);
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
            ] if tool == "clippy" && first.as_char() == ':' && second.as_char() == ':' => {
                let lint = lint.to_string();
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
        });
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
        }
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
                lowering(1, "allow", &["clippy::disallowed_method"], false),
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
    fn other_lints_comments_and_strings_do_not_count() {
        let text = "// #[allow(clippy::disallowed_methods)]\n/// #[allow(clippy::disallowed_methods)]\nconst S: &str = \"#[allow(clippy::disallowed_methods)]\";\n#[allow(clippy::unwrap_used, disallowed_methods)]\nfn f() {}\n";
        assert_eq!(found(text), []);
    }

    #[test]
    fn only_one_unconditional_allowance_of_the_methods_lint_is_the_wrappers() {
        assert!(lowering(1, "expect", &["clippy::disallowed_methods"], false).is_the_wrappers());
        assert!(lowering(1, "allow", &["clippy::disallowed_methods"], false).is_the_wrappers());
        assert!(!lowering(1, "warn", &["clippy::disallowed_methods"], false).is_the_wrappers());
        assert!(!lowering(1, "allow", &["clippy::disallowed_methods"], true).is_the_wrappers());
        assert!(!lowering(1, "allow", &["clippy::disallowed_types"], false).is_the_wrappers());
        assert!(
            !lowering(
                1,
                "allow",
                &["clippy::disallowed_methods", "clippy::style"],
                false
            )
            .is_the_wrappers()
        );
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
