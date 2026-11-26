use super::*;

const DIR: &str = "framework/render/tests/golden";
const MANIFEST: &str = "framework/render/tests/golden/MANIFEST";
const TESTS: &str = "framework/render/tests/golden.rs";

/// The bytes of the golden `name` in the fixtures.
fn bytes_of(name: &str) -> Vec<u8> {
    format!("pixels of {name}").into_bytes()
}

/// The manifest line of the golden `name` with the bytes of the fixtures.
fn line_of(name: &str) -> String {
    format!("{}  {name}\n", digest(&bytes_of(name)))
}

/// A test file that defines `matches_golden` on lines 3 to 5 and calls it
/// with each of `referenced`, the first on line 9 and each further one 5
/// lines later.
fn tests_of(referenced: &[&str]) -> String {
    let mut tests = String::from(
        "pub mod support;\n\nfn matches_golden(name: &str) -> Result<(), String> {\n    support::check(name)\n}\n",
    );
    for name in referenced {
        tests.push_str(&format!(
            "\n#[test]\nfn draws() -> Result<(), String> {{\n    matches_golden(\"{name}\")\n}}\n"
        ));
    }
    tests
}

/// A crate whose tests refer to `referenced` and whose golden directory
/// holds `files`, with the bytes of the fixtures, and `manifest`.
fn tree(files: &[&str], manifest: &str, referenced: &[&str]) -> Tree {
    let mut tree = Tree::from_files(&[(TESTS, &tests_of(referenced)), (MANIFEST, manifest)]);
    for name in files {
        let path = format!("{DIR}/{name}.px");
        tree.insert(&path, Entry::File(bytes_of(name)));
    }
    tree
}

/// Two goldens, each with its file, its line and a test.
fn valid() -> Tree {
    let manifest = line_of("fill-rect") + &line_of("glyphs");
    tree(
        &["fill-rect", "glyphs"],
        &manifest,
        &["fill-rect", "glyphs"],
    )
}

/// The problems of `tree` and how many goldens its manifests list.
fn problems(tree: &Tree) -> (Vec<String>, usize) {
    let mut problems = Problems::default();
    let count = check(tree, &SETS, &mut problems);
    (problems.lines(), count)
}

#[test]
fn digests_are_sha_256_in_lowercase_hexadecimal() {
    assert_eq!(
        digest(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        digest(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn goldens_with_their_files_lines_and_tests_pass() {
    assert_eq!(problems(&valid()), (Vec::new(), 2));
}

#[test]
fn a_crate_without_goldens_passes() {
    let tree = Tree::from_files(&[(TESTS, "fn matches_golden(name: &str) {}\n")]);
    assert_eq!(problems(&tree), (Vec::new(), 0));
}

#[test]
fn references_are_found_anywhere_in_the_crate_and_only_in_code() {
    let mut tree = valid();
    let support = "mod inner {\n    fn f() {\n        let _ = [support::golden::matches_golden(\"glyphs\")];\n    }\n}\n\
        /// matches_golden(\"orphan\")\nconst A: &str = \"matches_golden(\\\"orphan\\\")\";\n\
        // matches_golden(\"orphan\")\nfn g(name: &str) { matches_golden(name); matches_golden[\"orphan\"]; }\n";
    tree.insert(
        "framework/render/tests/support/golden.rs",
        Entry::File(support.as_bytes().to_vec()),
    );
    // The reference outside the crate does not count.
    tree.insert(
        "framework/ui/tests/draw.rs",
        Entry::File(b"fn f() { matches_golden(\"orphan\"); }\n".to_vec()),
    );
    let manifest = line_of("fill-rect") + &line_of("glyphs") + &line_of("orphan");
    tree.insert(MANIFEST, Entry::File(manifest.into_bytes()));
    tree.insert(&format!("{DIR}/orphan.px"), Entry::File(bytes_of("orphan")));
    let (found, _) = problems(&tree);
    assert_eq!(
        found,
        [format!(
            "{MANIFEST}:3: golden orphan is not referenced by any test"
        )]
    );
}

#[test]
fn a_hand_edited_golden_is_reported() {
    let mut tree = valid();
    let mut bytes = bytes_of("fill-rect");
    bytes[0] ^= 1;
    let changed = digest(&bytes);
    tree.insert(&format!("{DIR}/fill-rect.px"), Entry::File(bytes));
    assert_eq!(
        problems(&tree).0,
        [format!(
            "{DIR}/fill-rect.px: SHA-256 {changed} differs from MANIFEST; goldens change only through cargo xtask golden"
        )]
    );
}

#[test]
fn a_golden_that_no_test_references_is_reported() {
    let manifest = line_of("fill-rect") + &line_of("orphan");
    let tree = tree(&["fill-rect", "orphan"], &manifest, &["fill-rect"]);
    assert_eq!(
        problems(&tree).0,
        [format!(
            "{MANIFEST}:2: golden orphan is not referenced by any test"
        )]
    );
}

#[test]
fn a_golden_without_a_file_is_reported() {
    let manifest = line_of("fill-rect") + &line_of("missing");
    let tree = tree(&["fill-rect"], &manifest, &["fill-rect", "missing"]);
    assert_eq!(
        problems(&tree).0,
        [format!("{MANIFEST}:2: golden missing has no file")]
    );
}

#[test]
fn a_file_that_the_manifest_lacks_is_reported() {
    let mut tree = valid();
    tree.insert(&format!("{DIR}/extra.px"), Entry::File(bytes_of("extra")));
    assert_eq!(
        problems(&tree).0,
        [format!("{DIR}/extra.px: golden extra is not in MANIFEST")]
    );
}

#[test]
fn each_bad_line_is_reported() {
    let digest = digest(b"other");
    let upper = digest.to_uppercase();
    let manifest = [
        line_of("fill-rect"),
        format!("{}  short\n", &digest[..63]),
        format!("{upper}  upper\n"),
        format!("{digest} one-space\n"),
        format!("{digest}  three  fields\n"),
        format!("{digest}  Fill_Rect\n"),
        format!("{digest}  crlf\r\n"),
        "\n".to_string(),
        line_of("glyphs"),
    ]
    .concat();
    let tree = tree(
        &["fill-rect", "glyphs"],
        &manifest,
        &["fill-rect", "glyphs"],
    );
    let shape = "expected `<digest>  <name>`: the SHA-256 digest, two spaces and the name";
    let (found, count) = problems(&tree);
    assert_eq!(
        found,
        [
            format!(
                "{MANIFEST}:2: `{}` is not 64 lowercase hexadecimal digits",
                &digest[..63]
            ),
            format!("{MANIFEST}:3: `{upper}` is not 64 lowercase hexadecimal digits"),
            format!("{MANIFEST}:4: {shape}"),
            format!("{MANIFEST}:5: {shape}"),
            format!(
                "{MANIFEST}:6: `Fill_Rect` is not a golden name of lowercase letters, digits and `-`"
            ),
            format!(
                "{MANIFEST}:7: `crlf\r` is not a golden name of lowercase letters, digits and `-`"
            ),
            format!("{MANIFEST}:8: {shape}"),
        ]
    );
    assert_eq!(count, 2);
}

#[test]
fn a_name_listed_twice_is_reported() {
    let manifest = line_of("fill-rect") + &line_of("glyphs") + &line_of("glyphs");
    let tree = tree(
        &["fill-rect", "glyphs"],
        &manifest,
        &["fill-rect", "glyphs"],
    );
    assert_eq!(
        problems(&tree).0,
        [format!(
            "{MANIFEST}:3: golden glyphs is listed twice, first on line 2"
        )]
    );
}

#[test]
fn unsorted_lines_are_reported() {
    let manifest = line_of("glyphs") + &line_of("fill-rect");
    let tree = tree(
        &["fill-rect", "glyphs"],
        &manifest,
        &["fill-rect", "glyphs"],
    );
    assert_eq!(
        problems(&tree).0,
        [format!(
            "{MANIFEST}:2: golden fill-rect follows glyphs; lines are sorted by name"
        )]
    );
}

#[test]
fn a_reference_to_a_golden_that_the_manifest_lacks_is_reported() {
    let manifest = line_of("fill-rect") + &line_of("glyphs");
    let tree = tree(
        &["fill-rect", "glyphs"],
        &manifest,
        &["fill-rect", "nowhere", "glyphs"],
    );
    assert_eq!(
        problems(&tree).0,
        [format!("{TESTS}:14: golden nowhere is not in MANIFEST")]
    );
}

#[test]
fn a_missing_manifest_lists_no_goldens() {
    let mut tree = Tree::from_files(&[(TESTS, &tests_of(&["fill-rect"]))]);
    tree.insert(
        &format!("{DIR}/fill-rect.px"),
        Entry::File(bytes_of("fill-rect")),
    );
    assert_eq!(
        problems(&tree),
        (
            vec![
                format!("{TESTS}:9: golden fill-rect is not in MANIFEST"),
                format!("{DIR}/fill-rect.px: golden fill-rect is not in MANIFEST"),
            ],
            0
        )
    );
}

#[test]
fn other_files_and_golden_directories_elsewhere_are_reported() {
    let mut tree = valid();
    let other = "is not a golden: a golden directory holds MANIFEST and a regular file `<name>.px` for each golden";
    for path in ["README.md", "Fill.px", "sub/fill-rect.px", "link.px"] {
        let entry = match path {
            "link.px" => Entry::Symlink("fill-rect.px".to_string()),
            _ => Entry::File(Vec::new()),
        };
        tree.insert(&format!("{DIR}/{path}"), entry);
    }
    tree.insert(
        "framework/ui/tests/golden/button.px",
        Entry::File(Vec::new()),
    );
    tree.insert("tests/golden/MANIFEST", Entry::File(Vec::new()));
    // Not in a golden directory.
    tree.insert("framework/ui/tests/golden.rs", Entry::File(Vec::new()));
    assert_eq!(
        problems(&tree).0,
        [
            format!("{DIR}/Fill.px: {other}"),
            format!("{DIR}/README.md: {other}"),
            format!("{DIR}/link.px: {other}"),
            format!("{DIR}/sub/fill-rect.px: {other}"),
            "framework/ui/tests/golden/button.px: is in a golden directory that `cargo xtask golden` does not write".to_string(),
            "tests/golden/MANIFEST: is in a golden directory that `cargo xtask golden` does not write".to_string(),
        ]
    );
}

#[test]
fn a_manifest_that_cannot_be_read_is_reported() {
    let mut tree = valid();
    tree.insert(MANIFEST, Entry::File(vec![0xff, b'\n']));
    let (found, count) = problems(&tree);
    assert!(
        found.contains(&format!("{MANIFEST}: is not UTF-8")),
        "{found:?}"
    );
    assert_eq!(count, 0);
    tree.insert(MANIFEST, Entry::Symlink("elsewhere".to_string()));
    let (found, count) = problems(&tree);
    let expected = format!("{MANIFEST}: is not a regular file");
    assert!(found.contains(&expected), "{found:?}");
    assert_eq!(count, 0);
}

#[test]
fn every_problem_is_reported_in_order() {
    let manifest = line_of("fill-rect") + &line_of("missing") + &line_of("orphan");
    let mut tree = tree(
        &["fill-rect", "orphan", "extra"],
        &manifest,
        &["fill-rect", "missing", "nowhere"],
    );
    tree.insert(&format!("{DIR}/fill-rect.px"), Entry::File(Vec::new()));
    let (found, count) = problems(&tree);
    assert_eq!(
        found,
        [
            format!("{TESTS}:19: golden nowhere is not in MANIFEST"),
            format!("{MANIFEST}:2: golden missing has no file"),
            format!("{MANIFEST}:3: golden orphan is not referenced by any test"),
            format!("{DIR}/extra.px: golden extra is not in MANIFEST"),
            format!(
                "{DIR}/fill-rect.px: SHA-256 {} differs from MANIFEST; goldens change only through cargo xtask golden",
                digest(b"")
            ),
        ]
    );
    assert_eq!(count, 3);
}

#[test]
fn the_goldens_of_this_repository_pass() {
    let tree = Tree::load(&repo::root()).expect("the repository is a Git working copy");
    let (found, _) = problems(&tree);
    assert_eq!(found, Vec::<String>::new());
}
