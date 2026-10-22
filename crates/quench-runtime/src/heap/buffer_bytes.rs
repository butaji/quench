//! The byte store behind an ArrayBuffer.
//!
//! An ordinary buffer owns its bytes, shared copy-on-write with clones of the cell. The
//! buffer a `WebAssembly.Memory` exposes instead views the memory's linear bytes, so
//! JavaScript and Wasm see each other's writes. Such a view has a fixed length: growing the
//! memory detaches the view (replacing it with an empty owned store) before the linear bytes
//! can move, so a live view's pointer always names the memory's current bytes.

use crate::shared_backing::{SharedBacking, State};
use crate::wasm::memory::MemoryStorage;
use std::rc::Rc;
use std::sync::{Arc, MutexGuard};

/// One word, so an ArrayBuffer cell stays within the heap's cell size.
#[derive(Debug, Default)]
pub(crate) struct BufferBytes(Rc<BufferStore>);

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

    /// Whether these bytes are a view of this exact unshared Wasm memory.
    pub(crate) fn is_wasm_view_of(&self, memory: &Rc<MemoryStorage>) -> bool {
        matches!(
            &*self.0,
            BufferStore::Wasm(view) if Rc::ptr_eq(&view._memory, memory)
        )
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
    pub(crate) fn is_shared(&self) -> bool {
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
        let store = Rc::make_mut(&mut self.0);
        match store {
            BufferStore::Owned(bytes) => {
                let bytes = std::mem::take(bytes);
                *store = BufferStore::AddressPinned(bytes);
            }
            BufferStore::Shared(bytes) => {
                *store = BufferStore::AddressPinned(bytes.to_vec());
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

    /// The bytes for writing; an owned store is unshared from other cells first.
    pub(crate) fn make_mut(&mut self) -> BufferWrite<'_> {
        let store = Rc::make_mut(&mut self.0);
        if let BufferStore::Shared(bytes) = store {
            *store = BufferStore::Owned(bytes.to_vec());
        }
        match store {
            BufferStore::Owned(bytes) | BufferStore::AddressPinned(bytes) => {
                BufferWrite::Borrowed(bytes)
            }
            BufferStore::AtomicShared(backing) => BufferWrite::Shared(backing.lock()),
            BufferStore::Shared(_) => unreachable!("unshared above"),
            BufferStore::Wasm(view) => BufferWrite::Borrowed(unsafe {
                std::slice::from_raw_parts_mut(view.base, view.len)
            }),
        }
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
    pub(crate) fn len(&self) -> usize {
        match &*self.0 {
            BufferStore::AtomicShared(b) => b.length().expect("shared backing length"),
            _ => self.read().len(),
        }
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.len() == 0
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
            BufferStore::AtomicShared(b) => BufferRead::Shared(b.lock()),
            BufferStore::Wasm(v) => {
                BufferRead::Borrowed(unsafe { std::slice::from_raw_parts(v.base, v.len) })
            }
        }
    }

    /// A non-allocating mutable view for fast-lane operations. Shared,
    /// address-pinned, Wasm and atomic backing stores stay on the general path.
    pub(crate) fn lane_mut(&mut self) -> Option<&mut [u8]> {
        match Rc::get_mut(&mut self.0)? {
            BufferStore::Owned(bytes) => Some(bytes),
            BufferStore::AddressPinned(_)
            | BufferStore::Shared(_)
            | BufferStore::Wasm(_)
            | BufferStore::AtomicShared(_) => None,
        }
    }

    /// The bytes as one owned vector, without a copy when this is their only owner.
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

pub(crate) enum BufferRead<'a> {
    Borrowed(&'a [u8]),
    Shared(MutexGuard<'a, State>),
}
impl std::ops::Deref for BufferRead<'_> {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        match self {
            Self::Borrowed(b) => b,
            Self::Shared(s) => &s.bytes,
        }
    }
}
pub(crate) enum BufferWrite<'a> {
    Borrowed(&'a mut [u8]),
    Shared(MutexGuard<'a, State>),
}
impl std::ops::Deref for BufferWrite<'_> {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        match self {
            Self::Borrowed(b) => b,
            Self::Shared(s) => &s.bytes,
        }
    }
}
impl std::ops::DerefMut for BufferWrite<'_> {
    fn deref_mut(&mut self) -> &mut [u8] {
        match self {
            Self::Borrowed(b) => b,
            Self::Shared(s) => &mut s.bytes,
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
