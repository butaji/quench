//! One recursion policy for runtime and compiler transitions.
use std::{cell::Cell, marker::PhantomData, rc::Rc};

/// Native stack distance allowed for recursive runtime transitions.
pub const STACK_BUDGET_BYTES: usize = 32 * 1024 * 1024;
/// Space for host entry, error construction and unwinding after the guard fires.
pub const STACK_HEADROOM_BYTES: usize = 2 * 1024 * 1024;
/// Every worker derives its reservation from the runtime policy.
pub const WORKER_STACK_SIZE: usize = STACK_BUDGET_BYTES + STACK_HEADROOM_BYTES;

// Some parsers and traversals grow structured work without retaining Rust
// frames. This derived transition ceiling bounds those paths as well.
const STACK_TRANSITION_RESERVE_BYTES: usize = 64 * 1024;
const MAX_RECURSIVE_TRANSITIONS: usize = STACK_BUDGET_BYTES / STACK_TRANSITION_RESERVE_BYTES;

/// Maximum live guest function activations, including suspended callers.
pub const MAX_GUEST_CALL_DEPTH: usize = 8_192;
pub const STACK_EXHAUSTED_MESSAGE: &str = "Maximum call stack size exceeded";

thread_local! {
    static ACTIVE_TRANSITIONS: Cell<usize> = const { Cell::new(0) };
    static STACK_ORIGIN: Cell<usize> = const { Cell::new(0) };
    static ACTIVE_GUEST_CALLS: Cell<usize> = const { Cell::new(0) };
}

/// A transition is released on every return path, including Rust unwinding.
/// Thread affinity keeps the measurement correct across reentrant runtime owners.
pub struct StackGuard(PhantomData<Rc<()>>);
pub struct GuestCallGuard(PhantomData<Rc<()>>);

impl StackGuard {
    #[inline(never)]
    pub fn enter() -> Result<Self, ()> {
        let current_stack_pointer = current_stack_pointer();
        ACTIVE_TRANSITIONS.with(|depth| {
            let current = depth.get();
            if current == 0 {
                STACK_ORIGIN.with(|origin| origin.set(current_stack_pointer));
            }
            let origin = STACK_ORIGIN.with(Cell::get);
            let distance = current_stack_pointer.abs_diff(origin);
            if current == MAX_RECURSIVE_TRANSITIONS || distance > STACK_BUDGET_BYTES {
                return Err(());
            }
            depth.set(current + 1);
            Ok(Self(PhantomData))
        })
    }
}

impl Drop for StackGuard {
    fn drop(&mut self) {
        ACTIVE_TRANSITIONS.with(|depth| {
            let remaining = depth.get() - 1;
            depth.set(remaining);
            if remaining == 0 {
                STACK_ORIGIN.with(|origin| origin.set(0));
            }
        });
    }
}

impl GuestCallGuard {
    pub fn enter() -> Result<Self, ()> {
        ACTIVE_GUEST_CALLS.with(|depth| {
            let current = depth.get();
            if current == MAX_GUEST_CALL_DEPTH {
                return Err(());
            }
            depth.set(current + 1);
            Ok(Self(PhantomData))
        })
    }
}

impl Drop for GuestCallGuard {
    fn drop(&mut self) {
        ACTIVE_GUEST_CALLS.with(|depth| depth.set(depth.get() - 1));
    }
}

#[inline(never)]
fn current_stack_pointer() -> usize {
    let marker = 0_u8;
    std::hint::black_box(&marker as *const u8 as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NATIVE_TEST_FRAME_BYTES: usize = 128 * 1024;

    #[test]
    fn independent_threads_have_independent_guest_call_depth() {
        let guards = (0..MAX_GUEST_CALL_DEPTH)
            .map(|_| GuestCallGuard::enter().unwrap())
            .collect::<Vec<_>>();
        std::thread::spawn(|| assert!(GuestCallGuard::enter().is_ok()))
            .join()
            .unwrap();
        assert!(GuestCallGuard::enter().is_err());
        drop(guards);
    }

    #[test]
    fn unwinding_releases_recursive_transitions() {
        let result = std::panic::catch_unwind(|| {
            let _guards = (0..MAX_GUEST_CALL_DEPTH)
                .map(|_| GuestCallGuard::enter().unwrap())
                .collect::<Vec<_>>();
            panic!("host callback unwinds");
        });
        assert!(result.is_err());
        assert_eq!(ACTIVE_TRANSITIONS.with(Cell::get), 0);
        assert!(StackGuard::enter().is_ok());
    }

    #[test]
    fn guest_call_depth_recovers_after_exhaustion() {
        let guards = (0..MAX_GUEST_CALL_DEPTH)
            .map(|_| GuestCallGuard::enter().unwrap())
            .collect::<Vec<_>>();
        assert!(GuestCallGuard::enter().is_err());
        drop(guards);
        let guard = GuestCallGuard::enter().unwrap();
        drop(guard);
        assert_eq!(ACTIVE_GUEST_CALLS.with(Cell::get), 0);
    }

    #[test]
    fn native_stack_distance_exhausts_and_resets() {
        #[inline(never)]
        fn descend(remaining: usize) -> Result<(), ()> {
            let frame = [0_u8; NATIVE_TEST_FRAME_BYTES];
            std::hint::black_box(&frame);
            let _guard = StackGuard::enter()?;
            if remaining == 0 {
                return Ok(());
            }
            descend(remaining - 1)
        }

        std::thread::Builder::new()
            .stack_size(WORKER_STACK_SIZE)
            .spawn(|| {
                assert!(descend(usize::MAX).is_err());
                assert_eq!(ACTIVE_TRANSITIONS.with(Cell::get), 0);
                assert!(StackGuard::enter().is_ok());
            })
            .unwrap()
            .join()
            .unwrap();
    }
}
