//! The behaviour of the headless backend that the conformance suite does
//! not cover, through its inherent methods only, so these tests run with
//! and without the feature `test-hooks`.

use std::collections::BTreeMap;

use kosmo_backend_headless::{HEADLESS_EPOCH, HeadlessBackend};
use kosmo_platform::accesskit::{NodeId, TreeId, TreeUpdate};
use kosmo_platform::{
    AccessibilitySink, Clock, EventSource, PixelSize, Platform, PlatformCaps, PlatformError,
    PlatformEvent, Presenter, ResourceId, ResourceProvider, Stamp, SurfaceId, SurfacePort,
    SurfaceRequest, SurfaceRole, WaitEnd,
};
use kosmo_types::{
    DamageRect, Epoch, FocusGen, HidUsage, KeyAction, KeyEvent, KeyMeaning, ModSet, MonoTime,
    PixelFormat,
};

#[allow(clippy::unwrap_used, reason = "a fixed size that is valid")]
fn size() -> PixelSize {
    PixelSize::new(8, 6).unwrap()
}

fn backend() -> HeadlessBackend {
    let caps = PlatformCaps::new(vec![PixelFormat::Rgba8Premul], size());
    HeadlessBackend::new(
        caps,
        BTreeMap::from([(ResourceId::new("a"), vec![1, 2, 3])]),
    )
}

fn main_surface() -> SurfaceRequest {
    SurfaceRequest {
        role: SurfaceRole::Main,
        size: size(),
        format: PixelFormat::Rgba8Premul,
    }
}

#[allow(clippy::unwrap_used, reason = "a failed wait fails the calling test")]
fn wait(
    backend: &mut HeadlessBackend,
    deadline: Option<MonoTime>,
) -> (WaitEnd, Vec<PlatformEvent>) {
    let mut events = Vec::new();
    let end = backend.wait(deadline, &mut events).unwrap();
    (end, events)
}

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

#[test]
fn a_main_surface_is_configured_and_focused() {
    let mut backend = backend();
    let surface = backend.create_surface(main_surface()).unwrap();
    let stamp = Stamp::new(HEADLESS_EPOCH, FocusGen::new(1));
    assert_eq!(
        wait(&mut backend, None),
        (
            WaitEnd::Events,
            vec![
                PlatformEvent::Configure {
                    surface,
                    size: size()
                },
                PlatformEvent::FocusIn { surface, stamp },
            ]
        )
    );
    assert_eq!(wait(&mut backend, None), (WaitEnd::Idle, vec![]));
}

#[test]
fn inherent_methods_inject_like_the_test_hooks() {
    // The sequence of the first conformance requirement.
    let mut backend = backend();
    let surface = backend.create_surface(main_surface()).unwrap();
    wait(&mut backend, None);
    let old = Stamp::new(Epoch::new(1), FocusGen::new(1));
    let new = Stamp::new(Epoch::new(1), FocusGen::new(2));
    let injected = [
        key(surface, old, 1),
        PlatformEvent::FocusIn {
            surface,
            stamp: new,
        },
        key(surface, new, 2),
    ];
    for event in injected {
        backend.inject(event).unwrap();
    }
    assert_eq!(
        wait(&mut backend, None),
        (WaitEnd::Events, injected.to_vec())
    );
    assert_eq!(wait(&mut backend, None), (WaitEnd::Idle, vec![]));
}

#[test]
fn inherent_methods_advance_the_clock_like_the_test_hooks() {
    // The sequence of the fourth conformance requirement.
    let mut backend = backend();
    assert_eq!(backend.now(), MonoTime::ZERO);
    backend.advance_to(MonoTime::from_nanos(10)).unwrap();
    assert_eq!(backend.now(), MonoTime::from_nanos(10));
    wait(&mut backend, None);
    wait(&mut backend, Some(MonoTime::from_nanos(20)));
    assert_eq!(backend.now(), MonoTime::from_nanos(10));
    assert_eq!(
        backend.advance_to(MonoTime::from_nanos(5)),
        Err(PlatformError::InvalidArgument)
    );
    assert_eq!(backend.now(), MonoTime::from_nanos(10));
}

#[test]
fn destroying_a_surface_drops_only_the_events_that_the_backend_queued_for_it() {
    let mut backend = backend();
    let surface = backend.create_surface(main_surface()).unwrap();
    let stamp = Stamp::new(Epoch::new(1), FocusGen::new(1));
    backend.inject(key(surface, stamp, 1)).unwrap();
    let whole = DamageRect::new(0, 0, 8, 6).unwrap();
    backend.present(surface, &[whole]).unwrap();
    backend.destroy_surface(surface).unwrap();
    // Neither the configuration and focus of the surface nor the release
    // of its buffer stay.
    assert_eq!(
        wait(&mut backend, None),
        (WaitEnd::Events, vec![key(surface, stamp, 1)])
    );
    assert_eq!(wait(&mut backend, None), (WaitEnd::Idle, vec![]));
}

#[test]
fn a_release_comes_back_in_the_order_it_arose() {
    let mut backend = backend();
    let surface = backend.create_surface(main_surface()).unwrap();
    wait(&mut backend, None);
    let stamp = Stamp::new(Epoch::new(1), FocusGen::new(1));
    backend.inject(key(surface, stamp, 1)).unwrap();
    let whole = DamageRect::new(0, 0, 8, 6).unwrap();
    backend.present(surface, &[whole]).unwrap();
    backend.inject(key(surface, stamp, 2)).unwrap();
    assert_eq!(
        wait(&mut backend, None),
        (
            WaitEnd::Events,
            vec![
                key(surface, stamp, 1),
                PlatformEvent::Released { surface },
                key(surface, stamp, 2),
            ]
        )
    );
}

#[test]
fn a_presentation_keeps_its_rectangles_inside_the_buffer() {
    let mut backend = backend();
    let surface = backend.create_surface(main_surface()).unwrap();
    let outside = DamageRect::new(7, 0, 2, 1).unwrap();
    assert_eq!(
        backend.present(surface, &[outside]),
        Err(PlatformError::InvalidArgument)
    );
    let inside = DamageRect::new(7, 5, 1, 1).unwrap();
    assert_eq!(backend.present(surface, &[inside]), Ok(()));
    assert_eq!(
        backend.present(surface, &[inside]),
        Err(PlatformError::Busy)
    );
}

#[test]
fn a_surface_beyond_the_buffer_limit_is_an_invalid_argument() {
    let mut backend = backend();
    let mut request = main_surface();
    request.size = PixelSize::new(8_192, 8_192).unwrap();
    assert_eq!(
        backend.create_surface(request),
        Err(PlatformError::InvalidArgument)
    );
    assert_eq!(wait(&mut backend, None), (WaitEnd::Idle, vec![]));
}

#[test]
fn after_shutdown_every_port_is_disconnected() {
    let mut backend = backend();
    let surface = backend.create_surface(main_surface()).unwrap();
    assert_eq!(backend.shutdown(), Ok(()));
    let mut events = Vec::new();
    assert_eq!(
        backend.wait(None, &mut events),
        Err(PlatformError::Disconnected)
    );
    assert_eq!(
        backend.create_surface(main_surface()),
        Err(PlatformError::Disconnected)
    );
    assert_eq!(
        backend.buffer(surface).err(),
        Some(PlatformError::Disconnected)
    );
    assert_eq!(
        backend.resource(ResourceId::new("a")),
        Err(PlatformError::Disconnected)
    );
    assert_eq!(
        backend.inject(key(surface, Stamp::new(Epoch::new(1), FocusGen::new(1)), 1)),
        Err(PlatformError::Disconnected)
    );
    assert_eq!(
        backend.destroy_surface(surface),
        Err(PlatformError::Disconnected)
    );
    let whole = DamageRect::new(0, 0, 8, 6).unwrap();
    assert_eq!(
        backend.present(surface, &[whole]),
        Err(PlatformError::Disconnected)
    );
    let update = TreeUpdate {
        nodes: vec![],
        tree: None,
        tree_id: TreeId::ROOT,
        focus: NodeId(1),
    };
    assert_eq!(
        backend.update(surface, update),
        Err(PlatformError::Disconnected)
    );
    assert_eq!(
        backend.advance_to(MonoTime::from_nanos(1)),
        Err(PlatformError::Disconnected)
    );
    assert_eq!(backend.shutdown(), Ok(()));
}
