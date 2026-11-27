//! One recursion policy for runtime and compiler transitions.
pub mod projection;
use std::{cell::Cell, marker::PhantomData, rc::Rc};

/// Space for host entry, error construction and unwinding after the guard fires. It is kept
/// off the top of every stack budget, so a transition that passes the budget still has room.
pub const STACK_HEADROOM_BYTES: usize = 2 * 1024 * 1024;
/// The stack a thread quench creates for itself. Its budget is `budget_for_stack` of it.
pub const WORKER_STACK_SIZE: usize = STACK_BUDGET_BYTES + STACK_HEADROOM_BYTES;
/// The native stack distance a thread quench creates for itself may use for recursive runtime
/// transitions: `budget_for_stack(WORKER_STACK_SIZE)`.
pub const STACK_BUDGET_BYTES: usize = 32 * 1024 * 1024;

/// The stack distance a thread running on a stack of `stack_bytes` may use for recursive
/// transitions: the stack less the headroom.
pub const fn budget_for_stack(stack_bytes: usize) -> usize {
    stack_bytes.saturating_sub(STACK_HEADROOM_BYTES)
}

// Some parsers and traversals grow structured work without retaining Rust
// frames. This derived transition ceiling bounds those paths as well.
const STACK_TRANSITION_RESERVE_BYTES: usize = 4 * 1024;

thread_local! {
    /// The calling thread's stack distance budget (`set_thread_stack_budget`). A thread
    /// quench creates starts with its worker stack's; the main thread's is read from its
    /// real stack (`budget_for_stack` of that, by the process start).
    static BUDGET_BYTES: Cell<usize> = const { Cell::new(budget_for_stack(WORKER_STACK_SIZE)) };
}

/// The calling thread's stack distance budget.
#[inline(always)]
fn stack_budget() -> usize {
    BUDGET_BYTES.with(Cell::get)
}

/// The transitions the calling thread's budget admits.
#[inline(always)]
fn max_recursive_transitions() -> usize {
    stack_budget() / STACK_TRANSITION_RESERVE_BYTES
}

/// Sets the calling thread's stack distance budget. Only the thread that owns its stack
/// calls it, with the bound of that stack (`budget_for_stack`).
pub fn set_thread_stack_budget(bytes: usize) {
    BUDGET_BYTES.with(|budget| budget.set(bytes));
}

/// Maximum live guest function activations, including suspended callers. The
/// host's `--stack_size` lowers the active limit below it (`GuestCallLimitGuard`).
pub const MAX_GUEST_CALL_DEPTH: usize = 8_192;
/// Maximum requested locals and registers for live Wasm frames on one thread.
/// This bounds retained guest frame payload independently of activation count.
pub const MAX_GUEST_FRAME_STORAGE_BYTES: usize = 32 * 1024 * 1024;
pub const STACK_EXHAUSTED_MESSAGE: &str = "Maximum call stack size exceeded";

/// Node's `--stack_size` controls the guest stack in V8. Quench executes on a
/// larger host stack, so scale the guest activation limit separately to avoid
/// exhausting the Rust worker stack before producing a JavaScript RangeError.
pub const DEFAULT_GUEST_STACK_SIZE_KB: usize = 984;

thread_local! {
    static ACTIVE_TRANSITIONS: Cell<usize> = const { Cell::new(0) };
    static STACK_ORIGIN: Cell<usize> = const { Cell::new(0) };
    static ACTIVE_GUEST_CALLS: Cell<usize> = const { Cell::new(0) };
    static ACTIVE_GUEST_FRAME_STORAGE_BYTES: Cell<usize> = const { Cell::new(0) };
    static GUEST_CALL_LIMIT: Cell<usize> = const { Cell::new(MAX_GUEST_CALL_DEPTH) };
}

/// The thread's stack budget, guest call depth or Wasm frame storage is spent: the caller
/// raises `STACK_EXHAUSTED_MESSAGE` as a RangeError.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StackExhausted;

impl std::fmt::Display for StackExhausted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(STACK_EXHAUSTED_MESSAGE)
    }
}

impl std::error::Error for StackExhausted {}

/// A transition is released on every return path, including Rust unwinding.
/// Thread affinity keeps the measurement correct across reentrant runtime owners.
pub struct StackGuard(PhantomData<Rc<()>>);
pub struct GuestCallGuard(PhantomData<Rc<()>>, usize);

/// Temporarily set the guest activation limit for a Node-compatible stack
/// size option. The limit is thread-local and restored when this guard drops.
pub struct GuestCallLimitGuard(usize);

impl GuestCallLimitGuard {
    pub fn enter(limit: usize) -> Self {
        let limit = limit.clamp(1, MAX_GUEST_CALL_DEPTH);
        let previous = GUEST_CALL_LIMIT.with(|active| active.replace(limit));
        Self(previous)
    }
}

impl Drop for GuestCallLimitGuard {
    fn drop(&mut self) {
        GUEST_CALL_LIMIT.with(|active| active.set(self.0));
    }
}

impl StackGuard {
    #[inline]
    pub fn enter() -> Result<Self, StackExhausted> {
        let current_stack_pointer = current_stack_pointer();
        ACTIVE_TRANSITIONS.with(|depth| {
            let current = depth.get();
            if current == 0 {
                STACK_ORIGIN.with(|origin| origin.set(current_stack_pointer));
            }
            let origin = STACK_ORIGIN.with(Cell::get);
            let distance = current_stack_pointer.abs_diff(origin);
            if current == max_recursive_transitions() || distance > stack_budget() {
                return Err(StackExhausted);
            }
            depth.set(current + 1);
            Ok(Self(PhantomData))
        })
    }
}

impl StackGuard {
    /// Enter a transition that has an alternative without native recursion, such as running
    /// a call on the native stack rather than in the caller's dispatch loop: it is admitted
    /// only within the optional share of the budget, so declining it leaves the transitions
    /// execution requires their headroom.
    #[inline]
    pub fn enter_optional() -> Option<Self> {
        let current_stack_pointer = current_stack_pointer();
        // The share of the transitions and stack distance an optional transition may reach: the
        // rest stays for the transitions execution requires.
        let within = ACTIVE_TRANSITIONS.with(Cell::get) < max_recursive_transitions() / 2
            && STACK_ORIGIN.with(Cell::get).checked_sub(current_stack_pointer)
                .is_none_or(|distance| distance < stack_budget() / 2);
        within.then(Self::enter).and_then(Result::ok)
    }
}

/// The transitions a recursion that runs nothing else may still enter, read once at its
/// entry: level `depth` below it is admitted exactly when `StackGuard::enter` would admit
/// it there, without the thread-local reads and writes of a guard per level.
pub struct StackBudget {
    remaining: usize,
    origin: usize,
}

impl StackBudget {
    pub fn here() -> Self {
        let active = ACTIVE_TRANSITIONS.with(Cell::get);
        let origin = if active == 0 { current_stack_pointer() } else { STACK_ORIGIN.with(Cell::get) };
        Self { remaining: max_recursive_transitions().saturating_sub(active), origin }
    }

    /// Whether a transition `depth` levels below the entry fits.
    #[inline]
    pub fn admits(&self, depth: usize) -> bool {
        depth < self.remaining && current_stack_pointer().abs_diff(self.origin) <= stack_budget()
    }

    /// Whether a tree `depth` levels deep is admitted at every level without visiting it: the
    /// levels fit the transitions left, and the deepest level fits the stack distance at the
    /// reserve each transition may use. A walk of such a tree admits every level it enters.
    #[inline]
    pub fn admits_tree(&self, depth: usize) -> bool {
        depth < self.remaining
            && current_stack_pointer()
                .abs_diff(self.origin)
                .saturating_add(depth.saturating_mul(STACK_TRANSITION_RESERVE_BYTES))
                <= stack_budget()
    }
}

/// The stack bounds a run of native calls is checked against, read once at the run's entry
/// (a lane run: `LaneStack::enter_generation`) instead of a `StackGuard`'s thread-local
/// reads and writes per call. While the transition that set the thread's origin is held, the
/// origin does not move, so the bounds stay exactly `StackGuard::enter`'s distance bound;
/// read with no transition held, the current stack pointer stands in for the origin a later
/// guard would set deeper, which only narrows them. A call checked here holds no transition
/// of its own: the transition count bounds the guards it enters inside, as before.
#[derive(Clone, Copy, Debug)]
pub struct StackLimit {
    floor: usize,
    ceiling: usize,
}

impl StackLimit {
    /// Bounds that admit nothing, before any run read its own.
    pub const CLOSED: Self = Self { floor: usize::MAX, ceiling: 0 };

    /// The bounds of the thread's current origin. They do not depend on the transition count,
    /// so bounds read by a nested run equal its caller's while the origin's transition is held.
    pub fn here() -> Self {
        let origin = match STACK_ORIGIN.with(Cell::get) {
            0 => current_stack_pointer(),
            origin => origin,
        };
        let budget = stack_budget();
        Self { floor: origin.saturating_sub(budget), ceiling: origin.saturating_add(budget) }
    }

    /// Whether the stack pointer here is within the budget: `StackGuard::enter`'s distance
    /// bound as two compares, with no thread-local access.
    #[inline(always)]
    pub fn admits(self) -> bool {
        let current = current_stack_pointer();
        current >= self.floor && current <= self.ceiling
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
    /// How many more guest calls the thread admits before `enter` fails.
    #[inline] pub fn remaining() -> usize {
        GUEST_CALL_LIMIT
            .with(Cell::get)
            .saturating_sub(ACTIVE_GUEST_CALLS.with(Cell::get))
    }

    /// How many more requested Wasm frame bytes the thread admits.
    #[inline] pub fn remaining_storage_bytes() -> usize {
        MAX_GUEST_FRAME_STORAGE_BYTES - ACTIVE_GUEST_FRAME_STORAGE_BYTES.with(Cell::get)
    }

    #[inline] pub fn enter() -> Result<Self, StackExhausted> {
        Self::enter_with_storage(0)
    }

    /// Enter a guest activation and account its requested Wasm locals/registers.
    /// JavaScript activations use `enter`, which consumes depth but no Wasm storage.
    #[inline] pub fn enter_with_storage(storage_bytes: usize) -> Result<Self, StackExhausted> {
        ACTIVE_GUEST_CALLS.with(|depth| {
            let current = depth.get();
            let used_storage = ACTIVE_GUEST_FRAME_STORAGE_BYTES.with(Cell::get);
            if current >= GUEST_CALL_LIMIT.with(Cell::get)
                || storage_bytes > MAX_GUEST_FRAME_STORAGE_BYTES
                || storage_bytes > MAX_GUEST_FRAME_STORAGE_BYTES - used_storage
            {
                return Err(StackExhausted);
            }
            depth.set(current + 1);
            ACTIVE_GUEST_FRAME_STORAGE_BYTES.with(|storage| {
                storage.set(used_storage + storage_bytes);
            });
            Ok(Self(PhantomData, storage_bytes))
        })
    }
}

impl Drop for GuestCallGuard {
    #[inline] fn drop(&mut self) {
        ACTIVE_GUEST_CALLS.with(|depth| depth.set(depth.get() - 1));
        ACTIVE_GUEST_FRAME_STORAGE_BYTES
            .with(|storage| storage.set(storage.get() - self.1));
    }
}

/// The address the calling frame sits at: the stack pointer, to within a frame.
#[inline(always)]
pub fn current_stack_pointer() -> usize {
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
    fn guest_frame_storage_is_bounded_and_released() {
        let full = GuestCallGuard::enter_with_storage(MAX_GUEST_FRAME_STORAGE_BYTES).unwrap();
        assert_eq!(GuestCallGuard::remaining_storage_bytes(), 0);
        assert!(GuestCallGuard::enter_with_storage(1).is_err());
        assert!(GuestCallGuard::enter_with_storage(MAX_GUEST_FRAME_STORAGE_BYTES + 1).is_err());
        drop(full);
        assert_eq!(
            GuestCallGuard::remaining_storage_bytes(),
            MAX_GUEST_FRAME_STORAGE_BYTES
        );
        let guard = GuestCallGuard::enter_with_storage(1).unwrap();
        drop(guard);
        assert_eq!(ACTIVE_GUEST_FRAME_STORAGE_BYTES.with(Cell::get), 0);
    }

    #[test]
    fn stack_limit_matches_the_guard_distance_bound() {
        #[inline(never)]
        fn descend(limit: StackLimit, depth: usize, limit_failed: &mut Option<usize>) -> usize {
            let frame = [0_u8; NATIVE_TEST_FRAME_BYTES];
            std::hint::black_box(&frame);
            if !limit.admits() && limit_failed.is_none() {
                *limit_failed = Some(depth);
            }
            let Ok(_guard) = StackGuard::enter() else { return depth };
            descend(limit, depth + 1, limit_failed)
        }

        std::thread::Builder::new()
            .stack_size(WORKER_STACK_SIZE)
            .spawn(|| {
                let _outer = StackGuard::enter().unwrap();
                let limit = StackLimit::here();
                assert!(limit.admits());
                let mut limit_failed = None;
                let guard_failed = descend(limit, 0, &mut limit_failed);
                // The limit read under the outer guard closes where a guard does, give or
                // take the frame its own stack pointer is read in.
                let limit_failed = limit_failed.expect("the limit closes before the stack ends");
                assert!(guard_failed > 0 && limit_failed.abs_diff(guard_failed) <= 1);
                assert!(!StackLimit::CLOSED.admits());
            })
            .unwrap()
            .join()
            .unwrap();
    }

    #[test]
    fn native_stack_distance_exhausts_and_resets() {
        #[inline(never)]
        fn descend(remaining: usize) -> Result<(), StackExhausted> {
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
