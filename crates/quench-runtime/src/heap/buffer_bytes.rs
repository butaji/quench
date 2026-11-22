//! The byte store behind an ArrayBuffer.
//!
//! An ordinary buffer owns its bytes, shared copy-on-write with clones of the cell. The
//! buffer a `WebAssembly.Memory` exposes instead views the memory's linear bytes, so
//! JavaScript and Wasm see each other's writes. Such a view has a fixed length: growing the
//! unshared memory detaches its view before the linear bytes can move. Shared views lock
//! the common backing store on every access and retain their length after memory growth.

use crate::shared_backing::{SharedBacking, State};
use crate::wasm::memory::MemoryStorage;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, MutexGuard};

/// The count of events that move, free or share a buffer's bytes: a copy before a write, a
/// resize, a detach or replacement, a second handle on the store. A typed array view
/// published for native code (`NativeView`) names the count it was published at, and is
/// stale once it moves: the one invalidation of every published address, for the process
/// (an event of another heap only sends a view back to its publication).
static BUFFER_RELOCATIONS: AtomicU64 = AtomicU64::new(0);

/// The current `BUFFER_RELOCATIONS`.
#[inline(always)]
pub(crate) fn buffer_relocations() -> u64 {
    BUFFER_RELOCATIONS.load(Ordering::Relaxed)
}

/// Where native code reads `BUFFER_RELOCATIONS`.
pub fn buffer_relocations_address() -> *const u64 {
    BUFFER_RELOCATIONS.as_ptr().cast_const()
}

fn note_relocation() {
    BUFFER_RELOCATIONS.fetch_add(1, Ordering::Relaxed);
}

/// One word, so an ArrayBuffer cell stays within the heap's cell size.
#[derive(Debug)]
pub struct BufferBytes(Rc<BufferStore>);

/// The typed array view epoch (`view_epoch`): it changes whenever a typed array's element
/// bytes may have moved, been unshared or detached, or a slot a view occupied may hold another
/// cell, so a view's element address and length cached at one epoch
/// (`Vm::typed_cached_element`) hold while the epoch is unchanged. One counter for the process:
/// another thread's heap ending an epoch only makes this one's entries refill, and every end
/// is an atomic increment, so no end is lost. A plain load reads it (no thread-local access
/// on the element hit path).
static VIEW_EPOCH: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// The current typed array view epoch (`VIEW_EPOCH`).
#[inline(always)]
pub fn view_epoch() -> u64 {
    VIEW_EPOCH.load(std::sync::atomic::Ordering::Relaxed)
}

/// Ends the current typed array view epoch: every store made, dropped, cloned or written
/// other than in place (`BufferBytes::mut_in_lane`), and every sweep that frees slots.
#[inline]
pub(crate) fn bump_view_epoch() {
    VIEW_EPOCH.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

impl Default for BufferBytes {
    fn default() -> Self {
        bump_view_epoch();
        Self(Rc::default())
    }
}

/// The largest alignment an embedder observes of a pinned store: the widest typed array
/// element, which the offsets it computes align views to.
const OBSERVED_ALIGNMENT_BOUND: usize = size_of::<f64>();

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
    AtomicShared(Arc<SharedBacking>),
}

impl Clone for BufferBytes {
    fn clone(&self) -> Self {
        // A second handle makes the store shared: no view writes it in place after this.
        bump_view_epoch();
        if matches!(*self.0, BufferStore::AddressPinned(_)) {
            Self::from(self.to_vec())
        } else {
            // A store two handles share is copied before either writes.
            note_relocation();
            Self(self.0.clone())
        }
    }
}

/// The last handle on a store frees it: a view published over it points at nothing.
impl Drop for BufferBytes {
    fn drop(&mut self) {
        bump_view_epoch();
        if Rc::strong_count(&self.0) == 1 {
            note_relocation();
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
    /// The bytes a snapshot copies (`crate::snapshot`): an owned or host-shared store's. A
    /// Wasm memory's view, atomically shared memory and bytes whose address an embedder
    /// observed have identities a later process cannot restore.
    /// What a snapshot keeps of this store (`crate::snapshot`): its bytes and, for a store
    /// whose address an embedder observed, that address's residue modulo
    /// `OBSERVED_ALIGNMENT_BOUND`, which the restored store must share. `None` for a store
    /// over memory the snapshot cannot hold (Wasm or atomically shared).
    pub(crate) fn snapshot_contents(&self) -> Option<(&[u8], Option<usize>)> {
        match &*self.0 {
            BufferStore::Owned(bytes) => Some((bytes, None)),
            BufferStore::Shared(bytes) => Some((bytes, None)),
            BufferStore::AddressPinned(bytes) => Some((bytes, Some(bytes.as_ptr() as usize % OBSERVED_ALIGNMENT_BOUND))),
            BufferStore::Wasm(_) | BufferStore::AtomicShared(_) => None,
        }
    }

    /// The store `snapshot_contents` described; `None` when a pinned store's new address does
    /// not keep the residue its embedder observed.
    pub(crate) fn restored(bytes: Vec<u8>, pinned_residue: Option<usize>) -> Option<Self> {
        match pinned_residue {
            None => Some(Self::from(bytes)),
            Some(residue) => (bytes.as_ptr() as usize % OBSERVED_ALIGNMENT_BOUND == residue)
                .then(|| Self(Rc::new(BufferStore::AddressPinned(bytes)))),
        }
    }

    /// A fixed-length view of the memory, sharing its storage and synchronization.
    pub(crate) fn wasm_view(memory: &Rc<MemoryStorage>) -> Option<Self> {
        let state = match &**memory {
            MemoryStorage::Unshared(state) => state,
            MemoryStorage::Shared(memory) => return Some(Self::from_shared(memory.0.fixed_view())),
        };
        let mut state = state.borrow_mut();
        let bytes: &mut [u8] = &mut state;
        Some(Self(Rc::new(BufferStore::Wasm(WasmView {
            _memory: memory.clone(),
            base: bytes.as_mut_ptr(),
            len: bytes.len(),
        }))))
    }

    /// Whether these bytes directly view an unshared Wasm memory, which JavaScript
    /// cannot detach or resize. Shared buffers have their own detachment prohibition.
    pub(crate) fn is_wasm_view(&self) -> bool {
        matches!(*self.0, BufferStore::Wasm(_))
    }

    /// Whether these bytes share this exact Wasm memory's data block.
    pub(crate) fn is_wasm_view_of(&self, memory: &Rc<MemoryStorage>) -> bool {
        match (&*self.0, &**memory) {
            (BufferStore::Wasm(view), _) => Rc::ptr_eq(&view._memory, memory),
            (BufferStore::AtomicShared(view), MemoryStorage::Shared(memory)) => view.shares_data_block(&memory.0),
            _ => false,
        }
    }

    /// Whether the store holds a reference counted outside its own allocation (shared
    /// bytes or a memory), which the striped drop locks do not cover.
    pub(crate) fn holds_external_rc(&self) -> bool {
        !matches!(
            *self.0,
            BufferStore::Owned(_) | BufferStore::AddressPinned(_)
        )
    }

    /// The store's allocation, which identifies it across the cells sharing it.
    pub(crate) fn as_ptr(&self) -> *const () {
        Rc::as_ptr(&self.0).cast()
    }

    /// Whether dropping this handle may release memory other handles still reach: a
    /// shared store, or a view keeping its memory's bytes alive.
    pub(crate) fn shares_memory(&self) -> bool {
        Rc::strong_count(&self.0) > 1 || self.holds_external_rc()
    }

    /// The heap bytes this store owns; a view's bytes belong to its memory.
    pub(crate) fn capacity(&self) -> usize {
        match &*self.0 {
            BufferStore::Owned(bytes) | BufferStore::AddressPinned(bytes) => bytes.capacity(),
            BufferStore::Shared(_) | BufferStore::Wasm(_) | BufferStore::AtomicShared(_) => 0,
        }
    }

    /// Preserve this fixed allocation when future cell snapshots are cloned.
    pub(crate) fn pin_address(&mut self) -> usize {
        bump_view_epoch();
        let store = self.unshared_store();
        match store {
            BufferStore::Owned(bytes) => {
                let bytes = std::mem::take(bytes);
                *store = BufferStore::AddressPinned(bytes);
            }
            BufferStore::Shared(bytes) => {
                *store = BufferStore::AddressPinned(bytes.to_vec());
                note_relocation();
            }
            BufferStore::AddressPinned(_) | BufferStore::Wasm(_) => {}
            BufferStore::AtomicShared(_) => {
                panic!("shared backing cannot expose an unguarded address")
            }
        }
        self.as_ptr_bytes() as usize
    }

    pub(crate) fn as_ptr_bytes(&self) -> *const u8 {
        match &*self.0 {
            BufferStore::AtomicShared(_) => {
                panic!("shared backing cannot expose unguarded pointer")
            }
            _ => self.read().as_ptr(),
        }
    }

    /// The store for writing, copied first where another handle shares it.
    fn unshared_store(&mut self) -> &mut BufferStore {
        if Rc::strong_count(&self.0) > 1 {
            note_relocation();
        }
        Rc::make_mut(&mut self.0)
    }

    /// The bytes for writing; an owned store is unshared from other cells first.
    #[inline]
    pub(crate) fn make_mut(&mut self) -> BufferWrite<'_> {
        bump_view_epoch();
        let store = self.unshared_store();
        if let BufferStore::Shared(bytes) = store {
            *store = BufferStore::Owned(bytes.to_vec());
            note_relocation();
        }
        match store {
            BufferStore::Owned(bytes) | BufferStore::AddressPinned(bytes) => {
                BufferWrite::Borrowed(bytes)
            }
            BufferStore::AtomicShared(backing) => BufferWrite::Shared { state: backing.lock(), length: backing.len_for_view() },
            BufferStore::Shared(_) => unreachable!("unshared above"),
            BufferStore::Wasm(view) => BufferWrite::Borrowed(unsafe {
                std::slice::from_raw_parts_mut(view.base, view.len)
            }),
        }
    }
    /// Another handle on these bytes, for reading them while a different buffer is written:
    /// only for stores that any write copies first, so no write can reach the bytes it reads.
    pub(crate) fn unaliased_handle(&self) -> Option<Self> {
        bump_view_epoch();
        matches!(*self.0, BufferStore::Owned(_) | BufferStore::Shared(_)).then(|| Self(self.0.clone()))
    }
    pub(crate) fn from_shared(backing: Arc<SharedBacking>) -> Self {
        Self(Rc::new(BufferStore::AtomicShared(backing)))
    }
    pub(crate) fn shared_backing(&self) -> Option<Arc<SharedBacking>> {
        match &*self.0 {
            BufferStore::AtomicShared(b) => Some(b.clone()),
            _ => None,
        }
    }
    #[inline]
    pub(crate) fn len(&self) -> usize {
        match &*self.0 {
            BufferStore::AtomicShared(b) => b.length().expect("shared backing length"),
            BufferStore::Owned(bytes) | BufferStore::AddressPinned(bytes) => bytes.len(),
            BufferStore::Shared(bytes) => bytes.len(),
            BufferStore::Wasm(v) => v.len,
        }
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// The bytes of a store this heap owns, read in place with no lock: the typed array
    /// element hit path (`Vm::typed_element_quick`). `None` for a shared, atomic or Wasm
    /// store, which the general read serves.
    #[inline(always)]
    pub(crate) fn owned(&self) -> Option<&[u8]> {
        match &*self.0 {
            BufferStore::Owned(bytes) | BufferStore::AddressPinned(bytes) => Some(bytes),
            _ => None,
        }
    }
    pub(crate) fn to_vec(&self) -> Vec<u8> {
        self.read().to_vec()
    }
    pub(crate) fn read(&self) -> BufferRead<'_> {
        match &*self.0 {
            BufferStore::Owned(bytes) | BufferStore::AddressPinned(bytes) => {
                BufferRead::Borrowed(bytes)
            }
            BufferStore::Shared(bytes) => BufferRead::Borrowed(bytes),
            BufferStore::AtomicShared(b) => BufferRead::Shared { state: b.lock(), length: b.len_for_view() },
            BufferStore::Wasm(v) => {
                BufferRead::Borrowed(unsafe { std::slice::from_raw_parts(v.base, v.len) })
            }
        }
    }

    /// A non-allocating mutable view for fast-lane operations. Shared,
    /// address-pinned, Wasm and atomic backing stores stay on the general path.
    #[inline(always)]
    pub(crate) fn mut_in_lane(&mut self) -> Option<&mut [u8]> {
        match Rc::get_mut(&mut self.0)? {
            BufferStore::Owned(bytes) => Some(bytes),
            BufferStore::AddressPinned(_)
            | BufferStore::Shared(_)
            | BufferStore::Wasm(_)
            | BufferStore::AtomicShared(_) => None,
        }
    }

    /// The bytes as one owned vector, without a copy when this is their only owner.
    pub(crate) fn into_vec(mut self) -> Vec<u8> {
        match Rc::try_unwrap(std::mem::take(&mut self.0)) {
            Ok(BufferStore::Owned(bytes) | BufferStore::AddressPinned(bytes)) => bytes,
            Ok(store) => Self(Rc::new(store)).to_vec(),
            Err(shared) => Self(shared).to_vec(),
        }
    }

    /// The owned bytes for resizing. Views are never resizable buffers.
    pub(crate) fn owned_mut(&mut self) -> &mut Vec<u8> {
        bump_view_epoch();
        // The caller resizes the vector, which may move it.
        note_relocation();
        let store = self.unshared_store();
        if let BufferStore::Shared(bytes) = store {
            *store = BufferStore::Owned(bytes.to_vec());
        }
        match store {
            BufferStore::Owned(bytes) => bytes,
            BufferStore::AddressPinned(_) => {
                unreachable!("address-pinned ArrayBuffer is fixed length")
            }
            BufferStore::Shared(_) => unreachable!("unshared above"),
            BufferStore::Wasm(_) => unreachable!("a Wasm memory view is not resizable"),
            BufferStore::AtomicShared(_) => {
                unreachable!("shared grows through synchronized backing")
            }
        }
    }
}

/// A buffer's bytes for one access: borrowed directly, or under the shared backing's lock
/// for as many bytes as views may see.
pub(crate) enum BufferAccess<'a, B> {
    Borrowed(B),
    Shared { state: MutexGuard<'a, State>, length: Option<usize> },
}
pub type BufferRead<'a> = BufferAccess<'a, &'a [u8]>;
pub(crate) type BufferWrite<'a> = BufferAccess<'a, &'a mut [u8]>;

impl<B: std::ops::Deref<Target = [u8]>> std::ops::Deref for BufferAccess<'_, B> {
    type Target = [u8];
    #[inline(always)]
    fn deref(&self) -> &[u8] {
        match self {
            Self::Borrowed(b) => b,
            Self::Shared { state, length } => &state.bytes[..length.unwrap_or(state.bytes.len())],
        }
    }
}
impl std::ops::DerefMut for BufferWrite<'_> {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut [u8] {
        match self {
            Self::Borrowed(b) => b,
            Self::Shared { state, length } => {
                let length = length.unwrap_or(state.bytes.len());
                &mut state.bytes[..length]
            },
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
        assert_eq!(original.read()[0], 42);
        assert_eq!(original.read()[1], 0);
        assert_eq!(snapshot.read()[0], 0);
        assert_eq!(snapshot.read()[1], 17);
        assert_eq!(original.pin_address(), address);
    }
}
