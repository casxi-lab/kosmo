//! The font assets of `kosmo-text`, which the goldens step of
//! `cargo xtask check` compares with their asset manifest.
//!
//! The asset directory holds the fonts that kosmo bundles, each in a
//! directory of its own with its license file, and the asset manifest
//! `MANIFEST.toml`. The manifest has one `[[font]]` table for each font,
//! sorted by `file`, with the fields of [`Font`]; its paths are relative to
//! the asset directory. The check reads only tracked files and adds every
//! problem it finds to those of the goldens step:
//!
//! - the manifest lists exactly the fonts of [`Assets::fonts`], each once,
//!   sorted, with digests of 64 lowercase hexadecimal digits, the license
//!   [`LICENSE`], a license file in the directory of the font and a source,
//!   an upstream commit, a version and a role;
//! - the asset directory holds, besides the manifest, exactly the files that
//!   the manifest lists, as regular files, each with the SHA-256 digest that
//!   the manifest lists for it and each font with its size;
//! - no tracked font lies outside the asset directory.

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use serde::Deserialize;
use toml::Spanned;

use crate::deps::FONT_EXTENSIONS;
use crate::golden::{self, Problems};
use crate::layout;
use crate::repo::{self, Entry, Tree};

/// A directory of font assets with its manifest.
#[derive(Debug, Clone, Copy)]
pub struct Assets {
    /// The asset directory.
    pub dir: &'static str,
    /// The fonts that the manifest lists, by path in the asset directory,
    /// sorted: exactly these.
    pub fonts: &'static [&'static str],
}

/// The font assets of `kosmo-text`: Fira Mono and Fira Sans. kosmo bundles
/// exactly these two fonts; another font, a CJK font among them, is a design
/// change, which changes this list.
pub const FONTS: Assets = Assets {
    dir: "framework/text/assets/fonts",
    fonts: &[
        "firamono/FiraMono-Regular.ttf",
        "firasans/FiraSans-Regular.ttf",
    ],
};

/// The file name of the manifest in the asset directory.
pub const MANIFEST: &str = "MANIFEST.toml";

/// The license of every bundled font.
pub const LICENSE: &str = "OFL-1.1";

/// The asset manifest.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    #[serde(default)]
    font: Vec<Spanned<Font>>,
}

/// A font of the asset manifest.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct Font {
    /// The path of the font in the asset directory.
    file: String,
    /// Its size in bytes.
    bytes: u64,
    /// Its SHA-256 digest, in 64 lowercase hexadecimal digits.
    sha256: String,
    /// Where it was taken from: the repository, the commit and the path.
    source: String,
    /// The commit of the font's own repository that the source records.
    upstream: String,
    /// The version of the font.
    version: String,
    /// What kosmo uses the font for.
    role: String,
    /// Its license, [`LICENSE`].
    license: String,
    /// The path of its license file in the asset directory, in the
    /// directory of the font.
    license_file: String,
    /// The SHA-256 digest of the license file.
    license_sha256: String,
}

/// A file that the manifest lists: the line of its font, its digest and,
/// for a font, its size.
struct Listed {
    line: usize,
    sha256: String,
    bytes: Option<u64>,
}

/// Whether `path` is a font file, by its extension in any letter case.
fn is_font(path: &str) -> bool {
    layout::file_name(path)
        .rsplit_once('.')
        .is_some_and(|(_, extension)| {
            FONT_EXTENSIONS
                .iter()
                .any(|font| extension.eq_ignore_ascii_case(font))
        })
}

/// Whether `path` is a path relative to the asset directory: parts that are
/// neither empty nor `.` or `..`, separated by `/`.
fn is_relative(path: &str) -> bool {
    !path.contains('\\')
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

/// The fonts of the manifest at `path` in `tree`, with the line of the
/// table of each; `None`, with the problem, when the manifest is missing or
/// cannot be read.
fn read(tree: &Tree, path: &str, problems: &mut Problems) -> Option<Vec<(usize, Font)>> {
    let text = match tree.entry(path) {
        None => {
            let message = "is missing; the asset manifest lists the bundled fonts";
            problems.add(path, 0, message.to_string());
            return None;
        }
        Some(Entry::File(bytes)) => match std::str::from_utf8(bytes) {
            Ok(text) => text,
            Err(_) => {
                problems.add(path, 0, "is not UTF-8".to_string());
                return None;
            }
        },
        Some(_) => {
            problems.add(path, 0, "is not a regular file".to_string());
            return None;
        }
    };
    match toml::from_str::<Manifest>(text) {
        Ok(manifest) => Some(
            manifest
                .font
                .into_iter()
                .map(|font| (layout::line_of(text, font.span().start), font.into_inner()))
                .collect(),
        ),
        Err(error) => {
            let line = error
                .span()
                .map_or(0, |span| layout::line_of(text, span.start));
            problems.add(path, line, error.message().to_string());
            None
        }
    }
}

/// Checks the font `font` on `line` of the manifest at `path` and adds its
/// files to `listed`.
fn check_font(
    assets: &Assets,
    path: &str,
    (line, font): &(usize, Font),
    listed: &mut BTreeMap<String, Listed>,
    problems: &mut Problems,
) {
    let (line, file) = (*line, font.file.as_str());
    if !is_relative(file) {
        let message = format!("`{file}` is not a path relative to the asset directory");
        problems.add(path, line, message);
        return;
    }
    if !assets.fonts.contains(&file) {
        let fonts = assets.fonts.join(", ");
        let message = format!(
            "`{file}` is not one of the fonts that kosmo bundles, {fonts}; another font is a design change"
        );
        problems.add(path, line, message);
    }
    for (field, digest) in [
        ("sha256", &font.sha256),
        ("license-sha256", &font.license_sha256),
    ] {
        if !golden::is_digest(digest) {
            let message = format!(
                "the {field} of `{file}`, `{digest}`, is not 64 lowercase hexadecimal digits"
            );
            problems.add(path, line, message);
        }
    }
    if font.license != LICENSE {
        let message = format!(
            "the license of `{file}` is `{}`; kosmo bundles only unmodified {LICENSE} fonts",
            font.license
        );
        problems.add(path, line, message);
    }
    let license_file = font.license_file.as_str();
    if !is_relative(license_file)
        || license_file == file
        || repo::parent(license_file) != repo::parent(file)
    {
        let message = format!(
            "the license file `{license_file}` of `{file}` is not another file in the directory of the font"
        );
        problems.add(path, line, message);
    }
    let fields = [
        ("source", &font.source),
        ("upstream", &font.upstream),
        ("version", &font.version),
        ("role", &font.role),
    ];
    for (field, value) in fields {
        if value.trim().is_empty() {
            problems.add(path, line, format!("the {field} of `{file}` is empty"));
        }
    }
    let files = [
        (file, &font.sha256, Some(font.bytes)),
        (license_file, &font.license_sha256, None),
    ];
    for (file, sha256, bytes) in files {
        if !is_relative(file) {
            continue;
        }
        if let Some(first) = listed.get(file) {
            let message = format!("`{file}` is listed twice, first on line {}", first.line);
            problems.add(path, line, message);
            continue;
        }
        let entry = Listed {
            line,
            sha256: sha256.clone(),
            bytes,
        };
        listed.insert(file.to_string(), entry);
    }
}

/// Checks the asset directory of `assets` in `tree` against its manifest,
/// and that no tracked font lies outside it. A manifest that is missing or
/// cannot be read is reported alone, and the directory is not compared with
/// it. Returns how many fonts the manifest lists.
pub fn check(tree: &Tree, assets: &Assets, problems: &mut Problems) -> usize {
    let dir = assets.dir;
    for (path, _) in tree.entries() {
        if is_font(path) && !repo::is_within(path, dir) {
            let message = format!("is a font outside {dir}, the only place for bundled fonts");
            problems.add(path, 0, message);
        }
    }
    let manifest = format!("{dir}/{MANIFEST}");
    let Some(fonts) = read(tree, &manifest, problems) else {
        return 0;
    };
    let mut listed = BTreeMap::new();
    let mut last: Option<&str> = None;
    for font in &fonts {
        let (line, file) = (font.0, font.1.file.as_str());
        if let Some(before) = last.filter(|before| *before > file) {
            let message = format!("`{file}` follows `{before}`; the fonts are sorted by `file`");
            problems.add(&manifest, line, message);
        }
        last = Some(file);
        check_font(assets, &manifest, font, &mut listed, problems);
    }
    for font in assets.fonts {
        if !fonts.iter().any(|(_, listed)| listed.file == *font) {
            let message = format!("does not list `{font}`; the manifest lists every bundled font");
            problems.add(&manifest, 0, message);
        }
    }
    for (path, entry) in tree.entries() {
        let Some(file) = path
            .strip_prefix(dir)
            .and_then(|rest| rest.strip_prefix('/'))
        else {
            continue;
        };
        if path == manifest {
            continue;
        }
        let Some(expected) = listed.get(file) else {
            let message = format!(
                "is not listed in {MANIFEST}; the asset directory holds only the manifest and the files it lists"
            );
            problems.add(path, 0, message);
            continue;
        };
        let Entry::File(bytes) = entry else {
            problems.add(path, 0, "is not a regular file".to_string());
            continue;
        };
        let actual = golden::digest(bytes);
        if actual != expected.sha256 {
            let message = format!(
                "SHA-256 {actual} differs from {}, which {MANIFEST} lists; bundled fonts and their license files are unmodified",
                expected.sha256
            );
            problems.add(path, 0, message);
        }
        if let Some(size) = expected.bytes.filter(|size| *size != bytes.len() as u64) {
            let message = format!("is {} bytes; {MANIFEST} lists {size}", bytes.len());
            problems.add(path, 0, message);
        }
    }
    for (file, expected) in &listed {
        if tree.entry(&format!("{dir}/{file}")).is_none() {
            let message = format!("`{file}` has no file");
            problems.add(&manifest, expected.line, message);
        }
    }
    fonts.len()
}
