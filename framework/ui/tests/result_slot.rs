//! The result slot in rounds: results that arrive early and late, against
//! the region's versions and latest submission, which the calculator's
//! model holds and passes to the slot in the update pass. Versions are
//! written as (source, context, settings).

mod support;

use kosmo_math_model::version::{Judgement, Versions};
use kosmo_types::NamedKey;
use kosmo_ui::{
    CANCEL_JOB, Delivered, EVALUATE_SELECTION, INSERT_TEXT, Receipt, ResultSlot, Shown, Tagged,
};
use support::{Calc, Driver, Shape, char_press, focus_in, press, stamp, tagged};

/// A calculator whose line holds `text`, whose line control types
/// characters, and whose first round has run with focus.
fn calculator(text: &str) -> Driver {
    let mut calc = Calc::new(Shape::Line);
    for c in text.chars() {
        let _ = calc.line.insert_char(c);
    }
    assert_eq!(calc.text(), text);
    calc.parts[3].handles = vec![INSERT_TEXT];
    let mut driver = Driver::new(calc);
    driver.events(vec![focus_in(stamp(1, 1))]);
    driver
}

/// Runs a round with the acceptances of `submissions` as its receipts.
fn accept(driver: &mut Driver, submissions: &[u64]) {
    driver.receipts = submissions
        .iter()
        .map(|&id| Receipt::Accepted(id))
        .collect();
    driver.round(Vec::new(), Vec::new());
}

/// Runs a round with `report` and returns what the slot did with it.
fn report(driver: &mut Driver, report: Tagged<String, u64>) -> Delivered<String, u64> {
    let before = driver.app.delivered.len();
    driver.round(Vec::new(), vec![report]);
    assert_eq!(driver.app.delivered.len(), before + 1);
    driver.app.delivered[before].clone()
}

/// The type of the slot's delivery: the slot, the result, the region's
/// versions and its latest submission.
type Deliver = fn(
    &mut ResultSlot<String, u64>,
    Tagged<String, u64>,
    Versions,
    Option<&u64>,
) -> Delivered<String, u64>;

/// The type of the slot's display value: the slot, the region's versions
/// and its latest submission.
type Show =
    for<'a, 'b> fn(&'a ResultSlot<String, u64>, Versions, Option<&'b u64>) -> Shown<'a, String>;

/// What the slot shows against the calculator's region.
fn shown(driver: &Driver) -> Shown<'_, String> {
    driver
        .app
        .slot
        .shown(driver.app.versions(), driver.app.latest.as_ref())
}

/// Types `c` into the line, which changes its source version.
fn type_char(driver: &mut Driver, c: char) {
    driver.events(vec![char_press(c, stamp(1, 1))]);
}

#[test]
fn a_receipt_and_its_report_in_one_round_give_a_current_result() {
    let mut driver = calculator("x");
    assert_eq!(driver.app.numbers(), (1, 0, 0));
    let versions = driver.app.versions();
    driver.receipts = vec![Receipt::Accepted(1)];
    let output = driver.round(Vec::new(), vec![tagged("1", 1, versions)]);

    // The event pass takes the receipt first, so the latest submission is 1
    // when the update pass delivers the report.
    let lines = output.record.lines();
    let receipt = lines
        .iter()
        .position(|line| line == "receipt accepted 1")
        .unwrap();
    let reported = lines
        .iter()
        .position(|line| line.starts_with("report "))
        .unwrap();
    assert!(receipt < reported);
    let delivered = driver.app.delivered[0].clone();
    assert_eq!(delivered.judgement, Judgement::Current);
    assert_eq!(delivered.result, tagged("1", 1, versions));
    assert_eq!(shown(&driver), Shown::Current(&"1".to_string()));
}

#[test]
fn a_late_result_of_a_superseded_submission_is_not_written() {
    let mut driver = calculator("x");
    let versions = driver.app.versions();
    accept(&mut driver, &[1, 2]);
    let delivered = report(&mut driver, tagged("1", 1, versions));
    assert_eq!(delivered.judgement, Judgement::Superseded);
    assert_eq!(delivered.result, tagged("1", 1, versions));
    assert_eq!(shown(&driver), Shown::Empty);
}

#[test]
fn a_late_result_after_an_edit_is_stale() {
    let mut driver = calculator("x");
    let first = driver.app.versions();
    accept(&mut driver, &[1]);
    type_char(&mut driver, '+');
    assert_eq!(driver.app.numbers(), (2, 0, 0));
    let delivered = report(&mut driver, tagged("1", 1, first));
    assert_eq!(delivered.judgement, Judgement::Stale);
    assert_eq!(delivered.result, tagged("1", 1, first));
    assert_eq!(shown(&driver), Shown::Empty);
}

#[test]
fn a_late_result_after_a_settings_change_is_stale() {
    let mut driver = calculator("x");
    let first = driver.app.versions();
    accept(&mut driver, &[1]);
    // The test builds the region's state with settings version 1.
    driver.app.settings = 1;
    assert_eq!(driver.app.numbers(), (1, 0, 1));
    let delivered = report(&mut driver, tagged("1", 1, first));
    assert_eq!(delivered.judgement, Judgement::Stale);
    assert_eq!(delivered.result, tagged("1", 1, first));
    assert_eq!(shown(&driver), Shown::Empty);
}

#[test]
fn a_late_result_after_a_context_change_is_stale() {
    let mut driver = calculator("x");
    let first = driver.app.versions();
    accept(&mut driver, &[1]);
    // The test builds the region's state with context version 1.
    driver.app.context = 1;
    assert_eq!(driver.app.numbers(), (1, 1, 0));
    let delivered = report(&mut driver, tagged("1", 1, first));
    assert_eq!(delivered.judgement, Judgement::Stale);
    assert_eq!(delivered.result, tagged("1", 1, first));
    assert_eq!(shown(&driver), Shown::Empty);
}

#[test]
fn an_older_result_that_arrives_after_a_newer_one_does_not_replace_it() {
    let mut driver = calculator("x");
    let first = driver.app.versions();
    accept(&mut driver, &[1]);
    type_char(&mut driver, '+');
    let second = driver.app.versions();
    accept(&mut driver, &[2]);

    let newer = report(&mut driver, tagged("2", 2, second));
    assert_eq!(newer.judgement, Judgement::Current);
    let older = report(&mut driver, tagged("1", 1, first));
    assert_eq!(older.judgement, Judgement::Stale);
    assert_eq!(older.result, tagged("1", 1, first));
    assert_eq!(shown(&driver), Shown::Current(&"2".to_string()));
}

#[test]
fn a_written_result_is_shown_as_stale_after_an_edit() {
    let mut driver = calculator("x");
    let first = driver.app.versions();
    accept(&mut driver, &[1]);
    assert_eq!(
        report(&mut driver, tagged("1", 1, first)).judgement,
        Judgement::Current
    );
    assert_eq!(shown(&driver), Shown::Current(&"1".to_string()));

    type_char(&mut driver, '+');
    assert_eq!(shown(&driver), Shown::Stale(&"1".to_string()));
    let second = driver.app.versions();
    accept(&mut driver, &[2]);
    assert_eq!(shown(&driver), Shown::Stale(&"1".to_string()));
    assert_eq!(
        report(&mut driver, tagged("2", 2, second)).judgement,
        Judgement::Current
    );
    assert_eq!(shown(&driver), Shown::Current(&"2".to_string()));
}

#[test]
fn a_written_result_is_shown_as_stale_after_a_new_submission() {
    let mut driver = calculator("x");
    let versions = driver.app.versions();
    accept(&mut driver, &[1]);
    assert_eq!(
        report(&mut driver, tagged("1", 1, versions)).judgement,
        Judgement::Current
    );

    // Submission 2 of the same versions becomes the latest.
    accept(&mut driver, &[2]);
    assert_eq!(driver.app.versions(), versions);
    assert_eq!(shown(&driver), Shown::Stale(&"1".to_string()));
    assert_eq!(
        report(&mut driver, tagged("2", 2, versions)).judgement,
        Judgement::Current
    );
    assert_eq!(shown(&driver), Shown::Current(&"2".to_string()));
}

#[test]
fn writing_a_result_leaves_the_editing_structure_as_it_was() {
    let mut driver = calculator("1/3+1/6");
    for _ in 0..4 {
        driver.app.line.move_left();
    }
    assert_eq!(driver.app.line.cursor().index, 3);
    let before = driver.app.line.clone();
    let versions = driver.app.versions();
    accept(&mut driver, &[1]);
    assert_eq!(
        report(&mut driver, tagged("1/2", 1, versions)).judgement,
        Judgement::Current
    );

    assert_eq!(driver.app.line, before);
    assert_eq!(driver.app.line.items(), before.items());
    assert_eq!(driver.app.line.cursor(), before.cursor());
    assert_eq!(driver.app.line.version(), before.version());

    // The slot's operations take the result and the region's versions and
    // latest submission, and no editing structure or cursor.
    let _: Deliver = ResultSlot::deliver;
    let _: Show = ResultSlot::shown;
}

#[test]
fn a_refused_submission_keeps_the_latest_and_a_cancellation_gives_no_receipt() {
    let mut calc = Calc::new(Shape::Line);
    calc.line.insert_text("x").unwrap();
    calc.handles = vec![EVALUATE_SELECTION, CANCEL_JOB];
    let mut driver = Driver::new(calc);
    driver.events(vec![focus_in(stamp(1, 1))]);
    driver.events(vec![press(NamedKey::EXECUTE, stamp(1, 1))]);
    driver.port.refuse_next = true;
    driver.events(vec![
        press(NamedKey::EXECUTE, stamp(1, 1)),
        press(NamedKey::CANCEL, stamp(1, 1)),
    ]);
    assert_eq!(driver.app.latest, Some(1));
    driver.app.log.clear();

    let output = driver.events(Vec::new());
    assert_eq!(driver.app.latest, Some(1));
    assert_eq!(driver.app.log, ["rejected: too many live jobs"]);
    let lines = output.record.lines();
    assert_eq!(lines[1], "receipt rejected \"too many live jobs\"");
    assert_eq!(lines[2], "cancel answered \"cancel accepted\"");
}
