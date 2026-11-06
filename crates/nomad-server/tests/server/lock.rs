//! The Universe lock (N2 plan §4.2, §7 "Lock"; doc 20 §7).

use std::sync::Arc;

use nomad_server::{FakeClock, ServerError, ServerHandle, UniverseLock};

use crate::common::{T0, TestDir, config, create_universe, start_live};

#[test]
fn a_second_server_on_the_same_universe_is_refused() {
    let dir = TestDir::new("lock");
    let clock = FakeClock::new(T0);
    let created = create_universe(&dir, 1, &clock);

    let first = start_live(&created.dir, &clock);
    assert!(UniverseLock::is_held(&created.dir).expect("lock state"));
    let second = ServerHandle::start(config(&created.dir), Arc::new(clock.clone()));
    let Err(error) = second else {
        panic!("a second server started");
    };
    assert!(matches!(error, ServerError::Locked { .. }), "{error}");
    assert!(error.to_string().contains("already in use"), "{error}");

    // Once the first server has stopped, the Universe can be run again.
    first.shutdown().expect("clean shutdown");
    assert!(!UniverseLock::is_held(&created.dir).expect("lock state"));
    start_live(&created.dir, &clock)
        .shutdown()
        .expect("clean shutdown");
}

#[test]
fn a_crashed_server_releases_its_lock() {
    let dir = TestDir::new("lock-crash");
    let clock = FakeClock::new(T0);
    let created = create_universe(&dir, 2, &clock);
    drop(start_live(&created.dir, &clock));
    assert!(!UniverseLock::is_held(&created.dir).expect("lock state"));
}
