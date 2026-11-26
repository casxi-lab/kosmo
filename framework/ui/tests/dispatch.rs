//! Dispatch of commands from the focused control up to the region's root
//! and the application, and the focus check in front of it.

mod support;

use kosmo_types::NamedKey;
use kosmo_ui::{MOVE_LEFT, Output, WidgetId};
use support::{Calc, Driver, Shape, char_press, focus_in, focus_out, press, stamp};

/// The chain root, region's root, parent and focused control, of which the
/// controls named in `controls` handle `MoveLeft`, and the application too
/// when `app` is true. Its first round has run with focus (1, 1).
fn chain(controls: &[usize], app: bool) -> Driver {
    let mut calc = Calc::new(Shape::Chain);
    for &index in controls {
        calc.parts[index].handles = vec![MOVE_LEFT];
    }
    if app {
        calc.handles = vec![MOVE_LEFT];
    }
    let mut driver = Driver::new(calc);
    driver.events(vec![focus_in(stamp(1, 1))]);
    driver
}

/// The identities of the root, the region's root, the parent and the
/// focused control.
fn ids(driver: &Driver) -> [WidgetId; 4] {
    let tree = driver.runtime.tree();
    let root = tree.preorder()[0];
    let region = tree.children(root)[0];
    let parent = tree.children(region)[0];
    let focused = tree.children(parent)[0];
    assert_eq!(tree.focused(), Some(focused));
    assert_eq!(tree.region(), Some(region));
    [root, region, parent, focused]
}

/// How many commands reached each control of the chain; `u32::MAX` for a
/// control that does not exist.
fn reached(driver: &Driver) -> [u32; 4] {
    ids(driver).map(|id| driver.runtime.tree().state(id).copied().unwrap_or(u32::MAX))
}

fn press_left(driver: &mut Driver) -> Output {
    driver.events(vec![press(NamedKey::LEFT, stamp(1, 1))])
}

#[test]
fn the_focused_control_handles_its_command() {
    let mut driver = chain(&[3, 2, 1], true);
    let [_, _, _, focused] = ids(&driver);
    let output = press_left(&mut driver);
    assert_eq!(
        Driver::outcomes(&output),
        [format!("handled kosmo.MoveLeft by control {focused}")]
    );
    assert_eq!(reached(&driver), [0, 0, 0, 1]);
    assert_eq!(driver.app.log, ["kosmo.MoveLeft by focused"]);
}

#[test]
fn an_unhandled_command_bubbles_to_the_parent() {
    let mut driver = chain(&[2], true);
    let [_, _, parent, _] = ids(&driver);
    let output = press_left(&mut driver);
    assert_eq!(
        Driver::outcomes(&output),
        [format!("handled kosmo.MoveLeft by control {parent}")]
    );
    assert_eq!(reached(&driver), [0, 0, 1, 1]);
}

#[test]
fn an_unhandled_command_bubbles_to_the_regions_root() {
    let mut driver = chain(&[1], true);
    let [_, region, _, _] = ids(&driver);
    let output = press_left(&mut driver);
    assert_eq!(
        Driver::outcomes(&output),
        [format!("handled kosmo.MoveLeft by control {region}")]
    );
    assert_eq!(reached(&driver), [0, 1, 1, 1]);
}

#[test]
fn an_unhandled_command_bubbles_to_the_application() {
    let mut driver = chain(&[0], true);
    let output = press_left(&mut driver);
    assert_eq!(Driver::outcomes(&output), ["handled kosmo.MoveLeft by app"]);
    // The root above the region's root is not part of the region.
    assert_eq!(reached(&driver), [0, 1, 1, 1]);
    assert_eq!(driver.app.log, ["kosmo.MoveLeft by app"]);
}

#[test]
fn a_command_that_nobody_handles_is_unhandled() {
    let mut driver = chain(&[], false);
    let output = press_left(&mut driver);
    assert_eq!(Driver::outcomes(&output), ["unhandled kosmo.MoveLeft"]);
    assert_eq!(
        output
            .record
            .lines()
            .iter()
            .filter(|l| l.starts_with("key "))
            .count(),
        1
    );
    assert!(driver.app.log.is_empty());
}

#[test]
fn a_key_of_another_epoch_is_dropped() {
    let mut driver = chain(&[3], true);
    let output = driver.events(vec![press(NamedKey::LEFT, stamp(2, 1))]);
    assert_eq!(Driver::outcomes(&output), ["dropped epoch"]);
    assert_eq!(driver.runtime.counts().dropped_epoch, 1);
    assert_eq!(reached(&driver), [0, 0, 0, 0]);
    assert!(driver.app.log.is_empty());
}

#[test]
fn a_key_for_a_surface_without_focus_is_dropped() {
    let mut driver = chain(&[3], true);
    driver.events(vec![focus_out(stamp(1, 1))]);
    let output = press_left(&mut driver);
    assert_eq!(Driver::outcomes(&output), ["dropped no-focus"]);
    assert_eq!(driver.runtime.counts().dropped_no_focus, 1);
    assert_eq!(reached(&driver), [0, 0, 0, 0]);
    assert!(driver.app.log.is_empty());
}

#[test]
fn a_key_of_an_earlier_focus_generation_never_reaches_the_new_view() {
    let mut calc = Calc::new(Shape::Line);
    calc.parts[3].handles = vec![kosmo_ui::INSERT_TEXT];
    let mut driver = Driver::new(calc);
    driver.events(vec![focus_in(stamp(1, 1))]);
    driver.events(vec![focus_out(stamp(1, 1))]);
    driver.events(vec![focus_in(stamp(1, 2))]);
    let views = driver.runtime.tree().preorder();
    let output = driver.events(vec![char_press('1', stamp(1, 1))]);
    assert_eq!(Driver::outcomes(&output), ["dropped generation"]);
    assert_eq!(driver.runtime.counts().dropped_generation, 1);
    assert!(driver.app.log.is_empty());
    assert_eq!(driver.app.text(), "");
    assert_eq!(output.view, kosmo_ui::ViewPass::Skipped);
    assert_eq!(driver.runtime.tree().preorder(), views);
}
