use super::super::{TerminalControl, TerminalGuard};
use std::{
    io,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

struct MockTerminalControl {
    enters: Arc<AtomicUsize>,
    restores: Arc<AtomicUsize>,
    fail_enter: bool,
}

impl TerminalControl for MockTerminalControl {
    fn enter(&mut self) -> io::Result<()> {
        self.enters.fetch_add(1, Ordering::AcqRel);
        if self.fail_enter {
            Err(io::Error::other("fixture enter failure"))
        } else {
            Ok(())
        }
    }

    fn restore(&mut self) -> io::Result<()> {
        self.restores.fetch_add(1, Ordering::AcqRel);
        Ok(())
    }
}

fn mock(fail_enter: bool) -> (Box<dyn TerminalControl>, Arc<AtomicUsize>, Arc<AtomicUsize>) {
    let enters = Arc::new(AtomicUsize::new(0));
    let restores = Arc::new(AtomicUsize::new(0));
    (
        Box::new(MockTerminalControl {
            enters: enters.clone(),
            restores: restores.clone(),
            fail_enter,
        }),
        enters,
        restores,
    )
}

#[test]
fn terminal_guard_restores_once_when_dropped() -> io::Result<()> {
    let (control, enters, restores) = mock(false);
    let guard = TerminalGuard::with_control(control)?;
    assert_eq!(enters.load(Ordering::Acquire), 1);
    assert_eq!(restores.load(Ordering::Acquire), 0);
    drop(guard);
    assert_eq!(restores.load(Ordering::Acquire), 1);
    Ok(())
}

#[test]
fn terminal_guard_explicit_restore_is_not_repeated_by_drop() -> io::Result<()> {
    let (control, enters, restores) = mock(false);
    let mut guard = TerminalGuard::with_control(control)?;
    guard.restore()?;
    assert_eq!(enters.load(Ordering::Acquire), 1);
    assert_eq!(restores.load(Ordering::Acquire), 1);
    drop(guard);
    assert_eq!(restores.load(Ordering::Acquire), 1);
    Ok(())
}

#[test]
fn terminal_guard_enter_failure_does_not_attempt_restore() {
    let (control, enters, restores) = mock(true);
    assert!(TerminalGuard::with_control(control).is_err());
    assert_eq!(enters.load(Ordering::Acquire), 1);
    assert_eq!(restores.load(Ordering::Acquire), 0);
}
