//! The byte store behind an ArrayBuffer.
//!
//! An ordinary buffer owns its bytes, shared copy-on-write with clones of the cell. The
//! buffer a `WebAssembly.Memory` exposes instead views the memory's linear bytes, so
//! JavaScript and Wasm see each other's writes. Such a view has a fixed length: growing the
//! memory detaches the view (replacing it with an empty owned store) before the linear bytes
//! can move, so a live view's pointer always names the memory's current bytes.

use crate::wasm::memory::MemoryStorage;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// One word, so an ArrayBuffer cell stays within the heap's cell size.
#[derive(Debug, Default)]
pub(crate) struct BufferBytes(Rc<BufferStore>);

#[derive(Clone, Debug)]
enum BufferStore {
    Owned(Vec<u8>),
    /// A fixed buffer whose physical alignment was observed by an embedder.
    /// Clones eagerly copy, so later writes cannot relocate this allocation.
    AddressPinned(Vec<u8>),
    /// Bytes shared with an owner outside the heap, such as a module's data segment;
    /// copied into an owned store on the first write.
    Shared(Rc<Vec<u8>>),
    Wasm(WasmView),
    /// A SharedArrayBuffer data block that other agents (threads) may hold too.
    Concurrent(Arc<ConcurrentBytes>),
}

/// The widest element an `Atomics` operation addresses (BigInt64), which aligns the block.
const CONCURRENT_ALIGNMENT: usize = std::mem::size_of::<u64>();

/// A SharedArrayBuffer's data block, shared by every agent holding the buffer. The
/// allocation spans the maximum byte length and never moves; only the accessible length
/// grows. Ordinary JavaScript reads and writes are unsynchronized, as the memory model
/// permits for data races; `Atomics` operations synchronize among themselves.
#[derive(Debug)]
pub(crate) struct ConcurrentBytes {
    base: std::ptr::NonNull<u8>,
    capacity: usize,
    len: AtomicUsize,
}

// SAFETY: the block is plain bytes owned by the Arc; races on its contents are the
// SharedArrayBuffer memory model, not Rust aliasing of the block's bookkeeping.
unsafe impl Send for ConcurrentBytes {}
// SAFETY: as above.
unsafe impl Sync for ConcurrentBytes {}

impl ConcurrentBytes {
    /// `len` zero bytes growable to `capacity`; `None` when the host refuses the memory.
    pub(crate) fn zeroed(len: usize, capacity: usize) -> Option<Arc<Self>> {
        let layout = std::alloc::Layout::from_size_align(capacity.max(1), CONCURRENT_ALIGNMENT).ok()?;
        // SAFETY: a non-zero-sized layout.
        let base = std::ptr::NonNull::new(unsafe { std::alloc::alloc_zeroed(layout) })?;
        Some(Arc::new(Self { base, capacity, len: AtomicUsize::new(len.min(capacity)) }))
    }

    pub(crate) fn len(&self) -> usize {
        self.len.load(Ordering::Acquire)
    }

    /// Grow the accessible length to `len`; a block never shrinks. False beyond capacity.
    pub(crate) fn grow_to(&self, len: usize) -> bool {
        if len > self.capacity {
            return false;
        }
        self.len.fetch_max(len, Ordering::AcqRel);
        true
    }

    /// The address of byte `offset`; callers stay within the accessible length.
    pub(crate) fn address(&self, offset: usize) -> *mut u8 {
        self.base.as_ptr().wrapping_add(offset)
    }
}

impl Drop for ConcurrentBytes {
    fn drop(&mut self) {
        let layout = std::alloc::Layout::from_size_align(self.capacity.max(1), CONCURRENT_ALIGNMENT)
            .expect("layout validated at allocation");
        // SAFETY: allocated by `zeroed` with this layout; the last owner frees it.
        unsafe { std::alloc::dealloc(self.base.as_ptr(), layout) };
    }
}

impl Clone for BufferBytes {
    fn clone(&self) -> Self {
        if matches!(*self.0, BufferStore::AddressPinned(_)) {
            Self::from(self.to_vec())
        } else {
            Self(self.0.clone())
        }
    }
}

impl Default for BufferStore {
    fn default() -> Self {
        Self::Owned(Vec::new())
    }
}

/// The accessible bytes of an unshared Wasm memory when the view was created.
#[derive(Clone, Debug)]
struct WasmView {
    /// Keeps the linear bytes alive while the view exists; never read.
    _memory: Rc<MemoryStorage>,
    base: *mut u8,
    len: usize,
}

impl From<Vec<u8>> for BufferBytes {
    fn from(bytes: Vec<u8>) -> Self {
        Self(Rc::new(BufferStore::Owned(bytes)))
    }
}

impl From<Rc<Vec<u8>>> for BufferBytes {
    fn from(bytes: Rc<Vec<u8>>) -> Self {
        Self(Rc::new(BufferStore::Shared(bytes)))
    }
}

impl BufferBytes {
    /// A store over a SharedArrayBuffer data block.
    pub(crate) fn concurrent(block: Arc<ConcurrentBytes>) -> Self {
        Self(Rc::new(BufferStore::Concurrent(block)))
    }

    /// The data block when other agents may share these bytes.
    pub(crate) fn concurrent_block(&self) -> Option<&Arc<ConcurrentBytes>> {
        match &*self.0 {
            BufferStore::Concurrent(block) => Some(block),
            _ => None,
        }
    }

    /// A view of an unshared memory's current bytes; `None` for a shared memory.
    pub(crate) fn wasm_view(memory: &Rc<MemoryStorage>) -> Option<Self> {
        let MemoryStorage::Unshared(state) = &**memory else {
            return None;
        };
        let mut state = state.borrow_mut();
        let bytes: &mut [u8] = &mut state.bytes;
        Some(Self(Rc::new(BufferStore::Wasm(WasmView {
            _memory: memory.clone(),
            base: bytes.as_mut_ptr(),
            len: bytes.len(),
        }))))
    }

    /// Whether these bytes view a Wasm memory, which JavaScript cannot detach or resize.
    pub(crate) fn is_wasm_view(&self) -> bool {
        matches!(*self.0, BufferStore::Wasm(_))
    }

    /// Whether the store holds a reference counted outside its own allocation (shared
    /// bytes or a memory), which the striped drop locks do not cover.
    pub(crate) fn holds_external_rc(&self) -> bool {
        !matches!(*self.0, BufferStore::Owned(_) | BufferStore::AddressPinned(_))
    }

    /// The store's allocation, which identifies it across the cells sharing it.
    pub(crate) fn as_ptr(&self) -> *const () {
        Rc::as_ptr(&self.0).cast()
    }

    /// Whether dropping this handle may release memory other handles still reach: a
    /// shared store, or a view keeping its memory's bytes alive.
    pub(crate) fn is_shared(&self) -> bool {
        Rc::strong_count(&self.0) > 1 || self.holds_external_rc()
    }

    /// The heap bytes this store owns; a view's bytes belong to its memory.
    pub(crate) fn capacity(&self) -> usize {
        match &*self.0 {
            BufferStore::Owned(bytes) | BufferStore::AddressPinned(bytes) => bytes.capacity(),
            BufferStore::Shared(_) | BufferStore::Wasm(_) | BufferStore::Concurrent(_) => 0,
        }
    }

    /// Preserve this fixed allocation when future cell snapshots are cloned.
    pub(crate) fn pin_address(&mut self) -> usize {
        let store = Rc::make_mut(&mut self.0);
        match store {
            BufferStore::Owned(bytes) => {
                let bytes = std::mem::take(bytes);
                *store = BufferStore::AddressPinned(bytes);
            }
            BufferStore::Shared(bytes) => {
                *store = BufferStore::AddressPinned(bytes.to_vec());
            }
            BufferStore::AddressPinned(_) | BufferStore::Wasm(_) | BufferStore::Concurrent(_) => {}
        }
        self.as_ptr_bytes() as usize
    }

    pub(crate) fn as_ptr_bytes(&self) -> *const u8 {
        std::ops::Deref::deref(self).as_ptr()
    }

    /// The bytes for writing; an owned store is unshared from other cells first.
    pub(crate) fn make_mut(&mut self) -> &mut [u8] {
        if let BufferStore::Concurrent(block) = &*self.0 {
            // SAFETY: the block spans its accessible length; races are the memory model's.
            return unsafe { std::slice::from_raw_parts_mut(block.address(0), block.len()) };
        }
        let store = Rc::make_mut(&mut self.0);
        if let BufferStore::Shared(bytes) = store {
            *store = BufferStore::Owned(bytes.to_vec());
        }
        match store {
            BufferStore::Owned(bytes) | BufferStore::AddressPinned(bytes) => bytes,
            BufferStore::Shared(_) | BufferStore::Concurrent(_) => unreachable!("unshared above"),
            // SAFETY: a live view names the memory's current bytes (see the module
            // comment), and no other borrow of them is held while JavaScript runs.
            BufferStore::Wasm(view) => unsafe {
                std::slice::from_raw_parts_mut(view.base, view.len)
            },
        }
    }

    /// The bytes as an owned vector, moved out when this handle is their only owner.
    pub(crate) fn into_vec(self) -> Vec<u8> {
        match Rc::try_unwrap(self.0) {
            Ok(BufferStore::Owned(bytes) | BufferStore::AddressPinned(bytes)) => bytes,
            Ok(store) => Self(Rc::new(store)).to_vec(),
            Err(shared) => Self(shared).to_vec(),
        }
    }

    /// The owned bytes for resizing. Views are never resizable buffers.
    pub(crate) fn owned_mut(&mut self) -> &mut Vec<u8> {
        let store = Rc::make_mut(&mut self.0);
        if let BufferStore::Shared(bytes) = store {
            *store = BufferStore::Owned(bytes.to_vec());
        }
        match store {
            BufferStore::Owned(bytes) => bytes,
            BufferStore::AddressPinned(_) => unreachable!("address-pinned ArrayBuffer is fixed length"),
            BufferStore::Shared(_) => unreachable!("unshared above"),
            BufferStore::Wasm(_) => unreachable!("a Wasm memory view is not resizable"),
            BufferStore::Concurrent(_) => unreachable!("a shared data block grows in place"),
        }
    }
}

impl std::ops::Deref for BufferBytes {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        match &*self.0 {
            BufferStore::Owned(bytes) | BufferStore::AddressPinned(bytes) => bytes,
            BufferStore::Shared(bytes) => bytes,
            // SAFETY: as for `make_mut`.
            BufferStore::Wasm(view) => unsafe { std::slice::from_raw_parts(view.base, view.len) },
            // SAFETY: the block spans its accessible length.
            BufferStore::Concurrent(block) => unsafe { std::slice::from_raw_parts(block.address(0), block.len()) },
        }
    }
}

#[cfg(test)]
mod address_tests {
    use super::BufferBytes;
    #[test]
    fn observed_backing_addresses_survive_snapshots_and_independent_writes() {
        let mut original = BufferBytes::from(vec![0; 4096]);
        let address = original.pin_address();
        let mut snapshot = original.clone();
        original.make_mut()[0] = 42;
        snapshot.make_mut()[1] = 17;
        assert_eq!(original.as_ptr_bytes() as usize, address);
        assert_eq!(original[0], 42);
        assert_eq!(original[1], 0);
        assert_eq!(snapshot[0], 0);
        assert_eq!(snapshot[1], 17);
        assert_eq!(original.pin_address(), address);
    }
}
