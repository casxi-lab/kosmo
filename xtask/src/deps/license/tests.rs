//! Tests of the license checks: the evaluation of license expressions, the
//! licenses of the members, and the license exceptions.

use super::super::fixture::{self, Dep, Model, Pkg};
use super::super::tests::{
    QUX_UNDER_THE_GPL, add_qux_under_the_gpl, desktop_through_a_facade, entry, problems_with,
};
use super::*;
use crate::repo::Tree;

/// The allowlist entry of `bar` in the valid model.
fn bar_entry() -> String {
    entry(
        "bar",
        "2.1.0",
        "MIT OR Apache-2.0",
        &["default"],
        &["kosmo-render"],
    )
}

/// Gives `bar` in `model` the license expression `license`, in its package
/// and in its allowlist entry.
fn license_bar(model: &mut Model, license: Option<&str>) {
    model.pkg("framework", "bar@2.1.0").license = license.map(str::to_string);
    let to = match license {
        Some(license) => format!("license = {license:?}\n"),
        None => String::new(),
    };
    let bar = bar_entry();
    model.edit(
        "framework/allowlist.toml",
        &bar,
        &bar.replace("license = \"MIT OR Apache-2.0\"\n", &to),
    );
}

/// The problems of the valid model in which `bar` has the license
/// expression `license`, after `change`.
fn problems_of_bar(license: Option<&str>, change: impl FnOnce(&mut Model)) -> Vec<String> {
    problems_with(|model| {
        license_bar(model, license);
        change(model);
    })
}

/// A license exception with the fields that are given.
fn exception(
    name: &str,
    version: &str,
    license: Option<&str>,
    reading: Option<&str>,
    reason: Option<&str>,
) -> String {
    let mut text = format!("\n[[license-exception]]\nname = {name:?}\nversion = {version:?}\n");
    for (field, value) in [
        ("license", license),
        ("reading", reading),
        ("reason", reason),
    ] {
        if let Some(value) = value {
            text.push_str(&format!("{field} = {value:?}\n"));
        }
    }
    text
}

const REASON: &str = "Reviewed: the license allows the use in kosmo.";

#[test]
fn evaluates_the_allowed_licenses_and_the_operators() {
    // The allowed set that CONTRIBUTING.md gives.
    let listed = [
        "MIT",
        "Apache-2.0",
        "BSD-2-Clause",
        "BSD-3-Clause",
        "ISC",
        "Zlib",
        "Unicode-3.0",
    ];
    assert_eq!(ALLOWED, listed);
    for license in listed {
        assert_eq!(evaluate(license), Verdict::Allowed, "{license}");
    }
    for (license, verdict) in [
        ("MIT OR Apache-2.0", Verdict::Allowed),
        ("Apache-2.0 OR GPL-3.0-only", Verdict::Allowed),
        ("(MIT OR Apache-2.0) AND Unicode-3.0", Verdict::Allowed),
        ("Zlib OR Apache-2.0 OR MIT", Verdict::Allowed),
        ("Unlicense OR MIT", Verdict::Allowed),
        (
            "BSD-3-Clause OR (Unicode-3.0 AND MPL-2.0)",
            Verdict::Allowed,
        ),
        ("DocumentRef-x:LicenseRef-y OR ISC", Verdict::Allowed),
        // `AND` binds tighter than `OR`.
        ("GPL-3.0-only AND MIT OR Apache-2.0", Verdict::Allowed),
        ("MIT OR Apache-2.0 AND GPL-3.0-only", Verdict::Allowed),
        ("(MIT OR Apache-2.0) AND GPL-3.0-only", Verdict::NotAllowed),
        // Operators in lower case are operators too.
        ("MIT or Apache-2.0", Verdict::Allowed),
        ("MIT and GPL-3.0-only", Verdict::NotAllowed),
        ("MIT AND GPL-3.0-only", Verdict::NotAllowed),
        ("MIT AND OFL-1.1", Verdict::NotAllowed),
        ("GPL-3.0-only", Verdict::NotAllowed),
    ] {
        assert_eq!(evaluate(license), verdict, "{license}");
    }
}

#[test]
fn terms_with_an_exception_a_later_version_or_a_reference_are_outside_the_set() {
    for license in [
        "Apache-2.0 WITH LLVM-exception",
        "Apache-2.0+",
        "MIT+",
        "LicenseRef-Proprietary",
        "Unicode-DFS-2016",
        "MPL-2.0",
        "BSL-1.0",
        "CC0-1.0",
        // Identifiers close to those of the set.
        "Unlicense",
        "0BSD",
        "MIT-0",
        "Apache-1.1",
        "BSD-1-Clause",
        "BSD-2-Clause-Patent",
        "BSD-3-Clause-Clear",
        "ISC-Veillard",
        "zlib-acknowledgement",
    ] {
        assert_eq!(evaluate(license), Verdict::NotAllowed, "{license}");
    }
    assert_eq!(
        evaluate("Apache-2.0 WITH LLVM-exception OR MIT"),
        Verdict::Allowed
    );
}

#[test]
fn accepts_only_the_strict_spdx_forms() {
    for (license, reason) in [
        ("MIT/Apache-2.0", "invalid character(s)"),
        ("MIT / Apache-2.0", "invalid character(s)"),
        ("MIT OR", "expected one of"),
        ("", "empty expression"),
        ("(MIT", "unclosed parens"),
        ("Apache 2.0", "unknown term"),
        ("mit", "unknown term"),
        ("MIT OR apache-2.0", "unknown term"),
        ("MIT Or Apache-2.0", "unknown term"),
        ("GPL-2.0", "deprecated"),
    ] {
        match evaluate(license) {
            Verdict::Invalid(found) => assert!(found.contains(reason), "{license}: {found}"),
            verdict => panic!("{license}: {verdict:?}"),
        }
    }
}

#[test]
fn contributing_names_the_license_list_of_the_parser() {
    // A version of the parser that carries another SPDX License List
    // changes the accepted identifiers; CONTRIBUTING.md names the list.
    let text = std::fs::read_to_string(crate::repo::root().join("CONTRIBUTING.md")).unwrap();
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let list = format!("SPDX License List {}", spdx::identifiers::VERSION);
    assert!(
        text.contains(&list),
        "CONTRIBUTING.md does not name the {list}"
    );
}

#[test]
fn the_license_of_a_kosmo_crate_is_mit() {
    let found = problems_with(|model| {
        model.pkg("framework", "kosmo-ui@0.1.0-dev.1").license =
            Some("MIT OR Apache-2.0".to_string());
    });
    assert_eq!(
        found,
        [
            "framework/kosmo-ui/Cargo.toml: the license of a kosmo crate is MIT, not MIT OR Apache-2.0"
        ]
    );
    let found = problems_with(|model| {
        let render = model.pkg("framework", "kosmo-render@0.1.0-dev.1");
        render.license = None;
        render.license_file = Some("LICENSE".to_string());
    });
    assert_eq!(
        found,
        ["framework/kosmo-render/Cargo.toml: has no license; the license of a kosmo crate is MIT"]
    );
    let found = problems_with(|model| {
        model.pkg("xtask", "xtask@0.1.0-dev.1").license = Some("Apache-2.0".to_string());
    });
    assert_eq!(
        found,
        ["xtask/Cargo.toml: the license of a kosmo crate is MIT, not Apache-2.0"]
    );
    // The license of a kosmo crate is compared as written, not evaluated.
    let found = problems_with(|model| {
        model.pkg("desktop", "kosmo@0.1.0-dev.1").license = Some("MIT OR MIT".to_string());
    });
    assert_eq!(
        found,
        ["desktop/kosmo/Cargo.toml: the license of a kosmo crate is MIT, not MIT OR MIT"]
    );
}

#[test]
fn a_crate_with_font_assets_has_the_font_license_too() {
    const FONT: &str = "framework/kosmo-render/assets/fonts/Math-Regular.otf";
    let with_font = |model: &mut Model| {
        model.files.insert(FONT.to_string(), "font".to_string());
    };
    let found = problems_with(with_font);
    assert_eq!(
        found,
        [format!(
            "framework/kosmo-render/Cargo.toml: the license of a crate with font assets, such as {FONT}, is MIT AND OFL-1.1, not MIT"
        )]
    );
    let found = problems_with(|model| {
        with_font(model);
        model.pkg("framework", "kosmo-render@0.1.0-dev.1").license = None;
    });
    assert_eq!(
        found,
        [format!(
            "framework/kosmo-render/Cargo.toml: has no license; the license of a crate with font assets, such as {FONT}, is MIT AND OFL-1.1"
        )]
    );
    let found = problems_with(|model| {
        with_font(model);
        model.pkg("framework", "kosmo-render@0.1.0-dev.1").license =
            Some("MIT AND OFL-1.1".to_string());
    });
    assert_eq!(found, Vec::<String>::new());
    let found = problems_with(|model| {
        model.pkg("framework", "kosmo-ui@0.1.0-dev.1").license =
            Some("MIT AND OFL-1.1".to_string());
    });
    assert_eq!(
        found,
        [
            "framework/kosmo-ui/Cargo.toml: has no font asset, so its license is MIT, not MIT AND OFL-1.1"
        ]
    );
}

#[test]
fn font_assets_are_the_tracked_font_files_of_the_crate() {
    let tree = Tree::from_files(&[
        ("framework/kosmo-render/Cargo.toml", ""),
        ("framework/kosmo-render/assets/a.ttf", ""),
        ("framework/kosmo-render/assets/b.OTF", ""),
        ("framework/kosmo-render/assets/c.ttc", ""),
        ("framework/kosmo-render/assets/d.woff", ""),
        ("framework/kosmo-render/assets/e.Woff2", ""),
        ("framework/kosmo-render/assets/OFL.txt", ""),
        ("framework/kosmo-render/assets/ttf", ""),
        ("framework/kosmo-render/src/woff2.rs", ""),
        ("framework/kosmo-render/inner/Cargo.toml", ""),
        ("framework/kosmo-render/inner/f.ttf", ""),
        ("framework/kosmo-render-extra/g.ttf", ""),
        ("framework/h.ttf", ""),
    ]);
    assert_eq!(
        font_assets(&tree, "framework/kosmo-render"),
        [
            "framework/kosmo-render/assets/a.ttf",
            "framework/kosmo-render/assets/b.OTF",
            "framework/kosmo-render/assets/c.ttc",
            "framework/kosmo-render/assets/d.woff",
            "framework/kosmo-render/assets/e.Woff2",
        ]
    );
    assert_eq!(
        font_assets(&tree, "framework/kosmo-render/inner"),
        ["framework/kosmo-render/inner/f.ttf"]
    );
}

#[test]
fn evaluates_the_license_expression_of_each_package() {
    for license in [
        "Apache-2.0 OR GPL-3.0-only",
        "(MIT OR Apache-2.0) AND Unicode-3.0",
        "GPL-3.0-only AND MIT OR Apache-2.0",
        "MIT or Apache-2.0",
    ] {
        assert_eq!(
            problems_of_bar(Some(license), |_| {}),
            Vec::<String>::new(),
            "{license}"
        );
    }
    assert_eq!(
        problems_of_bar(Some("MIT AND GPL-3.0-only"), |_| {}),
        [
            "framework/Cargo.lock: bar 2.1.0: the license MIT AND GPL-3.0-only is not in the allowed set, and framework/allowlist.toml has no license exception for it"
        ]
    );
}

#[test]
fn the_legacy_slash_form_needs_an_exception_that_reads_it() {
    assert_eq!(
        problems_of_bar(Some("MIT/Apache-2.0"), |_| {}),
        [
            "framework/Cargo.lock: bar 2.1.0: the license MIT/Apache-2.0 is not a valid SPDX expression (invalid character(s)), and framework/allowlist.toml has no license exception for it"
        ]
    );
    let read = |reading: &str| {
        problems_of_bar(Some("MIT/Apache-2.0"), |model| {
            model.append(
                "framework/allowlist.toml",
                &exception(
                    "bar",
                    "2.1.0",
                    Some("MIT/Apache-2.0"),
                    Some(reading),
                    Some("The legacy form of crates.io, in which `/` reads as OR."),
                ),
            );
        })
    };
    assert_eq!(read("MIT OR Apache-2.0"), Vec::<String>::new());
    assert_eq!(read("MIT or Apache-2.0"), Vec::<String>::new());
    assert_eq!(
        read("Apache-2.0 OR MIT"),
        [
            "framework/allowlist.toml: the license exception for bar 2.1.0: the reading Apache-2.0 OR MIT is not MIT/Apache-2.0 with `/` read as OR"
        ]
    );
    assert_eq!(
        read("MIT"),
        [
            "framework/allowlist.toml: the license exception for bar 2.1.0: the reading MIT is not MIT/Apache-2.0 with `/` read as OR"
        ]
    );
    let unread = |license: &str| {
        problems_of_bar(Some(license), |model| {
            model.append(
                "framework/allowlist.toml",
                &exception("bar", "2.1.0", Some(license), None, Some(REASON)),
            );
        })
    };
    assert_eq!(
        unread("MIT/Apache-2.0"),
        [
            "framework/allowlist.toml: the license exception for bar 2.1.0 has no reading, which for the legacy form MIT/Apache-2.0 is the expression with `/` read as OR"
        ]
    );
    // A legacy form that is outside the set with `/` read as OR is an
    // exception like any other: its reading may be left out.
    assert_eq!(unread("GPL-3.0-only/MPL-2.0"), Vec::<String>::new());
    // Neither is an expression that is not valid with that reading either.
    assert_eq!(unread("MIT/Apache 2.0"), Vec::<String>::new());
}

#[test]
fn rejects_license_expressions_that_are_not_valid_spdx() {
    for (license, reason) in [
        ("MIT OR", "expected one of"),
        ("", "empty expression"),
        ("Apache 2.0", "unknown term"),
        ("mit", "unknown term"),
        ("MIT OR apache-2.0", "unknown term"),
        ("GPL-2.0", "a deprecated license identifier was used"),
        ("MIT Or Apache-2.0", "unknown term"),
    ] {
        let found = problems_of_bar(Some(license), |_| {});
        assert_eq!(found.len(), 1, "{license}: {found:?}");
        let prefix = format!(
            "framework/Cargo.lock: bar 2.1.0: the license {license} is not a valid SPDX expression ("
        );
        assert!(
            found[0].starts_with(&prefix)
                && found[0].contains(reason)
                && found[0]
                    .ends_with("), and framework/allowlist.toml has no license exception for it"),
            "{license}: {found:?}"
        );
    }
}

#[test]
fn rejects_licenses_outside_the_set() {
    for license in [
        "Apache-2.0 WITH LLVM-exception",
        "Apache-2.0+",
        "Unicode-DFS-2016",
        "MPL-2.0",
        "LicenseRef-Proprietary",
    ] {
        assert_eq!(
            problems_of_bar(Some(license), |_| {}),
            [format!(
                "framework/Cargo.lock: bar 2.1.0: the license {license} is not in the allowed set, and framework/allowlist.toml has no license exception for it"
            )]
        );
    }
    assert_eq!(
        problems_of_bar(Some("Apache-2.0 WITH LLVM-exception OR MIT"), |_| {}),
        Vec::<String>::new()
    );
}

#[test]
fn a_package_without_a_license_expression_needs_an_exception() {
    const MISSING: &str = "framework/Cargo.lock: bar 2.1.0 has no license expression, and framework/allowlist.toml has no license exception for it";
    let found = problems_of_bar(None, |model| {
        model.pkg("framework", "bar@2.1.0").license_file = Some("LICENSE".to_string());
    });
    assert_eq!(found, [MISSING]);
    assert_eq!(problems_of_bar(None, |_| {}), [MISSING]);
    let found = problems_of_bar(None, |model| {
        model.pkg("framework", "bar@2.1.0").license_file = Some("LICENSE".to_string());
        model.append(
            "framework/allowlist.toml",
            &exception("bar", "2.1.0", None, Some("MIT"), Some(REASON)),
        );
    });
    assert_eq!(found, Vec::<String>::new());
}

#[test]
fn a_license_exception_accepts_a_license_with_its_reason() {
    let with_exception = |text: String| {
        problems_with(|model| {
            add_qux_under_the_gpl(model);
            model.append("framework/allowlist.toml", &text);
        })
    };
    assert_eq!(
        with_exception(exception(
            "qux",
            "1.0.0",
            Some("GPL-3.0-only"),
            None,
            Some(REASON)
        )),
        Vec::<String>::new()
    );
    for reason in [None, Some(""), Some("  \n")] {
        assert_eq!(
            with_exception(exception(
                "qux",
                "1.0.0",
                Some("GPL-3.0-only"),
                None,
                reason
            )),
            ["framework/allowlist.toml: the license exception for qux 1.0.0 has no reason"]
        );
    }
    assert_eq!(
        with_exception(exception(
            "qux",
            "1.0.0",
            Some("GPL-2.0-only"),
            None,
            Some(REASON)
        )),
        [
            "framework/allowlist.toml: the license exception for qux 1.0.0 records GPL-2.0-only, but the package declares GPL-3.0-only"
        ]
    );
    assert_eq!(
        with_exception(exception("qux", "1.0.0", None, None, Some(REASON))),
        [
            "framework/allowlist.toml: the license exception for qux 1.0.0 records no license, but the package declares GPL-3.0-only"
        ]
    );
    assert_eq!(
        with_exception(exception(
            "qux",
            "1.0.0",
            Some("GPL-3.0-only"),
            Some("MIT AND GPL-3.0-only"),
            Some(REASON)
        )),
        [
            "framework/allowlist.toml: the license exception for qux 1.0.0: the reading MIT AND GPL-3.0-only is not in the allowed set"
        ]
    );
    let found = with_exception(exception(
        "qux",
        "1.0.0",
        Some("GPL-3.0-only"),
        Some("MIT/Apache-2.0"),
        Some(REASON),
    ));
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0].starts_with(
            "framework/allowlist.toml: the license exception for qux 1.0.0: the reading MIT/Apache-2.0 is not a valid SPDX expression ("
        ),
        "{found:?}"
    );
    // A valid expression outside the set may have any reading in the set.
    let found = problems_with(|model| {
        add_qux_under_the_gpl(model);
        const WITH: &str = "Apache-2.0 WITH LLVM-exception";
        model.pkg("framework", "qux@1.0.0").license = Some(WITH.to_string());
        model.edit(
            "framework/allowlist.toml",
            "license = \"GPL-3.0-only\"",
            &format!("license = {WITH:?}"),
        );
        model.append(
            "framework/allowlist.toml",
            &exception("qux", "1.0.0", Some(WITH), Some("Apache-2.0"), Some(REASON)),
        );
    });
    assert_eq!(found, Vec::<String>::new());
    let twice = exception("qux", "1.0.0", Some("GPL-3.0-only"), None, Some(REASON));
    assert_eq!(
        with_exception(format!("{twice}{twice}")),
        ["framework/allowlist.toml: lists the license exception for qux 1.0.0 twice"]
    );
}

#[test]
fn a_license_exception_is_needed() {
    let found = problems_with(|model| {
        model.append(
            "framework/allowlist.toml",
            &exception(
                "bar",
                "2.1.0",
                Some("MIT OR Apache-2.0"),
                None,
                Some(REASON),
            ),
        );
    });
    assert_eq!(
        found,
        [
            "framework/allowlist.toml: bar 2.1.0 needs no license exception: its license MIT OR Apache-2.0 is in the allowed set"
        ]
    );
    let found = problems_with(|model| {
        model.append(
            "framework/allowlist.toml",
            &exception("gone", "1.0.0", Some("GPL-3.0-only"), None, Some(REASON)),
        );
    });
    assert_eq!(
        found,
        [
            "framework/allowlist.toml: gone 1.0.0 has a license exception but is not resolved for aarch64-unknown-linux-gnu or aarch64-apple-darwin"
        ]
    );
    // An exception for another version is not one for this version.
    let found = problems_with(|model| {
        add_qux_under_the_gpl(model);
        model.append(
            "framework/allowlist.toml",
            &exception("qux", "1.0.1", Some("GPL-3.0-only"), None, Some(REASON)),
        );
    });
    assert_eq!(
        found,
        [
            QUX_UNDER_THE_GPL,
            "framework/allowlist.toml: qux 1.0.1 has a license exception but is not resolved for aarch64-unknown-linux-gnu or aarch64-apple-darwin",
        ]
    );
}

#[test]
fn each_workspace_has_its_own_license_exceptions() {
    // The desktop resolves `qux` too, through the facade, and its exception
    // does not hold for the framework.
    let mut model = desktop_through_a_facade();
    add_qux_under_the_gpl(&mut model);
    model.add(
        "desktop",
        Pkg::crates_io("qux", "1.0.0").license("GPL-3.0-only"),
    );
    model
        .pkg("desktop", "foo@1.0.0")
        .deps
        .push(Dep::new("qux@1.0.0", "^1"));
    model.append(
        "desktop/allowlist.toml",
        &entry("qux", "1.0.0", "GPL-3.0-only", &[], &["kosmo-render"]),
    );
    model.append(
        "desktop/allowlist.toml",
        &exception("qux", "1.0.0", Some("GPL-3.0-only"), None, Some(REASON)),
    );
    assert_eq!(model.problems(), [QUX_UNDER_THE_GPL]);
}

#[test]
fn the_licenses_of_the_packages_of_xtask_are_checked_too() {
    let found = problems_with(|model| {
        model.pkg("xtask", "yaml-rust2@0.13.0").license = Some("MIT/Apache-2.0".to_string());
        model.edit(
            "xtask/allowlist.toml",
            "license = \"MIT OR Apache-2.0\"",
            "license = \"MIT/Apache-2.0\"",
        );
    });
    assert_eq!(
        found,
        [
            "xtask/Cargo.lock: yaml-rust2 0.13.0: the license MIT/Apache-2.0 is not a valid SPDX expression (invalid character(s)), and xtask/allowlist.toml has no license exception for it"
        ]
    );
}

#[test]
fn an_allowlist_that_cannot_be_read_skips_the_license_exceptions() {
    let mut model = fixture::valid();
    add_qux_under_the_gpl(&mut model);
    model.append("framework/allowlist.toml", "[[package]\n");
    let found = model.problems();
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0].starts_with("framework/allowlist.toml: not valid TOML"),
        "{found:?}"
    );
}

#[test]
fn a_license_exception_has_no_unknown_field() {
    // A misspelt `reading` is not left out silently.
    let found = problems_with(|model| {
        add_qux_under_the_gpl(model);
        model.append(
            "framework/allowlist.toml",
            &format!(
                "{}readng = \"MIT\"\n",
                exception("qux", "1.0.0", Some("GPL-3.0-only"), None, Some(REASON))
            ),
        );
    });
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].contains("unknown field `readng`"), "{found:?}");
}

#[test]
fn a_reading_outside_the_set_is_reported_alone() {
    // The reading of a legacy form is not also compared with the form when
    // it is outside the allowed set.
    let found = problems_of_bar(Some("MIT/Apache-2.0"), |model| {
        model.append(
            "framework/allowlist.toml",
            &exception(
                "bar",
                "2.1.0",
                Some("MIT/Apache-2.0"),
                Some("GPL-3.0-only"),
                Some(REASON),
            ),
        );
    });
    assert_eq!(
        found,
        [
            "framework/allowlist.toml: the license exception for bar 2.1.0: the reading GPL-3.0-only is not in the allowed set"
        ]
    );
}
