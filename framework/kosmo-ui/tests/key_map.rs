//! The bindings of the minimal key map, reached after the tables of the
//! focus chain have no match.

mod support;

use kosmo_types::{KeyAction, KeyMeaning, NamedKey};
use kosmo_ui::{MINIMAL_KEY_MAP, check_table};
use support::{ALL, Calc, Driver, Shape, UNRELATED, focus_in, key, key_on, stamp};

/// A calculator whose focused control has a binding table that matches no
/// test key and handles nothing, and whose application handles every
/// command of the minimal key map. Its first round has run with focus.
fn calculator() -> Driver {
    let mut calc = Calc::new(Shape::Line);
    calc.parts[3].bindings = UNRELATED;
    calc.handles = ALL.to_vec();
    let mut driver = Driver::new(calc);
    driver.events(vec![focus_in(stamp(1, 1))]);
    driver
}

/// The outcomes of a key of `meaning` with the modifiers `mods`, pressed and
/// then with `then`.
fn outcomes(meaning: KeyMeaning, mods: u16, then: KeyAction) -> Vec<String> {
    let mut driver = calculator();
    let mut found = Vec::new();
    for action in [KeyAction::Press, then] {
        let output = driver.events(vec![key_on(stamp(1, 1), key(meaning, action, mods))]);
        found.extend(Driver::outcomes(&output));
    }
    found
}

fn named(key: NamedKey) -> KeyMeaning {
    KeyMeaning::Named(key)
}

#[test]
fn left_moves_left_on_press_and_repeat() {
    assert_eq!(
        outcomes(named(NamedKey::LEFT), 0, KeyAction::Repeat),
        [
            "handled kosmo.MoveLeft by app",
            "handled kosmo.MoveLeft by app"
        ]
    );
}

#[test]
fn right_moves_right_on_press_and_repeat() {
    assert_eq!(
        outcomes(named(NamedKey::RIGHT), 0, KeyAction::Repeat),
        [
            "handled kosmo.MoveRight by app",
            "handled kosmo.MoveRight by app"
        ]
    );
}

#[test]
fn delete_deletes_backwards_on_press_and_repeat() {
    assert_eq!(
        outcomes(named(NamedKey::DELETE), 0, KeyAction::Repeat),
        [
            "handled kosmo.DeleteBackward by app",
            "handled kosmo.DeleteBackward by app"
        ]
    );
}

#[test]
fn execute_evaluates_the_selection_on_press_only() {
    assert_eq!(
        outcomes(named(NamedKey::EXECUTE), 0, KeyAction::Repeat),
        ["handled kosmo.EvaluateSelection by app", "unhandled"]
    );
}

#[test]
fn cancel_cancels_the_job_on_press_only() {
    assert_eq!(
        outcomes(named(NamedKey::CANCEL), 0, KeyAction::Repeat),
        ["handled kosmo.CancelJob by app", "unhandled"]
    );
}

#[test]
fn a_character_inserts_itself_whatever_its_shift_and_not_on_release() {
    // 0x02 is the left Shift key.
    assert_eq!(
        outcomes(KeyMeaning::Char('@'), 0x02, KeyAction::Release),
        ["handled kosmo.InsertText '@' by app", "unhandled"]
    );
}

#[test]
fn long_press_and_synthetic_releases_are_unhandled() {
    let mut driver = calculator();
    let left = named(NamedKey::LEFT);
    let mut release = key(left, KeyAction::Release, 0);
    release.synthetic = true;
    let output = driver.events(vec![
        key_on(stamp(1, 1), key(left, KeyAction::LongPress, 0)),
        key_on(stamp(1, 1), release),
    ]);
    assert_eq!(Driver::outcomes(&output), ["unhandled", "unhandled"]);
    assert!(output.record.lines()[2].contains(" release synthetic "));
    assert!(driver.app.log.is_empty());
}

#[test]
fn the_minimal_key_map_has_six_bindings_without_conflict() {
    assert_eq!(MINIMAL_KEY_MAP.len(), 6);
    assert_eq!(check_table(MINIMAL_KEY_MAP), Ok(()));
    assert_eq!(check_table(UNRELATED), Ok(()));
}
