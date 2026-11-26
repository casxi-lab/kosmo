//! The accessibility tree that a backend keeps for a surface.

use std::collections::{BTreeMap, BTreeSet};

use accesskit::{Node, NodeId, TreeId, TreeInfo, TreeUpdate};

use crate::MAX_A11Y_NODES;
use crate::error::PlatformError;

/// An accessibility tree built from AccessKit tree updates applied in
/// order: the nodes reachable from the root, the tree information and the
/// focused node. Nodes are kept in an ordered map, so every traversal and
/// comparison is the same on every run.
///
/// ```
/// use kosmo_platform::AccessibilityTree;
/// use kosmo_platform::accesskit::{Node, NodeId, Role, TreeId, TreeInfo, TreeUpdate};
///
/// let mut root = Node::new(Role::Window);
/// root.set_children(vec![NodeId(2)]);
/// let mut button = Node::new(Role::Button);
/// button.set_label("Evaluate");
/// let mut tree = AccessibilityTree::new(TreeUpdate {
///     nodes: vec![(NodeId(1), root), (NodeId(2), button.clone())],
///     tree: Some(TreeInfo::new(NodeId(1))),
///     tree_id: TreeId::ROOT,
///     focus: NodeId(2),
/// })
/// .unwrap();
///
/// button.set_label("Stop");
/// tree.apply(TreeUpdate {
///     nodes: vec![(NodeId(2), button)],
///     tree: None,
///     tree_id: TreeId::ROOT,
///     focus: NodeId(2),
/// })
/// .unwrap();
/// assert_eq!(tree.node(NodeId(2)).unwrap().label(), Some("Stop"));
/// assert_eq!(tree.node_count(), 2);
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct AccessibilityTree {
    info: TreeInfo,
    focus: NodeId,
    nodes: BTreeMap<NodeId, Node>,
}

impl AccessibilityTree {
    /// The tree of a surface's first update, which holds the tree
    /// information and every node of the tree. Fails as
    /// [`apply`](AccessibilityTree::apply) does, and with
    /// [`PlatformError::InvalidArgument`] when the update has no tree
    /// information.
    pub fn new(update: TreeUpdate) -> Result<AccessibilityTree, PlatformError> {
        let Some(info) = update.tree.clone() else {
            return Err(PlatformError::InvalidArgument);
        };
        build(info, BTreeMap::new(), update)
    }

    /// Applies a later update: each node of the update replaces the node
    /// with its identifier, the tree information is replaced when the
    /// update has it, the focus is the update's, and nodes that are no
    /// longer reachable from the root go. On an error the tree stays as it
    /// was:
    ///
    /// - [`PlatformError::Unsupported`] for an update of a subtree, whose
    ///   tree identifier is not the root tree's;
    /// - [`PlatformError::InvalidArgument`] when the result names a child
    ///   that is not in the tree, reaches a node twice, or leaves a node of
    ///   the update or the focus unreachable from the root;
    /// - [`PlatformError::Exhausted`] when the result would have more than
    ///   [`MAX_A11Y_NODES`] nodes.
    pub fn apply(&mut self, update: TreeUpdate) -> Result<(), PlatformError> {
        let info = update.tree.clone().unwrap_or_else(|| self.info.clone());
        *self = build(info, self.nodes.clone(), update)?;
        Ok(())
    }

    /// The tree information.
    pub fn info(&self) -> &TreeInfo {
        &self.info
    }

    /// The root node's identifier.
    pub fn root(&self) -> NodeId {
        self.info.root
    }

    /// The focused node's identifier.
    pub fn focus(&self) -> NodeId {
        self.focus
    }

    /// The node `id`, if the tree has it.
    pub fn node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(&id)
    }

    /// The number of nodes, at least 1.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// The nodes in the order of their identifiers.
    pub fn nodes(&self) -> impl Iterator<Item = (NodeId, &Node)> {
        self.nodes.iter().map(|(id, node)| (*id, node))
    }
}

/// The tree of `info` after `update` replaced its nodes in `nodes`.
fn build(
    info: TreeInfo,
    mut nodes: BTreeMap<NodeId, Node>,
    update: TreeUpdate,
) -> Result<AccessibilityTree, PlatformError> {
    if update.tree_id != TreeId::ROOT {
        return Err(PlatformError::Unsupported);
    }
    let changed: BTreeSet<NodeId> = update.nodes.iter().map(|(id, _)| *id).collect();
    nodes.extend(update.nodes);
    let mut reached = BTreeSet::new();
    let mut stack = vec![info.root];
    while let Some(id) = stack.pop() {
        let node = nodes.get(&id).ok_or(PlatformError::InvalidArgument)?;
        if !reached.insert(id) {
            return Err(PlatformError::InvalidArgument);
        }
        if reached.len() > MAX_A11Y_NODES {
            return Err(PlatformError::Exhausted);
        }
        stack.extend(node.children().iter().rev());
    }
    if !changed.is_subset(&reached) || !reached.contains(&update.focus) {
        return Err(PlatformError::InvalidArgument);
    }
    nodes.retain(|id, _| reached.contains(id));
    Ok(AccessibilityTree {
        info,
        focus: update.focus,
        nodes,
    })
}

#[cfg(test)]
mod tests {
    use accesskit::{Role, Uuid};

    use super::*;

    fn node(children: &[u64]) -> Node {
        let mut node = Node::new(Role::GenericContainer);
        node.set_children(children.iter().map(|id| NodeId(*id)).collect::<Vec<_>>());
        node
    }

    fn update(nodes: Vec<(u64, Node)>, root: Option<u64>) -> TreeUpdate {
        TreeUpdate {
            nodes: nodes
                .into_iter()
                .map(|(id, node)| (NodeId(id), node))
                .collect(),
            tree: root.map(|root| TreeInfo::new(NodeId(root))),
            tree_id: TreeId::ROOT,
            focus: NodeId(root.unwrap_or(1)),
        }
    }

    fn three() -> AccessibilityTree {
        let first = update(
            vec![(1, node(&[2, 3])), (2, node(&[])), (3, node(&[]))],
            Some(1),
        );
        AccessibilityTree::new(first).unwrap()
    }

    #[test]
    fn a_first_update_needs_the_tree_information_and_every_node() {
        assert_eq!(
            AccessibilityTree::new(update(vec![(1, node(&[]))], None)),
            Err(PlatformError::InvalidArgument)
        );
        assert_eq!(
            AccessibilityTree::new(update(vec![(1, node(&[2]))], Some(1))),
            Err(PlatformError::InvalidArgument)
        );
        assert_eq!(three().node_count(), 3);
        assert_eq!(three().root(), NodeId(1));
    }

    #[test]
    fn updates_replace_nodes_and_drop_what_is_no_longer_reachable() {
        let mut tree = three();
        let mut named = node(&[]);
        named.set_label("two");
        tree.apply(update(vec![(2, named)], None)).unwrap();
        assert_eq!(tree.node(NodeId(2)).unwrap().label(), Some("two"));
        assert_eq!(tree.node_count(), 3);
        tree.apply(update(vec![(1, node(&[3]))], None)).unwrap();
        let ids: Vec<NodeId> = tree.nodes().map(|(id, _)| id).collect();
        assert_eq!(ids, [NodeId(1), NodeId(3)]);
    }

    #[test]
    fn a_rejected_update_leaves_the_tree_as_it_was() {
        let mut tree = three();
        let before = tree.clone();
        let cases = [
            (
                update(vec![(1, node(&[2, 4]))], None),
                PlatformError::InvalidArgument,
            ),
            (
                update(vec![(1, node(&[2, 2]))], None),
                PlatformError::InvalidArgument,
            ),
            (
                update(vec![(3, node(&[1]))], None),
                PlatformError::InvalidArgument,
            ),
            (
                update(vec![(9, node(&[]))], None),
                PlatformError::InvalidArgument,
            ),
        ];
        for (rejected, error) in cases {
            assert_eq!(tree.apply(rejected), Err(error));
            assert_eq!(tree, before);
        }
        let mut subtree = update(vec![(2, node(&[]))], None);
        subtree.tree_id = TreeId(Uuid::from_u128(1));
        assert_eq!(tree.apply(subtree), Err(PlatformError::Unsupported));
        let mut unfocusable = update(vec![(2, node(&[]))], None);
        unfocusable.focus = NodeId(9);
        assert_eq!(tree.apply(unfocusable), Err(PlatformError::InvalidArgument));
        assert_eq!(tree, before);
    }

    #[test]
    fn a_tree_holds_at_most_the_bound_of_nodes() {
        let last = MAX_A11Y_NODES as u64;
        let children: Vec<u64> = (2..=last).collect();
        let mut nodes = vec![(1, node(&children))];
        nodes.extend((2..=last).map(|id| (id, node(&[]))));
        let mut tree = AccessibilityTree::new(update(nodes, Some(1))).unwrap();
        assert_eq!(tree.node_count(), MAX_A11Y_NODES);
        let before = tree.clone();
        let mut more = children.clone();
        more.push(last + 1);
        let one_over = update(vec![(1, node(&more)), (last + 1, node(&[]))], None);
        assert_eq!(tree.apply(one_over), Err(PlatformError::Exhausted));
        assert_eq!(tree, before);
    }
}
