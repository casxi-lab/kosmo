//! The round function: the event and update passes, then the view pass
//! and the layout pass.

use std::collections::BTreeMap;

use kosmo_platform::PixelSize;
use kosmo_platform::accesskit::TreeUpdate;
use kosmo_render::DisplayList;
use kosmo_text::FontSet;
use kosmo_types::{Damage, Fx, KeyAction, KeyEvent, KeyMeaning, MonoTime, Size};

use crate::JOB_POLL_INTERVAL;
use crate::app::{App, Context, Control, Handling, Message, Out, Phase};
use crate::command::{MINIMAL_KEY_MAP, resolve};
use crate::focus::{Dropped, Event, FocusTable, Stamp, SurfaceId};
use crate::hit::LayoutError;
use crate::layout::{Constraint, Kept, LayoutTree, relayout};
use crate::port::{Effect, JobPort, Receipt};
use crate::record::Record;
use crate::timer::{Requester, TimerTable};
use crate::tokens::Tokens;
use crate::tree::{MAX_DEPTH, RebuildError, Tree, WidgetId};

/// The input of one round, in the order the event pass takes it.
#[non_exhaustive]
pub struct Input<P: JobPort> {
    /// The round's time, the one clock reading that every judgement of the
    /// round uses.
    pub time: MonoTime,
    /// The receipts of the previous round's effects, in the order of the
    /// effects.
    pub receipts: Vec<Receipt<P>>,
    /// The platform events, in the order the platform delivered them.
    pub events: Vec<Event>,
    /// The job reports taken from the execution port.
    pub reports: Vec<P::Report>,
    /// The number of jobs accepted through the execution port whose
    /// terminal state has not been taken, as its last take of reports
    /// returned it.
    pub unfinished_jobs: usize,
}

impl<P: JobPort> Input<P> {
    /// The input of a round at `time` with nothing else.
    pub fn new(time: MonoTime) -> Input<P> {
        Input {
            time,
            receipts: Vec::new(),
            events: Vec::new(),
            reports: Vec::new(),
            unfinished_jobs: 0,
        }
    }
}

/// What the layout pass of a round reads besides the control tree: the
/// font set that lays text out, the number that names it, the table of
/// design tokens, and the size of each surface to lay out.
///
/// The shell gives its own font set, which it builds once before its first
/// round, the table [`TOKENS`](crate::TOKENS) and the size of each
/// surface's latest `Configure`.
#[non_exhaustive]
pub struct LayoutInput<'f> {
    /// The font set that lays text out.
    pub fonts: &'f mut FontSet,
    /// Which font set `fonts` is. Font sets that do not hold the same two
    /// fonts in the same places need different numbers: a round whose
    /// number equals the round before takes the subtrees that did not
    /// change, their text included, from the layout before, which the font
    /// set before laid out. A round whose number differs from the round
    /// before lays every surface out whole. The shell builds one font set
    /// and always gives it the number 0.
    pub fonts_id: u64,
    /// The table of design tokens.
    pub tokens: Tokens,
    /// The surfaces to lay out, each with its size.
    pub sizes: BTreeMap<SurfaceId, PixelSize>,
}

impl<'f> LayoutInput<'f> {
    /// The input that lays out with `fonts`, named `fonts_id`, and
    /// `tokens`, without surfaces.
    pub fn new(fonts: &'f mut FontSet, fonts_id: u64, tokens: Tokens) -> LayoutInput<'f> {
        LayoutInput {
            fonts,
            fonts_id,
            tokens,
            sizes: BTreeMap::new(),
        }
    }

    /// The input with `surface` of `size` to lay out too.
    pub fn surface(mut self, surface: SurfaceId, size: PixelSize) -> LayoutInput<'f> {
        self.sizes.insert(surface, size);
        self
    }
}

/// What the view pass of a round did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewPass {
    /// The round handled no message, so `view` was not called.
    Skipped,
    /// `view` was called and the tree rebuilt from it.
    Rebuilt,
    /// `view` was called, and its tree was rejected; the control tree
    /// stayed as it was.
    Rejected(RebuildError),
}

/// The output of the second step of a round.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq)]
pub struct Output {
    /// The event handling record of the round.
    pub record: Record,
    /// The time by which the next round should run: the earlier of the
    /// earliest timer and, while jobs have not ended, the round's time plus
    /// [`JOB_POLL_INTERVAL`]; `None` when there is neither. A driver that
    /// holds receipts of this round's effects runs the next round without
    /// waiting, whatever the deadline.
    pub next_deadline: Option<MonoTime>,
    /// What the view pass did.
    pub view: ViewPass,
    /// The AccessKit tree updates of the round, one for each surface whose
    /// tree changed, in the order of the surfaces, which the shell pushes
    /// to the platform. Rounds have no accessibility pass yet, so this is
    /// empty.
    pub accessibility: Vec<(SurfaceId, TreeUpdate)>,
    /// The frames of the round: for each surface with update regions, in
    /// the order of the surfaces, its display list and its damage region,
    /// which the shell presents. Rounds do not run the drawing pass yet,
    /// so this is empty.
    pub frames: Vec<(SurfaceId, DisplayList, Damage)>,
}

/// The counts of what the runtime dropped or refused since it was built.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    /// Key events dropped because their epoch differed.
    pub dropped_epoch: u64,
    /// Key events dropped because their focus generation differed.
    pub dropped_generation: u64,
    /// Key events dropped because their surface did not hold focus.
    pub dropped_no_focus: u64,
    /// Due timers dropped because their control no longer existed.
    pub dropped_missing: u64,
    /// Timer requests refused because the timer table was full.
    pub timers_refused: u64,
}

/// The framework's state between rounds: the control tree, the focus of
/// each surface, the timer table, the counts and the latest layout of each
/// surface.
///
/// The runtime never reads a clock, never waits and never calls a port; it
/// creates no threads. Every surface has one region, and every key event
/// that passes the focus check is dispatched in the one control tree.
pub struct Runtime<A: App> {
    tree: Tree<A>,
    focus: FocusTable,
    timers: TimerTable,
    counts: Counts,
    rounds: u64,
    built: bool,
    layouts: BTreeMap<SurfaceId, Kept>,
}

impl<A: App> Default for Runtime<A> {
    fn default() -> Self {
        Runtime {
            tree: Tree::new(),
            focus: FocusTable::default(),
            timers: TimerTable::default(),
            counts: Counts::default(),
            rounds: 0,
            built: false,
            layouts: BTreeMap::new(),
        }
    }
}

/// A round between its two steps: the effects of the event and update
/// passes are ready, and [`Round::finish`] runs the view pass.
#[must_use = "a round ends with its second step, `finish`"]
pub struct Round<'r, A: App> {
    runtime: &'r mut Runtime<A>,
    effects: Vec<Effect<A::Port>>,
    record: Record,
    handled: bool,
    time: MonoTime,
    unfinished_jobs: usize,
}

impl<A: App> Round<'_, A> {
    /// The effects of the round, in the order they were produced.
    pub fn effects(&self) -> &[Effect<A::Port>] {
        &self.effects
    }

    /// Takes the effects of the round, to hand them to the execution port.
    pub fn take_effects(&mut self) -> Vec<Effect<A::Port>> {
        std::mem::take(&mut self.effects)
    }

    /// The second step without a layout pass: calls `view` if the round
    /// handled a message, or if the tree has not been built yet, and
    /// rebuilds the tree from it; then gives the next deadline and the
    /// round's record. The layouts of the surfaces stay as they were.
    pub fn finish(self, app: &A) -> Output {
        self.end(app, None)
    }

    /// The second step with the layout pass: the view pass of
    /// [`Round::finish`], then the layout pass, which lays out each surface
    /// of `input` in the order of the surfaces, with the size of the surface
    /// as the tight constraint of the root, the tokens and the font set of
    /// `input`; then the next deadline and the round's record.
    ///
    /// The runtime keeps each surface's layout for the next round, which
    /// lays out only dirty subtrees: a control is dirty when it is new,
    /// when its layout node differs other than in a scroll request, or when
    /// its children differ. A subtree without a dirty control under the
    /// constraint of the round before keeps its layout; a box whose control
    /// is not dirty keeps its size and its children's positions while its
    /// children come out with the sizes and baselines of the round before,
    /// so the work goes up from a dirty control only while sizes or
    /// baselines change, and stops at the latest at a box under a tight
    /// constraint, a layout boundary, whose baseline stays. A change of the
    /// request of a scroll container alone moves only the container's
    /// content. Another size, other tokens or another font set lays the
    /// surface out whole. The result is that of [`layout`](crate::layout())
    /// byte for byte.
    ///
    /// After the layout pass, each scroll container learns its offset
    /// through [`Control::scrolled`]. Then the kept layouts take each
    /// container's offset from the request that its node gives after that,
    /// brought into the range of its content, without laying anything out,
    /// so each is the layout of the tree as the round leaves it, unless
    /// `scrolled` changes a node in more than its request or to a request
    /// beyond its bounds. A surface that `input` does not list loses its
    /// layout, and so does a surface whose layout fails; the record names
    /// the error.
    pub fn finish_with(self, app: &A, input: LayoutInput<'_>) -> Output {
        self.end(app, Some(input))
    }

    /// The second step, with the layout pass when there is `input`.
    fn end(self, app: &A, input: Option<LayoutInput<'_>>) -> Output {
        let Round {
            runtime,
            mut record,
            handled,
            time,
            unfinished_jobs,
            ..
        } = self;
        let view = if handled || !runtime.built {
            match runtime.tree.rebuild(&app.view()) {
                Ok(()) => {
                    runtime.built = true;
                    record.push(format!("view rebuilt controls {}", runtime.tree.len()));
                    ViewPass::Rebuilt
                }
                Err(error) => {
                    record.push(format!("view rejected {error}"));
                    ViewPass::Rejected(error)
                }
            }
        } else {
            record.push("view skipped".to_string());
            ViewPass::Skipped
        };
        if let Some(input) = input {
            runtime.lay_out(input, &mut record);
        }
        let polling = (unfinished_jobs > 0).then(|| time.saturating_add_nanos(JOB_POLL_INTERVAL));
        let next_deadline = match (runtime.timers.next_deadline(), polling) {
            (Some(timer), Some(polling)) => Some(timer.min(polling)),
            (timer, polling) => timer.or(polling),
        };
        record.push(match next_deadline {
            Some(deadline) => format!("deadline {}", deadline.as_nanos()),
            None => "deadline none".to_string(),
        });
        Output {
            record,
            next_deadline,
            view,
            accessibility: Vec::new(),
            frames: Vec::new(),
        }
    }
}

/// The messages of a round's event pass, in their two groups.
struct Queue<A: App> {
    /// Receipts, then the messages of key events.
    first: Vec<Message<A>>,
    /// The messages of other platform events.
    platform: Vec<Message<A>>,
}

impl<A: App> Runtime<A> {
    /// A runtime with an empty control tree; its first round builds the
    /// tree.
    pub fn new() -> Runtime<A> {
        Runtime::default()
    }

    /// The control tree.
    pub fn tree(&self) -> &Tree<A> {
        &self.tree
    }

    /// The layout tree of `surface` from the latest layout pass that laid it
    /// out, or `None` when none did or the latest failed. Its
    /// [`LayoutTree::result`] is what hit testing reads.
    pub fn layout(&self, surface: SurfaceId) -> Option<&LayoutTree> {
        self.layouts.get(&surface).map(Kept::tree)
    }

    /// What the runtime dropped or refused so far.
    pub fn counts(&self) -> Counts {
        self.counts
    }

    /// The focus stamp of the latest focus in of `surface` that a round
    /// took, and whether the surface holds focus, as the focus check reads
    /// them; `None` before the surface's first focus in.
    pub fn focus(&self, surface: SurfaceId) -> Option<(Stamp, bool)> {
        self.focus.get(surface)
    }

    /// The first step of a round: the event pass and the update pass.
    ///
    /// The event pass queues the receipts, checks and dispatches the key
    /// events, hands focus changes to the region's root, queues the job
    /// reports and the due timers; the update pass calls `update` once for
    /// each message, in order. Platform events are taken in their order,
    /// so a focus change applies to the key events after it, and the
    /// messages of key events come before those of other platform events.
    /// The effects of handlers and of `update` are in the returned round;
    /// timer requests are registered once `update` has run.
    pub fn round(&mut self, app: &mut A, input: Input<A::Port>) -> Round<'_, A> {
        self.rounds = self.rounds.saturating_add(1);
        let mut record = Record::default();
        record.push(format!(
            "round {} time {}",
            self.rounds,
            input.time.as_nanos()
        ));
        let mut out = Out::new();
        let mut queue = Queue {
            first: Vec::new(),
            platform: Vec::new(),
        };

        for receipt in input.receipts {
            match receipt {
                Receipt::Accepted(submission) => {
                    record.push(format!("receipt accepted {submission:?}"));
                    queue.first.push(Message::Accepted(submission));
                }
                Receipt::Rejected(rejection) => {
                    record.push(format!("receipt rejected {rejection:?}"));
                    queue.first.push(Message::Rejected(rejection));
                }
                Receipt::Cancelled(answer) => {
                    record.push(format!("cancel answered {answer:?}"));
                }
            }
        }
        for event in input.events {
            self.event(app, event, &mut queue, &mut out, &mut record);
        }
        let mut messages = queue.first;
        messages.append(&mut queue.platform);
        for report in input.reports {
            record.push(format!("report {report:?}"));
            messages.push(Message::Report(report));
        }
        self.due_timers(input.time, &mut messages, &mut out, &mut record);

        let handled = !messages.is_empty();
        for message in messages {
            app.update(message, &mut out);
        }

        for effect in &out.effects {
            record.push(match effect {
                Effect::Submit(request) => format!("effect submit {request:?}"),
                Effect::Cancel(submission) => format!("effect cancel {submission:?}"),
            });
        }
        for request in std::mem::take(&mut out.timers) {
            let line = format!(
                "{} deadline {} label {}",
                request.requester,
                request.deadline.as_nanos(),
                request.label.get()
            );
            match self.timers.register(request) {
                Ok(()) => record.push(format!("timer set {line}")),
                Err(_) => {
                    self.counts.timers_refused = self.counts.timers_refused.saturating_add(1);
                    record.push(format!("timer refused full {line}"));
                }
            }
        }
        Round {
            runtime: self,
            effects: out.effects,
            record,
            handled,
            time: input.time,
            unfinished_jobs: input.unfinished_jobs,
        }
    }

    /// The layout pass: lays out each surface of `input` against its
    /// layout of the round before, keeps the layouts, records each, tells
    /// each scroll container its offset, and lets the kept layouts follow
    /// the requests that the controls wrote back.
    fn lay_out(&mut self, input: LayoutInput<'_>, record: &mut Record) {
        let LayoutInput {
            fonts,
            fonts_id,
            tokens,
            sizes,
        } = input;
        let mut before = std::mem::take(&mut self.layouts);
        for (surface, size) in sizes {
            let kept = before.remove(&surface);
            let laid = surface_size(size)
                .ok_or(LayoutError::InputOutOfRange)
                .and_then(|size| {
                    let constraint = Constraint::tight(size);
                    relayout(
                        &self.tree,
                        constraint,
                        &tokens,
                        fonts,
                        fonts_id,
                        kept.as_ref(),
                    )
                });
            match laid {
                Ok((kept, work)) => {
                    record.push(format!(
                        "layout surface {} boxes {} laid {} texts {}",
                        surface.get(),
                        kept.tree().boxes().len(),
                        work.laid,
                        work.texts
                    ));
                    self.layouts.insert(surface, kept);
                }
                Err(error) => record.push(format!(
                    "layout surface {} failed {}",
                    surface.get(),
                    describe_layout_error(error)
                )),
            }
        }
        let mut offsets: BTreeMap<WidgetId, Fx> = BTreeMap::new();
        for (widget, offset) in self.layouts.values().flat_map(Kept::offsets) {
            offsets
                .entry(widget)
                .and_modify(|smallest| *smallest = (*smallest).min(offset))
                .or_insert(offset);
        }
        if offsets.is_empty() {
            return;
        }
        for (widget, offset) in offsets {
            if let Some(node) = self.tree.node_mut(widget) {
                node.control.scrolled(offset, &mut node.state);
            }
        }
        // A surface whose range was larger laid a container out with a
        // larger offset than the one written back; its kept layout follows
        // the request now, without laying anything out, so that it is the
        // layout of the tree as the round leaves it.
        for kept in self.layouts.values_mut() {
            kept.follow_requests(&self.tree);
        }
    }

    /// Takes one platform event.
    fn event(
        &mut self,
        app: &A,
        event: Event,
        queue: &mut Queue<A>,
        out: &mut Out<A>,
        record: &mut Record,
    ) {
        match event {
            Event::Key {
                surface,
                stamp,
                event,
            } => {
                let line = describe(surface, stamp, &event);
                let outcome = match self.focus.check(surface, stamp) {
                    Ok(()) => self.dispatch(app, &event, &mut queue.first, out),
                    Err(reason) => {
                        let count = match reason {
                            Dropped::Epoch => &mut self.counts.dropped_epoch,
                            Dropped::Generation => &mut self.counts.dropped_generation,
                            Dropped::NoFocus => &mut self.counts.dropped_no_focus,
                        };
                        *count = count.saturating_add(1);
                        format!("dropped {reason}")
                    }
                };
                record.push(format!("{line} {outcome}"));
            }
            Event::FocusIn { surface, stamp } => {
                self.focus.enter(surface, stamp);
                let reached = self.focus_to_region(true, &mut queue.platform, out);
                record.push(format!(
                    "focus-in {}{}",
                    describe_stamp(surface, stamp),
                    no_region(reached)
                ));
            }
            Event::FocusOut { surface, stamp } => {
                if self.focus.leave(surface, stamp) {
                    let reached = self.focus_to_region(false, &mut queue.platform, out);
                    record.push(format!(
                        "focus-out {}{}",
                        describe_stamp(surface, stamp),
                        no_region(reached)
                    ));
                } else {
                    record.push(format!(
                        "focus-out ignored {}",
                        describe_stamp(surface, stamp)
                    ));
                }
            }
        }
    }

    /// Resolves a key event to a command along the focus chain and
    /// dispatches it, and describes its outcome.
    fn dispatch(
        &mut self,
        app: &A,
        event: &KeyEvent,
        messages: &mut Vec<Message<A>>,
        out: &mut Out<A>,
    ) -> String {
        let chain = self.chain();
        let tables = chain
            .iter()
            .filter_map(|&id| self.tree.control(id))
            .map(|control| control.bindings())
            .chain([app.bindings(), MINIMAL_KEY_MAP]);
        let Some(command) = resolve(event, tables) else {
            return "unhandled".to_string();
        };
        for (index, &id) in chain.iter().enumerate() {
            let phase = if index == 0 {
                Phase::Target
            } else {
                Phase::Bubble
            };
            let handled = self.with_control(id, messages, out, |control, cx| {
                control.handle(phase, &command, cx)
            });
            if handled == Some(Handling::Handled) {
                return format!("handled {command} by control {id}");
            }
        }
        match app.command(&command) {
            Some(message) => {
                messages.push(Message::Own(message));
                format!("handled {command} by app")
            }
            None => format!("unhandled {command}"),
        }
    }

    /// The focus chain: the focused control and its ancestors up to the
    /// region's root. The view check keeps the focused control at or below
    /// the region's root, so the chain never holds a control outside the
    /// region; it is empty without a focused control.
    fn chain(&self) -> Vec<WidgetId> {
        let Some(region) = self.tree.region() else {
            return Vec::new();
        };
        let mut chain = Vec::new();
        let mut next = self.tree.focused();
        while let Some(id) = next {
            if chain.len() >= MAX_DEPTH {
                break;
            }
            chain.push(id);
            if id == region {
                return chain;
            }
            next = self.tree.parent(id);
        }
        Vec::new()
    }

    /// Hands a focus change to the region's root, and says whether one
    /// received it: before the first view has been applied, there is none.
    fn focus_to_region(
        &mut self,
        holds: bool,
        messages: &mut Vec<Message<A>>,
        out: &mut Out<A>,
    ) -> bool {
        let Some(region) = self.tree.region() else {
            return false;
        };
        self.with_control(region, messages, out, |control, cx| {
            control.focus(holds, cx);
        })
        .is_some()
    }

    /// Takes the due timers: the application's become messages, a
    /// control's go to the control, and those of a control that no longer
    /// exists are dropped and counted.
    fn due_timers(
        &mut self,
        now: MonoTime,
        messages: &mut Vec<Message<A>>,
        out: &mut Out<A>,
        record: &mut Record,
    ) {
        for timer in self.timers.take_due(now) {
            let line = format!(
                "{} deadline {} label {}",
                timer.requester,
                timer.deadline.as_nanos(),
                timer.label.get()
            );
            match timer.requester {
                Requester::App => {
                    record.push(format!("timer due {line}"));
                    messages.push(Message::Timer(timer.label));
                }
                Requester::Control(id) => {
                    let delivered = self.with_control(id, messages, out, |control, cx| {
                        control.timer(timer.label, cx);
                    });
                    if delivered.is_some() {
                        record.push(format!("timer due {line}"));
                    } else {
                        self.counts.dropped_missing = self.counts.dropped_missing.saturating_add(1);
                        record.push(format!("timer dropped missing {line}"));
                    }
                }
            }
        }
    }

    /// Runs `f` with the control `id` and its context, or returns `None`
    /// when the control does not exist.
    fn with_control<R>(
        &mut self,
        id: WidgetId,
        messages: &mut Vec<Message<A>>,
        out: &mut Out<A>,
        f: impl FnOnce(&A::Control, &mut Context<'_, A, crate::app::StateOf<A>>) -> R,
    ) -> Option<R> {
        let node = self.tree.node_mut(id)?;
        let mut cx = Context {
            id,
            state: &mut node.state,
            messages,
            out,
        };
        Some(f(&node.control, &mut cx))
    }
}

/// The record form of a key event with its surface and stamp.
fn describe(surface: SurfaceId, stamp: Stamp, event: &KeyEvent) -> String {
    let meaning = match event.meaning {
        KeyMeaning::Char(c) => format!("char {c:?}"),
        KeyMeaning::Named(key) => format!("named {}", key.number()),
    };
    let action = match event.action {
        KeyAction::Press => "press",
        KeyAction::Release => "release",
        KeyAction::Repeat => "repeat",
        KeyAction::LongPress => "long-press",
        _ => "other",
    };
    let synthetic = if event.synthetic { " synthetic" } else { "" };
    format!(
        "key {} {meaning} {action}{synthetic} mods {:#06x} time {}",
        describe_stamp(surface, stamp),
        event.mods.bits(),
        event.time.as_nanos()
    )
}

/// The end of the record line of a focus change: ` no-region` when no
/// region's root received it.
fn no_region(reached: bool) -> &'static str {
    if reached { "" } else { " no-region" }
}

/// The size of a surface in `Fx`; `None` for a side that does not fit,
/// which a surface's size never has.
fn surface_size(size: PixelSize) -> Option<Size> {
    let side = |pixels: u32| i32::try_from(pixels).ok().and_then(Fx::from_pixels);
    Size::new(side(size.width())?, side(size.height())?)
}

/// The record form of a layout error.
fn describe_layout_error(error: LayoutError) -> &'static str {
    match error {
        LayoutError::Order => "order",
        LayoutError::TooManyBoxes => "too-many-boxes",
        LayoutError::Overflow => "overflow",
        LayoutError::InputOutOfRange => "input-out-of-range",
    }
}

/// The record form of a surface and a stamp.
fn describe_stamp(surface: SurfaceId, stamp: Stamp) -> String {
    format!(
        "surface {} epoch {} generation {}",
        surface.get(),
        stamp.epoch.get(),
        stamp.generation.get()
    )
}

#[cfg(test)]
mod tests;
