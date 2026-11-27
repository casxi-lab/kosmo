//! The headless backend of kosmo's platform interface: a backend without a
//! display, whose clock and input come only from the driver that builds it.
//! The script runner and the tests of the framework run on it.
//!
//! - It has no threads, reads no files and reads no real clock. Its clock
//!   starts at 0, and only the driver advances it, through
//!   [`HeadlessBackend::advance_to`] or the test hooks; waiting never
//!   advances it.
//! - [`EventSource::wait`] never blocks: it returns the queued events,
//!   otherwise [`WaitEnd::Deadline`] when the deadline is not later than
//!   the clock, otherwise [`WaitEnd::Idle`]. Events come only from the
//!   driver, through [`HeadlessBackend::inject`] or the test hooks, and
//!   from the backend itself.
//! - It is built with its capabilities, the accepted pixel formats and the
//!   output size, and with the table of its resources, whose bytes
//!   [`ResourceProvider::resource`] returns.
//! - It numbers its surfaces from 1 in the order it creates them and never
//!   gives a number twice, so the first surface of a new backend is
//!   surface 1.
//! - Once a surface of the role [`SurfaceRole::Main`] exists, the backend
//!   queues [`PlatformEvent::Configure`] with its size and
//!   [`PlatformEvent::FocusIn`] with the epoch [`HEADLESS_EPOCH`] and a
//!   focus generation that counts from 1, so input needs no focus of its
//!   own. Each surface of the role `Main` gets its own `FocusIn`; the
//!   backend does not move focus between them. Destroying a surface drops
//!   the events that the backend queued for it; injected events stay.
//! - Each surface has one buffer. [`Presenter::present`] keeps a copy of
//!   the buffer's bytes and the update rectangles as what the surface
//!   presented, which the test hooks read back, and queues
//!   [`PlatformEvent::Released`]; until a wait returns it,
//!   [`Presenter::buffer`] and [`Presenter::present`] return
//!   [`PlatformError::Busy`].
//! - It keeps the accessibility tree of each surface as an
//!   [`AccessibilityTree`], built from the updates in order.
//! - After [`Platform::shutdown`] every other port method that can fail
//!   returns [`PlatformError::Disconnected`], except the test hooks'
//!   `read_back` and `a11y_tree`, which find no surface and return
//!   [`PlatformError::StaleHandle`]; `shutdown` itself returns `Ok` again.
//!
//! With the feature `test-hooks`, which only test builds turn on, it
//! implements [`PlatformTestHooks`](kosmo_platform::PlatformTestHooks): a
//! fault injected at a port makes that port's next call return the error
//! once, and a second fault at a port that has one waiting returns
//! [`PlatformError::Busy`]. With the feature, the backend is not `Sync`: it
//! keeps the fault of [`ResourceProvider::resource`], which takes `&self`,
//! in a `Cell`.
//!
//! ```
//! use std::collections::BTreeMap;
//!
//! use kosmo_backend_headless::HeadlessBackend;
//! use kosmo_platform::{
//!     EventSource, PixelSize, PlatformCaps, PlatformEvent, Presenter, SurfacePort,
//!     SurfaceRequest, SurfaceRole, WaitEnd,
//! };
//! use kosmo_types::{DamageRect, MonoTime, PixelFormat};
//!
//! let size = PixelSize::new(4, 2).unwrap();
//! let caps = PlatformCaps::new(vec![PixelFormat::Rgba8Premul], size);
//! let mut backend = HeadlessBackend::new(caps, BTreeMap::new());
//! let request = SurfaceRequest { role: SurfaceRole::Main, size, format: PixelFormat::Rgba8Premul };
//! let surface = backend.create_surface(request).unwrap();
//!
//! let mut events = Vec::new();
//! assert_eq!(backend.wait(None, &mut events), Ok(WaitEnd::Events));
//! assert!(matches!(events[0], PlatformEvent::Configure { .. }));
//! assert!(matches!(events[1], PlatformEvent::FocusIn { .. }));
//!
//! backend.buffer(surface).unwrap().bytes.fill(0xFF);
//! backend.present(surface, &[DamageRect::new(0, 0, 4, 2).unwrap()]).unwrap();
//! events.clear();
//! backend.wait(Some(MonoTime::ZERO), &mut events).unwrap();
//! assert_eq!(events, [PlatformEvent::Released { surface }]);
//! assert_eq!(backend.wait(Some(MonoTime::ZERO), &mut events), Ok(WaitEnd::Deadline));
//! ```

#![forbid(unsafe_code)]

#[cfg(feature = "test-hooks")]
use std::cell::Cell;
use std::collections::{BTreeMap, VecDeque};

use kosmo_platform::accesskit::TreeUpdate;
use kosmo_platform::{
    AccessibilitySink, AccessibilityTree, Capabilities, Clock, EventSource, MAX_EVENTS_PER_WAIT,
    MAX_INJECTED, MAX_RECTS_PER_PRESENT, MAX_SURFACES, PixelBufferMut, Platform, PlatformCaps,
    PlatformError, PlatformEvent, Presenter, ResourceId, ResourceProvider, Stamp, SurfaceId,
    SurfacePort, SurfaceRequest, SurfaceRole, WaitEnd,
};
#[cfg(feature = "test-hooks")]
use kosmo_platform::{FaultPort, PlatformTestHooks, ReadBack};
use kosmo_types::{BufferDesc, DamageRect, Epoch, FocusGen, MonoTime};

/// The epoch of the focus stamps that the headless backend gives.
pub const HEADLESS_EPOCH: Epoch = Epoch::new(1);

/// The headless backend. See the [crate documentation](crate).
#[derive(Debug)]
pub struct HeadlessBackend {
    caps: PlatformCaps,
    resources: BTreeMap<ResourceId, Vec<u8>>,
    clock: MonoTime,
    queue: VecDeque<Queued>,
    /// The injected events in `queue`.
    injected: usize,
    surfaces: BTreeMap<SurfaceId, Surface>,
    next_surface: u32,
    next_generation: u64,
    shut_down: bool,
    #[cfg(feature = "test-hooks")]
    faults: BTreeMap<FaultPort, PlatformError>,
    /// The fault of the resource port, whose method takes `&self`.
    #[cfg(feature = "test-hooks")]
    resource_fault: Cell<Option<PlatformError>>,
}

/// A queued event and where it came from.
#[derive(Debug)]
struct Queued {
    event: PlatformEvent,
    origin: Origin,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Origin {
    /// Injected by the driver.
    Injected,
    /// Queued by the backend for the surface when it was created.
    Created(SurfaceId),
    /// The release of the surface's buffer, which frees the buffer when a
    /// wait returns it.
    Release(SurfaceId),
}

#[derive(Debug)]
struct Surface {
    desc: BufferDesc,
    buffer: Vec<u8>,
    /// The buffer's bytes and the update rectangles at the last
    /// presentation.
    presented: Option<(Vec<u8>, Vec<DamageRect>)>,
    /// Presented and not released.
    in_flight: bool,
    tree: Option<AccessibilityTree>,
}

impl HeadlessBackend {
    /// A backend with the capabilities `caps` and the resources
    /// `resources`, with its clock at 0, no surfaces and no events.
    pub fn new(caps: PlatformCaps, resources: BTreeMap<ResourceId, Vec<u8>>) -> HeadlessBackend {
        HeadlessBackend {
            caps,
            resources,
            clock: MonoTime::ZERO,
            queue: VecDeque::new(),
            injected: 0,
            surfaces: BTreeMap::new(),
            next_surface: 1,
            next_generation: 1,
            shut_down: false,
            #[cfg(feature = "test-hooks")]
            faults: BTreeMap::new(),
            #[cfg(feature = "test-hooks")]
            resource_fault: Cell::new(None),
        }
    }

    /// Queues `event`, with any stamp, after the queued events, as the test
    /// hooks' `inject` does: for a driver of a build without them. Returns
    /// [`PlatformError::Exhausted`] when
    /// [`MAX_INJECTED`](kosmo_platform::MAX_INJECTED) injected events wait
    /// to be returned.
    pub fn inject(&mut self, event: PlatformEvent) -> Result<(), PlatformError> {
        self.live()?;
        if self.injected >= MAX_INJECTED {
            return Err(PlatformError::Exhausted);
        }
        self.queue.push_back(Queued {
            event,
            origin: Origin::Injected,
        });
        self.injected += 1;
        Ok(())
    }

    /// Advances the clock to `time`, as the test hooks' `advance_to` does:
    /// for a driver of a build without them. Returns
    /// [`PlatformError::InvalidArgument`] for a time earlier than the clock.
    pub fn advance_to(&mut self, time: MonoTime) -> Result<(), PlatformError> {
        self.live()?;
        if time < self.clock {
            return Err(PlatformError::InvalidArgument);
        }
        self.clock = time;
        Ok(())
    }

    fn live(&self) -> Result<(), PlatformError> {
        if self.shut_down {
            Err(PlatformError::Disconnected)
        } else {
            Ok(())
        }
    }

    /// The fault injected at `port`, once.
    #[cfg(feature = "test-hooks")]
    fn fault(&mut self, port: FaultPort) -> Result<(), PlatformError> {
        self.faults.remove(&port).map_or(Ok(()), Err)
    }

    fn surface(&mut self, s: SurfaceId) -> Result<&mut Surface, PlatformError> {
        self.surfaces.get_mut(&s).ok_or(PlatformError::StaleHandle)
    }
}

impl Platform for HeadlessBackend {
    fn shutdown(&mut self) -> Result<(), PlatformError> {
        #[cfg(feature = "test-hooks")]
        self.fault(FaultPort::Shutdown)?;
        self.surfaces.clear();
        self.queue.clear();
        self.injected = 0;
        self.shut_down = true;
        Ok(())
    }
}

impl EventSource for HeadlessBackend {
    fn wait(
        &mut self,
        deadline: Option<MonoTime>,
        out: &mut Vec<PlatformEvent>,
    ) -> Result<WaitEnd, PlatformError> {
        self.live()?;
        #[cfg(feature = "test-hooks")]
        self.fault(FaultPort::Wait)?;
        let mut returned = 0;
        while returned < MAX_EVENTS_PER_WAIT {
            let Some(queued) = self.queue.pop_front() else {
                break;
            };
            match queued.origin {
                Origin::Injected => self.injected = self.injected.saturating_sub(1),
                Origin::Created(_) => {}
                Origin::Release(s) => {
                    if let Some(surface) = self.surfaces.get_mut(&s) {
                        surface.in_flight = false;
                    }
                }
            }
            out.push(queued.event);
            returned += 1;
        }
        Ok(if returned > 0 {
            WaitEnd::Events
        } else if deadline.is_some_and(|deadline| deadline <= self.clock) {
            WaitEnd::Deadline
        } else {
            WaitEnd::Idle
        })
    }
}

impl Clock for HeadlessBackend {
    fn now(&self) -> MonoTime {
        self.clock
    }
}

impl SurfacePort for HeadlessBackend {
    fn create_surface(&mut self, req: SurfaceRequest) -> Result<SurfaceId, PlatformError> {
        self.live()?;
        #[cfg(feature = "test-hooks")]
        self.fault(FaultPort::CreateSurface)?;
        if !self.caps.accepts(req.format) {
            return Err(PlatformError::Unsupported);
        }
        if self.surfaces.len() >= MAX_SURFACES {
            return Err(PlatformError::Exhausted);
        }
        let (width, height) = (req.size.width(), req.size.height());
        let stride = width
            .checked_mul(req.format.bytes_per_pixel())
            .ok_or(PlatformError::InvalidArgument)?;
        let desc = BufferDesc::new(width, height, stride, req.format)
            .map_err(|_| PlatformError::InvalidArgument)?;
        let len = usize::try_from(desc.byte_len()).map_err(|_| PlatformError::InvalidArgument)?;
        let next = self
            .next_surface
            .checked_add(1)
            .ok_or(PlatformError::Exhausted)?;
        let id = SurfaceId::new(self.next_surface);
        self.next_surface = next;
        self.surfaces.insert(
            id,
            Surface {
                desc,
                buffer: vec![0; len],
                presented: None,
                in_flight: false,
                tree: None,
            },
        );
        if req.role == SurfaceRole::Main {
            let generation = FocusGen::new(self.next_generation);
            self.next_generation = self.next_generation.saturating_add(1);
            for event in [
                PlatformEvent::Configure {
                    surface: id,
                    size: req.size,
                },
                PlatformEvent::FocusIn {
                    surface: id,
                    stamp: Stamp::new(HEADLESS_EPOCH, generation),
                },
            ] {
                self.queue.push_back(Queued {
                    event,
                    origin: Origin::Created(id),
                });
            }
        }
        Ok(id)
    }

    fn destroy_surface(&mut self, s: SurfaceId) -> Result<(), PlatformError> {
        self.live()?;
        #[cfg(feature = "test-hooks")]
        self.fault(FaultPort::DestroySurface)?;
        self.surfaces.remove(&s).ok_or(PlatformError::StaleHandle)?;
        self.queue.retain(|queued| match queued.origin {
            Origin::Injected => true,
            Origin::Created(of) | Origin::Release(of) => of != s,
        });
        Ok(())
    }
}

impl Presenter for HeadlessBackend {
    fn buffer(&mut self, s: SurfaceId) -> Result<PixelBufferMut<'_>, PlatformError> {
        self.live()?;
        #[cfg(feature = "test-hooks")]
        self.fault(FaultPort::Buffer)?;
        let surface = self.surface(s)?;
        if surface.in_flight {
            return Err(PlatformError::Busy);
        }
        Ok(PixelBufferMut {
            desc: surface.desc,
            bytes: &mut surface.buffer,
        })
    }

    fn present(&mut self, s: SurfaceId, damage: &[DamageRect]) -> Result<(), PlatformError> {
        self.live()?;
        #[cfg(feature = "test-hooks")]
        self.fault(FaultPort::Present)?;
        let surface = self.surface(s)?;
        if surface.in_flight {
            return Err(PlatformError::Busy);
        }
        if damage.len() > MAX_RECTS_PER_PRESENT {
            return Err(PlatformError::Exhausted);
        }
        let (width, height) = (surface.desc.width(), surface.desc.height());
        let outside = |rect: &DamageRect| {
            u64::from(rect.x()) + u64::from(rect.width()) > u64::from(width)
                || u64::from(rect.y()) + u64::from(rect.height()) > u64::from(height)
        };
        if damage.iter().any(outside) {
            return Err(PlatformError::InvalidArgument);
        }
        surface.presented = Some((surface.buffer.clone(), damage.to_vec()));
        surface.in_flight = true;
        self.queue.push_back(Queued {
            event: PlatformEvent::Released { surface: s },
            origin: Origin::Release(s),
        });
        Ok(())
    }
}

impl Capabilities for HeadlessBackend {
    fn caps(&self) -> PlatformCaps {
        self.caps.clone()
    }
}

impl ResourceProvider for HeadlessBackend {
    fn resource(&self, id: ResourceId) -> Result<&[u8], PlatformError> {
        self.live()?;
        #[cfg(feature = "test-hooks")]
        if let Some(error) = self.resource_fault.take() {
            return Err(error);
        }
        self.resources
            .get(&id)
            .map(Vec::as_slice)
            .ok_or(PlatformError::NotFound)
    }
}

impl AccessibilitySink for HeadlessBackend {
    fn update(&mut self, surface: SurfaceId, update: TreeUpdate) -> Result<(), PlatformError> {
        self.live()?;
        #[cfg(feature = "test-hooks")]
        self.fault(FaultPort::Update)?;
        let surface = self.surface(surface)?;
        match &mut surface.tree {
            Some(tree) => tree.apply(update),
            None => {
                surface.tree = Some(AccessibilityTree::new(update)?);
                Ok(())
            }
        }
    }
}

#[cfg(feature = "test-hooks")]
impl PlatformTestHooks for HeadlessBackend {
    fn inject(&mut self, e: PlatformEvent) -> Result<(), PlatformError> {
        HeadlessBackend::inject(self, e)
    }

    fn advance_to(&mut self, time: MonoTime) -> Result<(), PlatformError> {
        HeadlessBackend::advance_to(self, time)
    }

    fn read_back(&self, s: SurfaceId) -> Result<Option<ReadBack<'_>>, PlatformError> {
        let surface = self.surfaces.get(&s).ok_or(PlatformError::StaleHandle)?;
        Ok(surface.presented.as_ref().map(|(bytes, damage)| ReadBack {
            desc: surface.desc,
            bytes,
            damage,
        }))
    }

    fn inject_fault(&mut self, port: FaultPort, error: PlatformError) -> Result<(), PlatformError> {
        self.live()?;
        if port == FaultPort::Resource {
            if self.resource_fault.get().is_some() {
                return Err(PlatformError::Busy);
            }
            self.resource_fault.set(Some(error));
        } else {
            if self.faults.contains_key(&port) {
                return Err(PlatformError::Busy);
            }
            self.faults.insert(port, error);
        }
        Ok(())
    }

    fn a11y_tree(&self, s: SurfaceId) -> Result<Option<&AccessibilityTree>, PlatformError> {
        let surface = self.surfaces.get(&s).ok_or(PlatformError::StaleHandle)?;
        Ok(surface.tree.as_ref())
    }
}
