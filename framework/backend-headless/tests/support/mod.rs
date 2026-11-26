//! Defective backends: the headless backend wrapped so that it breaks
//! exactly one requirement of the conformance suite, which the suite must
//! then report alone.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use kosmo_backend_headless::HeadlessBackend;
use kosmo_platform::accesskit::{NodeId, TreeId, TreeUpdate};
use kosmo_platform::{
    AccessibilitySink, AccessibilityTree, Capabilities, Clock, EventSource, FaultPort,
    MAX_EVENTS_PER_WAIT, MAX_RECTS_PER_PRESENT, PixelBufferMut, Platform, PlatformCaps,
    PlatformError, PlatformEvent, PlatformTestHooks, Presenter, ReadBack, ResourceId,
    ResourceProvider, SurfaceId, SurfacePort, SurfaceRequest, WaitEnd,
};
use kosmo_testkit::conformance::Setup;
use kosmo_types::{BufferDesc, DamageRect, MonoTime};

/// The headless backend built from the suite's setup.
pub fn headless(setup: &Setup) -> HeadlessBackend {
    HeadlessBackend::new(setup.caps.clone(), setup.resources.clone())
}

/// The headless backend with the defect that breaks requirement `defect`
/// of the suite:
///
/// 1. a wait that returns both key and focus events returns them in reverse
///    order;
/// 2. a wait that reaches its deadline is idle;
/// 3. a wait after one that returned the bound of events returns one event
///    more;
/// 4. a wait advances the clock by 1 ns;
/// 5. presenting a destroyed surface succeeds;
/// 6. creating a surface over the bound returns a made-up identity;
/// 7. every read-back returns the surface that presented last;
/// 8. a read-back returns the presentation before the last one, once there
///    is one;
/// 9. a busy buffer is handed out anyway, as a separate copy;
/// 10. a presentation of more rectangles than the bound presents the first
///     ones;
/// 11. a surface in an unsupported format is created in the preferred one;
/// 12. a missing resource is empty;
/// 13. the tree keeps only the nodes of the last update;
/// 14. an update over the bound of nodes still renames the root;
/// 15. injected events over the bound are queued anyway;
/// 16. an injected disconnection is dropped.
pub struct Defective {
    inner: HeadlessBackend,
    defect: usize,
    /// The events over the bound of a wait, for defect 3.
    spill: VecDeque<PlatformEvent>,
    /// The surface that presented last, for defect 7.
    last_presented: Option<SurfaceId>,
    /// The read-back before the last presentation, for defect 8.
    previous: BTreeMap<SurfaceId, (BufferDesc, Vec<u8>, Vec<DamageRect>)>,
    /// A copy of each surface's last handed-out buffer, for defect 9.
    copies: BTreeMap<SurfaceId, (BufferDesc, Vec<u8>)>,
    /// The surfaces whose tree exists, for defect 13.
    trees: BTreeSet<SurfaceId>,
    /// The injected events over the bound, for defect 15.
    overflow: VecDeque<PlatformEvent>,
}

impl Defective {
    pub fn new(inner: HeadlessBackend, defect: usize) -> Defective {
        Defective {
            inner,
            defect,
            spill: VecDeque::new(),
            last_presented: None,
            previous: BTreeMap::new(),
            copies: BTreeMap::new(),
            trees: BTreeSet::new(),
            overflow: VecDeque::new(),
        }
    }

    /// Defect 13: the update with the root's children cut down to the
    /// nodes of the update.
    fn last_nodes_only(&self, s: SurfaceId, mut update: TreeUpdate) -> TreeUpdate {
        let kept = self.inner.a11y_tree(s).ok().flatten();
        let Some(root) = update
            .tree
            .as_ref()
            .map(|info| info.root)
            .or_else(|| kept.map(AccessibilityTree::root))
        else {
            return update;
        };
        let children: Vec<NodeId> = update
            .nodes
            .iter()
            .map(|(id, _)| *id)
            .filter(|id| *id != root)
            .collect();
        if let Some((_, node)) = update.nodes.iter_mut().find(|(id, _)| *id == root) {
            node.set_children(children);
        } else if let Some(mut node) = kept.and_then(|tree| tree.node(root)).cloned() {
            node.set_children(children);
            update.nodes.push((root, node));
        }
        update
    }
}

impl Platform for Defective {
    fn shutdown(&mut self) -> Result<(), PlatformError> {
        self.inner.shutdown()
    }
}

impl EventSource for Defective {
    fn wait(
        &mut self,
        deadline: Option<MonoTime>,
        out: &mut Vec<PlatformEvent>,
    ) -> Result<WaitEnd, PlatformError> {
        match self.defect {
            1 => {
                let start = out.len();
                let end = self.inner.wait(deadline, out)?;
                let returned = &mut out[start..];
                let has = |wanted: fn(&PlatformEvent) -> bool| returned.iter().any(wanted);
                let keys = has(|event| matches!(event, PlatformEvent::Key { .. }));
                let focus = has(|event| {
                    matches!(
                        event,
                        PlatformEvent::FocusIn { .. } | PlatformEvent::FocusOut { .. }
                    )
                });
                if keys && focus {
                    returned.reverse();
                }
                Ok(end)
            }
            2 => match self.inner.wait(deadline, out)? {
                WaitEnd::Deadline => Ok(WaitEnd::Idle),
                end => Ok(end),
            },
            3 => {
                let mut events: Vec<PlatformEvent> = self.spill.drain(..).collect();
                let mut end = self.inner.wait(deadline, &mut events)?;
                if events.len() >= MAX_EVENTS_PER_WAIT {
                    self.inner.wait(deadline, &mut events)?;
                }
                let kept = events.len().min(MAX_EVENTS_PER_WAIT + 1);
                self.spill.extend(events.drain(kept..));
                if !events.is_empty() {
                    end = WaitEnd::Events;
                }
                out.extend(events);
                Ok(end)
            }
            4 => {
                let end = self.inner.wait(deadline, out)?;
                let later = self.inner.now().saturating_add_nanos(1);
                self.inner.advance_to(later)?;
                Ok(end)
            }
            15 => {
                let start = out.len();
                let mut end = self.inner.wait(deadline, out)?;
                while out.len() - start < MAX_EVENTS_PER_WAIT {
                    let Some(event) = self.overflow.pop_front() else {
                        break;
                    };
                    out.push(event);
                    end = WaitEnd::Events;
                }
                Ok(end)
            }
            _ => self.inner.wait(deadline, out),
        }
    }
}

impl Clock for Defective {
    fn now(&self) -> MonoTime {
        self.inner.now()
    }
}

impl SurfacePort for Defective {
    fn create_surface(&mut self, req: SurfaceRequest) -> Result<SurfaceId, PlatformError> {
        match (self.defect, self.inner.create_surface(req)) {
            (6, Err(PlatformError::Exhausted)) => Ok(SurfaceId::new(u32::MAX)),
            (11, Err(PlatformError::Unsupported)) => {
                let caps = self.inner.caps();
                let format = *caps.formats().first().ok_or(PlatformError::Unsupported)?;
                self.inner.create_surface(SurfaceRequest { format, ..req })
            }
            (_, result) => result,
        }
    }

    fn destroy_surface(&mut self, s: SurfaceId) -> Result<(), PlatformError> {
        self.inner.destroy_surface(s)
    }
}

impl Presenter for Defective {
    fn buffer(&mut self, s: SurfaceId) -> Result<PixelBufferMut<'_>, PlatformError> {
        if self.defect != 9 {
            return self.inner.buffer(s);
        }
        match self.inner.buffer(s) {
            Ok(buffer) => {
                self.copies.insert(s, (buffer.desc, buffer.bytes.to_vec()));
                Ok(buffer)
            }
            Err(PlatformError::Busy) => {
                let (desc, bytes) = self.copies.get_mut(&s).ok_or(PlatformError::Busy)?;
                Ok(PixelBufferMut { desc: *desc, bytes })
            }
            Err(error) => Err(error),
        }
    }

    fn present(&mut self, s: SurfaceId, damage: &[DamageRect]) -> Result<(), PlatformError> {
        match self.defect {
            5 => match self.inner.present(s, damage) {
                Err(PlatformError::StaleHandle) => Ok(()),
                result => result,
            },
            7 => {
                self.inner.present(s, damage)?;
                self.last_presented = Some(s);
                Ok(())
            }
            8 => {
                let before = self
                    .inner
                    .read_back(s)?
                    .map(|read| (read.desc, read.bytes.to_vec(), read.damage.to_vec()));
                self.inner.present(s, damage)?;
                if let Some(before) = before {
                    self.previous.insert(s, before);
                }
                Ok(())
            }
            10 => self
                .inner
                .present(s, &damage[..damage.len().min(MAX_RECTS_PER_PRESENT)]),
            _ => self.inner.present(s, damage),
        }
    }
}

impl Capabilities for Defective {
    fn caps(&self) -> PlatformCaps {
        self.inner.caps()
    }
}

impl ResourceProvider for Defective {
    fn resource(&self, id: ResourceId) -> Result<&[u8], PlatformError> {
        match (self.defect, self.inner.resource(id)) {
            (12, Err(PlatformError::NotFound)) => Ok(&[]),
            (_, result) => result,
        }
    }
}

impl AccessibilitySink for Defective {
    fn update(&mut self, s: SurfaceId, update: TreeUpdate) -> Result<(), PlatformError> {
        match self.defect {
            13 => {
                let update = if self.trees.contains(&s) {
                    self.last_nodes_only(s, update)
                } else {
                    update
                };
                self.inner.update(s, update)?;
                self.trees.insert(s);
                Ok(())
            }
            14 => match self.inner.update(s, update) {
                Err(PlatformError::Exhausted) => {
                    let tree = self.inner.a11y_tree(s)?;
                    let root = tree.map(|tree| (tree.root(), tree.node(tree.root()).cloned()));
                    if let Some((root, Some(mut node))) = root {
                        node.set_label("changed");
                        self.inner.update(
                            s,
                            TreeUpdate {
                                nodes: vec![(root, node)],
                                tree: None,
                                tree_id: TreeId::ROOT,
                                focus: root,
                            },
                        )?;
                    }
                    Err(PlatformError::Exhausted)
                }
                result => result,
            },
            _ => self.inner.update(s, update),
        }
    }
}

impl PlatformTestHooks for Defective {
    fn inject(&mut self, e: PlatformEvent) -> Result<(), PlatformError> {
        match (self.defect, PlatformTestHooks::inject(&mut self.inner, e)) {
            (15, Err(PlatformError::Exhausted)) => {
                self.overflow.push_back(e);
                Ok(())
            }
            (_, result) => result,
        }
    }

    fn advance_to(&mut self, time: MonoTime) -> Result<(), PlatformError> {
        PlatformTestHooks::advance_to(&mut self.inner, time)
    }

    fn read_back(&self, s: SurfaceId) -> Result<Option<ReadBack<'_>>, PlatformError> {
        match self.defect {
            7 => self.inner.read_back(self.last_presented.unwrap_or(s)),
            8 => match self.previous.get(&s) {
                Some((desc, bytes, damage)) => Ok(Some(ReadBack {
                    desc: *desc,
                    bytes,
                    damage,
                })),
                None => self.inner.read_back(s),
            },
            _ => self.inner.read_back(s),
        }
    }

    fn inject_fault(&mut self, port: FaultPort, error: PlatformError) -> Result<(), PlatformError> {
        if self.defect == 16 && port == FaultPort::Wait && error == PlatformError::Disconnected {
            return Ok(());
        }
        self.inner.inject_fault(port, error)
    }

    fn a11y_tree(&self, s: SurfaceId) -> Result<Option<&AccessibilityTree>, PlatformError> {
        self.inner.a11y_tree(s)
    }
}
