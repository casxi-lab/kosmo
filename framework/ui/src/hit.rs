//! Hit testing: a geometric query over a layout result.

use kosmo_types::{Fx, Point, Rect};

use crate::tree::{MAX_WIDGETS, WidgetId};

/// The box of one control in a layout result.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LayoutBox {
    /// The control.
    pub widget: WidgetId,
    /// The index of the parent's box in the layout result, which comes
    /// earlier; `None` only for the first box, the root.
    pub parent: Option<usize>,
    /// The box in the content coordinates of its parent: its parent's
    /// position minus the parent's scroll offset, or the surface's origin
    /// for the root.
    pub rect: Rect,
    /// How far the control's content is scrolled: its children are moved
    /// left by `x` and up by `y`.
    pub scroll: Point,
    /// Whether the control draws. A control that draws nothing is never
    /// hit.
    pub draws: bool,
}

/// The boxes of a surface's controls in preorder, which is the drawing
/// order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LayoutResult {
    boxes: Vec<LayoutBox>,
}

/// Why a layout result cannot be built or queried.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LayoutError {
    /// A box names no earlier box as its parent, or a box after the first
    /// has no parent.
    Order,
    /// A layout result holds at most `MAX_WIDGETS` boxes.
    TooManyBoxes,
    /// A position or an edge does not fit the numeric representation.
    Overflow,
}

impl LayoutResult {
    /// An empty layout result.
    pub fn new() -> LayoutResult {
        LayoutResult::default()
    }

    /// Appends `layout_box` in preorder and returns its index.
    pub fn push(&mut self, layout_box: LayoutBox) -> Result<usize, LayoutError> {
        let index = self.boxes.len();
        if index >= MAX_WIDGETS {
            return Err(LayoutError::TooManyBoxes);
        }
        match layout_box.parent {
            None if index == 0 => {}
            Some(parent) if parent < index => {}
            _ => return Err(LayoutError::Order),
        }
        self.boxes.push(layout_box);
        Ok(index)
    }

    /// The boxes in preorder.
    pub fn boxes(&self) -> &[LayoutBox] {
        &self.boxes
    }
}

/// The path from the root to the control at `point`, or `None` when no
/// control is hit.
///
/// Each box is moved by the scroll offsets of all its ancestors and clipped
/// by all their boxes. A box holds its left and top edges but not its right
/// and bottom edges. Where boxes overlap, the last in drawing order is hit,
/// and a control that draws nothing is never hit; the path holds the
/// ancestors of the control hit, whether they draw or not. The function
/// only reads its input.
pub fn hit_test(layout: &LayoutResult, point: Point) -> Result<Option<Vec<WidgetId>>, LayoutError> {
    // For each box: the origin of its content, and the part of the surface
    // where it is visible.
    let mut placed: Vec<Placed> = Vec::with_capacity(layout.boxes.len());
    let mut hit = None;
    for (index, layout_box) in layout.boxes.iter().enumerate() {
        let parent = layout_box
            .parent
            .and_then(|parent| placed.get(parent))
            .copied();
        let place = Placed::of(parent, layout_box)?;
        placed.push(place);
        if layout_box.draws && place.visible.contains(point).ok_or(LayoutError::Overflow)? {
            hit = Some(index);
        }
    }
    let Some(hit) = hit else {
        return Ok(None);
    };
    let mut path = Vec::new();
    let mut next = Some(hit);
    while let Some(index) = next {
        let Some(layout_box) = layout.boxes.get(index) else {
            break;
        };
        path.push(layout_box.widget);
        next = layout_box.parent;
    }
    path.reverse();
    Ok(Some(path))
}

/// Where a box lies on its surface.
#[derive(Clone, Copy)]
pub(crate) struct Placed {
    /// The origin of the box's content: its position moved back by its
    /// scroll offset.
    pub(crate) content: Point,
    /// The part of the surface where the box is visible: the box, clipped
    /// by the boxes of all its ancestors.
    pub(crate) visible: Rect,
}

impl Placed {
    /// The place of `layout_box`, whose parent lies at `parent`, or at the
    /// surface's origin when it has none.
    pub(crate) fn of(
        parent: Option<Placed>,
        layout_box: &LayoutBox,
    ) -> Result<Placed, LayoutError> {
        let base = parent.map_or(Point::default(), |parent| parent.content);
        let at = layout_box.rect.origin();
        let origin = base.translated(at.x, at.y).ok_or(LayoutError::Overflow)?;
        let own = Rect::new(origin, layout_box.rect.size());
        let visible = match parent {
            Some(parent) => parent
                .visible
                .intersection(own)
                .ok_or(LayoutError::Overflow)?,
            None => own,
        };
        let content = origin
            .translated(negate(layout_box.scroll.x)?, negate(layout_box.scroll.y)?)
            .ok_or(LayoutError::Overflow)?;
        Ok(Placed { content, visible })
    }
}

pub(crate) fn negate(value: Fx) -> Result<Fx, LayoutError> {
    Fx::ZERO.checked_sub(value).ok_or(LayoutError::Overflow)
}

#[cfg(test)]
mod tests;
