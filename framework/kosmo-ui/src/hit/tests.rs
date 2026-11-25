use kosmo_types::Size;

use super::*;

fn px(pixels: i32) -> Fx {
    Fx::from_pixels(pixels).unwrap()
}

fn point(x: i32, y: i32) -> Point {
    Point::new(px(x), px(y))
}

/// The rectangle at `x`, `y` of `width` by `height`, in pixels.
fn rect(x: i32, y: i32, width: i32, height: i32) -> Rect {
    Rect::new(point(x, y), Size::new(px(width), px(height)).unwrap())
}

fn id(slot: u32) -> WidgetId {
    WidgetId::for_test(slot)
}

/// A box of the control `slot` that draws and is not scrolled.
fn drawn(slot: u32, parent: Option<usize>, rect: Rect) -> LayoutBox {
    LayoutBox {
        widget: id(slot),
        parent,
        rect,
        scroll: Point::default(),
        draws: true,
    }
}

fn layout(boxes: &[LayoutBox]) -> LayoutResult {
    let mut layout = LayoutResult::new();
    for &layout_box in boxes {
        layout.push(layout_box).unwrap();
    }
    layout
}

/// A root box of 100 by 100 pixels with one child of 20 by 20 at 10, 10.
fn root_and_child() -> LayoutResult {
    layout(&[
        drawn(0, None, rect(0, 0, 100, 100)),
        drawn(1, Some(0), rect(10, 10, 20, 20)),
    ])
}

#[test]
fn a_point_in_a_box_hits_it() {
    assert_eq!(
        hit_test(&root_and_child(), point(15, 15)),
        Ok(Some(vec![id(0), id(1)]))
    );
}

#[test]
fn right_and_bottom_edges_are_outside() {
    let layout = root_and_child();
    assert_eq!(hit_test(&layout, point(30, 15)), Ok(Some(vec![id(0)])));
    assert_eq!(hit_test(&layout, point(15, 30)), Ok(Some(vec![id(0)])));
}

#[test]
fn left_and_top_edges_are_inside() {
    assert_eq!(
        hit_test(&root_and_child(), point(10, 10)),
        Ok(Some(vec![id(0), id(1)]))
    );
}

#[test]
fn of_overlapping_boxes_the_later_in_drawing_order_is_hit() {
    let layout = layout(&[
        drawn(0, None, rect(0, 0, 100, 100)),
        drawn(1, Some(0), rect(10, 10, 40, 40)),
        drawn(2, Some(0), rect(30, 30, 40, 40)),
    ]);
    assert_eq!(
        hit_test(&layout, point(35, 35)),
        Ok(Some(vec![id(0), id(2)]))
    );
}

#[test]
fn a_box_is_clipped_by_all_its_ancestors() {
    let layout = layout(&[
        drawn(0, None, rect(0, 0, 100, 100)),
        drawn(1, Some(0), rect(0, 0, 50, 50)),
        drawn(2, Some(1), rect(40, 40, 30, 30)),
    ]);
    assert_eq!(hit_test(&layout, point(60, 60)), Ok(Some(vec![id(0)])));
    assert_eq!(
        hit_test(&layout, point(45, 45)),
        Ok(Some(vec![id(0), id(1), id(2)]))
    );
}

#[test]
fn a_box_moves_by_the_scroll_offset_of_its_ancestors() {
    let mut parent = drawn(1, Some(0), rect(0, 0, 50, 50));
    parent.scroll = point(0, 20);
    let layout = layout(&[
        drawn(0, None, rect(0, 0, 100, 100)),
        parent,
        drawn(2, Some(1), rect(0, 40, 50, 10)),
    ]);
    // The child lies at 0, 20 to 50, 30 on the surface.
    assert_eq!(
        hit_test(&layout, point(5, 25)),
        Ok(Some(vec![id(0), id(1), id(2)]))
    );
    assert_eq!(
        hit_test(&layout, point(5, 45)),
        Ok(Some(vec![id(0), id(1)]))
    );
}

#[test]
fn a_control_that_draws_nothing_is_not_hit() {
    let mut cover = drawn(2, Some(0), rect(10, 10, 40, 40));
    cover.draws = false;
    let layout = layout(&[
        drawn(0, None, rect(0, 0, 100, 100)),
        drawn(1, Some(0), rect(10, 10, 40, 40)),
        cover,
    ]);
    assert_eq!(
        hit_test(&layout, point(20, 20)),
        Ok(Some(vec![id(0), id(1)]))
    );
}

#[test]
fn a_point_outside_every_box_hits_nothing() {
    assert_eq!(hit_test(&root_and_child(), point(150, 150)), Ok(None));
    assert_eq!(hit_test(&LayoutResult::new(), point(0, 0)), Ok(None));
}

#[test]
fn boxes_come_in_preorder_and_positions_fit() {
    let mut layout = LayoutResult::new();
    assert_eq!(
        layout.push(drawn(0, Some(0), rect(0, 0, 1, 1))),
        Err(LayoutError::Order)
    );
    assert_eq!(layout.push(drawn(0, None, rect(0, 0, 1, 1))), Ok(0));
    assert_eq!(
        layout.push(drawn(1, None, rect(0, 0, 1, 1))),
        Err(LayoutError::Order)
    );
    assert_eq!(
        layout.push(drawn(1, Some(1), rect(0, 0, 1, 1))),
        Err(LayoutError::Order)
    );
    let far = Rect::new(
        Point::new(Fx::MAX, Fx::ZERO),
        Size::new(px(1), px(1)).unwrap(),
    );
    assert_eq!(layout.push(drawn(1, Some(0), far)), Ok(1));
    assert_eq!(layout.boxes().len(), 2);
    assert_eq!(hit_test(&layout, point(0, 0)), Err(LayoutError::Overflow));

    let mut full = LayoutResult::new();
    full.push(drawn(0, None, rect(0, 0, 1, 1))).unwrap();
    for slot in 1..MAX_WIDGETS as u32 {
        full.push(drawn(slot, Some(0), rect(0, 0, 1, 1))).unwrap();
    }
    assert_eq!(
        full.push(drawn(0, Some(0), rect(0, 0, 1, 1))),
        Err(LayoutError::TooManyBoxes)
    );
}
