//! The control tree: the controls in a slot table with their identities,
//! and the view rebuild that applies a view tree to it.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::app::{App, Control, StateOf};

/// The most controls that a control tree holds.
pub const MAX_WIDGETS: usize = 4_096;

/// The greatest depth of a control tree: the number of controls on the
/// longest path from the root down, both ends included. A tree of only a
/// root has depth 1.
pub const MAX_DEPTH: usize = 64;

/// The identity of a control: a slot of the control tree's slot table and
/// the generation of that slot. It fits in a `u64` without loss.
///
/// Removing a control increases the generation of its slot, so the old
/// identity finds no control. A slot whose generation would pass `u32::MAX`
/// is retired and never used again; generations never wrap.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WidgetId {
    slot: u32,
    generation: u32,
}

impl WidgetId {
    /// The slot.
    pub const fn slot(self) -> u32 {
        self.slot
    }

    /// The generation of the slot.
    pub const fn generation(self) -> u32 {
        self.generation
    }

    /// The identity as a `u64`: the slot in the high 32 bits and the
    /// generation in the low 32 bits.
    pub const fn to_bits(self) -> u64 {
        ((self.slot as u64) << 32) | self.generation as u64
    }

    /// The identity of `slot` at generation 0, for tests that need
    /// identities without a tree.
    #[cfg(test)]
    pub(crate) const fn for_test(slot: u32) -> WidgetId {
        WidgetId {
            slot,
            generation: 0,
        }
    }
}

impl fmt::Display for WidgetId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.slot, self.generation)
    }
}

/// The key of a view node, unique among its siblings.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Key(u64);

impl Key {
    /// The key `value`.
    pub const fn new(value: u64) -> Key {
        Key(value)
    }

    /// The key as a number.
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// The kind of a control, a constant name. With the key, it decides
/// whether a control keeps its identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Kind(&'static str);

impl Kind {
    /// The kind `name`.
    pub const fn new(name: &'static str) -> Kind {
        Kind(name)
    }

    /// The name of the kind.
    pub const fn name(self) -> &'static str {
        self.0
    }
}

/// A node of a view tree: a key, the control that the view gives, its
/// children in order, and whether it is the focused control or the
/// region's root.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct View<C> {
    key: Key,
    control: C,
    children: Vec<View<C>>,
    focused: bool,
    region: bool,
}

impl<C> View<C> {
    /// A node with `key` and `control`, without children.
    pub fn new(key: Key, control: C) -> View<C> {
        View {
            key,
            control,
            children: Vec::new(),
            focused: false,
            region: false,
        }
    }

    /// The node with `child` appended to its children.
    pub fn child(mut self, child: View<C>) -> View<C> {
        self.children.push(child);
        self
    }

    /// Appends `child` to the node's children.
    pub fn push(&mut self, child: View<C>) {
        self.children.push(child);
    }

    /// The node marked as the focused control. At most one node of a view
    /// tree is marked.
    pub fn focused(mut self) -> View<C> {
        self.focused = true;
        self
    }

    /// The node marked as the region's root. At most one node of a view
    /// tree is marked; without a mark, the root of the tree is the region's
    /// root. The focused control must be this node or below it; a view that
    /// marks it elsewhere is rejected with
    /// [`RebuildError::FocusOutsideRegion`].
    pub fn region(mut self) -> View<C> {
        self.region = true;
        self
    }

    /// The key.
    pub fn key(&self) -> Key {
        self.key
    }

    /// The control.
    pub fn control(&self) -> &C {
        &self.control
    }

    /// The children, in order.
    pub fn children(&self) -> &[View<C>] {
        &self.children
    }

    /// Whether the node is the focused control.
    pub fn is_focused(&self) -> bool {
        self.focused
    }

    /// Whether the node is the region's root.
    pub fn is_region(&self) -> bool {
        self.region
    }
}

/// Drops the nodes of a view one by one, so that dropping a view of any
/// depth uses a bounded stack.
impl<C> Drop for View<C> {
    fn drop(&mut self) {
        let mut pending = std::mem::take(&mut self.children);
        while let Some(mut node) = pending.pop() {
            pending.append(&mut node.children);
        }
    }
}

/// Why a view rebuild was rejected. The round leaves the control tree as
/// it was.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RebuildError {
    /// The view tree has more than `MAX_WIDGETS` nodes.
    TooManyControls,
    /// The view tree is deeper than `MAX_DEPTH`.
    TooDeep,
    /// Two siblings have this key.
    DuplicateKey(Key),
    /// More than one node is marked as the focused control.
    SecondFocus,
    /// More than one node is marked as the region's root.
    SecondRegion,
    /// A node is marked as the region's root, and the focused control is
    /// neither that node nor below it.
    FocusOutsideRegion,
}

impl fmt::Display for RebuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RebuildError::TooManyControls => write!(f, "too-many-controls"),
            RebuildError::TooDeep => write!(f, "too-deep"),
            RebuildError::DuplicateKey(key) => write!(f, "duplicate-key {}", key.get()),
            RebuildError::SecondFocus => write!(f, "second-focus"),
            RebuildError::SecondRegion => write!(f, "second-region"),
            RebuildError::FocusOutsideRegion => write!(f, "focus-outside-region"),
        }
    }
}

impl std::error::Error for RebuildError {}

/// A control in the tree.
pub(crate) struct Node<A: App> {
    pub(crate) key: Key,
    pub(crate) control: A::Control,
    pub(crate) state: StateOf<A>,
    pub(crate) parent: Option<WidgetId>,
    pub(crate) children: Vec<WidgetId>,
}

/// A slot of the slot table: its generation and the control it holds.
struct Slot<A: App> {
    generation: u32,
    node: Option<Node<A>>,
}

/// The control tree of an application: one root, and for each control its
/// parent and its ordered children, in view tree order. Traversals go in
/// preorder, never in slot order, and the tree uses no hash containers.
///
/// The free slots are used last in, first out, so the same sequence of
/// rebuilds gives the same identities.
pub struct Tree<A: App> {
    slots: Vec<Slot<A>>,
    free: Vec<u32>,
    root: Option<WidgetId>,
    focused: Option<WidgetId>,
    region: Option<WidgetId>,
    live: usize,
}

/// One node of a planned rebuild, in view preorder.
struct Planned<'v, C> {
    view: &'v View<C>,
    /// The control that keeps its identity for this node.
    kept: Option<WidgetId>,
    /// The index of the parent's entry.
    parent: Option<usize>,
}

impl<A: App> Tree<A> {
    /// An empty tree.
    pub(crate) fn new() -> Tree<A> {
        Tree {
            slots: Vec::new(),
            free: Vec::new(),
            root: None,
            focused: None,
            region: None,
            live: 0,
        }
    }

    /// The number of controls.
    pub fn len(&self) -> usize {
        self.live
    }

    /// Whether the tree has no control.
    pub fn is_empty(&self) -> bool {
        self.live == 0
    }

    /// The root.
    pub fn root(&self) -> Option<WidgetId> {
        self.root
    }

    /// The focused control, which the view marks.
    pub fn focused(&self) -> Option<WidgetId> {
        self.focused
    }

    /// The control that the view marks as the region's root, or else the
    /// root.
    pub fn region(&self) -> Option<WidgetId> {
        self.region.or(self.root)
    }

    /// Whether `id` names a control of the tree.
    pub fn contains(&self, id: WidgetId) -> bool {
        self.node(id).is_some()
    }

    /// The key of the control `id`.
    pub fn key(&self, id: WidgetId) -> Option<Key> {
        self.node(id).map(|node| node.key)
    }

    /// The control `id`.
    pub fn control(&self, id: WidgetId) -> Option<&A::Control> {
        self.node(id).map(|node| &node.control)
    }

    /// The presentation state of the control `id`.
    pub fn state(&self, id: WidgetId) -> Option<&StateOf<A>> {
        self.node(id).map(|node| &node.state)
    }

    /// The parent of the control `id`.
    pub fn parent(&self, id: WidgetId) -> Option<WidgetId> {
        self.node(id).and_then(|node| node.parent)
    }

    /// The children of the control `id`, in order; none when it does not
    /// exist.
    pub fn children(&self, id: WidgetId) -> &[WidgetId] {
        self.node(id).map_or(&[], |node| &node.children)
    }

    /// Every control, in preorder.
    pub fn preorder(&self) -> Vec<WidgetId> {
        let mut order = Vec::with_capacity(self.live);
        let mut stack: Vec<WidgetId> = self.root.into_iter().collect();
        while let Some(id) = stack.pop() {
            if order.len() >= self.live {
                break;
            }
            order.push(id);
            stack.extend(self.children(id).iter().rev());
        }
        order
    }

    pub(crate) fn node(&self, id: WidgetId) -> Option<&Node<A>> {
        let slot = self.slots.get(id.slot as usize)?;
        if slot.generation == id.generation {
            slot.node.as_ref()
        } else {
            None
        }
    }

    pub(crate) fn node_mut(&mut self, id: WidgetId) -> Option<&mut Node<A>> {
        let slot = self.slots.get_mut(id.slot as usize)?;
        if slot.generation == id.generation {
            slot.node.as_mut()
        } else {
            None
        }
    }

    /// Applies `view` to the tree: a control under the same parent with
    /// the same key and kind keeps its identity and presentation state and
    /// takes the view's control; every other node gets a new control; the
    /// controls that the view no longer has are removed. The view is
    /// checked first, and a view beyond a bound leaves the tree as it was.
    pub(crate) fn rebuild(&mut self, view: &View<A::Control>) -> Result<(), RebuildError> {
        check_view(view)?;
        let mut planned = Vec::new();
        let mut removed = Vec::new();
        let root = self.root.filter(|&root| self.matches(root, view));
        if let (Some(old), None) = (self.root, root) {
            self.collect(old, &mut removed);
        }
        self.plan(view, root, None, &mut planned, &mut removed);
        for id in removed {
            self.remove(id);
        }
        self.apply(planned);
        Ok(())
    }

    /// Whether the control `id` keeps its identity for the node `view`.
    fn matches(&self, id: WidgetId, view: &View<A::Control>) -> bool {
        self.node(id)
            .is_some_and(|node| node.key == view.key && node.control.kind() == view.control.kind())
    }

    /// Plans the node `view`, for which `kept` keeps its identity, and its
    /// subtree; the controls under `kept` that no child keeps go into
    /// `removed` with their subtrees. The depth is bounded by `check_view`.
    fn plan<'v>(
        &self,
        view: &'v View<A::Control>,
        kept: Option<WidgetId>,
        parent: Option<usize>,
        planned: &mut Vec<Planned<'v, A::Control>>,
        removed: &mut Vec<WidgetId>,
    ) {
        let index = planned.len();
        planned.push(Planned { view, kept, parent });
        let old_children = kept.map_or(&[][..], |id| self.children(id));
        let mut by_key: BTreeMap<Key, WidgetId> = old_children
            .iter()
            .filter_map(|&child| self.key(child).map(|key| (key, child)))
            .collect();
        let kept_children: Vec<Option<WidgetId>> = view
            .children
            .iter()
            .map(|child| {
                let id = by_key
                    .get(&child.key)
                    .copied()
                    .filter(|&id| self.matches(id, child))?;
                by_key.remove(&child.key);
                Some(id)
            })
            .collect();
        for &old in old_children {
            if self.key(old).and_then(|key| by_key.get(&key)) == Some(&old) {
                self.collect(old, removed);
            }
        }
        for (child, kept) in view.children.iter().zip(kept_children) {
            self.plan(child, kept, Some(index), planned, removed);
        }
    }

    /// Appends the subtree of `id` to `out` in preorder.
    fn collect(&self, id: WidgetId, out: &mut Vec<WidgetId>) {
        let mut stack = vec![id];
        while let Some(id) = stack.pop() {
            out.push(id);
            stack.extend(self.children(id).iter().rev());
        }
    }

    /// Writes the planned nodes into the tree in preorder.
    fn apply(&mut self, planned: Vec<Planned<'_, A::Control>>) {
        let mut ids: Vec<WidgetId> = Vec::with_capacity(planned.len());
        self.focused = None;
        self.region = None;
        for entry in planned {
            let parent = entry.parent.and_then(|index| ids.get(index).copied());
            let kept = entry
                .kept
                .and_then(|id| self.node_mut(id).map(|node| (id, node)));
            let id = match kept {
                Some((id, node)) => {
                    node.control = entry.view.control.clone();
                    node.parent = parent;
                    node.children.clear();
                    id
                }
                None => self.insert(Node {
                    key: entry.view.key,
                    control: entry.view.control.clone(),
                    state: Default::default(),
                    parent,
                    children: Vec::new(),
                }),
            };
            if let Some(node) = parent.and_then(|parent| self.node_mut(parent)) {
                node.children.push(id);
            }
            if entry.view.focused {
                self.focused = Some(id);
            }
            if entry.view.region {
                self.region = Some(id);
            }
            ids.push(id);
        }
        self.root = ids.first().copied();
    }

    /// Puts `node` into the last freed slot, or into a new slot.
    fn insert(&mut self, node: Node<A>) -> WidgetId {
        self.live += 1;
        while let Some(slot) = self.free.pop() {
            if let Some(entry) = self.slots.get_mut(slot as usize) {
                entry.node = Some(node);
                return WidgetId {
                    slot,
                    generation: entry.generation,
                };
            }
        }
        let slot = self.slots.len() as u32;
        self.slots.push(Slot {
            generation: 0,
            node: Some(node),
        });
        WidgetId {
            slot,
            generation: 0,
        }
    }

    /// Removes the control `id`: its slot's generation increases and the
    /// slot goes onto the free list, or is retired at the last generation.
    fn remove(&mut self, id: WidgetId) {
        let Some(slot) = self.slots.get_mut(id.slot as usize) else {
            return;
        };
        if slot.generation != id.generation || slot.node.take().is_none() {
            return;
        }
        self.live -= 1;
        if let Some(next) = slot.generation.checked_add(1) {
            slot.generation = next;
            self.free.push(id.slot);
        }
    }
}

/// Checks the bounds and marks of a view tree before anything changes: at
/// most `MAX_WIDGETS` nodes, at most `MAX_DEPTH` deep, keys unique among
/// siblings, at most one focused node and one region root, and a marked
/// region's root at or above the focused node. It stops at the first
/// problem, so its work is bounded whatever the view's size.
fn check_view<C>(view: &View<C>) -> Result<(), RebuildError> {
    let mut count = 0_usize;
    let (mut focused, mut region) = (false, false);
    let mut focus_in_region = false;
    // Each entry carries whether its node is in the subtree of a node
    // marked as the region's root.
    let mut stack = vec![(view, 1_usize, false)];
    while let Some((node, depth, above)) = stack.pop() {
        if depth > MAX_DEPTH {
            return Err(RebuildError::TooDeep);
        }
        count += 1;
        if count + stack.len() + node.children.len() > MAX_WIDGETS {
            return Err(RebuildError::TooManyControls);
        }
        if node.focused && std::mem::replace(&mut focused, true) {
            return Err(RebuildError::SecondFocus);
        }
        if node.region && std::mem::replace(&mut region, true) {
            return Err(RebuildError::SecondRegion);
        }
        let in_region = above || node.region;
        if node.focused {
            focus_in_region = in_region;
        }
        let mut keys = BTreeSet::new();
        for child in &node.children {
            if !keys.insert(child.key) {
                return Err(RebuildError::DuplicateKey(child.key));
            }
        }
        stack.extend(
            node.children
                .iter()
                .rev()
                .map(|child| (child, depth + 1, in_region)),
        );
    }
    if focused && region && !focus_in_region {
        return Err(RebuildError::FocusOutsideRegion);
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod check;
#[cfg(test)]
mod tests;
