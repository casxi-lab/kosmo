//! The conformance suite of the platform interface: 16 numbered
//! requirements that every backend keeps, checked through
//! [`PlatformTestHooks`].
//!
//! Each requirement is checked on a new backend, which the caller builds
//! from the suite's [`Setup`]: the capabilities and the resource table
//! that the requirements expect. [`check`] checks one requirement and
//! [`run`] all of them, in order. The requirements that concern a bound
//! check it exactly at the bound and one over it. Before injecting
//! events, a requirement takes the events that the backend queued itself,
//! such as those of a new surface, by waiting until a wait returns none.
//!
//! The requirements, numbered as [`TITLES`] lists them:
//!
//! 1. A wait returns injected events in the order they were injected, each
//!    once and as injected, whatever their focus stamps.
//! 2. A wait that returns no event ends with `Deadline` when its deadline
//!    is not later than the clock, and otherwise, or without a deadline,
//!    with `Idle`, without blocking.
//! 3. A wait returns at most `MAX_EVENTS_PER_WAIT` events; later events
//!    stay queued in order for the next wait.
//! 4. The clock reads 0 on a new backend and then the time that the driver
//!    advanced it to; waits and other port calls do not change it, and
//!    advancing to an earlier time returns `InvalidArgument` and changes
//!    nothing.
//! 5. The identities of surfaces are distinct, and the identity of a
//!    destroyed surface is stale: `destroy_surface`, `buffer`, `present` and
//!    `update` on it return `StaleHandle` and change nothing.
//! 6. At most `MAX_SURFACES` surfaces exist at once; creating one more
//!    returns `Exhausted`, and the others keep working.
//! 7. Two surfaces keep their presentations and read-backs, their
//!    accessibility trees and their events apart.
//! 8. A buffer is described with its surface's size and format, and after
//!    a presentation `read_back` returns that description and the presented
//!    bytes, byte for byte, also after a later presentation.
//! 9. After a presentation, `buffer` returns `Busy` until a wait returns
//!    the surface's `Released`; then it hands out the buffer again.
//! 10. A presentation has at most `MAX_RECTS_PER_PRESENT` rectangles; one
//!     with more returns `Exhausted`, presents nothing and leaves the
//!     read-back as it was.
//! 11. Every reading of `caps` is the same and holds the formats and the
//!     output size that the backend was built with; a surface in another
//!     format is `Unsupported` and is not created.
//! 12. `resource` returns the bytes of the backend's table; an identity
//!     outside the table and a missing resource injected with
//!     `inject_fault` return `NotFound`, and later calls work as before.
//! 13. Tree updates apply in order to the tree that the backend keeps for
//!     a surface, which `a11y_tree` returns; a first update without the
//!     whole tree returns `InvalidArgument` and creates no tree.
//! 14. The tree of a surface has at most `MAX_A11Y_NODES` nodes; an update
//!     that would pass the bound returns `Exhausted` and leaves the tree as
//!     it was.
//! 15. At most `MAX_INJECTED` injected events wait to be returned; one more
//!     returns `Exhausted`, the waits return exactly the injected ones, and
//!     once they are returned, injecting works again.
//! 16. A disconnection injected with `inject_fault` ends the next wait with
//!     `Disconnected`, and `shutdown` then returns `Ok` without panicking.

use std::collections::BTreeMap;
use std::fmt;

use kosmo_platform::accesskit::{Node, NodeId, Role, TreeId, TreeInfo, TreeUpdate};
use kosmo_platform::{
    AccessibilityTree, FaultPort, MAX_A11Y_NODES, MAX_EVENTS_PER_WAIT, MAX_INJECTED,
    MAX_RECTS_PER_PRESENT, MAX_SURFACES, PixelSize, PlatformCaps, PlatformError, PlatformEvent,
    PlatformTestHooks, ResourceId, Stamp, SurfaceId, SurfaceRequest, SurfaceRole, WaitEnd,
};
use kosmo_types::{
    BufferDesc, DamageRect, Epoch, FocusGen, HidUsage, KeyAction, KeyEvent, KeyMeaning, ModSet,
    MonoTime, PixelFormat,
};

/// The number of requirements.
pub const REQUIREMENTS: usize = 16;

/// The title of each requirement: requirement `n` is `TITLES[n - 1]`.
pub const TITLES: [&str; REQUIREMENTS] = [
    "a wait returns injected events in order, each once and as injected",
    "a wait without events ends at its deadline or is idle, without blocking",
    "a wait returns at most MAX_EVENTS_PER_WAIT events and keeps the rest in order",
    "the clock reads what the driver advanced it to, and only that",
    "surface identities are distinct, and a destroyed one is stale",
    "at most MAX_SURFACES surfaces exist at once",
    "two surfaces keep their presentations, trees and events apart",
    "a buffer has its surface's size and format and reads back as presented",
    "a presented buffer is busy until a wait returns its release",
    "a presentation has at most MAX_RECTS_PER_PRESENT rectangles",
    "the capabilities stay the same, and other formats are unsupported",
    "resources come from the backend's table, or are not found",
    "tree updates apply in order to a tree whose first update is whole",
    "the tree of a surface has at most MAX_A11Y_NODES nodes",
    "at most MAX_INJECTED injected events wait to be returned",
    "an injected disconnection ends the next wait, and shutdown still succeeds",
];

/// What the caller builds each backend under test with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Setup {
    /// The capabilities: [`Setup::FORMATS`] on an output of 64 by 48
    /// pixels.
    pub caps: PlatformCaps,
    /// The resources: two resources with bytes of their own.
    pub resources: BTreeMap<ResourceId, Vec<u8>>,
}

impl Setup {
    /// The accepted pixel formats, the preferred first. A surface in
    /// [`PixelFormat::Rgb565`] is unsupported.
    pub const FORMATS: [PixelFormat; 2] = [PixelFormat::Rgba8Premul, PixelFormat::Xrgb8888];
}

/// A requirement that a backend does not keep, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    /// The number of the requirement.
    pub requirement: usize,
    /// What the check observed.
    pub reason: String,
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let title = title(self.requirement).unwrap_or("no such requirement");
        write!(
            f,
            "requirement {} ({title}) fails: {}",
            self.requirement, self.reason
        )
    }
}

/// The result of every requirement, in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    results: Vec<Result<(), Failure>>,
}

impl Report {
    /// The result of requirement `n`, if there is one.
    pub fn result(&self, n: usize) -> Option<&Result<(), Failure>> {
        self.results.get(n.checked_sub(1)?)
    }

    /// The numbers of the requirements that failed, in order.
    pub fn failing(&self) -> Vec<usize> {
        self.results
            .iter()
            .filter_map(|result| result.as_ref().err().map(|failure| failure.requirement))
            .collect()
    }
}

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, result) in self.results.iter().enumerate() {
            match result {
                Ok(()) => writeln!(
                    f,
                    "requirement {} ({}) holds",
                    index + 1,
                    title(index + 1).unwrap_or_default()
                )?,
                Err(failure) => writeln!(f, "{failure}")?,
            }
        }
        Ok(())
    }
}

/// The title of requirement `n`, if there is one.
pub fn title(n: usize) -> Option<&'static str> {
    TITLES.get(n.checked_sub(1)?).copied()
}

/// Checks requirement `n` on a backend that `new` builds from the suite's
/// setup.
pub fn check<P: PlatformTestHooks>(n: usize, new: impl FnOnce(&Setup) -> P) -> Result<(), Failure> {
    let fail = |reason| Failure {
        requirement: n,
        reason,
    };
    let setup = setup().map_err(fail)?;
    let mut platform = new(&setup);
    let p = &mut platform;
    let outcome = match n {
        1 => injected_events_come_back_in_order(p),
        2 => a_wait_without_events_ends_at_its_deadline_or_is_idle(p),
        3 => a_wait_returns_at_most_the_bound_of_events(p),
        4 => the_clock_reads_what_the_driver_advanced_it_to(p),
        5 => surfaces_are_distinct_and_destroyed_ones_stale(p),
        6 => at_most_the_bound_of_surfaces_exist(p),
        7 => two_surfaces_stay_apart(p),
        8 => a_buffer_reads_back_as_presented(p),
        9 => a_presented_buffer_is_busy_until_released(p),
        10 => a_presentation_has_at_most_the_bound_of_rectangles(p),
        11 => the_capabilities_stay_and_other_formats_are_unsupported(p, &setup),
        12 => resources_come_from_the_table(p, &setup),
        13 => tree_updates_apply_in_order(p),
        14 => a_tree_has_at_most_the_bound_of_nodes(p),
        15 => at_most_the_bound_of_injected_events_wait(p),
        16 => an_injected_disconnection_ends_the_next_wait(p),
        _ => Err("no such requirement".to_string()),
    };
    outcome.map_err(fail)
}

/// Checks every requirement in order, each on a new backend that `new`
/// builds from the suite's setup.
pub fn run<P: PlatformTestHooks>(mut new: impl FnMut(&Setup) -> P) -> Report {
    Report {
        results: (1..=REQUIREMENTS).map(|n| check(n, &mut new)).collect(),
    }
}

/// What a check observed when it fails.
type Outcome = Result<(), String>;

/// The size of the surfaces that the checks create.
const SURFACE: (u32, u32) = (16, 12);

/// The most waits that a check makes to take the events of a backend.
const WAITS: usize = MAX_INJECTED + 16;

const FIRST: ResourceId = ResourceId::new("conformance-first.bin");
const SECOND: ResourceId = ResourceId::new("conformance-second.bin");
const MISSING: ResourceId = ResourceId::new("conformance-missing.bin");

fn setup() -> Result<Setup, String> {
    let output = PixelSize::new(64, 48).ok_or("the output size is not valid")?;
    Ok(Setup {
        caps: PlatformCaps::new(Setup::FORMATS.to_vec(), output),
        resources: BTreeMap::from([(FIRST, b"first".to_vec()), (SECOND, vec![0, 1, 254, 255])]),
    })
}

fn ensure(holds: bool, why: impl FnOnce() -> String) -> Outcome {
    if holds { Ok(()) } else { Err(why()) }
}

fn same<T: PartialEq + fmt::Debug>(what: &str, got: T, want: T) -> Outcome {
    ensure(got == want, || {
        format!("{what}: got {got:?}, want {want:?}")
    })
}

fn call<T>(what: &str, result: Result<T, PlatformError>) -> Result<T, String> {
    result.map_err(|error| format!("{what} failed with {error:?}"))
}

fn wait<P: PlatformTestHooks>(
    p: &mut P,
    deadline: Option<MonoTime>,
) -> Result<(WaitEnd, Vec<PlatformEvent>), String> {
    let mut events = Vec::new();
    let end = call("wait", p.wait(deadline, &mut events))?;
    Ok((end, events))
}

/// Takes events until a wait returns none, and returns them.
fn settle<P: PlatformTestHooks>(p: &mut P) -> Result<Vec<PlatformEvent>, String> {
    let mut taken = Vec::new();
    for _ in 0..WAITS {
        let (_, events) = wait(p, None)?;
        if events.is_empty() {
            return Ok(taken);
        }
        taken.extend(events);
    }
    Err(format!("{WAITS} waits kept returning events"))
}

fn request(format: PixelFormat) -> Result<SurfaceRequest, String> {
    let size = PixelSize::new(SURFACE.0, SURFACE.1).ok_or("the surface size is not valid")?;
    Ok(SurfaceRequest {
        role: SurfaceRole::Main,
        size,
        format,
    })
}

fn create<P: PlatformTestHooks>(p: &mut P, format: PixelFormat) -> Result<SurfaceId, String> {
    call("create_surface", p.create_surface(request(format)?))
}

/// A new surface in the preferred format, with the events of its creation
/// taken.
fn settled_surface<P: PlatformTestHooks>(p: &mut P) -> Result<SurfaceId, String> {
    let surface = create(p, Setup::FORMATS[0])?;
    settle(p)?;
    Ok(surface)
}

fn whole() -> Result<DamageRect, String> {
    DamageRect::new(0, 0, SURFACE.0, SURFACE.1).ok_or_else(|| "no whole rectangle".to_string())
}

/// Fills the surface's buffer with a pattern of `seed`, presents it with
/// `damage` and returns the description and the bytes.
fn draw_and_present<P: PlatformTestHooks>(
    p: &mut P,
    s: SurfaceId,
    seed: u8,
    damage: &[DamageRect],
) -> Result<(BufferDesc, Vec<u8>), String> {
    let buffer = call("buffer", p.buffer(s))?;
    for (index, byte) in buffer.bytes.iter_mut().enumerate() {
        *byte = (index as u8).wrapping_mul(31).wrapping_add(seed);
    }
    let drawn = (buffer.desc, buffer.bytes.to_vec());
    call("present", p.present(s, damage))?;
    Ok(drawn)
}

/// Waits until a wait returns the release of the surface's buffer.
fn release<P: PlatformTestHooks>(p: &mut P, s: SurfaceId) -> Outcome {
    for _ in 0..WAITS {
        let (_, events) = wait(p, None)?;
        if events.contains(&PlatformEvent::Released { surface: s }) {
            return Ok(());
        }
    }
    Err(format!("no wait returned the release of {s:?}"))
}

/// What a surface presented: the description, the bytes and the update
/// rectangles.
type Presented = (BufferDesc, Vec<u8>, Vec<DamageRect>);

/// The surface's read-back as owned values.
fn read_back<P: PlatformTestHooks>(p: &P, s: SurfaceId) -> Result<Option<Presented>, String> {
    let read = call("read_back", p.read_back(s))?;
    Ok(read.map(|read| (read.desc, read.bytes.to_vec(), read.damage.to_vec())))
}

fn tree<P: PlatformTestHooks>(p: &P, s: SurfaceId) -> Result<Option<AccessibilityTree>, String> {
    Ok(call("a11y_tree", p.a11y_tree(s))?.cloned())
}

fn stamp(generation: u64) -> Stamp {
    Stamp::new(Epoch::new(7), FocusGen::new(generation))
}

/// A key event for `surface` whose time is `nanos`, which tells events
/// apart.
fn key(surface: SurfaceId, stamp: Stamp, nanos: u64) -> PlatformEvent {
    PlatformEvent::Key {
        surface,
        stamp,
        event: KeyEvent {
            usage: HidUsage::new(0x07, 0x04),
            meaning: KeyMeaning::Char('a'),
            mods: ModSet::EMPTY,
            action: KeyAction::Press,
            time: MonoTime::from_nanos(nanos),
            synthetic: false,
        },
    }
}

fn keys(surface: SurfaceId, from: u64, count: usize) -> Vec<PlatformEvent> {
    (from..)
        .take(count)
        .map(|nanos| key(surface, stamp(1), nanos))
        .collect()
}

fn inject_all<P: PlatformTestHooks>(p: &mut P, events: &[PlatformEvent]) -> Outcome {
    for event in events {
        call("inject", p.inject(*event))?;
    }
    Ok(())
}

fn node(role: Role, label: &str, children: impl IntoIterator<Item = u64>) -> Node {
    let mut node = Node::new(role);
    node.set_label(label);
    node.set_children(children.into_iter().map(NodeId).collect::<Vec<_>>());
    node
}

/// A whole tree of three nodes: a window with two buttons named
/// `first_name` and `second_name`.
fn named_tree(first_name: &str, second_name: &str) -> TreeUpdate {
    TreeUpdate {
        nodes: vec![
            (NodeId(1), node(Role::Window, "window", [2, 3])),
            (NodeId(2), node(Role::Button, first_name, [])),
            (NodeId(3), node(Role::Button, second_name, [])),
        ],
        tree: Some(TreeInfo::new(NodeId(1))),
        tree_id: TreeId::ROOT,
        focus: NodeId(2),
    }
}

/// A tree of `count` nodes, a root and its children, with the tree
/// information when `whole`.
fn flat_tree(count: usize, whole: bool) -> TreeUpdate {
    let last = count as u64;
    let mut nodes = vec![(NodeId(1), node(Role::Window, "window", 2..=last))];
    nodes.extend((2..=last).map(|id| (NodeId(id), node(Role::Label, "label", []))));
    TreeUpdate {
        nodes,
        tree: whole.then(|| TreeInfo::new(NodeId(1))),
        tree_id: TreeId::ROOT,
        focus: NodeId(1),
    }
}

fn injected_events_come_back_in_order<P: PlatformTestHooks>(p: &mut P) -> Outcome {
    let s = settled_surface(p)?;
    let injected = [
        key(s, stamp(1), 1),
        PlatformEvent::FocusIn {
            surface: s,
            stamp: stamp(2),
        },
        key(s, stamp(2), 2),
    ];
    inject_all(p, &injected)?;
    same(
        "the wait after an old key, a focus in and a key",
        wait(p, None)?,
        (WaitEnd::Events, injected.to_vec()),
    )?;
    same("the events of the next wait", wait(p, None)?.1, vec![])
}

fn a_wait_without_events_ends_at_its_deadline_or_is_idle<P: PlatformTestHooks>(
    p: &mut P,
) -> Outcome {
    settle(p)?;
    call("advance_to", p.advance_to(MonoTime::from_nanos(10)))?;
    let now = p.now();
    same(
        "a wait whose deadline is the clock",
        wait(p, Some(now))?,
        (WaitEnd::Deadline, vec![]),
    )?;
    let earlier = MonoTime::from_nanos(p.now().as_nanos().saturating_sub(1));
    same(
        "a wait whose deadline is before the clock",
        wait(p, Some(earlier))?,
        (WaitEnd::Deadline, vec![]),
    )?;
    let later = p.now().saturating_add_nanos(1);
    same(
        "a wait whose deadline is after the clock",
        wait(p, Some(later))?,
        (WaitEnd::Idle, vec![]),
    )?;
    same(
        "a wait without a deadline",
        wait(p, None)?,
        (WaitEnd::Idle, vec![]),
    )
}

fn a_wait_returns_at_most_the_bound_of_events<P: PlatformTestHooks>(p: &mut P) -> Outcome {
    let s = settled_surface(p)?;
    let at_bound = keys(s, 0, MAX_EVENTS_PER_WAIT);
    inject_all(p, &at_bound)?;
    same(
        "the wait after the bound of events",
        wait(p, None)?,
        (WaitEnd::Events, at_bound),
    )?;
    let over = keys(s, 1_000, MAX_EVENTS_PER_WAIT + 1);
    inject_all(p, &over)?;
    let (first, rest) = over.split_at(MAX_EVENTS_PER_WAIT);
    same(
        "the first wait after one event over the bound",
        wait(p, None)?.1,
        first.to_vec(),
    )?;
    same("the next wait", wait(p, None)?.1, rest.to_vec())
}

fn the_clock_reads_what_the_driver_advanced_it_to<P: PlatformTestHooks>(p: &mut P) -> Outcome {
    same("the clock of a new backend", p.now(), MonoTime::ZERO)?;
    let ten = MonoTime::from_nanos(10);
    call("advance_to", p.advance_to(ten))?;
    same("the clock after advancing", p.now(), ten)?;
    wait(p, None)?;
    wait(p, Some(MonoTime::from_nanos(20)))?;
    same("the clock after two waits", p.now(), ten)?;
    p.caps();
    create(p, Setup::FORMATS[0])?;
    call("resource", p.resource(FIRST))?;
    same("the clock after other port calls", p.now(), ten)?;
    same("advancing to the same time", p.advance_to(ten), Ok(()))?;
    same(
        "advancing to an earlier time",
        p.advance_to(MonoTime::from_nanos(5)),
        Err(PlatformError::InvalidArgument),
    )?;
    same("the clock after that", p.now(), ten)
}

fn surfaces_are_distinct_and_destroyed_ones_stale<P: PlatformTestHooks>(p: &mut P) -> Outcome {
    let a = create(p, Setup::FORMATS[0])?;
    let b = create(p, Setup::FORMATS[0])?;
    ensure(a != b, || format!("two surfaces are both {a:?}"))?;
    call("destroy_surface", p.destroy_surface(a))?;
    let stale = Err(PlatformError::StaleHandle);
    same(
        "destroy_surface of a destroyed surface",
        p.destroy_surface(a),
        stale,
    )?;
    same(
        "buffer of a destroyed surface",
        p.buffer(a).err(),
        Some(PlatformError::StaleHandle),
    )?;
    same(
        "present of a destroyed surface",
        p.present(a, &[whole()?]),
        stale,
    )?;
    same(
        "update of a destroyed surface",
        p.update(a, named_tree("a", "b")),
        stale,
    )?;
    let c = create(p, Setup::FORMATS[0])?;
    ensure(c != a && c != b, || {
        format!("a new surface is {c:?}, which {a:?} or {b:?} was")
    })?;
    for s in [b, c] {
        call("buffer of a live surface", p.buffer(s).map(|_| ()))?;
    }
    Ok(())
}

fn at_most_the_bound_of_surfaces_exist<P: PlatformTestHooks>(p: &mut P) -> Outcome {
    let mut surfaces = Vec::new();
    for _ in 0..MAX_SURFACES {
        surfaces.push(create(p, Setup::FORMATS[0])?);
    }
    for (seed, s) in surfaces.iter().enumerate() {
        draw_and_present(p, *s, seed as u8, &[whole()?])?;
    }
    same(
        "a surface over the bound",
        p.create_surface(request(Setup::FORMATS[0])?),
        Err(PlatformError::Exhausted),
    )?;
    settle(p)?;
    for s in surfaces {
        call("buffer of a surface at the bound", p.buffer(s).map(|_| ()))?;
        call(
            "present of a surface at the bound",
            p.present(s, &[whole()?]),
        )?;
    }
    Ok(())
}

fn two_surfaces_stay_apart<P: PlatformTestHooks>(p: &mut P) -> Outcome {
    let a = create(p, Setup::FORMATS[0])?;
    let b = create(p, Setup::FORMATS[0])?;
    settle(p)?;
    let (desc_a, bytes_a) = draw_and_present(p, a, 1, &[whole()?])?;
    let (desc_b, bytes_b) = draw_and_present(p, b, 2, &[whole()?])?;
    let whole = vec![whole()?];
    same(
        "the read-back of the first surface",
        read_back(p, a)?,
        Some((desc_a, bytes_a, whole.clone())),
    )?;
    same(
        "the read-back of the second surface",
        read_back(p, b)?,
        Some((desc_b, bytes_b, whole)),
    )?;
    call("update", p.update(a, named_tree("a1", "a2")))?;
    let tree_a = tree(p, a)?;
    call("update", p.update(b, named_tree("b1", "b2")))?;
    same(
        "the first tree after an update of the second",
        tree(p, a)?,
        tree_a.clone(),
    )?;
    let mut tree_b = AccessibilityTree::new(named_tree("b1", "b2")).map_err(|e| e.to_string())?;
    let mut renamed = named_tree("b3", "b2");
    renamed.tree = None;
    call("update", p.update(b, renamed.clone()))?;
    tree_b.apply(renamed).map_err(|e| e.to_string())?;
    same("the second tree", tree(p, b)?, Some(tree_b))?;
    same("the first tree after that", tree(p, a)?, tree_a)?;
    settle(p)?;
    let to_b = key(b, stamp(1), 3);
    call("inject", p.inject(to_b))?;
    same(
        "the events of a key for the second surface",
        wait(p, None)?.1,
        vec![to_b],
    )
}

fn a_buffer_reads_back_as_presented<P: PlatformTestHooks>(p: &mut P) -> Outcome {
    let format = Setup::FORMATS[1];
    let s = create(p, format)?;
    settle(p)?;
    let buffer = call("buffer", p.buffer(s))?;
    let desc = buffer.desc;
    same(
        "the buffer's size and format",
        (desc.width(), desc.height(), desc.format()),
        (SURFACE.0, SURFACE.1, format),
    )?;
    same(
        "the buffer's length",
        buffer.bytes.len(),
        desc.byte_len() as usize,
    )?;
    for seed in [5, 6] {
        let (desc, bytes) = draw_and_present(p, s, seed, &[whole()?])?;
        same(
            "the read-back of the latest presentation",
            read_back(p, s)?,
            Some((desc, bytes, vec![whole()?])),
        )?;
        release(p, s)?;
    }
    Ok(())
}

fn a_presented_buffer_is_busy_until_released<P: PlatformTestHooks>(p: &mut P) -> Outcome {
    let s = settled_surface(p)?;
    draw_and_present(p, s, 3, &[whole()?])?;
    same(
        "buffer before the release",
        p.buffer(s).err(),
        Some(PlatformError::Busy),
    )?;
    let (_, events) = wait(p, None)?;
    ensure(
        events.contains(&PlatformEvent::Released { surface: s }),
        || format!("the wait after the presentation returned {events:?}"),
    )?;
    call("buffer after the release", p.buffer(s).map(|_| ()))
}

fn a_presentation_has_at_most_the_bound_of_rectangles<P: PlatformTestHooks>(p: &mut P) -> Outcome {
    let s = settled_surface(p)?;
    let rects: Vec<DamageRect> = (0..=MAX_RECTS_PER_PRESENT as u32)
        .map(|n| DamageRect::new(n % SURFACE.0, n / SURFACE.0, 1, 1))
        .collect::<Option<_>>()
        .ok_or("the rectangles are not valid")?;
    let at_bound = &rects[..MAX_RECTS_PER_PRESENT];
    let (desc, bytes) = draw_and_present(p, s, 4, at_bound)?;
    release(p, s)?;
    call("buffer", p.buffer(s))?.bytes.fill(0x55);
    same(
        "a presentation of one rectangle over the bound",
        p.present(s, &rects),
        Err(PlatformError::Exhausted),
    )?;
    same(
        "the read-back after it",
        read_back(p, s)?,
        Some((desc, bytes, at_bound.to_vec())),
    )?;
    call("buffer after it", p.buffer(s).map(|_| ()))
}

fn the_capabilities_stay_and_other_formats_are_unsupported<P: PlatformTestHooks>(
    p: &mut P,
    setup: &Setup,
) -> Outcome {
    let caps = p.caps();
    same("the capabilities", &caps, &setup.caps)?;
    same("a second reading", p.caps(), caps)?;
    for format in Setup::FORMATS {
        create(p, format)?;
    }
    same(
        "a surface in a format outside the capabilities",
        p.create_surface(request(PixelFormat::Rgb565)?),
        Err(PlatformError::Unsupported),
    )?;
    // The refused request took no surface: the bound still allows the rest.
    for _ in Setup::FORMATS.len()..MAX_SURFACES {
        create(p, Setup::FORMATS[0])?;
    }
    Ok(())
}

fn resources_come_from_the_table<P: PlatformTestHooks>(p: &mut P, setup: &Setup) -> Outcome {
    for (id, bytes) in &setup.resources {
        same(id.name(), p.resource(*id), Ok(bytes.as_slice()))?;
    }
    same(
        "a resource outside the table",
        p.resource(MISSING),
        Err(PlatformError::NotFound),
    )?;
    call(
        "inject_fault",
        p.inject_fault(FaultPort::Resource, PlatformError::NotFound),
    )?;
    same(
        "a resource after an injected missing resource",
        p.resource(FIRST),
        Err(PlatformError::NotFound),
    )?;
    let first = setup.resources.get(&FIRST).ok_or("no first resource")?;
    same(
        "the call after that",
        p.resource(FIRST),
        Ok(first.as_slice()),
    )
}

fn tree_updates_apply_in_order<P: PlatformTestHooks>(p: &mut P) -> Outcome {
    let s = create(p, Setup::FORMATS[0])?;
    let mut partial = named_tree("first", "second");
    partial.tree = None;
    same(
        "a first update without the whole tree",
        p.update(s, partial),
        Err(PlatformError::InvalidArgument),
    )?;
    same("the tree after it", tree(p, s)?, None)?;
    let whole = named_tree("first", "second");
    let before: BTreeMap<NodeId, Node> = whole.nodes.iter().cloned().collect();
    call("update", p.update(s, whole))?;
    let renamed = node(Role::Button, "renamed", []);
    let increment = TreeUpdate {
        nodes: vec![(NodeId(2), renamed.clone())],
        tree: None,
        tree_id: TreeId::ROOT,
        focus: NodeId(2),
    };
    call("update", p.update(s, increment))?;
    let kept = tree(p, s)?.ok_or("no tree after two updates")?;
    let nodes: BTreeMap<NodeId, Node> = kept.nodes().map(|(id, n)| (id, n.clone())).collect();
    let mut want = before;
    want.insert(NodeId(2), renamed);
    same("the nodes after a renaming update", nodes, want)
}

fn a_tree_has_at_most_the_bound_of_nodes<P: PlatformTestHooks>(p: &mut P) -> Outcome {
    let s = create(p, Setup::FORMATS[0])?;
    call(
        "an update to the bound of nodes",
        p.update(s, flat_tree(MAX_A11Y_NODES, true)),
    )?;
    let before = tree(p, s)?;
    same(
        "the nodes of the tree",
        before.as_ref().map(AccessibilityTree::node_count),
        Some(MAX_A11Y_NODES),
    )?;
    let last = MAX_A11Y_NODES as u64 + 1;
    let increment = TreeUpdate {
        nodes: vec![
            (NodeId(1), node(Role::Window, "window", 2..=last)),
            (NodeId(last), node(Role::Label, "label", [])),
        ],
        tree: None,
        tree_id: TreeId::ROOT,
        focus: NodeId(1),
    };
    same(
        "an increment that adds one node over the bound",
        p.update(s, increment),
        Err(PlatformError::Exhausted),
    )?;
    same("the tree after the increment", tree(p, s)?, before.clone())?;
    // The update holds every node again and one more.
    same(
        "an update to one node over the bound",
        p.update(s, flat_tree(MAX_A11Y_NODES + 1, false)),
        Err(PlatformError::Exhausted),
    )?;
    same("the tree after it", tree(p, s)?, before)
}

fn at_most_the_bound_of_injected_events_wait<P: PlatformTestHooks>(p: &mut P) -> Outcome {
    let s = settled_surface(p)?;
    let at_bound = keys(s, 0, MAX_INJECTED);
    inject_all(p, &at_bound)?;
    same(
        "an injected event over the bound",
        p.inject(key(s, stamp(1), MAX_INJECTED as u64)),
        Err(PlatformError::Exhausted),
    )?;
    same("the returned events", settle(p)?, at_bound)?;
    let after = key(s, stamp(1), 2 * MAX_INJECTED as u64);
    call("inject after the queue emptied", p.inject(after))?;
    same("the events after that", settle(p)?, vec![after])
}

fn an_injected_disconnection_ends_the_next_wait<P: PlatformTestHooks>(p: &mut P) -> Outcome {
    settle(p)?;
    call(
        "inject_fault",
        p.inject_fault(FaultPort::Wait, PlatformError::Disconnected),
    )?;
    let mut events = Vec::new();
    same(
        "the wait after an injected disconnection",
        p.wait(None, &mut events),
        Err(PlatformError::Disconnected),
    )?;
    same("shutdown", p.shutdown(), Ok(()))
}
