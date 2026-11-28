//! The bundled fonts cover the interface strings: each character of every
//! string of `tests/data/interface-strings.txt` is in the character map of
//! the font of its group, as skrifa reads it from the file of the asset
//! directory. The comments at the start of the file describe its format.
//! There is no fallback, so a character that a font lacks would be drawn
//! as the font's `.notdef` glyph; the check reports every such character,
//! with its string and font, before it fails.

pub mod support;

use std::collections::BTreeSet;
use std::fmt;

use kosmo_text::{MONO_FILE, SANS_FILE};
use skrifa::{FontRef, MetadataProvider};
use support::{Checked, MONO_BYTES, SANS_BYTES, failed};

/// The interface strings, grouped by font.
const INTERFACE_STRINGS: &str = include_str!("data/interface-strings.txt");

/// A string of the interface and the font that shows it.
#[derive(Debug, PartialEq)]
struct Entry {
    /// The file of the font in the asset manifest.
    font: &'static str,
    /// The name of the string, unique in the group of its font.
    name: String,
    /// The string.
    text: String,
}

/// A character of a string that the string's font lacks.
#[derive(Debug, PartialEq)]
struct Missing {
    /// The file of the font in the asset manifest.
    font: &'static str,
    /// The name of the string.
    name: String,
    /// The character.
    character: char,
}

impl fmt::Display for Missing {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: {}: U+{:04X} {:?}",
            self.font,
            self.name,
            u32::from(self.character),
            self.character
        )
    }
}

/// The file of the asset manifest named `file` and its bytes, for the two
/// bundled fonts.
fn font(file: &str) -> Option<(&'static str, &'static [u8])> {
    [(SANS_FILE, SANS_BYTES), (MONO_FILE, MONO_BYTES)]
        .into_iter()
        .find(|&(name, _)| name == file)
}

/// The strings of a file in the format of `interface-strings.txt`, in the
/// order of the file.
fn parse(file: &str) -> Checked<Vec<Entry>> {
    let mut entries: Vec<Entry> = Vec::new();
    let mut groups: Vec<&'static str> = Vec::new();
    for (index, line) in file.lines().enumerate() {
        let at = |message: String| format!("line {}: {message}", index + 1);
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(header) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            let (file, _) = font(header).ok_or_else(|| at(format!("no bundled font {header}")))?;
            if groups.contains(&file) {
                return Err(at(format!("a second group of {file}")));
            }
            groups.push(file);
            continue;
        }
        let group = *groups
            .last()
            .ok_or_else(|| at("a string before the first group".into()))?;
        let (name, raw) = line
            .split_once(" = ")
            .ok_or_else(|| at("neither a comment, a group nor `<name> = <text>`".into()))?;
        let valid = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-';
        if name.is_empty() || !name.chars().all(valid) {
            return Err(at(format!(
                "the name {name:?} is empty or has a character other than a-z, 0-9 and -"
            )));
        }
        if entries.iter().any(|e| e.font == group && e.name == name) {
            return Err(at(format!("a second string {name} of {group}")));
        }
        let text = unescape(raw).map_err(at)?;
        entries.push(Entry {
            font: group,
            name: name.into(),
            text,
        });
    }
    Ok(entries)
}

/// The text that `raw`, the text of a line, writes: `\\` is a backslash,
/// `\u{<hex>}` the character with that code point, and any other character
/// itself, except that a space at either end, other whitespace and control
/// characters are written as escapes.
fn unescape(raw: &str) -> Checked<String> {
    if raw.is_empty() {
        return Err("an empty text".into());
    }
    if raw.starts_with(' ') || raw.ends_with(' ') {
        return Err("a space at an end of the text, which is written \\u{20}".into());
    }
    let mut text = String::new();
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c != ' ' && (c.is_whitespace() || c.is_control()) {
            return Err(format!("{c:?}, which is written as an escape"));
        }
        if c != '\\' {
            text.push(c);
            continue;
        }
        match chars.next() {
            Some('\\') => text.push('\\'),
            Some('u') => {
                let rest = chars.as_str();
                let (hex, tail) = rest
                    .strip_prefix('{')
                    .and_then(|r| r.split_once('}'))
                    .filter(|(hex, _)| (1..=6).contains(&hex.len()))
                    .filter(|(hex, _)| hex.chars().all(|c| c.is_ascii_hexdigit()))
                    .ok_or_else(|| format!("an escape \\u{rest} that is not \\u{{<hex>}}"))?;
                let code = u32::from_str_radix(hex, 16).map_err(failed)?;
                let character = char::from_u32(code)
                    .ok_or_else(|| format!("\\u{{{hex}}}, which is not a character"))?;
                text.push(character);
                chars = tail.chars();
            }
            Some(other) => {
                return Err(format!(
                    "the escape \\{other}, which is neither \\\\ nor \\u{{<hex>}}"
                ));
            }
            None => return Err("a backslash at the end of the text".into()),
        }
    }
    Ok(text)
}

/// Each character of a string of `entries` that its font lacks, once for
/// each string, in the order of the strings and then of the characters.
fn missing(entries: &[Entry]) -> Checked<Vec<Missing>> {
    let mut missing = Vec::new();
    for entry in entries {
        let (_, bytes) = font(entry.font).ok_or_else(|| format!("no font {}", entry.font))?;
        let charmap = FontRef::new(bytes).map_err(failed)?.charmap();
        let mut seen = Vec::new();
        for character in entry.text.chars() {
            if charmap.map(character).is_none() && !seen.contains(&character) {
                seen.push(character);
                missing.push(Missing {
                    font: entry.font,
                    name: entry.name.clone(),
                    character,
                });
            }
        }
    }
    Ok(missing)
}

/// Fails, listing every character of `entries` that its font lacks, when
/// there is one.
fn covered(entries: &[Entry]) -> Checked {
    let missing = missing(entries)?;
    if missing.is_empty() {
        return Ok(());
    }
    let lines: Vec<String> = missing.iter().map(|each| format!("\n  {each}")).collect();
    Err(format!(
        "the fonts lack {} of the characters of the interface strings:{}",
        missing.len(),
        lines.concat()
    ))
}

/// The strings of the characters of the interface by Unicode block, which
/// the group of each font holds.
const BLOCKS: [&str; 4] = ["ascii", "latin-1", "greek", "operators-and-arrows"];

#[test]
fn the_bundled_fonts_cover_every_interface_string() {
    let entries = parse(INTERFACE_STRINGS).unwrap();
    for (file, least) in [(SANS_FILE, 257), (MONO_FILE, 255)] {
        for name in BLOCKS {
            assert!(
                entries
                    .iter()
                    .any(|entry| entry.font == file && entry.name == name),
                "no string {name} of {file}"
            );
        }
        let characters: BTreeSet<char> = entries
            .iter()
            .filter(|entry| entry.font == file)
            .flat_map(|entry| entry.text.chars())
            .collect();
        assert!(
            characters.len() >= least,
            "{file} has {} distinct characters, fewer than {least}",
            characters.len()
        );
    }
    if let Err(report) = covered(&entries) {
        panic!("{report}");
    }
}

#[test]
fn a_hole_in_the_proportional_font_fails_the_check() {
    let entries = [Entry {
        font: SANS_FILE,
        name: "hole".into(),
        text: "1/\u{25a1}".into(),
    }];
    let missing = missing(&entries).unwrap();
    assert_eq!(
        missing,
        [Missing {
            font: SANS_FILE,
            name: "hole".into(),
            character: '\u{25a1}',
        }]
    );
    assert_eq!(
        covered(&entries).unwrap_err(),
        "the fonts lack 1 of the characters of the interface strings:\n  firasans/FiraSans-Regular.ttf: hole: U+25A1 '\u{25a1}'"
    );
}

#[test]
fn the_check_lists_every_missing_character_once_for_each_string() {
    let entries = [
        Entry {
            font: MONO_FILE,
            name: "increment".into(),
            text: "\u{2206}x \u{22c5} \u{2206}y".into(),
        },
        Entry {
            font: SANS_FILE,
            name: "increment".into(),
            text: "\u{2206}x \u{22c5} \u{2206}y".into(),
        },
        Entry {
            font: SANS_FILE,
            name: "holes".into(),
            text: "\u{25a1}/\u{3a2}\u{25a1}".into(),
        },
        Entry {
            font: SANS_FILE,
            name: "hole".into(),
            text: "1/\u{25a1}".into(),
        },
    ];
    let listed: Vec<String> = missing(&entries)
        .unwrap()
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(
        listed,
        [
            "firamono/FiraMono-Regular.ttf: increment: U+2206 '\u{2206}'",
            "firamono/FiraMono-Regular.ttf: increment: U+22C5 '\u{22c5}'",
            "firasans/FiraSans-Regular.ttf: holes: U+25A1 '\u{25a1}'",
            "firasans/FiraSans-Regular.ttf: holes: U+03A2 '\\u{3a2}'",
            "firasans/FiraSans-Regular.ttf: hole: U+25A1 '\u{25a1}'",
        ]
    );
    assert!(
        covered(&entries)
            .unwrap_err()
            .starts_with("the fonts lack 5 of the characters")
    );
}

#[test]
fn the_format_groups_strings_by_font_and_reads_escapes() {
    let file = "# A comment.\n\n[firasans/FiraSans-Regular.ttf]\n\
                edges = \\u{20}a\\\\b\\u{A0}c\\u{ad} = d\n\
                [firamono/FiraMono-Regular.ttf]\n# Another.\nedges = x y\n";
    assert_eq!(
        parse(file).unwrap(),
        [
            Entry {
                font: SANS_FILE,
                name: "edges".into(),
                text: " a\\b\u{a0}c\u{ad} = d".into(),
            },
            Entry {
                font: MONO_FILE,
                name: "edges".into(),
                text: "x y".into(),
            },
        ]
    );
}

#[test]
fn the_format_rejects_what_it_does_not_define() {
    let sans = "[firasans/FiraSans-Regular.ttf]\n";
    let cases = [
        (
            "a = b\n".to_string(),
            "line 1: a string before the first group",
        ),
        (
            "[firasans/OFL.txt]\n".into(),
            "line 1: no bundled font firasans/OFL.txt",
        ),
        (
            format!("{sans}{sans}"),
            "line 2: a second group of firasans",
        ),
        (format!("{sans}a=b\n"), "line 2: neither a comment"),
        (format!("{sans} # b\n"), "line 2: neither a comment"),
        (format!("{sans}A = b\n"), "line 2: the name \"A\""),
        (format!("{sans} = b\n"), "line 2: the name \"\""),
        (format!("{sans}a = b\na = c\n"), "line 3: a second string a"),
        (format!("{sans}a = \n"), "line 2: an empty text"),
        (format!("{sans}a =  b\n"), "line 2: a space at an end"),
        (format!("{sans}a = b \n"), "line 2: a space at an end"),
        (format!("{sans}a = b\u{a0}c\n"), "line 2: '\\u{a0}'"),
        (format!("{sans}a = b\tc\n"), "line 2: '\\t'"),
        (format!("{sans}a = b\\x\n"), "line 2: the escape \\x, which"),
        (format!("{sans}a = b\\\n"), "line 2: a backslash at the end"),
        (format!("{sans}a = \\u{{}}\n"), "line 2: an escape \\u{}"),
        (format!("{sans}a = \\u{{20\n"), "line 2: an escape \\u{20"),
        (
            format!("{sans}a = \\u{{+20}}\n"),
            "line 2: an escape \\u{+20}",
        ),
        (
            format!("{sans}a = \\u{{1234567}}\n"),
            "line 2: an escape \\u{1234567}",
        ),
        (
            format!("{sans}a = \\u{{110000}}\n"),
            "line 2: \\u{110000}, which",
        ),
        (
            format!("{sans}a = \\u{{d800}}\n"),
            "line 2: \\u{d800}, which",
        ),
    ];
    for (file, expected) in cases {
        let error = parse(&file).unwrap_err();
        assert!(error.starts_with(expected), "{file:?} gave {error:?}");
    }
}
