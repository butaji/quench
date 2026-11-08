//! The memory bytes code addresses in place.

use super::{LinearBytes, MemoryStorage};

impl MemoryStorage {
    /// The bytes compiled and lane code address in place, and the memory's guard reservation.
    /// An unshared memory's while no suspended host access borrows them. A shared memory's
    /// when they never move (a guarded reservation): agents' plain accesses may race, and the
    /// memory model gives a racing access some bytes, while atomics keep taking the lock. The
    /// length is the memory's now; other agents only grow it, so an access past a stale
    /// length takes the general path, which locks.
    pub(crate) fn direct_view(&self) -> Option<((*mut u8, usize), Option<(*mut u8, usize)>)> {
        if !self.is_direct() {
            return None;
        }
        let view = |bytes: &mut LinearBytes| ((bytes.as_mut_ptr(), bytes.len()), bytes.guard());
        let (bytes, guard) = match self {
            Self::Unshared(state) => view(&mut *state.try_borrow_mut().ok()?),
            Self::Shared(memory) => view(&mut memory.0.lock().bytes),
        };
        (!bytes.0.is_null()).then_some((bytes, guard))
    }

    /// Whether code may address the memory's bytes in place (`direct_view`): an unshared
    /// memory, or a shared one whose bytes never move.
    pub(crate) fn is_direct(&self) -> bool {
        match self {
            Self::Unshared(_) => true,
            Self::Shared(memory) => memory.0.lock().bytes.guard().is_some(),
        }
    }
}
