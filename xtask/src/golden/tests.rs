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

/// The two ignored tests that write the goldens.
const WRITERS: &str = "\n#[test]\n#[ignore = \"writes the goldens\"]\nfn write_goldens() -> Result<(), String> {\n    support::write()\n}\n\n#[test]\n#[ignore = \"draws the goldens again\"]\nfn redraw_goldens() -> Result<(), String> {\n    support::redraw()\n}\n";

/// A test file that defines `matches_golden` on lines 3 to 5, calls it
/// with each of `referenced`, the first on line 9 and each further one 5
/// lines later, and ends with the ignored tests that write the goldens.
fn tests_of(referenced: &[&str]) -> String {
    let mut tests = String::from(
        "pub mod support;\n\nfn matches_golden(name: &str) -> Result<(), String> {\n    support::check(name)\n}\n",
    );
    for name in referenced {
        tests.push_str(&format!(
            "\n#[test]\nfn draws() -> Result<(), String> {{\n    matches_golden(\"{name}\")\n}}\n"
        ));
    }
    tests.push_str(WRITERS);
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
    let tree = Tree::from_files(&[(TESTS, &tests_of(&[]))]);
    assert_eq!(problems(&tree), (Vec::new(), 0));
}

/// The tree of [`valid`] with `old` replaced by `new`, once, in its test
/// file.
fn valid_with(old: &str, new: &str) -> Tree {
    let tests = tests_of(&["fill-rect", "glyphs"]);
    assert!(tests.contains(old), "{old}");
    let mut tree = valid();
    tree.insert(TESTS, Entry::File(tests.replacen(old, new, 1).into_bytes()));
    tree
}

#[test]
fn a_writer_that_is_not_ignored_is_reported() {
    let tree = valid_with("#[ignore = \"writes the goldens\"]\n", "");
    assert_eq!(
        problems(&tree).0,
        [format!(
            "{TESTS}: has no ignored test `write_goldens`, which `cargo xtask golden` runs"
        )]
    );
}

#[test]
fn a_renamed_writer_is_reported() {
    let tree = valid_with("fn redraw_goldens()", "fn redraw_all_goldens()");
    assert_eq!(
        problems(&tree).0,
        [
            format!(
                "{TESTS}: has no ignored test `redraw_goldens`, which `cargo xtask golden` runs"
            ),
            format!(
                "{TESTS}: ignores the test `redraw_all_goldens`; only the writers of the goldens are ignored"
            ),
        ]
    );
}

#[test]
fn an_ignored_comparison_is_reported() {
    let tree = valid_with("#[test]\nfn draws()", "#[test]\n#[ignore]\nfn draws()");
    assert_eq!(
        problems(&tree).0,
        [format!(
            "{TESTS}: ignores the test `draws`; only the writers of the goldens are ignored"
        )]
    );
}

#[test]
fn references_are_found_anywhere_in_the_crate_and_only_in_code() {
    let mut tree = valid();
    let support = "mod inner {\n    fn f() {\n        let _ = [support::golden::matches_golden(\"glyphs\")];\n    }\n}\n\
        /// matches_golden(\"orphan\")\nconst A: &str = \"matches_golden(\\\"orphan\\\")\";\n\
        // matches_golden(\"orphan\")\nfn g(name: &str) { matches_golden(name); matches_golden(&\"orphan\".to_string()); matches_golden[\"orphan\"]; }\n";
    tree.insert(
        "framework/render/tests/support/golden.rs",
        Entry::File(support.as_bytes().to_vec()),
    );
    // The reference outside the crate does not count.
    tree.insert(
        "framework/ui/tests/draw.rs",
        Entry::File(b"fn f() { matches_golden(\"orphan\"); }\n".to_vec()),
    );
    // Nor does a call in a file that is not Rust.
    tree.insert(
        "framework/render/README.md",
        Entry::File(b"matches_golden(\"orphan\")\n".to_vec()),
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
        format!("{}g  past-f\n", &digest[..63]),
        format!("{digest}  Fill-Rect\n"),
        format!("{digest}  fill_rect\n"),
        format!("{digest}  \n"),
        format!("{digest}0  long\n"),
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
            format!(
                "{MANIFEST}:9: `{}g` is not 64 lowercase hexadecimal digits",
                &digest[..63]
            ),
            format!(
                "{MANIFEST}:10: `Fill-Rect` is not a golden name of lowercase letters, digits and `-`"
            ),
            format!(
                "{MANIFEST}:11: `fill_rect` is not a golden name of lowercase letters, digits and `-`"
            ),
            format!("{MANIFEST}:12: `` is not a golden name of lowercase letters, digits and `-`"),
            format!("{MANIFEST}:13: `{digest}0` is not 64 lowercase hexadecimal digits"),
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

#[test]
fn the_step_fails_with_its_problems_and_counts_the_goldens() {
    let mut tree = valid();
    tree.insert(
        &format!("{DIR}/fill-rect.px"),
        Entry::File(b"edited".to_vec()),
    );
    let mut facts = Facts::new();
    let error = report(&tree, &SETS, &mut facts).unwrap_err();
    assert!(
        error.starts_with(&format!("{DIR}/fill-rect.px: SHA-256 ")),
        "{error}"
    );
    assert_eq!(facts, [("goldens".to_string(), "2".to_string())]);
    assert_eq!(report(&valid(), &SETS, &mut Facts::new()), Ok(()));
}

#[test]
fn the_check_runs_the_goldens_step_on_this_repository() {
    let step = check::steps()
        .into_iter()
        .find(|step| step.name == "goldens")
        .unwrap();
    let check::Action::Internal(run) = step.action else {
        panic!("the goldens step is written in this crate");
    };
    let mut facts = Facts::new();
    assert_eq!(run(&repo::root(), &mut facts), Ok(()));
    let listed = fs::read_to_string(repo::root().join(MANIFEST))
        .unwrap()
        .lines()
        .count();
    assert_eq!(facts, [("goldens".to_string(), listed.to_string())]);
}

#[test]
fn the_command_runs_the_ignored_writers_of_each_crate() {
    let args = |test: &str| SETS[0].writer(test).args.join(" ");
    let start = "test --locked --manifest-path framework/Cargo.toml -p kosmo-render --test golden";
    assert_eq!(
        args(WRITE),
        format!("{start} -- --ignored --exact write_goldens")
    );
    assert_eq!(
        args(REDRAW),
        format!("{start} -- --ignored --exact redraw_goldens")
    );
    assert_eq!(SETS[0].golden_dir(), DIR);
    assert_eq!(SETS[0].redrawn(), "framework/target/golden/kosmo-render");
}

/// An empty scratch directory in this crate's ignored build directory.
fn scratch(name: &str) -> std::path::PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join(format!("golden-test-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn the_manifest_lists_every_golden_of_the_directory_by_name() {
    let dir = scratch("manifest");
    for name in ["glyphs", "clip-group", "fill-rect"] {
        fs::write(dir.join(format!("{name}.px")), bytes_of(name)).unwrap();
    }
    fs::write(dir.join("MANIFEST"), "stale\n").unwrap();
    // A file of the Finder, which the manifest leaves out.
    fs::write(dir.join(".DS_Store"), b"\0\0\0\x01Bud1").unwrap();
    let text = manifest_text(&digests_in(&dir).unwrap());
    let expected = line_of("clip-group") + &line_of("fill-rect") + &line_of("glyphs");
    assert_eq!(text, expected);
    let mut problems = Problems::default();
    let listed = parse_manifest("MANIFEST", &text, &mut problems);
    assert_eq!(problems.lines(), Vec::<String>::new());
    assert_eq!(listed.len(), 3);
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_command_writes_every_golden_again_and_fails_without_any() {
    let root = scratch("update");
    let set = Goldens {
        workspace: "w",
        package: "p",
        dir: "c",
    };
    let dir = root.join(set.golden_dir());
    fs::create_dir_all(&dir).unwrap();
    for name in ["fill-rect", "gone"] {
        fs::write(dir.join(format!("{name}.px")), bytes_of(name)).unwrap();
    }
    fs::write(dir.join(".DS_Store"), b"").unwrap();
    // A writer that writes nothing leaves no golden and no manifest.
    assert_eq!(
        write_set(&root, &set, || Ok(())),
        Err("golden: the test write_goldens of p wrote no golden into c/tests/golden".to_string())
    );
    assert!(!dir.join("fill-rect.px").exists() && !dir.join("gone.px").exists());
    assert!(!dir.join("MANIFEST").exists());
    // The file of a golden that the writer no longer writes goes.
    fs::write(dir.join("gone.px"), bytes_of("gone")).unwrap();
    let write = || {
        fs::write(dir.join("fill-rect.px"), bytes_of("fill-rect"))
            .map_err(|error| error.to_string())
    };
    assert_eq!(write_set(&root, &set, write), Ok(1));
    assert!(!dir.join("gone.px").exists() && dir.join(".DS_Store").exists());
    let manifest = fs::read_to_string(dir.join("MANIFEST")).unwrap();
    assert_eq!(manifest, line_of("fill-rect"));
    // The first writing creates the golden directory.
    let new = Goldens { dir: "d", ..set };
    let write = || {
        let dir = root.join(new.golden_dir());
        fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
        fs::write(dir.join("glyphs.px"), bytes_of("glyphs")).map_err(|error| error.to_string())
    };
    assert_eq!(write_set(&root, &new, write), Ok(1));
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn a_directory_with_another_file_gives_no_manifest() {
    for other in ["notes.txt", "Fill.px", "sub.px/"] {
        let dir = scratch("other");
        fs::write(dir.join("fill-rect.px"), bytes_of("fill-rect")).unwrap();
        match other.strip_suffix('/') {
            Some(sub) => fs::create_dir(dir.join(sub)).unwrap(),
            None => fs::write(dir.join(other), b"").unwrap(),
        }
        let error = digests_in(&dir).unwrap_err();
        let file = other.trim_end_matches('/');
        assert!(
            error.ends_with(&format!("/{file} is not a golden `<name>.px`")),
            "{error}"
        );
        fs::remove_dir_all(&dir).unwrap();
    }
}

#[test]
fn verify_fails_when_any_golden_differs() {
    assert_eq!(verdict(6, 0), Ok(()));
    assert_eq!(
        verdict(6, 1),
        Err("golden: 1 of 6 goldens differ from MANIFEST".to_string())
    );
}

#[test]
fn a_test_that_runs_only_under_a_cfg_is_reported() {
    let message = |name: &str| {
        format!(
            "{TESTS}: compiles `{name}` only under a `cfg`; the tests of the goldens run on every target"
        )
    };
    let attribute = "#[cfg_attr(target_os = \"linux\", ignore)]";
    let tree = valid_with(
        "#[test]\nfn draws()",
        &format!("#[test]\n{attribute}\nfn draws()"),
    );
    assert_eq!(problems(&tree).0, [message("draws")]);
    let tree = valid_with("#[test]\nfn draws()", "#[cfg(any())]\n#[test]\nfn draws()");
    assert_eq!(problems(&tree).0, [message("draws")]);
    let tree = valid_with("pub mod support;\n", "#![cfg(any())]\n\npub mod support;\n");
    assert_eq!(problems(&tree).0, [message("crate")]);
    let inner = "#![cfg_attr(target_os = \"linux\", cfg(any()))]";
    let tree = valid_with(
        "pub mod support;\n",
        &format!("{inner}\n\npub mod support;\n"),
    );
    assert_eq!(problems(&tree).0, [message("crate")]);
    let tree = valid_with("pub mod support;\n", "#[cfg(any())]\npub mod support;\n");
    assert_eq!(problems(&tree).0, [message("support")]);
}

#[test]
fn a_comparison_outside_a_test_is_reported() {
    let tree = valid_with(
        "#[test]\nfn draws()",
        "#[allow(dead_code, reason = \"r\")]\nfn draws()",
    );
    assert_eq!(
        problems(&tree).0,
        [format!(
            "{TESTS}:9: golden fill-rect is compared in a function that is not a test"
        )]
    );
}

#[test]
fn verify_compares_each_golden_drawn_or_listed_by_name() {
    let of = |name: &str| digest(&bytes_of(name));
    let drawn: BTreeMap<String, String> = [
        ("fill-rect", of("fill-rect")),
        ("glyphs", of("glyphs, drawn otherwise")),
        ("new", of("new")),
    ]
    .into_iter()
    .map(|(name, digest)| (name.to_string(), digest))
    .collect();
    let mut problems = Problems::default();
    let manifest = line_of("fill-rect") + &line_of("glyphs") + &line_of("gone");
    let listed = parse_manifest(MANIFEST, &manifest, &mut problems);
    assert_eq!(problems.lines(), Vec::<String>::new());
    let (lines, differ) = compare_digests(&drawn, &listed);
    assert_eq!(
        lines,
        [
            format!(
                "fill-rect: drawn {}, MANIFEST {}: same",
                of("fill-rect"),
                of("fill-rect")
            ),
            format!(
                "glyphs: drawn {}, MANIFEST {}: differs",
                of("glyphs, drawn otherwise"),
                of("glyphs")
            ),
            format!("gone: drawn -, MANIFEST {}: differs", of("gone")),
            format!("new: drawn {}, MANIFEST -: differs", of("new")),
        ]
    );
    assert_eq!(differ, 3);
    assert_eq!(compare_digests(&drawn, &BTreeMap::new()).1, 3);
    assert_eq!(
        compare_digests(&BTreeMap::new(), &BTreeMap::new()),
        (Vec::new(), 0)
    );
}
