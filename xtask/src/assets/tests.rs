use super::*;
use crate::check::Facts;
use crate::golden::digest;

const DIR: &str = "framework/text/assets/fonts";
const MANIFEST_PATH: &str = "framework/text/assets/fonts/MANIFEST.toml";
const MONO: &str = "firamono/FiraMono-Regular.ttf";
const MONO_LICENSE: &str = "firamono/OFL.txt";
const SANS: &str = "firasans/FiraSans-Regular.ttf";
const SANS_LICENSE: &str = "firasans/OFL.txt";

/// The four files of the fixtures, by path in the asset directory.
const FILES: [&str; 4] = [MONO, MONO_LICENSE, SANS, SANS_LICENSE];

/// The bytes of the fixture file `file` of the asset directory.
fn bytes_of(file: &str) -> Vec<u8> {
    format!("bytes of {file}").into_bytes()
}

/// The repository path of the file `file` of the asset directory.
fn at(file: &str) -> String {
    format!("{DIR}/{file}")
}

/// The `[[font]]` table of the font `file` with the license file
/// `license_file`, with the sizes and digests of the fixtures: 11 lines.
fn table(file: &str, license_file: &str) -> String {
    format!(
        "[[font]]\n\
         file = \"{file}\"\n\
         bytes = {}\n\
         sha256 = \"{}\"\n\
         source = \"google/fonts@29b4ec6df2522a8aee279414b78a07910b065874:ofl/{file}\"\n\
         upstream = \"googlefonts/FiraGFVersion@a7d6892325d5b7382e426bea13bd3ef2c32bd53e\"\n\
         version = \"1.000\"\n\
         role = \"The role of {file}\"\n\
         license = \"OFL-1.1\"\n\
         license-file = \"{license_file}\"\n\
         license-sha256 = \"{}\"\n",
        bytes_of(file).len(),
        digest(&bytes_of(file)),
        digest(&bytes_of(license_file)),
    )
}

/// A manifest of both fonts: a comment and a blank line, the table of Fira
/// Mono from line 3 and that of Fira Sans from line 15.
fn manifest() -> String {
    format!(
        "# The asset manifest.\n\n{}\n{}",
        table(MONO, MONO_LICENSE),
        table(SANS, SANS_LICENSE)
    )
}

/// The asset directory with `manifest` and the fixture files `files`.
fn tree_of(manifest: &str, files: &[&str]) -> Tree {
    let mut tree = Tree::from_files(&[(MANIFEST_PATH, manifest)]);
    for file in files {
        tree.insert(&at(file), Entry::File(bytes_of(file)));
    }
    tree
}

/// Both fonts with their license files and a manifest that lists them.
fn valid() -> Tree {
    tree_of(&manifest(), &FILES)
}

/// The tree of [`valid`] with `old` replaced by `new`, once, in its
/// manifest.
fn valid_with(old: &str, new: &str) -> Tree {
    let manifest = manifest();
    assert!(manifest.contains(old), "{old}");
    tree_of(&manifest.replacen(old, new, 1), &FILES)
}

/// The problems of `tree` and how many fonts its manifest lists.
fn problems(tree: &Tree) -> (Vec<String>, usize) {
    let mut problems = Problems::default();
    let count = check(tree, &FONTS, &mut problems);
    (problems.lines(), count)
}

#[test]
fn both_fonts_with_their_license_files_and_the_manifest_pass() {
    assert_eq!(problems(&valid()), (Vec::new(), 2));
}

#[test]
fn a_font_changed_by_one_byte_is_reported_with_both_digests() {
    let mut tree = valid();
    let mut bytes = bytes_of(SANS);
    bytes[0] ^= 1;
    let changed = digest(&bytes);
    tree.insert(&at(SANS), Entry::File(bytes));
    assert_eq!(
        problems(&tree),
        (
            vec![format!(
                "{}: SHA-256 {changed} differs from {}, which MANIFEST.toml lists; bundled fonts and their license files are unmodified",
                at(SANS),
                digest(&bytes_of(SANS))
            )],
            2
        )
    );
}

#[test]
fn the_bundled_fira_sans_changed_by_one_byte_is_reported_with_both_digests() {
    // The tracked files of this repository, with one byte of the bundled
    // Fira Sans flipped; its manifest lists the digest of the google/fonts
    // file.
    let mut tree = Tree::load(&repo::root()).expect("the repository is a Git working copy");
    let path = at(SANS);
    let Some(Entry::File(mut bytes)) = tree.entry(&path).cloned() else {
        panic!("{path} is a regular file");
    };
    bytes[0] ^= 1;
    let changed = digest(&bytes);
    tree.insert(&path, Entry::File(bytes));
    assert_eq!(
        problems(&tree),
        (
            vec![format!(
                "{path}: SHA-256 {changed} differs from c29556a2719bf613ef3d5e070e40d903a8965d9c081beca1375dc1e6e0f93c23, which MANIFEST.toml lists; bundled fonts and their license files are unmodified"
            )],
            2
        )
    );
}

#[test]
fn a_changed_license_file_is_reported() {
    let mut tree = valid();
    tree.insert(&at(MONO_LICENSE), Entry::File(b"another license".to_vec()));
    assert_eq!(
        problems(&tree).0,
        [format!(
            "{}: SHA-256 {} differs from {}, which MANIFEST.toml lists; bundled fonts and their license files are unmodified",
            at(MONO_LICENSE),
            digest(b"another license"),
            digest(&bytes_of(MONO_LICENSE))
        )]
    );
}

#[test]
fn a_file_that_the_manifest_does_not_list_is_reported() {
    // A CJK font dropped into the asset directory.
    let cjk = "notosanscjk/NotoSansCJKsc-Regular.otf";
    let mut tree = valid();
    tree.insert(&at(cjk), Entry::File(bytes_of(cjk)));
    tree.insert(&at("firasans/METADATA.pb"), Entry::File(Vec::new()));
    assert_eq!(
        problems(&tree),
        (
            vec![
                format!(
                    "{}: is not listed in MANIFEST.toml; the asset directory holds only the manifest and the files it lists",
                    at("firasans/METADATA.pb")
                ),
                format!(
                    "{}: is not listed in MANIFEST.toml; the asset directory holds only the manifest and the files it lists",
                    at(cjk)
                ),
            ],
            2
        )
    );
}

#[test]
fn a_second_manifest_in_the_asset_directory_is_reported() {
    let mut tree = valid();
    tree.insert(&at("firasans/MANIFEST.toml"), Entry::File(Vec::new()));
    assert_eq!(
        problems(&tree),
        (
            vec![format!(
                "{}: is not listed in MANIFEST.toml; the asset directory holds only the manifest and the files it lists",
                at("firasans/MANIFEST.toml")
            )],
            2
        )
    );
}

#[test]
fn a_third_font_is_reported_even_when_the_manifest_lists_it() {
    let (font, license) = (
        "notosanscjk/NotoSansCJKsc-Regular.otf",
        "notosanscjk/OFL.txt",
    );
    let manifest = format!("{}\n{}", manifest(), table(font, license));
    let tree = tree_of(
        &manifest,
        &[MONO, MONO_LICENSE, SANS, SANS_LICENSE, font, license],
    );
    assert_eq!(
        problems(&tree),
        (
            vec![format!(
                "{MANIFEST_PATH}:27: `{font}` is not one of the fonts that kosmo bundles, {MONO}, {SANS}; another font is a design change"
            )],
            3
        )
    );
}

#[test]
fn a_font_outside_the_asset_directory_is_reported() {
    let mut tree = valid();
    tree.insert(
        "framework/ui/assets/NotoSansCJK.TTF",
        Entry::File(Vec::new()),
    );
    tree.insert("docs/fonts/extra.woff2", Entry::File(Vec::new()));
    tree.insert("docs/fonts/README.md", Entry::File(Vec::new()));
    // A directory beside the asset directory whose name starts with the
    // name of the asset directory is outside it too.
    tree.insert(
        "framework/text/assets/fonts-extra/Extra.ttf",
        Entry::File(Vec::new()),
    );
    assert_eq!(
        problems(&tree).0,
        [
            format!(
                "docs/fonts/extra.woff2: is a font outside {DIR}, the only place for bundled fonts"
            ),
            format!(
                "framework/text/assets/fonts-extra/Extra.ttf: is a font outside {DIR}, the only place for bundled fonts"
            ),
            format!(
                "framework/ui/assets/NotoSansCJK.TTF: is a font outside {DIR}, the only place for bundled fonts"
            ),
        ]
    );
}

#[test]
fn binary_fonts_and_links_outside_the_asset_directory_are_reported() {
    let mut tree = valid();
    // The first bytes of a TrueType font and a byte that is not UTF-8, as
    // the bytes of a real font are not.
    tree.insert(
        "docs/fonts/Extra.ttf",
        Entry::File(vec![0x00, 0x01, 0x00, 0x00, 0xff]),
    );
    tree.insert(
        "docs/fonts/Link.otf",
        Entry::Symlink(
            "../../framework/text/assets/fonts/firasans/FiraSans-Regular.ttf".to_string(),
        ),
    );
    assert_eq!(
        problems(&tree).0,
        [
            format!(
                "docs/fonts/Extra.ttf: is a font outside {DIR}, the only place for bundled fonts"
            ),
            format!(
                "docs/fonts/Link.otf: is a font outside {DIR}, the only place for bundled fonts"
            ),
        ]
    );
}

#[test]
fn a_font_outside_the_asset_directory_is_found_by_its_last_extension() {
    let mut tree = valid();
    tree.insert("docs/fonts/NotoSans.Regular.ttf", Entry::File(Vec::new()));
    tree.insert("docs/fonts/notes.ttf.md", Entry::File(Vec::new()));
    assert_eq!(
        problems(&tree).0,
        [format!(
            "docs/fonts/NotoSans.Regular.ttf: is a font outside {DIR}, the only place for bundled fonts"
        )]
    );
}

#[test]
fn a_listed_file_that_is_missing_is_reported() {
    let tree = tree_of(&manifest(), &[MONO, SANS, SANS_LICENSE]);
    assert_eq!(
        problems(&tree),
        (
            vec![format!("{MANIFEST_PATH}:3: `{MONO_LICENSE}` has no file")],
            2
        )
    );
}

#[test]
fn a_size_that_differs_from_the_manifest_is_reported() {
    let size = bytes_of(SANS).len();
    let tree = valid_with(
        &format!("bytes = {size}\nsha256 = \"{}\"", digest(&bytes_of(SANS))),
        &format!(
            "bytes = {}\nsha256 = \"{}\"",
            size + 1,
            digest(&bytes_of(SANS))
        ),
    );
    assert_eq!(
        problems(&tree).0,
        [format!(
            "{}: is {size} bytes; MANIFEST.toml lists {}",
            at(SANS),
            size + 1
        )]
    );
}

#[test]
fn a_license_other_than_ofl_1_1_is_reported() {
    let tree = valid_with("license = \"OFL-1.1\"", "license = \"OFL-1.0\"");
    assert_eq!(
        problems(&tree).0,
        [format!(
            "{MANIFEST_PATH}:3: the license of `{MONO}` is `OFL-1.0`; kosmo bundles only unmodified OFL-1.1 fonts"
        )]
    );
}

#[test]
fn a_license_file_outside_the_directory_of_its_font_is_reported() {
    let message = |license: &str| {
        format!(
            "{MANIFEST_PATH}:15: the license file `{license}` of `{SANS}` is not another file in the directory of the font"
        )
    };
    let tree = valid_with(
        &format!("license-file = \"{SANS_LICENSE}\""),
        "license-file = \"OFL.txt\"",
    );
    // The license file that the manifest names instead has no file, and
    // the one in the directory is not listed any more.
    assert_eq!(
        problems(&tree).0,
        [
            format!("{MANIFEST_PATH}:15: `OFL.txt` has no file"),
            message("OFL.txt"),
            format!(
                "{}: is not listed in MANIFEST.toml; the asset directory holds only the manifest and the files it lists",
                at(SANS_LICENSE)
            ),
        ]
    );
    for license in ["firamono/OFL.txt", SANS, "firasans/../firasans/OFL.txt"] {
        let tree = valid_with(
            &format!("license-file = \"{SANS_LICENSE}\""),
            &format!("license-file = \"{license}\""),
        );
        assert!(problems(&tree).0.contains(&message(license)), "{license}");
    }
}

#[test]
fn a_license_file_with_another_name_is_reported() {
    // Another file of the directory of Fira Sans, listed with its digest as
    // the license file in place of the original OFL.txt.
    let renamed = "firasans/LICENSE.txt";
    let old = format!(
        "license-file = \"{SANS_LICENSE}\"\nlicense-sha256 = \"{}\"",
        digest(&bytes_of(SANS_LICENSE))
    );
    let new = format!(
        "license-file = \"{renamed}\"\nlicense-sha256 = \"{}\"",
        digest(&bytes_of(renamed))
    );
    let manifest = manifest();
    assert!(manifest.contains(&old), "{old}");
    let tree = tree_of(
        &manifest.replacen(&old, &new, 1),
        &[MONO, MONO_LICENSE, SANS, renamed],
    );
    assert_eq!(LICENSE_FILE, "OFL.txt");
    assert_eq!(
        problems(&tree),
        (
            vec![format!(
                "{MANIFEST_PATH}:15: the license file `{renamed}` of `{SANS}` is not named OFL.txt, as the original license file is"
            )],
            2
        )
    );
}

#[test]
fn license_names_that_only_resemble_ofl_txt_are_reported() {
    for renamed in [
        "firasans/ofl.txt",
        "firasans/OFL.TXT",
        "firasans/NOTOFL.txt",
    ] {
        let old = format!(
            "license-file = \"{SANS_LICENSE}\"\nlicense-sha256 = \"{}\"",
            digest(&bytes_of(SANS_LICENSE))
        );
        let new = format!(
            "license-file = \"{renamed}\"\nlicense-sha256 = \"{}\"",
            digest(&bytes_of(renamed))
        );
        let tree = tree_of(
            &manifest().replacen(&old, &new, 1),
            &[MONO, MONO_LICENSE, SANS, renamed],
        );
        assert_eq!(
            problems(&tree),
            (
                vec![format!(
                    "{MANIFEST_PATH}:15: the license file `{renamed}` of `{SANS}` is not named OFL.txt, as the original license file is"
                )],
                2
            ),
            "{renamed}"
        );
    }
}

#[test]
fn a_missing_or_unreadable_manifest_is_reported() {
    let mut tree = Tree::default();
    for file in FILES {
        tree.insert(&at(file), Entry::File(bytes_of(file)));
    }
    // The manifest is reported alone, and the files are not compared with
    // it.
    assert_eq!(
        problems(&tree),
        (
            vec![format!(
                "{MANIFEST_PATH}: is missing; the asset manifest lists the bundled fonts"
            )],
            0
        )
    );
    tree.insert(MANIFEST_PATH, Entry::File(vec![0xff]));
    assert_eq!(
        problems(&tree),
        (vec![format!("{MANIFEST_PATH}: is not UTF-8")], 0)
    );
    tree.insert(MANIFEST_PATH, Entry::Symlink("elsewhere".to_string()));
    assert_eq!(
        problems(&tree),
        (vec![format!("{MANIFEST_PATH}: is not a regular file")], 0)
    );
}

#[test]
fn a_manifest_that_does_not_parse_is_reported_with_its_line() {
    let tree = valid_with(
        "version = \"1.000\"\n",
        "version = \"1.000\"\nweight = 400\n",
    );
    let (found, count) = problems(&tree);
    assert_eq!(count, 0);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0].starts_with(&format!("{MANIFEST_PATH}:10: unknown field `weight`")),
        "{found:?}"
    );
    let tree = valid_with("bytes = ", "bytes = \"many\" # ");
    let found = problems(&tree).0;
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0].starts_with(&format!("{MANIFEST_PATH}:5: invalid type")),
        "{found:?}"
    );
}

#[test]
fn fonts_out_of_order_or_listed_twice_are_reported() {
    let swapped = format!(
        "{}\n{}",
        table(SANS, SANS_LICENSE),
        table(MONO, MONO_LICENSE)
    );
    assert_eq!(
        problems(&tree_of(&swapped, &FILES)).0,
        [format!(
            "{MANIFEST_PATH}:13: `{MONO}` follows `{SANS}`; the fonts are sorted by `file`"
        )]
    );
    let twice = format!("{}\n{}", manifest(), table(SANS, SANS_LICENSE));
    assert_eq!(
        problems(&tree_of(&twice, &FILES)),
        (
            vec![
                format!("{MANIFEST_PATH}:27: `{SANS}` is listed twice, first on line 15"),
                format!("{MANIFEST_PATH}:27: `{SANS_LICENSE}` is listed twice, first on line 15"),
            ],
            3
        )
    );
}

#[test]
fn a_font_that_the_manifest_leaves_out_is_reported() {
    let manifest = table(SANS, SANS_LICENSE);
    let tree = tree_of(&manifest, &[SANS, SANS_LICENSE]);
    assert_eq!(
        problems(&tree),
        (
            vec![format!(
                "{MANIFEST_PATH}: does not list `{MONO}`; the manifest lists every bundled font"
            )],
            1
        )
    );
}

#[test]
fn malformed_fields_are_reported() {
    let digest = digest(&bytes_of(MONO));
    let tree = valid_with(&digest, &digest.to_uppercase());
    assert_eq!(
        problems(&tree).0[0],
        format!(
            "{MANIFEST_PATH}:3: the sha256 of `{MONO}`, `{}`, is not 64 lowercase hexadecimal digits",
            digest.to_uppercase()
        )
    );
    for field in ["source", "upstream", "version", "role"] {
        let tree = valid_with(&format!("{field} = \""), &format!("{field} = \" \" # "));
        assert_eq!(
            problems(&tree).0,
            [format!(
                "{MANIFEST_PATH}:3: the {field} of `{MONO}` is empty"
            )]
        );
    }
    // The last path holds a backslash, escaped in the TOML string.
    for file in [
        "./firamono",
        "firamono//FiraMono-Regular.ttf",
        "firamono\\\\FiraMono-Regular.ttf",
    ] {
        let tree = valid_with(&format!("file = \"{MONO}\""), &format!("file = \"{file}\""));
        let shown = file.replace("\\\\", "\\");
        let message =
            format!("{MANIFEST_PATH}:3: `{shown}` is not a path relative to the asset directory");
        assert!(problems(&tree).0.contains(&message), "{file}");
    }
}

#[test]
fn a_listed_file_that_is_not_a_regular_file_is_reported() {
    let mut tree = valid();
    tree.insert(&at(SANS_LICENSE), Entry::Symlink("../OFL.txt".to_string()));
    assert_eq!(
        problems(&tree).0,
        [format!("{}: is not a regular file", at(SANS_LICENSE))]
    );
}

#[test]
fn every_problem_is_reported_at_once_in_order() {
    let mut tree = valid_with("license = \"OFL-1.1\"", "license = \"MIT\"");
    let mut bytes = bytes_of(MONO);
    bytes.push(b'!');
    tree.insert(&at(MONO), Entry::File(bytes.clone()));
    tree.insert(&at("extra.ttf"), Entry::File(Vec::new()));
    let size = bytes_of(MONO).len();
    assert_eq!(
        problems(&tree).0,
        [
            format!(
                "{MANIFEST_PATH}:3: the license of `{MONO}` is `MIT`; kosmo bundles only unmodified OFL-1.1 fonts"
            ),
            format!(
                "{}: is not listed in MANIFEST.toml; the asset directory holds only the manifest and the files it lists",
                at("extra.ttf")
            ),
            format!(
                "{}: SHA-256 {} differs from {}, which MANIFEST.toml lists; bundled fonts and their license files are unmodified",
                at(MONO),
                digest(&bytes),
                digest(&bytes_of(MONO))
            ),
            format!(
                "{}: is {} bytes; MANIFEST.toml lists {size}",
                at(MONO),
                size + 1
            ),
        ]
    );
}

#[test]
fn the_font_assets_of_this_repository_pass() {
    let tree = Tree::load(&repo::root()).expect("the repository is a Git working copy");
    assert_eq!(problems(&tree), (Vec::new(), FONTS.fonts.len()));
}

#[test]
fn the_manifest_of_this_repository_lists_the_unmodified_fonts_of_google_fonts() {
    // The fonts and license files of google/fonts at commit
    // 29b4ec6df2522a8aee279414b78a07910b065874, with their sizes and
    // SHA-256 digests, the commit of FiraGFVersion that google/fonts
    // records for both fonts and the version in the name table of each.
    let tree = Tree::load(&repo::root()).expect("the repository is a Git working copy");
    let mut problems = Problems::default();
    let fonts = read(&tree, MANIFEST_PATH, &mut problems).expect("the manifest reads");
    assert_eq!(problems.lines(), Vec::<String>::new());
    let commit = "google/fonts@29b4ec6df2522a8aee279414b78a07910b065874";
    let upstream = "googlefonts/FiraGFVersion@a7d6892325d5b7382e426bea13bd3ef2c32bd53e";
    let found: Vec<_> = fonts
        .iter()
        .map(|(_, font)| {
            (
                font.file.as_str(),
                font.bytes,
                font.sha256.as_str(),
                font.source.as_str(),
                font.upstream.as_str(),
                font.version.as_str(),
                font.license_file.as_str(),
                font.license_sha256.as_str(),
            )
        })
        .collect();
    assert_eq!(
        found,
        [
            (
                MONO,
                174_596,
                "2e00b0cf3106a3d792f35711b7722740b76c02ee7300ea8c60f940584b7a8ac9",
                format!("{commit}:ofl/{MONO}").as_str(),
                upstream,
                "3.206",
                MONO_LICENSE,
                "d1829ff3455bbf3b9d13dfb0a57758a863107c9108aab48558f26f37f1094180",
            ),
            (
                SANS,
                456_996,
                "c29556a2719bf613ef3d5e070e40d903a8965d9c081beca1375dc1e6e0f93c23",
                format!("{commit}:ofl/{SANS}").as_str(),
                upstream,
                "4.203",
                SANS_LICENSE,
                "8f24842e9174beda18a556c2ae7d54f5dc444340c19a3a9ef77e23bca366adbd",
            ),
        ]
    );
}

#[test]
fn the_goldens_step_reports_the_problems_of_the_font_assets() {
    let mut tree = valid();
    tree.insert(&at(SANS), Entry::File(Vec::new()));
    let mut facts = Facts::new();
    let error = golden::report(&tree, &[], Some(&FONTS), &mut facts).unwrap_err();
    assert!(
        error.starts_with(&format!("{}: SHA-256 ", at(SANS))),
        "{error}"
    );
    let counts = [("goldens", "0"), ("fonts", "2")];
    assert_eq!(
        facts,
        counts.map(|(key, value)| (key.to_string(), value.to_string()))
    );
    let mut facts = Facts::new();
    assert_eq!(
        golden::report(&valid(), &[], Some(&FONTS), &mut facts),
        Ok(())
    );
}
