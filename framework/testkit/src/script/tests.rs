use super::*;

const HEADER: &str = "kosmo-script 1 surface 64x8 xrgb8888\n";

/// The problem of `text` and its line.
fn problem(text: &str) -> (usize, Problem) {
    let error = Script::parse(text.as_bytes()).unwrap_err();
    (error.line, error.problem)
}

/// The problem of `line` after the header, which must be on line 2.
fn after_header(line: &str) -> Problem {
    let (number, problem) = problem(&format!("{HEADER}{line}\n"));
    assert_eq!(number, 2, "{line}");
    problem
}

fn invalid(field: &'static str, value: &str) -> Problem {
    Problem::Invalid {
        field,
        value: value.to_string(),
    }
}

#[test]
fn every_kind_of_line_is_read_with_its_fields() {
    let text = format!(
        "{HEADER}\
         at 0 press 7:4 char:a mods=0000\n\
         at 5 release 7:4 char:a mods=0102 synthetic\n\
         at 5 repeat 7:2c char:U+0020 mods=ffff stamp=1/2\n\
         at 18446744073709551615 long-press ffff:ffff named:mod-gui mods=0000 synthetic stamp=0/18446744073709551615\n\
         time 18446744073709551615\n\
         focus out 1 stamp=1/1\n\
         focus in 4294967295 stamp=3/4\n\
         checkpoint\n"
    );
    let script = Script::parse(text.as_bytes()).unwrap();
    assert_eq!(
        script.header(),
        Header {
            size: PixelSize::new(64, 8).unwrap(),
            format: PixelFormat::Xrgb8888
        }
    );
    let key = |time, action, usage: (u16, u16), meaning, mods, synthetic| KeyEvent {
        usage: HidUsage::new(usage.0, usage.1),
        meaning,
        mods: ModSet::from_bits(mods),
        action,
        time: MonoTime::from_nanos(time),
        synthetic,
    };
    let stamp = |epoch, generation| Stamp::new(Epoch::new(epoch), FocusGen::new(generation));
    assert_eq!(
        script.lines(),
        [
            (
                2,
                Line::Key {
                    event: key(0, KeyAction::Press, (7, 4), KeyMeaning::Char('a'), 0, false),
                    stamp: None
                }
            ),
            (
                3,
                Line::Key {
                    event: key(
                        5,
                        KeyAction::Release,
                        (7, 4),
                        KeyMeaning::Char('a'),
                        0x0102,
                        true
                    ),
                    stamp: None
                }
            ),
            (
                4,
                Line::Key {
                    event: key(
                        5,
                        KeyAction::Repeat,
                        (7, 0x2c),
                        KeyMeaning::Char(' '),
                        0xffff,
                        false
                    ),
                    stamp: Some(stamp(1, 2))
                }
            ),
            (
                5,
                Line::Key {
                    event: key(
                        u64::MAX,
                        KeyAction::LongPress,
                        (0xffff, 0xffff),
                        KeyMeaning::Named(NamedKey::MOD_GUI),
                        0,
                        true
                    ),
                    stamp: Some(stamp(0, u64::MAX))
                }
            ),
            (6, Line::Time(MonoTime::from_nanos(u64::MAX))),
            (
                7,
                Line::Focus {
                    enter: false,
                    surface: SurfaceId::new(1),
                    stamp: stamp(1, 1)
                }
            ),
            (
                8,
                Line::Focus {
                    enter: true,
                    surface: SurfaceId::new(u32::MAX),
                    stamp: stamp(3, 4)
                }
            ),
            (9, Line::Checkpoint),
        ]
    );
}

#[test]
fn every_named_key_and_format_has_its_name() {
    for (name, key) in NAMED_KEYS {
        let text = format!("{HEADER}at 1 press 7:4 named:{name} mods=0000\n");
        let script = Script::parse(text.as_bytes()).unwrap();
        assert!(matches!(
            script.lines()[0].1,
            Line::Key { event: KeyEvent { meaning: KeyMeaning::Named(named), .. }, .. } if named == key
        ));
    }
    for name in ["rgba8-premul", "xrgb8888", "rgb565", "argb8888-premul"] {
        let text = format!("kosmo-script 1 surface 1x8192 {name}\n");
        let header = Script::parse(text.as_bytes()).unwrap().header();
        assert_eq!(format_named(name), Some(header.format));
    }
}

#[test]
fn unknown_lines_are_errors_with_their_line_numbers() {
    assert_eq!(after_header("jump 5"), Problem::Unknown("jump".to_string()));
    assert_eq!(
        after_header("Checkpoint"),
        Problem::Unknown("Checkpoint".to_string())
    );
    assert_eq!(
        after_header("# a comment"),
        Problem::Unknown("#".to_string())
    );
    assert_eq!(after_header(HEADER.trim_end()), Problem::SecondHeader);
    let (line, found) = problem(&format!("{HEADER}checkpoint\ntime 5\nwait 9\n"));
    assert_eq!((line, found), (4, Problem::Unknown("wait".to_string())));
    assert_eq!(
        problem(&format!("{HEADER}checkpoint\nunknown\n"))
            .1
            .to_string(),
        "no kind of line starts with unknown"
    );
    assert_eq!(
        Script::parse(format!("{HEADER}checkpoint\nunknown\n").as_bytes())
            .unwrap_err()
            .to_string(),
        "line 3: no kind of line starts with unknown"
    );
}

#[test]
fn missing_fields_are_errors() {
    for (line, field) in [
        ("at", "time"),
        ("at 5", "action"),
        ("at 5 press", "usage"),
        ("at 5 press 7:4", "meaning"),
        ("at 5 press 7:4 char:a", "mods"),
        ("time", "time"),
        ("focus", "direction"),
        ("focus in", "surface"),
        ("focus in 1", "stamp"),
    ] {
        assert_eq!(after_header(line), Problem::Missing(field), "{line}");
    }
    for (header, field) in [
        ("kosmo-script", "version"),
        ("kosmo-script 1", "surface"),
        ("kosmo-script 1 surface", "size"),
        ("kosmo-script 1 surface 64x8", "format"),
    ] {
        assert_eq!(
            problem(&format!("{header}\n")),
            (1, Problem::Missing(field)),
            "{header}"
        );
    }
}

#[test]
fn extra_fields_are_errors() {
    for (line, extra) in [
        ("checkpoint now", "now"),
        ("time 5 6", "6"),
        ("focus in 1 stamp=1/1 again", "again"),
        ("at 5 press 7:4 char:a mods=0000 shift", "shift"),
        (
            "at 5 press 7:4 char:a mods=0000 synthetic synthetic",
            "synthetic",
        ),
        (
            "at 5 press 7:4 char:a mods=0000 stamp=1/1 synthetic",
            "synthetic",
        ),
        (
            "at 5 press 7:4 char:a mods=0000 stamp=1/1 stamp=1/2",
            "stamp=1/2",
        ),
    ] {
        assert_eq!(
            after_header(line),
            Problem::Extra(extra.to_string()),
            "{line}"
        );
    }
    assert_eq!(
        problem("kosmo-script 1 surface 64x8 xrgb8888 more\n"),
        (1, Problem::Extra("more".to_string()))
    );
}

#[test]
fn fields_with_invalid_values_are_errors() {
    for (line, field, value) in [
        ("at -5 press 7:4 char:a mods=0000", "time", "-5"),
        ("at 05 press 7:4 char:a mods=0000", "time", "05"),
        ("at 5ns press 7:4 char:a mods=0000", "time", "5ns"),
        (
            "at 18446744073709551616 press 7:4 char:a mods=0000",
            "time",
            "18446744073709551616",
        ),
        ("at 5 tap 7:4 char:a mods=0000", "action", "tap"),
        ("at 5 Press 7:4 char:a mods=0000", "action", "Press"),
        ("at 5 press 7 char:a mods=0000", "usage", "7"),
        ("at 5 press 7:4:1 char:a mods=0000", "usage", "7:4:1"),
        ("at 5 press 7:2C char:a mods=0000", "usage", "7:2C"),
        ("at 5 press 7:10000 char:a mods=0000", "usage", "7:10000"),
        ("at 5 press 7:4 a mods=0000", "meaning", "a"),
        ("at 5 press 7:4 char: mods=0000", "meaning", "char:"),
        ("at 5 press 7:4 char:ab mods=0000", "meaning", "char:ab"),
        (
            "at 5 press 7:4 char:U+d800 mods=0000",
            "meaning",
            "char:U+d800",
        ),
        (
            "at 5 press 7:4 char:U+D800 mods=0000",
            "meaning",
            "char:U+D800",
        ),
        (
            "at 5 press 7:4 char:U+110000 mods=0000",
            "meaning",
            "char:U+110000",
        ),
        (
            "at 5 press 7:4 named:enter mods=0000",
            "meaning",
            "named:enter",
        ),
        ("at 5 press 7:4 named:4 mods=0000", "meaning", "named:4"),
        ("at 5 press 7:4 char:a mods=00", "mods", "mods=00"),
        ("at 5 press 7:4 char:a mods=00000", "mods", "mods=00000"),
        ("at 5 press 7:4 char:a mods=FFFF", "mods", "mods=FFFF"),
        ("at 5 press 7:4 char:a 0000", "mods", "0000"),
        ("at 5 press 7:4 char:a mods=0000 stamp=1", "stamp", "1"),
        ("at 5 press 7:4 char:a mods=0000 stamp=1/x", "stamp", "1/x"),
        ("focus up 1 stamp=1/1", "direction", "up"),
        ("focus in main stamp=1/1", "surface", "main"),
        ("focus in 4294967296 stamp=1/1", "surface", "4294967296"),
        ("focus in 1 1/1", "stamp", "1/1"),
        ("focus in 1 stamp=/1", "stamp", "/1"),
        ("time 1e6", "time", "1e6"),
    ] {
        assert_eq!(after_header(line), invalid(field, value), "{line}");
    }
    for (header, field, value) in [
        ("kosmo-script 1 window 64x8 xrgb8888", "surface", "window"),
        ("kosmo-script 1 surface 64 xrgb8888", "size", "64"),
        ("kosmo-script 1 surface 0x8 xrgb8888", "size", "0x8"),
        ("kosmo-script 1 surface 8193x8 xrgb8888", "size", "8193x8"),
        ("kosmo-script 1 surface 64x08 xrgb8888", "size", "64x08"),
        ("kosmo-script 1 surface 64x8 rgb888", "format", "rgb888"),
        ("kosmo-script 1 surface 64x8 XRGB8888", "format", "XRGB8888"),
    ] {
        assert_eq!(
            problem(&format!("{header}\n")),
            (1, invalid(field, value)),
            "{header}"
        );
    }
}

#[test]
fn a_time_earlier_than_the_line_before_is_an_error() {
    let earlier = |time, clock| Problem::Earlier {
        time: MonoTime::from_nanos(time),
        clock: MonoTime::from_nanos(clock),
    };
    for (lines, line, time, clock) in [
        ("time 10\nat 9 press 7:4 char:a mods=0000\n", 3, 9, 10),
        ("at 10 press 7:4 char:a mods=0000\ntime 9\n", 3, 9, 10),
        (
            "time 10\ncheckpoint\nfocus in 1 stamp=1/1\ntime 3\n",
            5,
            3,
            10,
        ),
        (
            "at 7 press 7:4 char:a mods=0000\nat 7 release 7:4 char:a mods=0000\nat 6 press 7:4 char:a mods=0000\n",
            4,
            6,
            7,
        ),
    ] {
        assert_eq!(
            problem(&format!("{HEADER}{lines}")),
            (line, earlier(time, clock)),
            "{lines}"
        );
    }
    // The same time again is not earlier.
    let same = format!("{HEADER}time 4\nat 4 press 7:4 char:a mods=0000\ntime 4\n");
    assert_eq!(Script::parse(same.as_bytes()).unwrap().lines().len(), 3);
}

#[test]
fn the_header_comes_first_with_version_1() {
    assert_eq!(problem(""), (1, Problem::NoHeader));
    assert_eq!(problem("checkpoint\n"), (1, Problem::NoHeader));
    assert_eq!(
        problem(&format!("checkpoint\n{HEADER}")),
        (1, Problem::NoHeader)
    );
    assert_eq!(
        problem(&format!("{HEADER}{HEADER}")),
        (2, Problem::SecondHeader)
    );
    for version in ["2", "0", "01", "1.0"] {
        assert_eq!(
            problem(&format!("kosmo-script {version} surface 64x8 xrgb8888\n")),
            (1, Problem::Version(version.to_string()))
        );
    }
    // A later version may have other fields: the version is read first.
    assert_eq!(
        problem("kosmo-script 2 window\n"),
        (1, Problem::Version("2".to_string()))
    );
    let only = Script::parse(HEADER.as_bytes()).unwrap();
    assert!(only.lines().is_empty());
}

#[test]
fn lines_are_utf8_with_line_feeds_single_spaces_and_no_control_characters() {
    assert_eq!(problem(HEADER.trim_end()), (1, Problem::NoLineFeed));
    assert_eq!(
        problem(&format!("{HEADER}checkpoint")),
        (2, Problem::NoLineFeed)
    );
    assert_eq!(after_header(""), Problem::EmptyField);
    assert_eq!(after_header(" checkpoint"), Problem::EmptyField);
    assert_eq!(after_header("checkpoint "), Problem::EmptyField);
    assert_eq!(after_header("time  5"), Problem::EmptyField);
    assert_eq!(after_header("checkpoint\r"), Problem::Control);
    assert_eq!(after_header("time\t5"), Problem::Control);
    assert_eq!(
        after_header("at 5 press 7:4 char:\u{85} mods=0000"),
        Problem::Control
    );
    let mut bytes = HEADER.as_bytes().to_vec();
    bytes.extend_from_slice(b"time \xff\n");
    let error = Script::parse(&bytes).unwrap_err();
    assert_eq!((error.line, error.problem), (2, Problem::NotUtf8));
    // Characters beyond ASCII are fine where a field takes them.
    let text =
        format!("{HEADER}at 5 press 7:4 char:é mods=0000\nat 5 press 7:4 char:□ mods=0000\n");
    assert_eq!(Script::parse(text.as_bytes()).unwrap().lines().len(), 2);
}

#[test]
fn a_script_has_at_most_max_lines_lines() {
    let mut text = HEADER.to_string();
    for _ in 1..MAX_LINES {
        text.push_str("checkpoint\n");
    }
    assert_eq!(
        Script::parse(text.as_bytes()).unwrap().lines().len(),
        MAX_LINES - 1
    );
    text.push_str("checkpoint\n");
    assert_eq!(problem(&text), (MAX_LINES + 1, Problem::TooManyLines));
    assert_eq!(
        Problem::TooManyLines.to_string(),
        "the script has more than 65536 lines"
    );
}

#[test]
fn a_line_has_at_most_max_line_bytes_bytes() {
    let longest = "x".repeat(MAX_LINE_BYTES);
    assert_eq!(after_header(&longest), Problem::Unknown(longest.clone()));
    let longer = "x".repeat(MAX_LINE_BYTES + 1);
    assert_eq!(after_header(&longer), Problem::TooLong(MAX_LINE_BYTES + 1));
    // Without its line feed, a long last line is too long before it lacks one.
    assert_eq!(
        problem(&format!("{HEADER}{longer}")),
        (2, Problem::TooLong(MAX_LINE_BYTES + 1))
    );
    // Bytes, not characters, count.
    let wide = "é".repeat(MAX_LINE_BYTES / 2 + 1);
    assert_eq!(after_header(&wide), Problem::TooLong(MAX_LINE_BYTES + 2));
}

/// The number of random scripts that the next test reads.
const RANDOM_SCRIPTS: usize = 2_000;

#[test]
fn random_scripts_give_results_or_errors_and_never_panic() {
    const PIECES: [&str; 24] = [
        "kosmo-script",
        "1",
        "surface",
        "64x8",
        "xrgb8888",
        "at",
        "time",
        "focus",
        "in",
        "out",
        "checkpoint",
        "press",
        "release",
        "7:4",
        "char:a",
        "named:left",
        "mods=0000",
        "synthetic",
        "stamp=1/1",
        "5",
        "0",
        "18446744073709551616",
        "\t",
        "é",
    ];
    // A fixed linear congruential generator, so every run reads the same
    // scripts.
    let mut state: u64 = 0x2545_f491_4f6c_dd1d;
    let mut next = |bound: usize| {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        usize::try_from(state >> 33).unwrap() % bound
    };
    let (mut read, mut refused) = (0, 0);
    for _ in 0..RANDOM_SCRIPTS {
        let mut text = if next(4) == 0 {
            String::new()
        } else {
            HEADER.to_string()
        };
        for _ in 0..next(6) {
            let fields: Vec<&str> = (0..next(8)).map(|_| PIECES[next(PIECES.len())]).collect();
            text.push_str(&fields.join(if next(10) == 0 { "  " } else { " " }));
            if next(12) != 0 {
                text.push('\n');
            }
        }
        match Script::parse(text.as_bytes()) {
            Ok(script) => {
                assert!(script.lines().iter().all(|(line, _)| *line >= 2));
                read += 1;
            }
            Err(error) => {
                assert!(error.line >= 1 && error.line <= 7, "{error}");
                refused += 1;
            }
        }
    }
    assert!(read > 0 && refused > 0, "{read} read, {refused} refused");
}
