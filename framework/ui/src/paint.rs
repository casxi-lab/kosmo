//! The drawing pass: the display list of a surface's controls for its
//! damage region.

use kosmo_render::{Command, DisplayList, ListBuilder, ListError};
use kosmo_types::{Damage, DamageRect, Fx, Point, Rect, Size};

use crate::app::{App, Canvas, Control, DrawContext};
use crate::hit::{LayoutError, LayoutResult, Placed, negate};
use crate::tree::{Tree, WidgetId};

/// Why the drawing pass gave no display list.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PaintError {
    /// The display list refused a command, or a control left a group open.
    List(ListError),
    /// A box is out of preorder, or a position does not fit the numeric
    /// representation.
    Layout(LayoutError),
    /// A box names a control that the tree does not hold.
    MissingControl(WidgetId),
}

impl From<ListError> for PaintError {
    fn from(error: ListError) -> PaintError {
        PaintError::List(error)
    }
}

impl From<LayoutError> for PaintError {
    fn from(error: LayoutError) -> PaintError {
        PaintError::Layout(error)
    }
}

/// A box whose groups the pass has started: its index, and whether the
/// group of its children is started.
struct Open {
    index: usize,
    children: bool,
}

/// The display list of the controls of `layout`, the layout result that hit
/// testing reads, for the region `damage` of their surface.
///
/// The boxes are visited in preorder, which is the drawing order. A box
/// that is visible inside no rectangle of `damage`, moved by the scroll
/// offsets of its ancestors and clipped by their boxes as
/// [`hit_test`](crate::hit_test) does, is skipped with all its
/// descendants, which lie inside it. Each other box whose control draws
/// has [`Control::draw`] called, with commands relative to the box and
/// clipped to it. The children of a box are drawn in a group that clips
/// them to the box and moves them to its content, its position minus its
/// scroll offset. A group starts only when something is drawn in it, so a
/// box that draws nothing adds no command.
///
/// The pass reads only its input. It returns an error, and no list, when a
/// box is out of preorder, when a position does not fit, when a box names
/// no control of `tree`, and when the list refuses a command or a control
/// leaves a group open.
pub fn paint<A: App>(
    tree: &Tree<A>,
    layout: &LayoutResult,
    damage: &Damage,
) -> Result<DisplayList, PaintError> {
    let damage = damage
        .rects()
        .iter()
        .map(|&rect| in_units(rect))
        .collect::<Result<Vec<Rect>, LayoutError>>()?;
    let boxes = layout.boxes();
    let mut list = ListBuilder::new();
    let mut placed: Vec<Option<Placed>> = Vec::with_capacity(boxes.len());
    let mut open: Vec<Open> = Vec::new();
    for (index, layout_box) in boxes.iter().enumerate() {
        let parent = match layout_box.parent {
            Some(parent) => match placed.get(parent).copied().flatten() {
                Some(place) => Some((parent, place)),
                None => {
                    // The parent was skipped.
                    placed.push(None);
                    continue;
                }
            },
            None => None,
        };
        let place = Placed::of(parent.map(|(_, place)| place), layout_box)?;
        let mut visible = false;
        for &rect in &damage {
            let both = place
                .visible
                .intersection(rect)
                .ok_or(LayoutError::Overflow)?;
            visible |= both.size().width() > Fx::ZERO && both.size().height() > Fx::ZERO;
        }
        if !visible {
            placed.push(None);
            continue;
        }
        placed.push(Some(place));
        // End the groups of the boxes before this one that are not its
        // ancestors.
        if let Some((parent, _)) = parent {
            if !open.iter().any(|started| started.index == parent) {
                return Err(LayoutError::Order.into());
            }
            while let Some(last) = open.pop_if(|started| started.index != parent) {
                end(&mut list, last)?;
            }
        }
        // Start the group of the parent's children.
        if let Some(last) = open.last_mut()
            && !last.children
        {
            let parent = boxes.get(last.index).ok_or(LayoutError::Order)?;
            let origin = parent.rect.origin();
            let offset = origin
                .translated(negate(parent.scroll.x)?, negate(parent.scroll.y)?)
                .ok_or(LayoutError::Overflow)?;
            list.push(Command::PushClip {
                rect: parent.rect,
                offset,
            })?;
            last.children = true;
        }
        if layout_box.draws {
            let id = layout_box.widget;
            let (Some(control), Some(state)) = (tree.control(id), tree.state(id)) else {
                return Err(PaintError::MissingControl(id));
            };
            let canvas = Canvas::new(&mut list, layout_box.rect);
            let mut cx = DrawContext::new(id, state, layout_box.rect.size(), canvas);
            control.draw(&mut cx);
            cx.into_canvas().finish()?;
        }
        open.push(Open {
            index,
            children: false,
        });
    }
    while let Some(last) = open.pop() {
        end(&mut list, last)?;
    }
    Ok(list.finish()?)
}

/// Ends the group of the children of a box, if it was started.
fn end(list: &mut ListBuilder, open: Open) -> Result<(), ListError> {
    if open.children {
        list.push(Command::PopClip)?;
    }
    Ok(())
}

/// A damage rectangle in the numeric representation.
fn in_units(rect: DamageRect) -> Result<Rect, LayoutError> {
    let pixels = |value: u32| {
        i32::try_from(value)
            .ok()
            .and_then(Fx::from_pixels)
            .ok_or(LayoutError::Overflow)
    };
    let origin = Point::new(pixels(rect.x())?, pixels(rect.y())?);
    let size =
        Size::new(pixels(rect.width())?, pixels(rect.height())?).ok_or(LayoutError::Overflow)?;
    Ok(Rect::new(origin, size))
}

#[cfg(test)]
mod tests;
