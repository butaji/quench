//! The memory bytes code addresses in place.

use super::{LinearBytes, MemoryStorage};

impl MemoryStorage {
    /// The bytes compiled and lane code address in place, and the memory's guard reservation.
    /// An unshared memory's while no suspended host access borrows them. A shared memory's
    /// when they never move (a guarded reservation): agents' plain accesses may race, and the
    /// memory model gives a racing access some bytes, while atomics keep taking the lock. The
    /// length is the memory's now; other agents only grow it, so an access past a stale
    /// length takes the general path, which locks.
    pub fn direct_view(&self) -> Option<((*mut u8, usize), Option<(*mut u8, usize)>)> {
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

    /// Whether code may address the memory's bytes in place (`direct_view`, `addressable_in_place`).
    pub fn is_direct(&self) -> bool {
        match self {
            Self::Unshared(_) => addressable_in_place(false, false),
            Self::Shared(memory) => addressable_in_place(true, memory.0.lock().bytes.guard().is_some()),
        }
    }
}

impl LinearBytes {
    /// Grows by `delta` pages of `page_size` bytes: the size in pages before, or `None` where
    /// the new length passes `maximum` bytes or the host refuses the memory, and nothing
    /// changes. The one place the rule of `memory.grow` is written.
    pub(crate) fn grow_pages(&mut self, delta: u64, page_size: usize, maximum: usize) -> Option<u64> {
        let length = usize::try_from(delta)
            .ok()
            .and_then(|delta| delta.checked_mul(page_size))
            .and_then(|extra| self.len().checked_add(extra))
            .filter(|length| *length <= maximum)?;
        let pages = (self.len() / page_size) as u64;
        self.grow_to(length).then_some(pages)
    }
}

/// Whether code may address a memory's bytes in place: an unshared memory, or a shared one whose bytes
/// never move (a guarded reservation). The one rule an instance's memory (`MemoryStorage::is_direct`) and a
/// module's declaration (`WasmModule::binding_types`, which knows no reservation ahead of an instance) read.
pub fn addressable_in_place(shared: bool, guarded: bool) -> bool {
    !shared || guarded
}
