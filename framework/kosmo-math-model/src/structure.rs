//! Blocks, items, identities, positions and fragments.

use std::collections::BTreeMap;
use std::ops::Range;

/// The identity of a block or a template.
///
/// An editing buffer hands out identities in increasing order and never
/// reuses one; its root block takes the first. Characters have no identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId(u64);

impl NodeId {
    /// The identity of the root block of every buffer.
    pub(crate) const ROOT: NodeId = NodeId(0);

    /// The identity as a number.
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// One item of a block.
///
/// One-line input holds characters only. Items are added as variants, never
/// changed.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Item {
    /// A character of the character set.
    Char(char),
    /// A template with its identity and kind. No kind exists yet, so no
    /// template can be built.
    Template(NodeId, TemplateKind),
}

/// The kind of a two-dimensional template, such as a fraction, and so the
/// number of its slots.
///
/// No kind exists yet; two-dimensional editing adds them as variants.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TemplateKind {}

/// A place between the items of a block: `index` runs from 0, before the
/// first item, to the number of items, after the last.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Position {
    /// The block.
    pub block: NodeId,
    /// The place between its items.
    pub index: usize,
}

/// A half-open range of items of one block. Fragments are copied into other
/// lines and mark the parts of a line that a diagnostic is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Fragment {
    block: NodeId,
    start: usize,
    end: usize,
}

impl Fragment {
    /// The items of `block` in `range`, or `None` when the range runs
    /// backwards.
    pub fn new(block: NodeId, range: Range<usize>) -> Option<Fragment> {
        (range.start <= range.end).then_some(Fragment {
            block,
            start: range.start,
            end: range.end,
        })
    }

    /// The items of `block` between two places, in either order.
    pub(crate) fn between(block: NodeId, one: usize, other: usize) -> Fragment {
        Fragment {
            block,
            start: one.min(other),
            end: one.max(other),
        }
    }

    /// The block.
    pub const fn block(self) -> NodeId {
        self.block
    }

    /// The index of the first item.
    pub const fn start(self) -> usize {
        self.start
    }

    /// The index after the last item.
    pub const fn end(self) -> usize {
        self.end
    }

    /// The item range, `start()..end()`.
    pub const fn range(self) -> Range<usize> {
        self.start..self.end
    }

    /// The number of items.
    pub const fn len(self) -> usize {
        self.end - self.start
    }

    /// Whether the fragment holds no item.
    pub const fn is_empty(self) -> bool {
        self.start == self.end
    }
}

/// Where in a line something lies that is about a part of it, such as an
/// error of a computation.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Place {
    /// The whole line, for an error about no single part of it.
    Line,
    /// The items of a fragment.
    Fragment(Fragment),
    /// The place between two items, such as the end of the line.
    Position(Position),
}

/// A selection: an anchor and an active end in the same block, which are
/// never equal. The cursor is the active end.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Selection {
    anchor: Position,
    active: Position,
}

impl Selection {
    pub(crate) const fn new(anchor: Position, active: Position) -> Selection {
        Selection { anchor, active }
    }

    /// The end that stays when the selection is extended.
    pub const fn anchor(self) -> Position {
        self.anchor
    }

    /// The end that moves when the selection is extended; the cursor.
    pub const fn active(self) -> Position {
        self.active
    }

    /// The selected items, from the earlier end to the later one.
    pub fn fragment(self) -> Fragment {
        Fragment::between(self.anchor.block, self.anchor.index, self.active.index)
    }
}

/// The blocks of a structure, keyed by identity, and the identity of its
/// root. One-line input has only the root.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Structure {
    blocks: BTreeMap<NodeId, Vec<Item>>,
    root: NodeId,
    /// The identity that the next new block or template takes. Identities
    /// are never reused, so clearing the line keeps it.
    next: u64,
}

impl Structure {
    /// An empty root block, which takes the first identity.
    pub(crate) fn new() -> Structure {
        Structure {
            blocks: BTreeMap::from([(NodeId::ROOT, Vec::new())]),
            root: NodeId::ROOT,
            next: NodeId::ROOT.0 + 1,
        }
    }

    pub(crate) const fn root(&self) -> NodeId {
        self.root
    }

    /// The items of the root block.
    pub(crate) fn root_items(&self) -> &[Item] {
        self.blocks.get(&self.root).map_or(&[], Vec::as_slice)
    }

    /// Replaces the items of the root block.
    pub(crate) fn set_root_items(&mut self, items: Vec<Item>) {
        self.blocks.insert(self.root, items);
    }

    /// The items of `fragment`, when it lies in this structure.
    pub(crate) fn items_of(&self, fragment: Fragment) -> Option<&[Item]> {
        self.blocks.get(&fragment.block)?.get(fragment.range())
    }

    #[cfg(test)]
    pub(crate) const fn next_identity(&self) -> u64 {
        self.next
    }
}

/// The character of an item. Templates cannot exist yet.
pub(crate) const fn character(item: &Item) -> char {
    match *item {
        Item::Char(c) => c,
        Item::Template(_, kind) => match kind {},
    }
}

/// A copy of an item for another line. A copied template will take new
/// identities; only characters exist yet, and they have none.
pub(crate) const fn copy_item(item: &Item) -> Item {
    match *item {
        Item::Char(c) => Item::Char(c),
        Item::Template(_, kind) => match kind {},
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_root_takes_the_first_identity() {
        let structure = Structure::new();
        assert_eq!(structure.root(), NodeId::ROOT);
        assert_eq!(structure.root().get(), 0);
        assert_eq!(structure.next_identity(), 1);
        assert!(structure.root_items().is_empty());
    }

    #[test]
    fn a_fragment_never_runs_backwards() {
        let (start, end) = (2, 1);
        assert_eq!(Fragment::new(NodeId::ROOT, start..end), None);
        let fragment = Fragment::new(NodeId::ROOT, 1..3).unwrap();
        assert_eq!(
            (fragment.start(), fragment.end(), fragment.len()),
            (1, 3, 2)
        );
        assert!(!fragment.is_empty());
        assert!(Fragment::new(NodeId::ROOT, 2..2).unwrap().is_empty());
    }

    #[test]
    fn a_selection_normalizes_to_a_fragment() {
        let at = |index| Position {
            block: NodeId::ROOT,
            index,
        };
        let backwards = Selection::new(at(3), at(1));
        assert_eq!(backwards.fragment().range(), 1..3);
        assert_eq!(
            Selection::new(at(1), at(3)).fragment(),
            backwards.fragment()
        );
    }

    #[test]
    fn fragments_outside_the_structure_have_no_items() {
        let mut structure = Structure::new();
        structure.set_root_items(vec![Item::Char('1'), Item::Char('2')]);
        let items = |range| structure.items_of(Fragment::new(NodeId::ROOT, range).unwrap());
        assert_eq!(items(0..2), Some(&[Item::Char('1'), Item::Char('2')][..]));
        assert_eq!(items(2..2), Some(&[][..]));
        assert_eq!(items(1..3), None);
        let elsewhere = Fragment::new(NodeId(7), 0..0).unwrap();
        assert_eq!(structure.items_of(elsewhere), None);
    }
}
