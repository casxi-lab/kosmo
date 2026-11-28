//! The accessibility pass: the AccessKit node of each control, the tree of
//! each surface with a layout, and the update that brings the tree that the
//! platform keeps from the tree last pushed for the surface to the new one.

use std::collections::BTreeMap;

use kosmo_platform::accesskit::{Node, NodeId, Rect, Role, TreeId, TreeInfo, TreeUpdate};
use kosmo_types::Size;

use crate::app::{App, Control};
use crate::layout::{Axis, LayoutTree, Node as LayoutNode};
use crate::tree::{Tree, WidgetId};

/// The identifier of the accessibility node of the control `id`: its
/// identity as one `u64`. Nodes and controls correspond one to one, a
/// control keeps its node's identifier while it keeps its identity, and
/// the identifier of a removed control is never used again.
pub(crate) fn node_id(id: WidgetId) -> NodeId {
    NodeId(id.to_bits())
}

/// The node of each control apart from what depends on a surface's layout:
/// the default of its layout node as the control's entry changed it, with
/// its children, and the axis of a scroll container.
pub(crate) struct Controls {
    nodes: BTreeMap<WidgetId, (Node, Option<Axis>)>,
}

/// The tree of one surface as the accessibility pass builds it: its root,
/// the focused node and every node, in preorder.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Built {
    pub(crate) root: NodeId,
    pub(crate) focus: NodeId,
    pub(crate) nodes: Vec<(NodeId, Node)>,
}

/// What the runtime keeps of the tree that it last gave the platform for a
/// surface: its root, its focus and its nodes, which it reads only to work
/// out the next update.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Pushed {
    root: NodeId,
    focus: NodeId,
    nodes: BTreeMap<NodeId, Node>,
}

/// The nodes of the controls of `tree`, calling each control's
/// accessibility entry once.
///
/// A control's node starts from its layout node: a text leaf is a
/// `Role::Label` named by its text, a scroll container a `Role::ScrollView`,
/// and any other node a `Role::GenericContainer` without a name. The
/// control's entry may change it; then its children are those of the
/// control in the tree, in order, and the transform, the actions, the tree
/// identifier and every property that the layout of a surface gives are
/// cleared, whatever the entry wrote there.
pub(crate) fn controls<A: App>(tree: &Tree<A>) -> Controls {
    let mut nodes = BTreeMap::new();
    for id in tree.preorder() {
        let Some(entry) = tree.node(id) else {
            continue;
        };
        let (mut node, axis) = match entry.control.layout(&entry.state) {
            LayoutNode::Text(text) => {
                let mut node = Node::new(Role::Label);
                node.set_label(text.text());
                (node, None)
            }
            LayoutNode::Scroll(scroll) => (Node::new(Role::ScrollView), Some(scroll.axis())),
            _ => (Node::new(Role::GenericContainer), None),
        };
        entry.control.accessibility(&entry.state, &mut node);
        let children: Vec<NodeId> = tree.children(id).iter().map(|&c| node_id(c)).collect();
        node.set_children(children);
        node.clear_transform();
        node.clear_actions();
        node.clear_child_actions();
        node.clear_tree_id();
        node.clear_bounds();
        node.clear_scroll_x();
        node.clear_scroll_x_min();
        node.clear_scroll_x_max();
        node.clear_scroll_y();
        node.clear_scroll_y_min();
        node.clear_scroll_y_max();
        node.clear_clips_children();
        nodes.insert(id, (node, axis));
    }
    Controls { nodes }
}

/// The tree of the surface laid out as `layout`, from the nodes of the
/// controls of `tree`; `None` for a tree without controls.
///
/// Each node's bounding box is its control's box in the coordinates of the
/// surface: the box's position moved by the positions of its ancestors and
/// back by their scroll offsets, the geometry that hit testing reads,
/// neither clipped nor transformed. Each coordinate, a whole number of
/// 1/64 pixel, is converted exactly. A scroll container's node also has
/// its offset along its axis, the range of the offset from 0 to the length
/// of its content less its own, or 0 when the content is shorter, and
/// clips its children. The focus is the control that the view marks as
/// focused, or the root without one.
pub(crate) fn surface<A: App>(
    tree: &Tree<A>,
    controls: &Controls,
    layout: &LayoutTree,
) -> Option<Built> {
    let root = tree.root()?;
    let boxes = layout.boxes();
    // The origin of each box's content in the surface, in 1/64 pixel.
    let mut contents: Vec<(i64, i64)> = Vec::with_capacity(boxes.len());
    let mut nodes = Vec::with_capacity(boxes.len());
    for (index, laid) in boxes.iter().enumerate() {
        let (left, top) = laid
            .parent
            .and_then(|parent| contents.get(parent))
            .copied()
            .unwrap_or((0, 0));
        let x = left + i64::from(laid.position.x.units());
        let y = top + i64::from(laid.position.y.units());
        contents.push((
            x - i64::from(laid.scroll.x.units()),
            y - i64::from(laid.scroll.y.units()),
        ));
        let Some((node, axis)) = controls.nodes.get(&laid.widget) else {
            continue;
        };
        let mut node = node.clone();
        let (width, height) = lengths(laid.size);
        node.set_bounds(Rect {
            x0: pixels(x),
            y0: pixels(y),
            x1: pixels(x + width),
            y1: pixels(y + height),
        });
        if let Some(axis) = axis {
            // The content is the first child, the next box in preorder.
            let content = boxes
                .get(index + 1)
                .filter(|child| child.parent == Some(index))
                .map_or((0, 0), |child| lengths(child.size));
            match axis {
                Axis::X => {
                    node.set_scroll_x(pixels(i64::from(laid.scroll.x.units())));
                    node.set_scroll_x_min(0.0);
                    node.set_scroll_x_max(pixels((content.0 - width).max(0)));
                }
                Axis::Y => {
                    node.set_scroll_y(pixels(i64::from(laid.scroll.y.units())));
                    node.set_scroll_y_min(0.0);
                    node.set_scroll_y_max(pixels((content.1 - height).max(0)));
                }
            }
            node.set_clips_children();
        }
        nodes.push((node_id(laid.widget), node));
    }
    let root = node_id(root);
    let focus = tree.focused().map_or(root, node_id);
    Some(Built { root, focus, nodes })
}

/// The update that brings the tree that the platform keeps from `last`,
/// the tree last given to it, to `built`, or `None` when nothing changed,
/// and what to keep of `built` for the next update.
///
/// Without `last` the update holds every node and the tree information;
/// otherwise it holds the nodes whose content differs from `last`, new
/// nodes and parents whose children changed among them, and the tree
/// information only when the root changed. The platform drops the nodes
/// that are no longer reachable. Every update holds the focus. When no
/// node and not the focus changed, there is no update. The tree
/// information names no toolkit.
pub(crate) fn update(last: Option<&Pushed>, built: Built) -> (Option<TreeUpdate>, Pushed) {
    let Built { root, focus, nodes } = built;
    let (changed, tree): (Vec<(NodeId, Node)>, Option<TreeInfo>) = match last {
        None => (nodes.clone(), Some(TreeInfo::new(root))),
        Some(last) => (
            nodes
                .iter()
                .filter(|(id, node)| last.nodes.get(id) != Some(node))
                .cloned()
                .collect(),
            (last.root != root).then(|| TreeInfo::new(root)),
        ),
    };
    let same = last.is_some_and(|last| last.focus == focus);
    let update = (!changed.is_empty() || tree.is_some() || !same).then_some(TreeUpdate {
        nodes: changed,
        tree,
        tree_id: TreeId::ROOT,
        focus,
    });
    let kept = Pushed {
        root,
        focus,
        nodes: nodes.into_iter().collect(),
    };
    (update, kept)
}

/// The width and height of `size` in 1/64 pixel.
fn lengths(size: Size) -> (i64, i64) {
    (
        i64::from(size.width().units()),
        i64::from(size.height().units()),
    )
}

/// `units` of 1/64 pixel as pixels, exactly: every value here is far
/// within the integers that an `f64` holds, and the division by a power of
/// two is exact.
fn pixels(units: i64) -> f64 {
    units as f64 / 64.0
}

#[cfg(test)]
mod tests;
