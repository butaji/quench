//! Owned execution cancellation. Neither the handle nor native wakeups carry a Vm.
use std::sync::{
    Arc, Mutex, Weak,
    atomic::{AtomicBool, Ordering},
};
type NativeWaker = dyn Fn() + Send + Sync;
#[derive(Default)]
struct State {
    requested: AtomicBool,
    wakers: Mutex<Vec<Weak<NativeWaker>>>,
}
#[derive(Clone, Default)]
pub struct ExecutionInterrupt {
    state: Arc<State>,
}
impl ExecutionInterrupt {
    /// Irreversibly terminate this execution owner. No guest error is thrown.
    pub fn request_termination(&self) {
        if self.state.requested.swap(true, Ordering::AcqRel) {
            return;
        }
        let wakers = {
            let mut registered = self.state.wakers.lock().unwrap();
            let live = registered
                .iter()
                .filter_map(Weak::upgrade)
                .collect::<Vec<_>>();
            registered.clear();
            live
        };
        // Native waiters own their wait mutex; never call them with our lock held.
        for wake in wakers {
            wake();
        }
    }
    pub fn is_termination_requested(&self) -> bool {
        self.state.requested.load(Ordering::Acquire)
    }
    /// Register a native, idempotent cancellation wakeup for a blocked operation.
    /// The caller holds the strong callback only while its native wait is active.
    /// The wakeup must set cancellation under its wait mutex before notifying.
    pub(crate) fn register_termination_waker(&self, wake: &Arc<NativeWaker>) {
        {
            let mut registered = self.state.wakers.lock().unwrap();
            registered.retain(|old| old.strong_count() != 0);
            registered.push(Arc::downgrade(wake));
        }
        // A request racing registration cannot be lost. Wakeups are idempotent.
        if self.is_termination_requested() {
            wake();
        }
    }
    pub(crate) fn atomic_flag(&self) -> &AtomicBool {
        &self.state.requested
    }
}

#[cfg(test)]
#[path = "execution_interrupt_tests.rs"]
mod tests;
