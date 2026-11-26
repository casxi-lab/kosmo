use super::check::{Invariant, Violation, breaks, check, depth, identities, same_tree};
use super::*;
use crate::testing::{TestApp, Widget, node};

/// A tree rebuilt view by view, with the view it was last rebuilt from and
/// the identities of the controls that the rebuilds removed.
struct Rounds {
    tree: Tree<TestApp>,
    view: View<Widget>,
    removed: Vec<WidgetId>,
}

impl Rounds {
    fn new() -> Rounds {
        Rounds {
            tree: Tree::new(),
            view: node(0, "root", 0),
            removed: Vec::new(),
        }
    }

    /// Rebuilds the tree from `view`, which must be accepted.
    fn round(&mut self, view: View<Widget>) {
        let before = self.tree.preorder();
        self.tree.rebuild(&view).unwrap();
        let after = self.tree.preorder();
        self.removed
            .extend(before.into_iter().filter(|id| !after.contains(id)));
        self.view = view;
    }

    fn check(&self) -> Vec<Violation> {
        check(&self.tree, &self.view, &self.removed)
    }

    /// The control at the path of keys below the root.
    fn id(&self, path: &[u64]) -> WidgetId {
        let mut at = self.tree.root().unwrap();
        for &key in path {
            at = *self
                .tree
                .children(at)
                .iter()
                .find(|&&child| self.tree.key(child) == Some(Key::new(key)))
                .unwrap();
        }
        at
    }
}

/// The views of a sequence that adds, removes and changes controls, nests
/// them six deep, removes a whole subtree, reorders children and changes a
/// control's kind.
fn sequence() -> Vec<View<Widget>> {
    let chain = node(20, "x", 0).child(
        node(21, "y", 0).child(node(22, "z", 0).child(node(23, "w", 0).child(node(24, "v", 0)))),
    );
    vec![
        node(0, "root", 0)
            .child(node(1, "a", 0))
            .child(node(2, "b", 0)),
        node(0, "root", 0)
            .child(node(1, "a", 0))
            .child(node(2, "b", 0))
            .child(node(3, "c", 0)),
        node(0, "root", 0)
            .child(node(1, "a", 7))
            .child(node(3, "c", 0)),
        node(0, "root", 0)
            .child(
                node(1, "a", 7)
                    .child(node(10, "a1", 0).child(chain.clone()))
                    .child(node(11, "a2", 0)),
            )
            .child(node(3, "c", 0)),
        node(0, "root", 0)
            .child(node(1, "a", 7).child(node(11, "a2", 0)))
            .child(node(3, "c", 0)),
        node(0, "root", 0)
            .child(node(3, "c", 0))
            .child(node(1, "a", 7).child(node(11, "a2", 0))),
        node(0, "root", 0)
            .child(node(3, "d", 0))
            .child(node(1, "a", 7).child(node(11, "a2", 0))),
    ]
}

/// Runs the sequence, checking every invariant after each view.
fn run_sequence() -> Rounds {
    let mut rounds = Rounds::new();
    for view in sequence() {
        rounds.round(view);
        assert_eq!(rounds.check(), []);
    }
    rounds
}

/// A view of `count` nodes: a root and `count - 1` children.
fn wide(count: u64) -> View<Widget> {
    let mut root = node(0, "root", 0);
    for key in 1..count {
        root.push(node(key, "leaf", 0));
    }
    root
}

/// A view of a chain `length` controls deep.
fn chain(length: u64) -> View<Widget> {
    let mut view = node(length, "link", 0);
    for key in (1..length).rev() {
        view = node(key, "link", 0).child(view);
    }
    view
}

#[test]
fn parents_and_children_agree_while_views_change() {
    let mut rounds = Rounds::new();
    let views = sequence();
    rounds.round(views[0].clone());
    let a = rounds.id(&[1]);
    for view in views {
        rounds.round(view);
        assert!(!breaks(&rounds.check(), Invariant::ParentsAgree));
        // The same key and kind under the same parent keep the identity.
        assert_eq!(rounds.id(&[1]), a);
    }
    assert_eq!(rounds.tree.control(a).map(|w| w.value), Some(7));
}

#[test]
fn a_child_that_names_its_sibling_as_parent_breaks_the_agreement() {
    let mut rounds = Rounds::new();
    rounds.round(sequence()[1].clone());
    let (a, b) = (rounds.id(&[1]), rounds.id(&[2]));
    rounds.tree.set_parent(b, Some(a));
    let found = rounds.check();
    assert!(breaks(&found, Invariant::ParentsAgree));
    assert!(
        found
            .iter()
            .any(|v| v.invariant == Invariant::ParentsAgree && v.widgets.contains(&b))
    );
}

#[test]
fn no_cycle_with_six_levels_of_nesting() {
    let mut rounds = Rounds::new();
    for view in sequence().into_iter().take(4) {
        rounds.round(view);
        assert!(!breaks(&rounds.check(), Invariant::NoCycle));
    }
    assert_eq!(depth(&rounds.tree), 8);
    let v = rounds.id(&[1, 10, 20, 21, 22, 23, 24]);
    assert_eq!(rounds.tree.key(v), Some(Key::new(24)));
}

#[test]
fn a_parent_and_child_that_are_each_others_parent_form_a_cycle() {
    let mut rounds = Rounds::new();
    rounds.round(sequence()[4].clone());
    let (root, a, a2) = (rounds.id(&[]), rounds.id(&[1]), rounds.id(&[1, 11]));
    // a and a2 become each other's parent, with children lists to match.
    rounds.tree.set_parent(a, Some(a2));
    rounds.tree.children_mut(a2).unwrap().push(a);
    rounds
        .tree
        .children_mut(root)
        .unwrap()
        .retain(|&id| id != a);
    let found = rounds.check();
    assert!(
        found
            .iter()
            .any(|v| v.invariant == Invariant::NoCycle && (v.widgets == [a] || v.widgets == [a2]))
    );
}

#[test]
fn each_control_is_in_the_tree_once_after_a_subtree_is_removed() {
    let mut rounds = Rounds::new();
    let views = sequence();
    rounds.round(views[3].clone());
    let before = rounds.tree.len();
    rounds.round(views[4].clone());
    assert!(!breaks(&rounds.check(), Invariant::OncePerControl));
    // a1 and the chain of five below it are gone.
    assert_eq!(rounds.tree.len(), before - 6);
    assert_eq!(rounds.tree.preorder().len(), rounds.tree.len());
}

#[test]
fn a_control_taken_out_of_its_parents_children_is_not_in_the_tree() {
    let mut rounds = Rounds::new();
    rounds.round(sequence()[1].clone());
    let (root, b) = (rounds.id(&[]), rounds.id(&[2]));
    rounds
        .tree
        .children_mut(root)
        .unwrap()
        .retain(|&id| id != b);
    let found = rounds.check();
    assert!(
        found
            .iter()
            .any(|v| v.invariant == Invariant::OncePerControl && v.widgets == [b])
    );
}

#[test]
fn traversal_follows_the_views_order_after_a_reorder() {
    let mut rounds = Rounds::new();
    let views = sequence();
    rounds.round(views[4].clone());
    let (a, c) = (rounds.id(&[1]), rounds.id(&[3]));
    rounds.round(views[5].clone());
    assert!(!breaks(&rounds.check(), Invariant::ViewOrder));
    let root = rounds.id(&[]);
    assert_eq!(rounds.tree.children(root), [c, a]);
}

#[test]
fn swapping_two_children_without_the_view_breaks_the_order() {
    let mut rounds = Rounds::new();
    rounds.round(sequence()[1].clone());
    let root = rounds.id(&[]);
    rounds.tree.children_mut(root).unwrap().swap(0, 2);
    let found = rounds.check();
    assert!(
        found
            .iter()
            .any(|v| v.invariant == Invariant::ViewOrder && v.widgets == [root])
    );
}

#[test]
fn a_root_whose_key_is_not_the_views_breaks_the_order() {
    let mut rounds = Rounds::new();
    rounds.round(sequence()[1].clone());
    let root = rounds.id(&[]);
    // The view of the tree with another key at the root.
    let other = node(9, "root", 0)
        .child(node(1, "a", 0))
        .child(node(2, "b", 0))
        .child(node(3, "c", 0));
    let found = check(&rounds.tree, &other, &rounds.removed);
    assert_eq!(
        found,
        [Violation {
            invariant: Invariant::ViewOrder,
            widgets: vec![root],
        }]
    );
}

#[test]
fn an_old_identity_finds_nothing_when_its_slot_is_used_again() {
    let mut rounds = Rounds::new();
    let views = sequence();
    rounds.round(views[1].clone());
    let b = rounds.id(&[2]);
    rounds.round(views[2].clone());
    rounds.round(views[3].clone());
    // The first control created after b's removal takes b's slot, last in
    // first out, with a greater generation.
    let a1 = rounds.id(&[1, 10]);
    assert_eq!(a1.slot(), b.slot());
    assert!(a1.generation() > b.generation());
    assert!(!rounds.tree.contains(b));
    assert!(rounds.removed.contains(&b));
    assert!(!breaks(&rounds.check(), Invariant::OldIdentities));
}

#[test]
fn a_removal_that_keeps_the_generation_lets_an_old_identity_find_a_control() {
    let mut rounds = Rounds::new();
    let views = sequence();
    rounds.round(views[1].clone());
    let b = rounds.id(&[2]);
    rounds.tree.remove_keeping_generation(b);
    rounds.removed.push(b);
    rounds.round(views[2].clone().child(node(4, "d", 0)));
    assert_eq!(rounds.id(&[4]), b);
    let found = rounds.check();
    assert!(
        found
            .iter()
            .any(|v| v.invariant == Invariant::OldIdentities && v.widgets == [b])
    );
}

#[test]
fn a_tree_built_once_equals_the_tree_built_round_by_round() {
    let rounds = run_sequence();
    let mut once: Tree<TestApp> = Tree::new();
    once.rebuild(sequence().last().unwrap()).unwrap();
    assert!(same_tree(&rounds.tree, &once));
    // Identities differ, so the comparison does not look at them.
    assert_ne!(identities(&rounds.tree), identities(&once));
}

#[test]
fn trees_that_differ_in_one_key_are_told_apart() {
    let rounds = run_sequence();
    let last = sequence().last().unwrap().clone();
    let other = node(0, "root", 0)
        .child(node(3, "d", 0))
        .child(node(1, "a", 7).child(node(12, "a2", 0)));
    assert_ne!(last, other);
    let mut once: Tree<TestApp> = Tree::new();
    once.rebuild(&other).unwrap();
    assert!(!same_tree(&rounds.tree, &once));
}

#[test]
fn trees_that_differ_only_in_the_focus_mark_are_told_apart() {
    // A root with the children 1 and 2, of which `focused` is marked.
    let marked = |focused: u64| {
        let mut root = node(0, "root", 0);
        for key in [1, 2] {
            let child = node(key, "leaf", 0);
            root.push(if key == focused {
                child.focused()
            } else {
                child
            });
        }
        root
    };
    let mut rounds = Rounds::new();
    rounds.round(marked(1));
    assert_eq!(rounds.check(), []);
    let mut same: Tree<TestApp> = Tree::new();
    same.rebuild(&marked(1)).unwrap();
    assert!(same_tree(&rounds.tree, &same));
    let mut other: Tree<TestApp> = Tree::new();
    other.rebuild(&marked(2)).unwrap();
    assert!(!same_tree(&rounds.tree, &other));

    // Against the view that marks the other child, the tree's focused
    // control is not the view's.
    let focused = rounds.id(&[1]);
    let found = check(&rounds.tree, &marked(2), &rounds.removed);
    assert_eq!(
        found,
        [Violation {
            invariant: Invariant::ViewOrder,
            widgets: vec![focused],
        }]
    );
}

#[test]
fn trees_that_differ_only_in_the_region_mark_are_told_apart() {
    // A root with the children 1 and 2, of which `region` is marked.
    let marked = |region: u64| {
        let mut root = node(0, "root", 0);
        for key in [1, 2] {
            let child = node(key, "leaf", 0);
            root.push(if key == region { child.region() } else { child });
        }
        root
    };
    let mut rounds = Rounds::new();
    rounds.round(marked(1));
    assert_eq!(rounds.check(), []);
    let mut same: Tree<TestApp> = Tree::new();
    same.rebuild(&marked(1)).unwrap();
    assert!(same_tree(&rounds.tree, &same));
    let mut other: Tree<TestApp> = Tree::new();
    other.rebuild(&marked(2)).unwrap();
    assert!(!same_tree(&rounds.tree, &other));

    // Against the view that marks the other child, the tree's region's
    // root is not the view's.
    let region = rounds.id(&[1]);
    let found = check(&rounds.tree, &marked(2), &rounds.removed);
    assert_eq!(
        found,
        [Violation {
            invariant: Invariant::ViewOrder,
            widgets: vec![region],
        }]
    );
}

#[test]
fn the_same_sequence_gives_the_same_identities() {
    let (mut first, mut second) = (Rounds::new(), Rounds::new());
    for view in sequence() {
        first.round(view.clone());
        second.round(view);
        assert_eq!(identities(&first.tree), identities(&second.tree));
    }
}

/// The views of a sequence whose third step removes the control `removed`.
fn removing(removed: u64) -> Vec<View<Widget>> {
    let all = |keys: &[u64]| {
        let mut root = node(0, "root", 0);
        for &key in keys {
            root.push(node(key, "leaf", 0));
        }
        root
    };
    let kept: Vec<u64> = [1, 2, 3, 4]
        .into_iter()
        .filter(|&key| key != removed)
        .collect();
    let mut last = kept.clone();
    last.push(5);
    vec![all(&[1, 2, 3]), all(&[1, 2, 3, 4]), all(&kept), all(&last)]
}

#[test]
fn sequences_that_differ_in_one_removal_give_other_identities() {
    let (mut first, mut second) = (Rounds::new(), Rounds::new());
    for (one, other) in removing(2).into_iter().zip(removing(3)) {
        first.round(one);
        second.round(other);
    }
    assert_ne!(identities(&first.tree), identities(&second.tree));
}

#[test]
fn a_tree_exactly_at_its_bounds_is_built() {
    let mut rounds = Rounds::new();
    rounds.round(wide(MAX_WIDGETS as u64));
    assert_eq!(rounds.tree.len(), MAX_WIDGETS);
    assert_eq!(rounds.check(), []);

    let mut rounds = Rounds::new();
    rounds.round(chain(MAX_DEPTH as u64));
    assert_eq!(depth(&rounds.tree), MAX_DEPTH);
    assert_eq!(rounds.check(), []);
}

#[test]
fn one_control_beyond_a_bound_leaves_the_tree_as_it_was() {
    let mut rounds = run_sequence();
    let before = identities(&rounds.tree);
    let mut reference: Tree<TestApp> = Tree::new();
    reference.rebuild(&rounds.view).unwrap();

    assert_eq!(
        rounds.tree.rebuild(&wide(MAX_WIDGETS as u64 + 1)),
        Err(RebuildError::TooManyControls)
    );
    assert_eq!(identities(&rounds.tree), before);
    assert!(same_tree(&rounds.tree, &reference));
    assert_eq!(
        rounds.tree.rebuild(&chain(MAX_DEPTH as u64 + 1)),
        Err(RebuildError::TooDeep)
    );
    assert_eq!(identities(&rounds.tree), before);
    assert!(same_tree(&rounds.tree, &reference));
    assert_eq!(rounds.check(), []);
}

#[test]
fn a_view_far_deeper_than_the_bound_is_rejected_and_dropped() {
    let mut rounds = run_sequence();
    let before = identities(&rounds.tree);
    let mut reference: Tree<TestApp> = Tree::new();
    reference.rebuild(&rounds.view).unwrap();

    // Built and dropped on the test thread's stack, without recursion.
    let deep = chain(100_000);
    assert_eq!(rounds.tree.rebuild(&deep), Err(RebuildError::TooDeep));
    drop(deep);
    assert_eq!(identities(&rounds.tree), before);
    assert!(same_tree(&rounds.tree, &reference));
    assert_eq!(rounds.check(), []);
}

#[test]
fn a_slot_at_its_last_generation_is_retired() {
    let mut rounds = Rounds::new();
    rounds.round(node(0, "root", 0).child(node(1, "a", 0)));
    let a = rounds.id(&[1]);
    let a = rounds.tree.set_generation(a, u32::MAX);
    assert_eq!(rounds.check(), []);
    rounds.round(node(0, "root", 0));
    rounds.round(node(0, "root", 0).child(node(2, "b", 0)));
    assert!(!rounds.tree.contains(a));
    assert_ne!(rounds.id(&[2]).slot(), a.slot());
    assert_eq!(rounds.check(), []);
}

#[test]
fn views_with_repeated_keys_or_marks_are_rejected() {
    let mut tree: Tree<TestApp> = Tree::new();
    let twice = node(0, "root", 0)
        .child(node(1, "a", 0))
        .child(node(1, "b", 0));
    assert_eq!(
        tree.rebuild(&twice),
        Err(RebuildError::DuplicateKey(Key::new(1)))
    );
    let focus = node(0, "root", 0)
        .child(node(1, "a", 0).focused())
        .child(node(2, "a", 0).focused());
    assert_eq!(tree.rebuild(&focus), Err(RebuildError::SecondFocus));
    let region = node(0, "root", 0).region().child(node(1, "a", 0).region());
    assert_eq!(tree.rebuild(&region), Err(RebuildError::SecondRegion));
    // The focused control in a sibling of the region's root, and above it.
    let beside = node(0, "root", 0)
        .child(node(1, "a", 0).region())
        .child(node(2, "b", 0).focused());
    assert_eq!(tree.rebuild(&beside), Err(RebuildError::FocusOutsideRegion));
    let above = node(0, "root", 0).child(node(1, "a", 0).focused().child(node(2, "b", 0).region()));
    assert_eq!(tree.rebuild(&above), Err(RebuildError::FocusOutsideRegion));
    assert!(tree.is_empty());

    // The focused control may be the region's root or any node below it.
    let at = node(0, "root", 0).child(node(1, "a", 0).region().focused());
    assert_eq!(tree.rebuild(&at), Ok(()));
    let below = node(0, "root", 0).child(
        node(1, "a", 0)
            .region()
            .child(node(2, "b", 0).child(node(3, "c", 0).focused())),
    );
    assert_eq!(tree.rebuild(&below), Ok(()));
}

#[test]
fn identities_fit_in_64_bits() {
    let mut rounds = Rounds::new();
    rounds.round(node(0, "root", 0).child(node(1, "a", 0)));
    let a = rounds.tree.set_generation(rounds.id(&[1]), 0x0102_0304);
    assert_eq!(a.to_bits(), (1_u64 << 32) | 0x0102_0304);
    assert_eq!(a.to_string(), "1:16909060");
}

#[test]
fn the_drawing_entry_gets_the_identity_and_presentation_state() {
    let mut rounds = Rounds::new();
    rounds.round(node(0, "root", 0));
    let root = rounds.id(&[]);
    let state = rounds.tree.state(root).copied().unwrap();
    let mut list = kosmo_render::ListBuilder::new();
    let canvas = crate::app::Canvas::new(&mut list, kosmo_types::Rect::default());
    let cx = crate::app::DrawContext::new(root, &state, kosmo_types::Size::ZERO, canvas);
    assert_eq!(
        (cx.id(), *cx.state(), cx.size()),
        (root, 0, kosmo_types::Size::ZERO)
    );
}
