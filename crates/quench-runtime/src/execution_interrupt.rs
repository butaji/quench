//! Owned execution cancellation. Neither the handle nor native wakeups carry a Vm.
use std::sync::{
    Arc, Mutex, Weak,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

/// The lane floor (`ExecutionInterrupt::lane_floor`) that sends a run's next safepoint jump
/// through its slow path: no stack address lies at or above it.
pub(crate) const LANE_FLOOR_RAISED: usize = usize::MAX;
type NativeWaker = dyn Fn() + Send + Sync;
#[derive(Default)]
struct State {
    requested: AtomicBool,
    wakers: Mutex<Vec<Weak<NativeWaker>>>,
    /// The page compiled code reads at its interrupt checks, once code asks for it.
    poll: std::sync::OnceLock<Option<PollPage>>,
    /// The stack floor of the owner's running lane (`dispatch_fast`): a taken safepoint jump
    /// with the stack pointer below it leaves through the slow path. A request raises it, so
    /// the lane's safepoints test only the floor.
    lane_floor: AtomicUsize,
}

/// A readable page that a termination request makes unreadable: compiled code reads it at
/// each interrupt check, and the read faults once termination is requested (the fault resumes
/// as the termination). It sits below 2 GiB, so code addresses it by a 32-bit constant.
struct PollPage {
    address: usize,
}

impl PollPage {
    pub(crate) const BYTES: usize = 4096;
    /// Where the page is asked to start; the kernel may place it elsewhere.
    #[cfg(all(target_arch = "x86_64", target_os = "linux"))]
    const HINT: usize = 1 << 29;

    #[cfg(all(target_arch = "x86_64", target_os = "linux"))]
    fn new() -> Option<Self> {
        // SAFETY: an anonymous private read-only page; the result is checked.
        let pointer = unsafe {
            libc::mmap(
                Self::HINT as *mut libc::c_void,
                Self::BYTES,
                libc::PROT_READ,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                -1,
                0,
            )
        };
        if pointer == libc::MAP_FAILED {
            return None;
        }
        let page = Self {
            address: pointer as usize,
        };
        (page.address + Self::BYTES <= i32::MAX as usize).then_some(page)
    }

    #[cfg(not(all(target_arch = "x86_64", target_os = "linux")))]
    fn new() -> Option<Self> {
        None
    }

    /// Makes every later read fault; idempotent.
    fn close(&self) {
        #[cfg(all(target_arch = "x86_64", target_os = "linux"))]
        // SAFETY: the page this value maps.
        unsafe {
            libc::mprotect(
                self.address as *mut libc::c_void,
                Self::BYTES,
                libc::PROT_NONE,
            );
        }
    }
}

impl Drop for PollPage {
    fn drop(&mut self) {
        #[cfg(all(target_arch = "x86_64", target_os = "linux"))]
        // SAFETY: the page this value maps; no compiled code of its VM runs any more.
        unsafe {
            libc::munmap(self.address as *mut libc::c_void, Self::BYTES);
        }
    }
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
        // Either this request sees the poll page or the page's creation sees the request.
        std::sync::atomic::fence(Ordering::SeqCst);
        // Either the running lane sees the raised floor or its entry sees the request
        // (`dispatch_fast`'s `run_fast_lane`).
        self.state.lane_floor.store(LANE_FLOOR_RAISED, Ordering::SeqCst);
        if let Some(Some(page)) = self.state.poll.get() {
            page.close();
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
    /// The owner's lane floor, which lives as long as this handle's state.
    pub(crate) fn lane_floor(&self) -> &AtomicUsize {
        &self.state.lane_floor
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

    /// The address of the poll page compiled code reads at its interrupt checks, and how many
    /// bytes it spans; `None` where the host cannot provide one. The page is unreadable once
    /// termination is requested.
    pub(crate) fn poll_page(&self) -> Option<(usize, usize)> {
        // Once the page exists, a request closes it itself (`request_termination`); only its
        // creation races a request, which the fence below orders.
        if let Some(page) = self.state.poll.get() {
            return page.as_ref().map(|page| (page.address, PollPage::BYTES));
        }
        let page = self.state.poll.get_or_init(PollPage::new).as_ref()?;
        std::sync::atomic::fence(Ordering::SeqCst);
        if self.state.requested.load(Ordering::Relaxed) {
            page.close();
        }
        Some((page.address, PollPage::BYTES))
    }
}

#[cfg(test)]
#[path = "execution_interrupt_tests.rs"]
mod tests;
