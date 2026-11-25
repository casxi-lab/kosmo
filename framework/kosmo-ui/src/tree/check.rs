//! The invariants of the control tree, checked by test code, and the
//! entries that tests use to break them on purpose. None of this is in a
//! build without tests.
//!
//! - Parents and children agree: the children of each control's parent
//!   hold it exactly once, and the parent of each child is the control.
//! - No cycle: from any control, following parents reaches the root in at
//!   most `MAX_DEPTH` controls.
//! - Each control is in the tree exactly once: a preorder traversal from
//!   the root visits each control once and as many as the tree holds.
//! - Traversal follows the children lists in view order: each control's
//!   children have the keys of the view node's children, in the same order.
//! - An identity of a removed control finds no control, and its slot's
//!   generation has increased or the slot is retired.
//! - Building the tree once from the last view gives the same tree as
//!   rebuilding it round by round: the same keys, kinds, controls and
//!   children order, whatever the identities and presentation states.
//! - The same sequence of rebuilds gives the same identities.
//! - Bounds: at most `MAX_WIDGETS` controls, at most `MAX_DEPTH` deep.

use std::collections::BTreeSet;

use super::{Node, Tree, View, WidgetId};
use crate::app::{App, Control};
use crate::tree::{MAX_DEPTH, MAX_WIDGETS};

/// An invariant of the tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Invariant {
    ParentsAgree,
    NoCycle,
    OncePerControl,
    ViewOrder,
    OldIdentities,
    Bounds,
}

/// A violated invariant, with the controls that it concerns.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Violation {
    pub(crate) invariant: Invariant,
    pub(crate) widgets: Vec<WidgetId>,
}

/// The live controls of the tree, in slot order.
fn live<A: App>(tree: &Tree<A>) -> Vec<(WidgetId, &Node<A>)> {
    tree.slots
        .iter()
        .enumerate()
        .filter_map(|(slot, entry)| {
            entry.node.as_ref().map(|node| {
                let id = WidgetId {
                    slot: slot as u32,
                    generation: entry.generation,
                };
                (id, node)
            })
        })
        .collect()
}

/// Checks every invariant but the two comparisons against `view`, the view that the tree was last
/// rebuilt from, and `removed`, identities of removed controls. Each
/// violation names the controls that break the invariant.
pub(crate) fn check<A: App>(
    tree: &Tree<A>,
    view: &View<A::Control>,
    removed: &[WidgetId],
) -> Vec<Violation> {
    let mut found = Vec::new();
    let mut report = |invariant, widgets: Vec<WidgetId>| {
        found.push(Violation { invariant, widgets });
    };
    let controls = live(tree);

    // Parents and children agree.
    for &(id, node) in &controls {
        match node.parent {
            Some(parent) => {
                let holds = tree.children(parent).iter().filter(|&&c| c == id).count();
                if holds != 1 {
                    report(Invariant::ParentsAgree, vec![id, parent]);
                }
            }
            None if tree.root != Some(id) => report(Invariant::ParentsAgree, vec![id]),
            None => {}
        }
        for &child in &node.children {
            if tree.node(child).and_then(|child| child.parent) != Some(id) {
                report(Invariant::ParentsAgree, vec![id, child]);
            }
        }
    }

    // No cycle.
    for &(id, _) in &controls {
        let mut at = id;
        let mut depth = 1;
        while let Some(parent) = tree.node(at).and_then(|node| node.parent) {
            depth += 1;
            if depth > MAX_DEPTH {
                report(Invariant::NoCycle, vec![id]);
                break;
            }
            at = parent;
        }
    }

    // Each control is in the tree once.
    let mut visited = BTreeSet::new();
    let mut stack: Vec<WidgetId> = tree.root.into_iter().collect();
    while let Some(id) = stack.pop() {
        if !visited.insert(id) || tree.node(id).is_none() {
            report(Invariant::OncePerControl, vec![id]);
            continue;
        }
        stack.extend(tree.children(id).iter().rev());
    }
    let unreached: Vec<WidgetId> = controls
        .iter()
        .map(|&(id, _)| id)
        .filter(|id| !visited.contains(id))
        .collect();
    if !unreached.is_empty() || controls.len() != tree.live {
        report(Invariant::OncePerControl, unreached);
    }

    // Traversal follows the view order.
    if let Some(root) = tree.root {
        let mut pairs = vec![(root, view)];
        while let Some((id, view)) = pairs.pop() {
            let keys: Vec<_> = tree.children(id).iter().map(|&c| tree.key(c)).collect();
            let expected: Vec<_> = view.children.iter().map(|child| Some(child.key)).collect();
            if keys != expected {
                report(Invariant::ViewOrder, vec![id]);
                continue;
            }
            pairs.extend(tree.children(id).iter().copied().zip(&view.children));
        }
    }

    // Old identities find nothing.
    for &id in removed {
        let slot = tree.slots.get(id.slot as usize);
        let retired = slot.is_some_and(|slot| slot.node.is_none() && !tree.free.contains(&id.slot));
        let advanced = slot.is_some_and(|slot| slot.generation > id.generation);
        if tree.contains(id) || !(advanced || retired) {
            report(Invariant::OldIdentities, vec![id]);
        }
    }

    // Bounds.
    if tree.live > MAX_WIDGETS || depth(tree) > MAX_DEPTH {
        report(Invariant::Bounds, tree.root.into_iter().collect());
    }
    found
}

/// The depth of the tree, following children lists and counting each
/// control once.
pub(crate) fn depth<A: App>(tree: &Tree<A>) -> usize {
    let mut deepest = 0;
    let mut seen = BTreeSet::new();
    let mut stack: Vec<(WidgetId, usize)> = tree.root.into_iter().map(|id| (id, 1)).collect();
    while let Some((id, depth)) = stack.pop() {
        if !seen.insert(id) {
            continue;
        }
        deepest = deepest.max(depth);
        stack.extend(tree.children(id).iter().map(|&child| (child, depth + 1)));
    }
    deepest
}

/// Whether the invariants that `check` reports include `invariant`.
pub(crate) fn breaks(found: &[Violation], invariant: Invariant) -> bool {
    found
        .iter()
        .any(|violation| violation.invariant == invariant)
}

/// Whether two trees have the same keys, kinds, controls and children
/// order, whatever their identities and presentation states.
pub(crate) fn same_tree<A: App>(a: &Tree<A>, b: &Tree<A>) -> bool
where
    A::Control: PartialEq,
{
    let mut pairs = match (a.root, b.root) {
        (Some(x), Some(y)) => vec![(x, y)],
        (None, None) => return true,
        _ => return false,
    };
    while let Some((x, y)) = pairs.pop() {
        let (Some(left), Some(right)) = (a.node(x), b.node(y)) else {
            return false;
        };
        if left.key != right.key
            || left.control.kind() != right.control.kind()
            || left.control != right.control
            || left.children.len() != right.children.len()
        {
            return false;
        }
        pairs.extend(
            left.children
                .iter()
                .copied()
                .zip(right.children.iter().copied()),
        );
    }
    true
}

/// The identities of a tree in preorder, which the same sequence of
/// rebuilds gives again.
pub(crate) fn identities<A: App>(tree: &Tree<A>) -> Vec<WidgetId> {
    tree.preorder()
}

/// Direct entries that break the tree on purpose.
impl<A: App> Tree<A> {
    /// Sets the parent of `id` without changing any children list.
    pub(crate) fn set_parent(&mut self, id: WidgetId, parent: Option<WidgetId>) {
        if let Some(node) = self.node_mut(id) {
            node.parent = parent;
        }
    }

    /// The children list of `id`, to change it without changing parents.
    pub(crate) fn children_mut(&mut self, id: WidgetId) -> Option<&mut Vec<WidgetId>> {
        self.node_mut(id).map(|node| &mut node.children)
    }

    /// Removes the control `id` from its slot without increasing the slot's
    /// generation, and frees the slot.
    pub(crate) fn remove_keeping_generation(&mut self, id: WidgetId) {
        if let Some(slot) = self.slots.get_mut(id.slot as usize)
            && slot.node.take().is_some()
        {
            self.live -= 1;
            self.free.push(id.slot);
        }
    }

    /// Sets the generation of the slot of `id`, which keeps its control,
    /// and renames the control in its parent, its children and the marks.
    pub(crate) fn set_generation(&mut self, id: WidgetId, generation: u32) -> WidgetId {
        let renamed = WidgetId {
            slot: id.slot,
            generation,
        };
        let Some(slot) = self.slots.get_mut(id.slot as usize) else {
            return id;
        };
        slot.generation = generation;
        let Some(node) = self.node(renamed) else {
            return renamed;
        };
        let (parent, children) = (node.parent, node.children.clone());
        if let Some(list) = parent.and_then(|parent| self.children_mut(parent)) {
            for child in list.iter_mut().filter(|child| **child == id) {
                *child = renamed;
            }
        }
        for child in children {
            self.set_parent(child, Some(renamed));
        }
        for mark in [&mut self.root, &mut self.focused, &mut self.region] {
            if *mark == Some(id) {
                *mark = Some(renamed);
            }
        }
        renamed
    }
}
