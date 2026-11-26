//! The test hooks of the headless backend beyond the conformance suite.

// These tests need the test hooks: the check builds this package once
// more with them, and a build without them compiles this file empty.
#![cfg(feature = "test-hooks")]

use std::collections::BTreeMap;

use kosmo_backend_headless::HeadlessBackend;
use kosmo_platform::{
    EventSource, FaultPort, PixelSize, PlatformCaps, PlatformError, PlatformTestHooks, ResourceId,
    ResourceProvider, WaitEnd,
};
use kosmo_types::PixelFormat;

#[allow(clippy::unwrap_used, reason = "a fixed size that is valid")]
fn backend() -> HeadlessBackend {
    let caps = PlatformCaps::new(
        vec![PixelFormat::Rgba8Premul],
        PixelSize::new(8, 6).unwrap(),
    );
    HeadlessBackend::new(
        caps,
        BTreeMap::from([(ResourceId::new("a"), vec![1, 2, 3])]),
    )
}

#[test]
fn a_second_fault_at_a_port_is_busy_and_the_first_takes_effect_once() {
    let mut backend = backend();
    assert_eq!(
        backend.inject_fault(FaultPort::Wait, PlatformError::Backend),
        Ok(())
    );
    assert_eq!(
        backend.inject_fault(FaultPort::Wait, PlatformError::Disconnected),
        Err(PlatformError::Busy)
    );
    // A fault waiting at one port does not make another port busy.
    assert_eq!(
        backend.inject_fault(FaultPort::Resource, PlatformError::NotFound),
        Ok(())
    );
    assert_eq!(
        backend.inject_fault(FaultPort::Resource, PlatformError::Backend),
        Err(PlatformError::Busy)
    );
    let mut events = Vec::new();
    assert_eq!(backend.wait(None, &mut events), Err(PlatformError::Backend));
    assert_eq!(backend.wait(None, &mut events), Ok(WaitEnd::Idle));
    let a = ResourceId::new("a");
    assert_eq!(backend.resource(a), Err(PlatformError::NotFound));
    assert_eq!(backend.resource(a), Ok(&[1, 2, 3][..]));
    // Once the fault took effect, the port takes a new one.
    assert_eq!(
        backend.inject_fault(FaultPort::Wait, PlatformError::Disconnected),
        Ok(())
    );
    assert_eq!(
        backend.wait(None, &mut events),
        Err(PlatformError::Disconnected)
    );
}
