//! The accessibility pass of a round, for eight kinds of control: the
//! first update holds the whole tree, in which the control's node has its
//! role, name and state; after a second round changes that state, or the
//! view, the update holds only the nodes that changed and the focus.

use kosmo_platform::PixelSize;
use kosmo_platform::accesskit::{Node, NodeId, Rect, Role, TreeUpdate};
use kosmo_text::FontSet;
use kosmo_types::{Fx, KeyAction, KeyMeaning, MonoTime, NamedKey, Size};

use super::node_id;
use crate::app::{App, Context, Control, DrawContext, Handling, Message, Out, Phase};
use crate::command::{Binding, Command, CommandId, EVALUATE_SELECTION, Mods, Trigger};
use crate::focus::Event;
use crate::layout::{Axis, Column, Font, Node as LayoutNode, Scroll, Text};
use crate::round::{Input, LayoutInput, Runtime};
use crate::testing::{SURFACE, TestPort, font_set, key_event, press, stamp};
use crate::tokens::{FontSize, TOKENS};
use crate::tree::{Key, Kind, View, WidgetId};

/// A command of the tests that turns the presentation state of the
/// control that handles it on.
const TURN_ON: CommandId = CommandId::new("test", "TurnOn", false);

/// The binding table of every control of the tests: `o` turns it on.
const BINDINGS: &[Binding] = &[Binding {
    trigger: Trigger::Char('o'),
    mods: Mods::NONE,
    command: TURN_ON,
}];

fn px(pixels: i32) -> Fx {
    Fx::from_pixels(pixels).unwrap()
}

/// A control of the tests, whose presentation state is off until it
/// handles `TurnOn`.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Part {
    /// A column, the window named `Main`.
    Window,
    /// A text leaf, disabled while it is on.
    Label(&'static str),
    /// A one-line input that shows its text, named `Input`, with its text
    /// as its value, read only while it is on.
    Input(&'static str),
    /// A column, the list named `History`.
    List,
    /// A text leaf, a list item named by its text, selected while it is on.
    Item(&'static str),
    /// A vertical scroll container named `History` that asks for an offset
    /// of 30 pixels, and of 500 pixels while it is on.
    Scroller,
    /// A column without a name.
    Column,
    /// A fixed-size leaf of 100 × 20 pixels without a name.
    Line,
    /// A text leaf, the result line named `Result`, with its text as its
    /// value, busy while it is on: a job computes it.
    Result(&'static str),
    /// A fixed-size leaf, the button named `Evaluate`, disabled while it is
    /// on.
    Button,
}

impl Control<Views> for Part {
    type State = bool;

    fn kind(&self) -> Kind {
        Kind::new(match self {
            Part::Window => "window",
            Part::Label(_) => "label",
            Part::Input(_) => "input",
            Part::List => "list",
            Part::Item(_) => "item",
            Part::Scroller => "scroller",
            Part::Column => "column",
            Part::Line => "line",
            Part::Result(_) => "result",
            Part::Button => "button",
        })
    }

    fn bindings(&self) -> &'static [Binding] {
        BINDINGS
    }

    fn handle(&self, _: Phase, command: &Command, cx: &mut Context<'_, Views, bool>) -> Handling {
        if command.id == TURN_ON {
            *cx.state_mut() = true;
            Handling::Handled
        } else {
            Handling::Unhandled
        }
    }

    fn layout(&self, on: &bool) -> LayoutNode<'_> {
        match self {
            Part::Window | Part::List | Part::Column => LayoutNode::Column(Column::new()),
            Part::Label(text) | Part::Input(text) | Part::Item(text) | Part::Result(text) => {
                LayoutNode::Text(Text::new(text, Font::Sans, FontSize::Body))
            }
            Part::Scroller => {
                LayoutNode::Scroll(Scroll::new(Axis::Y, px(if *on { 500 } else { 30 })))
            }
            Part::Line => LayoutNode::Fixed(Size::new(px(100), px(20)).unwrap()),
            Part::Button => LayoutNode::Fixed(Size::new(px(80), px(20)).unwrap()),
        }
    }

    fn accessibility(&self, on: &bool, node: &mut Node) {
        match self {
            Part::Window => {
                node.set_role(Role::Window);
                node.set_label("Main");
            }
            Part::Label(_) => {
                if *on {
                    node.set_disabled();
                }
            }
            Part::Input(text) => {
                node.set_role(Role::TextInput);
                node.set_label("Input");
                node.set_value(*text);
                if *on {
                    node.set_read_only();
                }
            }
            Part::List => {
                node.set_role(Role::List);
                node.set_label("History");
            }
            Part::Item(_) => {
                node.set_role(Role::ListItem);
                node.set_selected(*on);
            }
            Part::Scroller => node.set_label("History"),
            Part::Column | Part::Line => {}
            Part::Result(text) => {
                node.set_role(Role::Status);
                node.set_label("Result");
                node.set_value(*text);
                if *on {
                    node.set_busy();
                }
            }
            Part::Button => {
                node.set_role(Role::Button);
                node.set_label("Evaluate");
                if *on {
                    node.set_disabled();
                }
            }
        }
    }

    fn draw(&self, _: &mut DrawContext<'_, bool>) {}
}

/// The application of the tests: its view, and the view that the next
/// `Execute` puts in its place.
struct Views {
    view: View<Part>,
    next: Option<View<Part>>,
}

impl App for Views {
    type Message = ();
    type Control = Part;
    type Port = TestPort;

    fn update(&mut self, message: Message<Self>, _: &mut Out<Self>) {
        if let Message::Own(()) = message
            && let Some(next) = self.next.take()
        {
            self.view = next;
        }
    }

    fn view(&self) -> View<Part> {
        self.view.clone()
    }

    fn command(&self, command: &Command) -> Option<()> {
        (command.id == EVALUATE_SELECTION).then_some(())
    }
}

/// A view node with `key` of `part`.
fn node(key: u64, part: Part) -> View<Part> {
    View::new(Key::new(key), part)
}

/// The key `o`, which the focused control binds to `TurnOn`.
fn turn_on() -> Event {
    Event::Key {
        surface: SURFACE,
        stamp: stamp(1, 1),
        event: key_event(KeyMeaning::Char('o'), KeyAction::Press),
    }
}

/// The key Execute, which the application handles by taking its next view.
fn execute() -> Event {
    press(NamedKey::EXECUTE, stamp(1, 1))
}

/// Two rounds of `app` that lay the surface out at `width` × `height`
/// with the font set and the tokens of the shell: the first, in which the
/// surface gains focus, and the second with `event`. Returns the runtime
/// and the accessibility updates of the two rounds.
fn two_rounds(
    app: &mut Views,
    width: u32,
    height: u32,
    event: Event,
) -> (Runtime<Views>, Vec<TreeUpdate>, Vec<TreeUpdate>) {
    let mut fonts = font_set();
    let mut runtime = Runtime::new();
    let focus_in = Event::FocusIn {
        surface: SURFACE,
        stamp: stamp(1, 1),
    };
    let first = round(&mut runtime, app, &mut fonts, focus_in, width, height);
    let second = round(&mut runtime, app, &mut fonts, event, width, height);
    (runtime, first, second)
}

/// One round with `event` that lays the surface out at `width` × `height`,
/// and the updates of its output, all of them for the surface.
fn round(
    runtime: &mut Runtime<Views>,
    app: &mut Views,
    fonts: &mut FontSet,
    event: Event,
    width: u32,
    height: u32,
) -> Vec<TreeUpdate> {
    let mut input = Input::new(MonoTime::ZERO);
    input.events.push(event);
    let size = PixelSize::new(width, height).unwrap();
    let layout = LayoutInput::new(fonts, 0, TOKENS).surface(SURFACE, size);
    let output = runtime.round(app, input).finish_with(app, layout);
    output
        .accessibility
        .into_iter()
        .map(|(surface, update)| {
            assert_eq!(surface, SURFACE);
            update
        })
        .collect()
}

/// The node of the control `id` in `update`.
fn node_of(update: &TreeUpdate, id: WidgetId) -> &Node {
    let id = node_id(id);
    &update.nodes.iter().find(|(node, _)| *node == id).unwrap().1
}

/// The identifiers of the nodes of `update`, in its order.
fn ids(update: &TreeUpdate) -> Vec<NodeId> {
    update.nodes.iter().map(|(id, _)| *id).collect()
}

/// The control at `path` of child indices from the root.
fn control(runtime: &Runtime<Views>, path: &[usize]) -> WidgetId {
    let tree = runtime.tree();
    path.iter()
        .fold(tree.root().unwrap(), |id, &index| tree.children(id)[index])
}

/// The identifiers of the nodes of every control, in preorder.
fn every_node(runtime: &Runtime<Views>) -> Vec<NodeId> {
    runtime.tree().preorder().into_iter().map(node_id).collect()
}

/// A window with `child`, the window and the child's subtree as `view`
/// gives them.
fn window(child: View<Part>) -> View<Part> {
    node(0, Part::Window).child(child)
}

#[test]
fn a_window_is_focused_until_the_view_marks_its_child() {
    let view = window(node(1, Part::Label("1+2")));
    let mut app = Views {
        view,
        next: Some(window(node(1, Part::Label("1+2")).focused())),
    };
    let (runtime, first, second) = two_rounds(&mut app, 200, 100, execute());
    let (root, child) = (control(&runtime, &[]), control(&runtime, &[0]));
    let [first] = first.as_slice() else {
        panic!("not one update: {first:?}");
    };
    assert_eq!(first.tree.as_ref().unwrap().root, node_id(root));
    assert_eq!(ids(first), every_node(&runtime));
    let node = node_of(first, root);
    assert_eq!(node.role(), Role::Window);
    assert_eq!(node.label(), Some("Main"));
    assert_eq!(node.children(), [node_id(child)]);
    assert_eq!(first.focus, node_id(root));
    // The second view keeps every node and marks the child.
    let [second] = second.as_slice() else {
        panic!("not one update: {second:?}");
    };
    assert_eq!(second.nodes, []);
    assert_eq!(second.tree, None);
    assert_eq!(second.focus, node_id(child));
}

#[test]
fn a_label_is_named_by_its_text_and_disabled_by_its_state() {
    let mut app = Views {
        view: window(node(1, Part::Label("1+2")).focused()),
        next: None,
    };
    let (runtime, first, second) = two_rounds(&mut app, 200, 100, turn_on());
    let label = control(&runtime, &[0]);
    let node = node_of(&first[0], label);
    assert_eq!(node.role(), Role::Label);
    assert_eq!(node.label(), Some("1+2"));
    assert!(!node.is_disabled());
    let [second] = second.as_slice() else {
        panic!("not one update: {second:?}");
    };
    assert_eq!(ids(second), [node_id(label)]);
    assert!(node_of(second, label).is_disabled());
    assert_eq!(second.focus, node_id(label));
}

#[test]
fn a_one_line_input_has_its_text_as_value_and_becomes_read_only() {
    let mut app = Views {
        view: window(node(1, Part::Input("1+2")).focused()),
        next: None,
    };
    let (runtime, first, second) = two_rounds(&mut app, 200, 100, turn_on());
    let input = control(&runtime, &[0]);
    let node = node_of(&first[0], input);
    assert_eq!(node.role(), Role::TextInput);
    assert_eq!(node.label(), Some("Input"));
    assert_eq!(node.value(), Some("1+2"));
    assert!(!node.is_read_only());
    let [second] = second.as_slice() else {
        panic!("not one update: {second:?}");
    };
    assert_eq!(ids(second), [node_id(input)]);
    assert!(node_of(second, input).is_read_only());
}

#[test]
fn a_list_item_is_named_by_its_text_and_becomes_selected() {
    let list = node(1, Part::List)
        .child(node(1, Part::Item("1+2 = 3")))
        .child(node(2, Part::Item("2*3 = 6")).focused());
    let mut app = Views {
        view: window(list),
        next: None,
    };
    let (runtime, first, second) = two_rounds(&mut app, 200, 100, turn_on());
    let list = control(&runtime, &[0]);
    let items = [control(&runtime, &[0, 0]), control(&runtime, &[0, 1])];
    let parent = node_of(&first[0], list);
    assert_eq!(parent.role(), Role::List);
    assert_eq!(parent.children(), items.map(node_id));
    for (item, name) in items.into_iter().zip(["1+2 = 3", "2*3 = 6"]) {
        let node = node_of(&first[0], item);
        assert_eq!(node.role(), Role::ListItem);
        assert_eq!(node.label(), Some(name));
        assert_eq!(node.is_selected(), Some(false));
    }
    let [second] = second.as_slice() else {
        panic!("not one update: {second:?}");
    };
    assert_eq!(ids(second), [node_id(items[1])]);
    assert_eq!(node_of(second, items[1]).is_selected(), Some(true));
}

#[test]
fn a_scroll_container_gives_its_offset_and_range_and_moves_its_content() {
    let mut column = node(0, Part::Column);
    for key in 0..6 {
        column = column.child(node(key, Part::Line));
    }
    let mut app = Views {
        view: node(0, Part::Scroller).focused().child(column),
        next: None,
    };
    let (runtime, first, second) = two_rounds(&mut app, 100, 50, turn_on());
    let scroller = control(&runtime, &[]);
    let column = control(&runtime, &[0]);
    let lines: Vec<WidgetId> = (0..6).map(|index| control(&runtime, &[0, index])).collect();
    let rect = |x0: f64, y0: f64, x1: f64, y1: f64| Rect { x0, y0, x1, y1 };
    let node = node_of(&first[0], scroller);
    assert_eq!(node.role(), Role::ScrollView);
    assert_eq!(node.label(), Some("History"));
    assert_eq!(node.scroll_y(), Some(30.0));
    assert_eq!(node.scroll_y_min(), Some(0.0));
    assert_eq!(node.scroll_y_max(), Some(70.0));
    assert!(node.clips_children());
    assert_eq!(node.bounds(), Some(rect(0.0, 0.0, 100.0, 50.0)));
    let content = node_of(&first[0], column);
    assert_eq!(content.bounds(), Some(rect(0.0, -30.0, 100.0, 90.0)));
    assert!(!content.clips_children());
    assert_eq!(content.scroll_y(), None);
    for (index, &line) in lines.iter().enumerate() {
        let top = -30.0 + 20.0 * index as f64;
        let bounds = node_of(&first[0], line).bounds();
        assert_eq!(bounds, Some(rect(0.0, top, 100.0, top + 20.0)));
    }
    // A request of 500 pixels is brought to the end of the range, which
    // moves the column and its six lines 40 pixels up.
    let [second] = second.as_slice() else {
        panic!("not one update: {second:?}");
    };
    let mut moved = vec![node_id(scroller), node_id(column)];
    moved.extend(lines.iter().map(|&line| node_id(line)));
    assert_eq!(ids(second), moved);
    assert_eq!(node_of(second, scroller).scroll_y(), Some(70.0));
    assert_eq!(node_of(second, scroller).scroll_y_max(), Some(70.0));
    let content = node_of(second, column);
    assert_eq!(content.bounds(), Some(rect(0.0, -70.0, 100.0, 50.0)));
    for (index, &line) in lines.iter().enumerate() {
        let top = -70.0 + 20.0 * index as f64;
        let bounds = node_of(second, line).bounds();
        assert_eq!(bounds, Some(rect(0.0, top, 100.0, top + 20.0)));
    }
}

#[test]
fn a_result_line_has_its_value_and_becomes_busy() {
    let mut app = Views {
        view: window(node(1, Part::Result("1/3")).focused()),
        next: None,
    };
    let (runtime, first, second) = two_rounds(&mut app, 200, 100, turn_on());
    let result = control(&runtime, &[0]);
    let node = node_of(&first[0], result);
    assert_eq!(node.role(), Role::Status);
    assert_eq!(node.label(), Some("Result"));
    assert_eq!(node.value(), Some("1/3"));
    assert!(!node.is_busy());
    let [second] = second.as_slice() else {
        panic!("not one update: {second:?}");
    };
    assert_eq!(ids(second), [node_id(result)]);
    assert!(node_of(second, result).is_busy());
}

#[test]
fn a_list_names_its_items_in_order_and_takes_a_new_one() {
    let list = |items: &[&'static str]| {
        let mut list = node(1, Part::List);
        for (key, &item) in items.iter().enumerate() {
            list = list.child(node(key as u64, Part::Label(item)));
        }
        window(list)
    };
    let mut app = Views {
        view: list(&["1+2 = 3", "2*3 = 6"]),
        next: Some(list(&["1+2 = 3", "2*3 = 6", "4-1 = 3"])),
    };
    let (runtime, first, second) = two_rounds(&mut app, 200, 100, execute());
    let parent = control(&runtime, &[0]);
    let items: Vec<WidgetId> = (0..3).map(|index| control(&runtime, &[0, index])).collect();
    let node = node_of(&first[0], parent);
    assert_eq!(node.role(), Role::List);
    assert_eq!(node.label(), Some("History"));
    assert_eq!(node.children(), [node_id(items[0]), node_id(items[1])]);
    let [second] = second.as_slice() else {
        panic!("not one update: {second:?}");
    };
    assert_eq!(ids(second), [node_id(parent), node_id(items[2])]);
    let ordered: Vec<NodeId> = items.iter().map(|&item| node_id(item)).collect();
    assert_eq!(node_of(second, parent).children(), ordered);
    assert_eq!(node_of(second, items[2]).label(), Some("4-1 = 3"));
}

#[test]
fn a_button_is_named_and_becomes_disabled_without_actions() {
    let mut app = Views {
        view: window(node(1, Part::Button).focused()),
        next: None,
    };
    let (runtime, first, second) = two_rounds(&mut app, 200, 100, turn_on());
    let button = control(&runtime, &[0]);
    let node = node_of(&first[0], button);
    assert_eq!(node.role(), Role::Button);
    assert_eq!(node.label(), Some("Evaluate"));
    assert!(!node.is_disabled());
    // Nodes compare their actions too.
    let mut without = node.clone();
    without.clear_actions();
    without.clear_child_actions();
    assert_eq!(&without, node);
    let [second] = second.as_slice() else {
        panic!("not one update: {second:?}");
    };
    assert_eq!(ids(second), [node_id(button)]);
    assert!(node_of(second, button).is_disabled());
}
