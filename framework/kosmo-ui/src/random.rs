//! Random rounds from fixed seeds. Each group runs up to `MAX_ROUNDS`
//! rounds of a test application whose model is a tree that changes at
//! random: controls are added, removed, moved, given another kind or other
//! values and commands, reordered and focused, and now and then the view is
//! too deep to be built. Every round also takes random key events, with
//! stamps that are sometimes stale, and focus changes. After every round
//! nothing has panicked and every invariant of `tree::check` holds; at the
//! end of each group the tree equals one built once from the last view,
//! and a second run of the group gives the same identities. A failure
//! names the seed and the round.

use std::panic::{AssertUnwindSafe, catch_unwind};

use kosmo_types::{
    Epoch, FocusGen, HidUsage, KeyAction, KeyEvent, KeyMeaning, ModSet, MonoTime, NamedKey,
};

use crate::app::{App, Context, Control, DrawContext, Handling, Message, Out, Phase};
use crate::command::{Command, CommandId, DELETE_BACKWARD, INSERT_TEXT, MOVE_LEFT, MOVE_RIGHT};
use crate::focus::{Event, Stamp, SurfaceId};
use crate::round::{Input, Runtime, ViewPass};
use crate::testing::TestPort;
use crate::timer::TimerLabel;
use crate::tree::check::{check, identities, same_tree};
use crate::tree::{Key, Kind, MAX_DEPTH, Tree, View, WidgetId};

/// The seed of the first group; group `n` uses `SEED + n`.
const SEED: u64 = 0x6b6f_736d_6f00_0300;

/// The number of groups.
const GROUPS: u64 = 1_000;

/// The most rounds in one group.
const MAX_ROUNDS: usize = 64;

/// The most nodes of the model.
const MAX_NODES: usize = 48;

/// The kinds of the model's controls.
const KINDS: [&str; 3] = ["label", "row", "field"];

/// The commands that the model's controls may handle.
const HANDLED: [CommandId; 3] = [MOVE_LEFT, MOVE_RIGHT, DELETE_BACKWARD];

/// The SplitMix64 generator.
struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    fn new(seed: u64) -> SplitMix64 {
        SplitMix64 { state: seed }
    }

    fn next(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// A value in `0..bound`, up to the generator's bias; 0 when `bound` is 0.
    fn below(&mut self, bound: usize) -> usize {
        if bound == 0 {
            0
        } else {
            (self.next() % bound as u64) as usize
        }
    }
}

/// What a control does when it handles its command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Reply {
    Message,
    StateOnly,
    Timer(u64),
}

/// A control of the model.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Part {
    kind: usize,
    value: u32,
    handles: Option<CommandId>,
    reply: Reply,
}

impl Control<Model> for Part {
    type State = u32;

    fn kind(&self) -> Kind {
        Kind::new(KINDS[self.kind])
    }

    fn handle(&self, _: Phase, command: &Command, cx: &mut Context<'_, Model, u32>) -> Handling {
        if self.handles != Some(command.id) {
            return Handling::Unhandled;
        }
        *cx.state_mut() += 1;
        match self.reply {
            Reply::Message => cx.message(Apply::Noted),
            Reply::StateOnly => {}
            Reply::Timer(at) => cx.timer(MonoTime::from_nanos(at), TimerLabel::new(at)),
        }
        Handling::Handled
    }

    fn timer(&self, _: TimerLabel, cx: &mut Context<'_, Model, u32>) {
        cx.message(Apply::Noted);
    }

    fn draw(&self, _: &mut DrawContext<'_, u32>) {}
}

/// A node of the model's tree.
#[derive(Clone, Debug)]
struct Node {
    key: u64,
    part: Part,
    focused: bool,
    region: bool,
    children: Vec<Node>,
}

impl Node {
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

    fn paths(&self, prefix: &mut Vec<usize>, out: &mut Vec<Vec<usize>>) {
        out.push(prefix.clone());
        for (index, child) in self.children.iter().enumerate() {
            prefix.push(index);
            child.paths(prefix, out);
            prefix.pop();
        }
    }

    fn at(&mut self, path: &[usize]) -> &mut Node {
        path.iter()
            .fold(self, |node, &index| &mut node.children[index])
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
}

/// The application's message: apply the planned change, or note that a
/// control handled something.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Apply {
    Change,
    Noted,
}

/// The test application: a model tree, the change that the next `Change`
/// message applies, and whether this round's view is too deep.
struct Model {
    root: Node,
    next: Option<Node>,
    too_deep: bool,
}

impl App for Model {
    type Message = Apply;
    type Control = Part;
    type Port = TestPort;

    fn update(&mut self, message: Message<Self>, _: &mut Out<Self>) {
        if let Message::Own(Apply::Change) = message
            && let Some(next) = self.next.take()
        {
            self.root = next;
        }
    }

    fn view(&self) -> View<Part> {
        let mut view = self.root.view();
        if self.too_deep {
            for key in 0..MAX_DEPTH as u64 {
                view = View::new(Key::new(1_000_000 + key), part(0, 0)).child(view);
            }
        }
        view
    }

    fn command(&self, command: &Command) -> Option<Apply> {
        (command.id == INSERT_TEXT && command.text == Some('m')).then_some(Apply::Change)
    }
}

fn part(kind: usize, value: u32) -> Part {
    Part {
        kind,
        value,
        handles: None,
        reply: Reply::Message,
    }
}

/// A random new control.
fn random_part(rng: &mut SplitMix64) -> Part {
    let handles = match rng.below(4) {
        0 => None,
        n => Some(HANDLED[n - 1]),
    };
    let reply = match rng.below(3) {
        0 => Reply::Message,
        1 => Reply::StateOnly,
        _ => Reply::Timer(rng.below(4_000) as u64),
    };
    Part {
        kind: rng.below(KINDS.len()),
        value: rng.below(8) as u32,
        handles,
        reply,
    }
}

/// The model after one random change.
fn change(root: &Node, rng: &mut SplitMix64, keys: &mut u64) -> Node {
    let mut root = root.clone();
    let mut paths = Vec::new();
    root.paths(&mut Vec::new(), &mut paths);
    let path = paths[rng.below(paths.len())].clone();
    match rng.below(7) {
        0 if paths.len() < MAX_NODES => {
            *keys += 1;
            let node = Node {
                key: *keys,
                part: random_part(rng),
                focused: false,
                region: false,
                children: Vec::new(),
            };
            let target = root.at(&path);
            let at = rng.below(target.children.len() + 1);
            target.children.insert(at, node);
        }
        1 if !path.is_empty() => {
            let (last, parent) = path.split_last().unwrap();
            root.at(parent).children.remove(*last);
        }
        2 if !path.is_empty() => {
            let (last, parent) = path.split_last().unwrap();
            let moved = root.at(parent).children.remove(*last);
            let mut targets = Vec::new();
            root.paths(&mut Vec::new(), &mut targets);
            let target = root.at(&targets[rng.below(targets.len())]);
            let at = rng.below(target.children.len() + 1);
            target.children.insert(at, moved);
        }
        3 => {
            let node = root.at(&path);
            node.part.kind = (node.part.kind + 1 + rng.below(KINDS.len() - 1)) % KINDS.len();
        }
        4 => {
            let node = root.at(&path);
            node.part = Part {
                kind: node.part.kind,
                ..random_part(rng)
            };
        }
        5 => {
            let node = root.at(&path);
            let count = node.children.len();
            if count >= 2 {
                node.children.swap(rng.below(count), rng.below(count));
            }
        }
        6 => {
            let focus = rng.below(2) == 0;
            root.clear_marks(focus);
            if rng.below(4) != 0 {
                let node = root.at(&path);
                if focus {
                    node.focused = true;
                } else {
                    node.region = true;
                }
            }
        }
        _ => {}
    }
    root
}

/// The surface of the rounds.
const SURFACE: SurfaceId = SurfaceId::new(7);

fn key(meaning: KeyMeaning, action: KeyAction, mods: u16, time: u64) -> KeyEvent {
    KeyEvent {
        usage: HidUsage::new(0x07, 0x04),
        meaning,
        mods: ModSet::from_bits(mods),
        action,
        time: MonoTime::from_nanos(time),
        synthetic: false,
    }
}

/// The focus of the surface as the platform sees it: the generation of
/// its latest focus in, and whether it holds focus.
struct Focus {
    generation: u64,
    holds: bool,
}

/// The platform events of one round: random keys and focus changes, and
/// the key `m`, which applies the planned change when it is delivered.
fn events(rng: &mut SplitMix64, focus: &mut Focus, time: u64) -> Vec<Event> {
    let mut events = Vec::new();
    let count = rng.below(4);
    let apply_at = rng.below(count + 1);
    for index in 0..=count {
        let stamp = Stamp::new(Epoch::new(1), FocusGen::new(focus.generation));
        if !focus.holds && rng.below(2) == 0 {
            focus.generation += 1;
            focus.holds = true;
            let stamp = Stamp::new(Epoch::new(1), FocusGen::new(focus.generation));
            events.push(Event::FocusIn {
                surface: SURFACE,
                stamp,
            });
        } else if focus.holds && rng.below(12) == 0 {
            events.push(Event::FocusOut {
                surface: SURFACE,
                stamp,
            });
            focus.holds = false;
        }
        let stamp = Stamp::new(Epoch::new(1), FocusGen::new(focus.generation));
        if index == apply_at {
            let event = key(KeyMeaning::Char('m'), KeyAction::Press, 0, time);
            events.push(Event::Key {
                surface: SURFACE,
                stamp,
                event,
            });
            continue;
        }
        let meaning = match rng.below(8) {
            0 => KeyMeaning::Named(NamedKey::LEFT),
            1 => KeyMeaning::Named(NamedKey::RIGHT),
            2 => KeyMeaning::Named(NamedKey::DELETE),
            3 => KeyMeaning::Named(NamedKey::EXECUTE),
            4 => KeyMeaning::Named(NamedKey::CANCEL),
            5 => KeyMeaning::Named(NamedKey::new(42)),
            6 => KeyMeaning::Char('a'),
            _ => KeyMeaning::Char('m'),
        };
        let action = match rng.below(4) {
            0 => KeyAction::Release,
            1 => KeyAction::Repeat,
            2 => KeyAction::LongPress,
            _ => KeyAction::Press,
        };
        let mods = [0, 0x02, 0x01, 0x0100][rng.below(4)];
        let stamp = match rng.below(8) {
            0 => Stamp::new(Epoch::new(2), stamp.generation),
            1 => Stamp::new(stamp.epoch, FocusGen::new(focus.generation.wrapping_sub(1))),
            _ => stamp,
        };
        let event = key(meaning, action, mods, time);
        events.push(Event::Key {
            surface: SURFACE,
            stamp,
            event,
        });
    }
    events
}

/// Runs group `group` and returns the identities of its final tree;
/// checks the invariants after every round, and at the end compares the
/// tree with one built once from the last view.
fn run_group(group: u64) -> Vec<WidgetId> {
    let seed = SEED.wrapping_add(group);
    let mut rng = SplitMix64::new(seed);
    let mut keys = 0;
    let mut app = Model {
        root: Node {
            key: 0,
            part: part(0, 0),
            focused: true,
            region: true,
            children: Vec::new(),
        },
        next: None,
        too_deep: false,
    };
    let mut runtime: Runtime<Model> = Runtime::new();
    let mut focus = Focus {
        generation: 1,
        holds: true,
    };
    // The view that the tree was last rebuilt from; none while the tree
    // has not been built, when it is empty.
    let mut last_view: Option<View<Part>> = None;
    let mut removed: Vec<WidgetId> = Vec::new();
    let rounds = 1 + rng.below(MAX_ROUNDS);
    for round in 0..rounds {
        let time = round as u64 * 100;
        let mut input = Input::new(MonoTime::from_nanos(time));
        if round == 0 {
            let stamp = Stamp::new(Epoch::new(1), FocusGen::new(focus.generation));
            input.events.push(Event::FocusIn {
                surface: SURFACE,
                stamp,
            });
        }
        input.events.extend(events(&mut rng, &mut focus, time));
        app.next = Some(change(&app.root, &mut rng, &mut keys));
        app.too_deep = rng.below(16) == 0;

        let before = runtime.tree().preorder();
        // A panic of the round fails the test with the seed and the round.
        let ran = catch_unwind(AssertUnwindSafe(|| {
            runtime.round(&mut app, input).finish(&app)
        }));
        let Ok(output) = ran else {
            panic!("seed {seed:#x}, round {round}: the round panicked");
        };
        if output.view == ViewPass::Rebuilt {
            last_view = Some(app.view());
        }
        app.too_deep = false;
        let after = runtime.tree().preorder();
        removed.extend(before.into_iter().filter(|id| !after.contains(id)));
        let empty = View::new(Key::new(0), part(0, 0));
        let found = check(
            runtime.tree(),
            last_view.as_ref().unwrap_or(&empty),
            &removed,
        );
        assert!(found.is_empty(), "seed {seed:#x}, round {round}: {found:?}");
    }
    match &last_view {
        Some(view) => {
            let mut once: Tree<Model> = Tree::new();
            assert_eq!(once.rebuild(view), Ok(()), "seed {seed:#x}");
            assert!(
                same_tree(runtime.tree(), &once),
                "seed {seed:#x}: not the tree built once"
            );
        }
        None => assert!(
            runtime.tree().is_empty(),
            "seed {seed:#x}: not the tree built once"
        ),
    }
    identities(runtime.tree())
}

#[test]
fn random_rounds_keep_the_invariants() {
    for group in 0..GROUPS {
        let first = run_group(group);
        let second = run_group(group);
        assert_eq!(
            first,
            second,
            "seed {:#x}: other identities",
            SEED.wrapping_add(group)
        );
    }
}
