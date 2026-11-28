//! Random rounds from fixed seeds for the accessibility pass. Each group
//! runs up to `MAX_ROUNDS` rounds of an application whose model is a tree
//! of the eight kinds of control of the mapping tests, rows and columns,
//! which changes at random: controls are added, removed, moved, given
//! another kind or other values, names and scroll requests, reordered, and
//! focused and made the region's root or not; now and then the view is too
//! deep to be built. A bound command flips the state flags of the focused
//! control or an ancestor. Each group lays out one or two surfaces, whose
//! sizes change, and now and then leaves a surface without a size. The
//! updates of each round are applied in order to an `AccessibilityTree` of
//! their surface, as the headless backend keeps it, except in one group of
//! `DROP_EVERY`, where one update of one surface is dropped and the runtime
//! is told so.
//!
//! After every round nothing has panicked, every update was taken, and the
//! tree of each surface with a layout has a node for each control and no
//! other, the control tree's root as its root and no identifier of a
//! control that left the tree in the group; each node's children are those
//! of its control, in order; the tree equals the whole tree that the pass
//! builds for the control tree, the layouts and the focus; its focus is
//! the focused control or the root; each bounding box is the control's box
//! in the coordinates of the surface, and each scroll container has the
//! request brought into its range as its offset; and the round's update
//! held every node and the tree information when it was the first, came
//! after a dropped one or had another root, was left out when no node and
//! not the focus changed, and otherwise held exactly the nodes that
//! changed. A rejected view leaves the control tree as it was and gives no
//! update. Each group runs twice and gives the same updates, in their debug
//! form, byte for byte. A failure names the seed, the group and the round.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::panic::{AssertUnwindSafe, catch_unwind};

use kosmo_platform::accesskit::{Node, NodeId, Rect, Role, TreeInfo, TreeUpdate};
use kosmo_platform::{AccessibilityTree, PixelSize};
use kosmo_text::FontSet;
use kosmo_types::{Fx, KeyAction, KeyMeaning, MonoTime, Point, Size};

use crate::access::{controls, node_id, surface};
use crate::app::{App, Context, Control, DrawContext, Handling, Message, Out, Phase};
use crate::command::{Binding, Command, CommandId, INSERT_TEXT, Mods, Trigger};
use crate::focus::{Event, SurfaceId};
use crate::hit::Placed;
use crate::layout::{Axis, Column, Font, MAX_LAYOUT_INPUT, Node as LayoutNode, Row, Scroll, Text};
use crate::random::SplitMix64;
use crate::round::{Input, LayoutInput, Runtime, ViewPass};
use crate::testing::{font_set, key_event, stamp};
use crate::tokens::{FontSize, TOKENS};
use crate::tree::{Key, Kind, MAX_DEPTH, Tree, View, WidgetId};

/// The seed of the first group; group `n` uses `SEED + n`.
const SEED: u64 = 0x6b6f_736d_6f00_0305;

/// The number of groups.
const GROUPS: u64 = 1_000;

/// The most rounds in one group.
const MAX_ROUNDS: usize = 64;

/// The most nodes of the model.
const MAX_NODES: usize = 48;

/// The greatest depth of the model, the root counted as 1.
const MAX_MODEL_DEPTH: usize = 12;

/// The most text leaves of the model.
const MAX_TEXTS: usize = 8;

/// About one round in this many has a view too deep to be built.
const TOO_DEEP_EVERY: usize = 16;

/// About one round in this many leaves a surface without a size.
const UNSIZED_EVERY: usize = 8;

/// One group in this many, the last of each run of this many, drops one
/// update of one surface.
const DROP_EVERY: u64 = 16;

/// The names, values and texts of the controls, with repeats and the empty
/// string.
const NAMES: [&str; 6] = ["", "1+2", "1+2", "Result", "2*3 = 6", "x²−1"];

/// The state flags that the entry writes, as bits of the presentation
/// state.
const DISABLED: u8 = 1;
const SELECTED: u8 = 2;
const READ_ONLY: u8 = 4;
const BUSY: u8 = 8;

/// A command of the tests that flips the state flags of the control that
/// handles it.
const FLIP: CommandId = CommandId::new("test", "Flip", false);

/// The application's binding table: `f` flips the flags of the focused
/// control or of the first of its ancestors that flips any.
const BINDINGS: &[Binding] = &[Binding {
    trigger: Trigger::Char('f'),
    mods: Mods::NONE,
    command: FLIP,
}];

/// The kinds of the model's controls: the eight of the mapping tests, a row
/// and a column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Sort {
    Window,
    Label,
    Input,
    Item,
    Scroller,
    Result,
    List,
    Button,
    Row,
    Column,
}

const SORTS: [Sort; 10] = [
    Sort::Window,
    Sort::Label,
    Sort::Input,
    Sort::Item,
    Sort::Scroller,
    Sort::Result,
    Sort::List,
    Sort::Button,
    Sort::Row,
    Sort::Column,
];

impl Sort {
    /// Whether the control is a text leaf.
    fn is_text(self) -> bool {
        matches!(self, Sort::Label | Sort::Input | Sort::Item | Sort::Result)
    }
}

/// A control of the model: its kind and the values that its layout node
/// and accessibility node read, and the flags that `Flip` flips.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Part {
    sort: Sort,
    name: &'static str,
    font: Font,
    size: FontSize,
    axis: Axis,
    request: Fx,
    fixed: Size,
    flips: u8,
}

impl Control<Model> for Part {
    type State = u8;

    fn kind(&self) -> Kind {
        Kind::new(match self.sort {
            Sort::Window => "window",
            Sort::Label => "label",
            Sort::Input => "input",
            Sort::Item => "item",
            Sort::Scroller => "scroller",
            Sort::Result => "result",
            Sort::List => "list",
            Sort::Button => "button",
            Sort::Row => "row",
            Sort::Column => "column",
        })
    }

    fn handle(&self, _: Phase, command: &Command, cx: &mut Context<'_, Model, u8>) -> Handling {
        if command.id != FLIP || self.flips == 0 {
            return Handling::Unhandled;
        }
        *cx.state_mut() ^= self.flips;
        Handling::Handled
    }

    fn layout(&self, _: &u8) -> LayoutNode<'_> {
        match self.sort {
            Sort::Window | Sort::List | Sort::Column => LayoutNode::Column(Column::new()),
            Sort::Row => LayoutNode::Row(Row::new()),
            Sort::Label | Sort::Input | Sort::Item | Sort::Result => {
                LayoutNode::Text(Text::new(self.name, self.font, self.size))
            }
            Sort::Scroller => LayoutNode::Scroll(Scroll::new(self.axis, self.request)),
            Sort::Button => LayoutNode::Fixed(self.fixed),
        }
    }

    fn accessibility(&self, flags: &u8, node: &mut Node) {
        let on = |flag: u8| flags & flag != 0;
        match self.sort {
            Sort::Window => {
                node.set_role(Role::Window);
                node.set_label("Main");
            }
            Sort::Label => {
                if on(DISABLED) {
                    node.set_disabled();
                }
            }
            Sort::Input => {
                node.set_role(Role::TextInput);
                node.set_label("Input");
                node.set_value(self.name);
                if on(READ_ONLY) {
                    node.set_read_only();
                }
            }
            Sort::Item => {
                node.set_role(Role::ListItem);
                node.set_selected(on(SELECTED));
            }
            Sort::Scroller => node.set_label(self.name),
            Sort::Result => {
                node.set_role(Role::Status);
                node.set_label("Result");
                node.set_value(self.name);
                if on(BUSY) {
                    node.set_busy();
                }
            }
            Sort::List => {
                node.set_role(Role::List);
                node.set_label(self.name);
            }
            Sort::Button => {
                node.set_role(Role::Button);
                node.set_label(self.name);
                if on(DISABLED) {
                    node.set_disabled();
                }
            }
            Sort::Row | Sort::Column => {}
        }
    }

    fn draw(&self, _: &mut DrawContext<'_, u8>) {}
}

/// A node of the model's tree.
#[derive(Clone, Debug)]
struct Shape {
    key: u64,
    part: Part,
    focused: bool,
    region: bool,
    children: Vec<Shape>,
}

impl Shape {
    fn view(&self) -> View<Part> {
        let mut view = View::new(Key::new(self.key), self.part.clone());
        if self.focused {
            view = view.focused();
        }
        if self.region {
            view = view.region();
        }
        for child in &self.children {
            view.push(child.view());
        }
        view
    }

    /// The paths of child indices to every node, in preorder.
    fn paths(&self, prefix: &mut Vec<usize>, out: &mut Vec<Vec<usize>>) {
        out.push(prefix.clone());
        for (index, child) in self.children.iter().enumerate() {
            prefix.push(index);
            child.paths(prefix, out);
            prefix.pop();
        }
    }

    fn at(&mut self, path: &[usize]) -> &mut Shape {
        path.iter()
            .fold(self, |node, &index| &mut node.children[index])
    }

    /// The number of nodes, the depth and the number of text leaves.
    fn measure(&self) -> (usize, usize, usize) {
        let (mut count, mut depth, mut texts) = (1, 0, usize::from(self.part.sort.is_text()));
        for child in &self.children {
            let (c, d, t) = child.measure();
            count += c;
            depth = depth.max(d);
            texts += t;
        }
        (count, depth + 1, texts)
    }

    /// Whether a node of the subtree is marked as focused.
    fn holds_focus(&self) -> bool {
        self.focused || self.children.iter().any(Shape::holds_focus)
    }

    fn clear_marks(&mut self, focus: bool) {
        if focus {
            self.focused = false;
        } else {
            self.region = false;
        }
        for child in &mut self.children {
            child.clear_marks(focus);
        }
    }

    /// Whether the focused control, if any, is the region's root or below
    /// it; without a region mark the root is the region's root.
    fn focus_in_region(&self) -> bool {
        fn region(node: &Shape) -> Option<&Shape> {
            if node.region {
                return Some(node);
            }
            node.children.iter().find_map(region)
        }
        region(self).is_none_or(Shape::holds_focus) || !self.holds_focus()
    }
}

/// The application's message: apply the planned change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Change;

/// The test application: the model's tree, the change that the next
/// `Change` applies, and whether this round's view is too deep.
struct Model {
    root: Shape,
    next: Option<Shape>,
    too_deep: bool,
}

impl App for Model {
    type Message = Change;
    type Control = Part;
    type Port = crate::testing::TestPort;

    fn update(&mut self, message: Message<Self>, _: &mut Out<Self>) {
        if let Message::Own(Change) = message
            && let Some(next) = self.next.take()
        {
            self.root = next;
        }
    }

    fn view(&self) -> View<Part> {
        let mut view = self.root.view();
        if self.too_deep {
            for key in 0..MAX_DEPTH as u64 {
                view = View::new(Key::new(1_000_000 + key), part(Sort::Column)).child(view);
            }
        }
        view
    }

    fn bindings(&self) -> &'static [Binding] {
        BINDINGS
    }

    fn command(&self, command: &Command) -> Option<Change> {
        (command.id == INSERT_TEXT && command.text == Some('m')).then_some(Change)
    }
}

fn px(pixels: i32) -> Fx {
    Fx::from_pixels(pixels).unwrap()
}

/// A control of `sort` with the first name and no flags.
fn part(sort: Sort) -> Part {
    Part {
        sort,
        name: NAMES[0],
        font: Font::Sans,
        size: FontSize::Body,
        axis: Axis::Y,
        request: Fx::ZERO,
        fixed: Size::new(px(10), px(10)).unwrap(),
        flips: 0,
    }
}

/// A scroll request from `-MAX_LAYOUT_INPUT` to `MAX_LAYOUT_INPUT`, both
/// ends and zero included.
fn request(rng: &mut SplitMix64) -> Fx {
    let limit = MAX_LAYOUT_INPUT.units();
    Fx::from_units(match rng.below(6) {
        0 => 0,
        1 => limit,
        2 => -limit,
        3 => rng.below(200 * 64) as i32,
        _ => rng.below(2 * limit as usize + 1) as i32 - limit,
    })
}

/// A random control of `sort`.
fn random_part(rng: &mut SplitMix64, sort: Sort) -> Part {
    let fonts = [Font::Sans, Font::Mono];
    let sizes = [FontSize::Small, FontSize::Body, FontSize::Large];
    let flags = [
        0,
        DISABLED,
        SELECTED,
        READ_ONLY,
        BUSY,
        DISABLED | SELECTED | READ_ONLY | BUSY,
    ];
    Part {
        sort,
        name: NAMES[rng.below(NAMES.len())],
        font: fonts[rng.below(fonts.len())],
        size: sizes[rng.below(sizes.len())],
        axis: if rng.below(2) == 0 { Axis::X } else { Axis::Y },
        request: request(rng),
        fixed: Size::new(px(rng.below(201) as i32), px(rng.below(61) as i32)).unwrap(),
        flips: flags[rng.below(flags.len())],
    }
}

/// The model after one random change, or the model as it was when the
/// change would break its bounds or put the focus outside the region.
fn change(root: &Shape, rng: &mut SplitMix64, keys: &mut u64) -> Shape {
    let mut next = root.clone();
    let mut paths = Vec::new();
    next.paths(&mut Vec::new(), &mut paths);
    let path = paths[rng.below(paths.len())].clone();
    match rng.below(8) {
        0 | 1 => {
            *keys += 1;
            let sort = SORTS[rng.below(SORTS.len())];
            let node = Shape {
                key: *keys,
                part: random_part(rng, sort),
                focused: false,
                region: false,
                children: Vec::new(),
            };
            let target = next.at(&path);
            let at = rng.below(target.children.len() + 1);
            target.children.insert(at, node);
        }
        2 if !path.is_empty() => {
            let (last, parent) = path.split_last().unwrap();
            next.at(parent).children.remove(*last);
        }
        3 if !path.is_empty() => {
            let (last, parent) = path.split_last().unwrap();
            let moved = next.at(parent).children.remove(*last);
            let mut targets = Vec::new();
            next.paths(&mut Vec::new(), &mut targets);
            let target = next.at(&targets[rng.below(targets.len())]);
            let at = rng.below(target.children.len() + 1);
            target.children.insert(at, moved);
        }
        4 => {
            let node = next.at(&path);
            node.part.sort = SORTS[rng.below(SORTS.len())];
        }
        5 => {
            let node = next.at(&path);
            node.part = random_part(rng, node.part.sort);
        }
        6 => {
            let node = next.at(&path);
            let count = node.children.len();
            if count >= 2 {
                node.children.swap(rng.below(count), rng.below(count));
            }
        }
        _ => {
            let focus = rng.below(2) == 0;
            next.clear_marks(focus);
            if rng.below(4) != 0 {
                let node = next.at(&path);
                if focus {
                    node.focused = true;
                } else {
                    node.region = true;
                }
            }
        }
    }
    let (count, depth, texts) = next.measure();
    if count > MAX_NODES || depth > MAX_MODEL_DEPTH || texts > MAX_TEXTS || !next.focus_in_region()
    {
        return root.clone();
    }
    next
}

/// The surfaces of a group.
const SURFACES: [SurfaceId; 2] = [SurfaceId::new(1), SurfaceId::new(2)];

/// A random size of a surface, from 1 × 1 to 800 × 480 pixels.
fn random_size(rng: &mut SplitMix64) -> PixelSize {
    PixelSize::new(1 + rng.below(800) as u32, 1 + rng.below(480) as u32).unwrap()
}

/// A key press of `character` on the first surface.
fn char_press(character: char) -> Event {
    Event::Key {
        surface: SURFACES[0],
        stamp: stamp(1, 1),
        event: key_event(KeyMeaning::Char(character), KeyAction::Press),
    }
}

/// What the test keeps of a surface between rounds.
struct Kept {
    /// The size of its latest round with a size.
    size: PixelSize,
    /// The tree that the platform keeps, from the updates applied in order.
    tree: Option<AccessibilityTree>,
    /// Its next update must hold the whole tree: none has been applied, or
    /// the latest was dropped.
    whole: bool,
}

/// Runs group `group` and returns the debug form of its updates, round by
/// round; checks every round.
fn run_group(group: u64, fonts: &mut FontSet) -> String {
    let seed = SEED.wrapping_add(group);
    let mut rng = SplitMix64::new(seed);
    let fail =
        |round: usize, what: &str| format!("seed {seed:#x}, group {group}, round {round}: {what}");
    let mut keys = 0;
    let sort = SORTS[rng.below(SORTS.len())];
    let mut root = Shape {
        key: 0,
        part: random_part(&mut rng, sort),
        focused: false,
        region: false,
        children: Vec::new(),
    };
    for _ in 0..rng.below(16) {
        root = change(&root, &mut rng, &mut keys);
    }
    let mut app = Model {
        root,
        next: None,
        too_deep: false,
    };
    let mut runtime: Runtime<Model> = Runtime::new();
    let mut kept: Vec<Kept> = (0..1 + rng.below(SURFACES.len()))
        .map(|_| Kept {
            size: random_size(&mut rng),
            tree: None,
            whole: true,
        })
        .collect();
    let rounds = 1 + rng.below(MAX_ROUNDS);
    // The round from which one update of the surface `drop.1` is dropped.
    let mut drop =
        (group % DROP_EVERY == DROP_EVERY - 1).then(|| (rng.below(rounds), rng.below(kept.len())));
    let mut sized: Vec<bool> = vec![true; kept.len()];
    let mut dropped_last = false;
    let mut left: BTreeSet<WidgetId> = BTreeSet::new();
    let mut log = String::new();
    for round in 0..rounds {
        let mut input = Input::new(MonoTime::from_nanos(round as u64 * 100));
        if round == 0 {
            for &surface in &SURFACES[..kept.len()] {
                input.events.push(Event::FocusIn {
                    surface,
                    stamp: stamp(1, 1),
                });
            }
        }
        app.too_deep = !dropped_last && rng.below(TOO_DEEP_EVERY) == 0;
        if app.too_deep {
            // Nothing else changes in a round whose view is rejected.
            app.next = None;
            input.events.push(char_press('m'));
        } else {
            app.next = Some(change(&app.root, &mut rng, &mut keys));
            for _ in 0..rng.below(3) {
                input.events.push(char_press('f'));
            }
            if rng.below(4) != 0 {
                input.events.push(char_press('m'));
            }
            for (surface, sized) in kept.iter_mut().zip(sized.iter_mut()) {
                *sized = rng.below(UNSIZED_EVERY) != 0;
                if rng.below(4) == 0 {
                    surface.size = random_size(&mut rng);
                }
            }
        }
        let mut layout = LayoutInput::new(fonts, 0, TOKENS);
        for (index, surface) in kept.iter().enumerate() {
            if sized[index] {
                layout = layout.surface(SURFACES[index], surface.size);
            }
        }
        let before = runtime.tree().preorder();
        let states: Vec<Option<u8>> = before
            .iter()
            .map(|&id| runtime.tree().state(id).copied())
            .collect();
        let ran = catch_unwind(AssertUnwindSafe(|| {
            runtime.round(&mut app, input).finish_with(&app, layout)
        }));
        let Ok(output) = ran else {
            panic!("{}", fail(round, "the round panicked"));
        };
        app.too_deep = false;
        let _ = writeln!(log, "{:?}", output.accessibility);
        let after = runtime.tree().preorder();
        if matches!(output.view, ViewPass::Rejected(_)) {
            let kept_states: Vec<Option<u8>> = after
                .iter()
                .map(|&id| runtime.tree().state(id).copied())
                .collect();
            assert!(
                before == after && states == kept_states,
                "{}",
                fail(round, "a rejected view changed the tree")
            );
            assert!(
                output.accessibility.is_empty(),
                "{}",
                fail(round, "a rejected view gave an update")
            );
        }
        left.extend(before.iter().filter(|id| !after.contains(id)));
        let mut dropped = None;
        let built = whole_trees(&runtime, &kept);
        for (surface, update) in &output.accessibility {
            let index = SURFACES.iter().position(|s| s == surface).unwrap();
            let expected = built[index].as_ref().unwrap_or_else(|| {
                panic!(
                    "{}",
                    fail(round, "an update for a surface without a layout")
                )
            });
            check_update(&kept[index], expected, update)
                .unwrap_or_else(|what| panic!("{}", fail(round, &what)));
            if drop.is_some_and(|(from, at)| round >= from && at == index) {
                drop = None;
                dropped = Some(index);
                runtime.push_failed(*surface);
                kept[index].whole = true;
                continue;
            }
            let taken = match kept[index].tree.as_mut() {
                None => {
                    AccessibilityTree::new(update.clone()).map(|tree| kept[index].tree = Some(tree))
                }
                Some(tree) => tree.apply(update.clone()),
            };
            assert_eq!(taken, Ok(()), "{}", fail(round, "an update was refused"));
            kept[index].whole = false;
        }
        dropped_last = dropped.is_some();
        for (index, expected) in built.iter().enumerate() {
            // A surface without a layout, or whose update was dropped,
            // keeps a tree that may be behind the controls.
            let Some(expected) = expected.as_ref().filter(|_| dropped != Some(index)) else {
                continue;
            };
            assert!(!kept[index].whole, "{}", fail(round, "no whole update"));
            let tree = kept[index].tree.as_ref().unwrap_or_else(|| {
                panic!("{}", fail(round, "a surface with a layout has no tree"))
            });
            check_tree(&runtime, SURFACES[index], tree, expected, &left)
                .unwrap_or_else(|what| panic!("{}", fail(round, &what)));
        }
    }
    log
}

/// The whole tree that the pass builds for each surface of `kept` with a
/// layout, from the runtime's control tree and layouts.
fn whole_trees(runtime: &Runtime<Model>, kept: &[Kept]) -> Vec<Option<crate::access::Built>> {
    let nodes = controls(runtime.tree());
    (0..kept.len())
        .map(|index| {
            let layout = runtime.layout(SURFACES[index])?;
            surface(runtime.tree(), &nodes, layout)
        })
        .collect()
}

/// Checks an update against the whole tree `built` and the tree that the
/// platform keeps for the surface before it.
fn check_update(
    kept: &Kept,
    built: &crate::access::Built,
    update: &TreeUpdate,
) -> Result<(), String> {
    let all: Vec<NodeId> = built.nodes.iter().map(|(id, _)| *id).collect();
    let ids: Vec<NodeId> = update.nodes.iter().map(|(id, _)| *id).collect();
    let tree = kept.tree.as_ref().filter(|_| !kept.whole);
    let root_changed = tree.is_some_and(|tree| tree.root() != built.root);
    match tree {
        None => {
            if update.tree != Some(TreeInfo::new(built.root)) || ids != all {
                return Err(format!("a first update is not whole: {ids:?}"));
            }
        }
        Some(tree) => {
            let changed: Vec<NodeId> = built
                .nodes
                .iter()
                .filter(|(id, node)| tree.node(*id) != Some(node))
                .map(|(id, _)| *id)
                .collect();
            if ids != changed {
                return Err(format!(
                    "the update holds {ids:?}, not the changed {changed:?}"
                ));
            }
            let info = root_changed.then(|| TreeInfo::new(built.root));
            if update.tree != info {
                return Err(format!("the tree information is {:?}", update.tree));
            }
            if changed.is_empty() && !root_changed && tree.focus() == built.focus {
                return Err("an update without a change".to_string());
            }
        }
    }
    if update.focus != built.focus {
        return Err(format!("the update's focus is {:?}", update.focus));
    }
    Ok(())
}

/// Checks the tree that the platform keeps for `surface` against the
/// control tree, its layout and the whole tree `built`.
fn check_tree(
    runtime: &Runtime<Model>,
    surface: SurfaceId,
    tree: &AccessibilityTree,
    built: &crate::access::Built,
    left: &BTreeSet<WidgetId>,
) -> Result<(), String> {
    let controls: &Tree<Model> = runtime.tree();
    let order = controls.preorder();
    let root = controls.root().ok_or("a tree without controls")?;
    // One node for each control, with the root of the control tree.
    let ids: BTreeSet<NodeId> = tree.nodes().map(|(id, _)| id).collect();
    let expected: BTreeSet<NodeId> = order.iter().map(|&id| node_id(id)).collect();
    if ids != expected || tree.root() != node_id(root) {
        return Err(format!(
            "the nodes are {ids:?}, not those of the controls {expected:?}"
        ));
    }
    if left.iter().any(|&id| ids.contains(&node_id(id))) {
        return Err("a control that left the tree has a node".to_string());
    }
    // The children, in order.
    for &id in &order {
        let children: Vec<NodeId> = controls.children(id).iter().map(|&c| node_id(c)).collect();
        if tree.node(node_id(id)).map(Node::children) != Some(children.as_slice()) {
            return Err(format!("the children of {id} differ"));
        }
    }
    // The whole tree, node by node.
    for (id, node) in &built.nodes {
        if tree.node(*id) != Some(node) {
            return Err(format!("the node {id:?} is not that of the whole tree"));
        }
    }
    // The focus.
    let focus = controls.focused().map_or(node_id(root), node_id);
    if tree.focus() != focus {
        return Err(format!("the focus is {:?}, not {focus:?}", tree.focus()));
    }
    // The bounding boxes, from the geometry of hit testing, and the scroll
    // offsets, from the requests of the controls.
    let layout = runtime.layout(surface).ok_or("no layout")?;
    let result = layout.result().map_err(|error| format!("{error:?}"))?;
    let mut placed: Vec<Placed> = Vec::new();
    for (index, (laid, boxed)) in layout.boxes().iter().zip(result.boxes()).enumerate() {
        let parent = boxed.parent.and_then(|parent| placed.get(parent)).copied();
        let place = Placed::of(parent, boxed).map_err(|error| format!("{error:?}"))?;
        placed.push(place);
        let base = parent.map_or(Point::default(), |parent| parent.content);
        let origin = base
            .translated(boxed.rect.origin().x, boxed.rect.origin().y)
            .ok_or("overflow")?;
        let pixels = |value: Fx| f64::from(value.units()) / 64.0;
        let bounds = Rect {
            x0: pixels(origin.x),
            y0: pixels(origin.y),
            x1: pixels(origin.x) + pixels(boxed.rect.size().width()),
            y1: pixels(origin.y) + pixels(boxed.rect.size().height()),
        };
        let node = tree
            .node(node_id(laid.widget))
            .ok_or("a box without a node")?;
        if node.bounds() != Some(bounds) {
            return Err(format!(
                "the bounds of {} are {:?}, not {bounds:?}",
                laid.widget,
                node.bounds()
            ));
        }
        let part = controls
            .control(laid.widget)
            .ok_or("a box without a control")?;
        if part.sort != Sort::Scroller {
            if node.scroll_x().is_some() || node.scroll_y().is_some() || node.clips_children() {
                return Err(format!("{} scrolls", laid.widget));
            }
            continue;
        }
        let content = layout
            .boxes()
            .get(index + 1)
            .filter(|child| child.parent == Some(index))
            .map_or(Size::ZERO, |child| child.size);
        let (length, viewport) = match part.axis {
            Axis::X => (content.width(), laid.size.width()),
            Axis::Y => (content.height(), laid.size.height()),
        };
        let range = (length.units() - viewport.units()).max(0);
        let offset = part.request.units().clamp(0, range);
        let (scroll, min, max) = match part.axis {
            Axis::X => (node.scroll_x(), node.scroll_x_min(), node.scroll_x_max()),
            Axis::Y => (node.scroll_y(), node.scroll_y_min(), node.scroll_y_max()),
        };
        let units = |value: i32| Some(f64::from(value) / 64.0);
        if (scroll, min, max) != (units(offset), Some(0.0), units(range)) || !node.clips_children()
        {
            return Err(format!(
                "{} scrolls by {scroll:?} in {min:?} to {max:?}",
                laid.widget
            ));
        }
    }
    Ok(())
}

#[test]
fn random_rounds_keep_the_accessibility_trees_in_step_with_the_controls() {
    let mut fonts = font_set();
    for group in 0..GROUPS {
        let first = run_group(group, &mut fonts);
        let second = run_group(group, &mut fonts);
        assert!(
            first == second,
            "seed {:#x}, group {group}: two runs gave other updates",
            SEED.wrapping_add(group)
        );
    }
}
