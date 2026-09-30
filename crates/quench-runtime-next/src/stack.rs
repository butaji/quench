//! One recursion policy for runtime and compiler transitions.
use std::{cell::Cell, marker::PhantomData, rc::Rc};

/// Space reserved for recursive runtime transitions on a worker thread.
pub const STACK_BUDGET_BYTES: usize = 32 * 1024 * 1024;
/// Reserved for the host entry, error construction and unwinding.
pub const STACK_HEADROOM_BYTES: usize = 2 * 1024 * 1024;
/// Every worker derives its reservation from the runtime policy.
pub const WORKER_STACK_SIZE: usize = STACK_BUDGET_BYTES + STACK_HEADROOM_BYTES;

// Conservative transition reservation; task 56 qualifies this against the
// largest recursive frame chains on the supported toolchain/platform.
const STACK_TRANSITION_RESERVE_BYTES: usize = 64 * 1024;
const MAX_RECURSIVE_TRANSITIONS: usize = STACK_BUDGET_BYTES / STACK_TRANSITION_RESERVE_BYTES;
pub(crate) const STACK_EXHAUSTED_MESSAGE: &str = "Maximum call stack size exceeded";

thread_local! {
    static ACTIVE_TRANSITIONS: Cell<usize> = const { Cell::new(0) };
}

/// A transition is released on every return path, including Rust unwinding.
/// Thread affinity keeps the counter correct across reentrant runtime owners.
pub(crate) struct StackGuard(PhantomData<Rc<()>>);

impl StackGuard {
    pub(crate) fn enter() -> Result<Self, ()> {
        ACTIVE_TRANSITIONS.with(|depth| {
            let current = depth.get();
            if current == MAX_RECURSIVE_TRANSITIONS {
                return Err(());
            }
            depth.set(current + 1);
            Ok(Self(PhantomData))
        })
    }
}

impl Drop for StackGuard {
    fn drop(&mut self) {
        ACTIVE_TRANSITIONS.with(|depth| depth.set(depth.get() - 1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recursion_budget_recovers_after_exhaustion() {
        let guards = (0..MAX_RECURSIVE_TRANSITIONS)
            .map(|_| StackGuard::enter().unwrap())
            .collect::<Vec<_>>();
        assert!(StackGuard::enter().is_err());
        drop(guards);
        let guard = StackGuard::enter().unwrap();
        drop(guard);
        assert_eq!(ACTIVE_TRANSITIONS.with(Cell::get), 0);
    }
}
