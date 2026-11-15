//! The cache of typed array views' element addresses (`TypedViewCache`), which lane element
//! accesses read before the general path.

use super::*;

/// How many views `TypedViewCache` holds, a power of two: a view's slot index picks its entry.
const TYPED_VIEW_ENTRIES: usize = 64;

/// The element addresses of recently accessed typed array views, by view: what a lane
/// element access reads after one compare of the view and one of the epoch, where the general
/// access reads the view's cell and its buffer's cell (`Vm::typed_cached_element`). An entry
/// holds while the view epoch it was made at is current (`heap::view_epoch`), which every
/// event that could move, unshare, detach or resize a store, or reuse a slot, ends.
pub(in crate::vm) struct TypedViewCache([TypedViewEntry; TYPED_VIEW_ENTRIES]);

impl Default for TypedViewCache {
    fn default() -> Self {
        Self([TypedViewEntry::EMPTY; TYPED_VIEW_ENTRIES])
    }
}

#[derive(Clone, Copy)]
struct TypedViewEntry {
    /// The view's value bits.
    view: u64,
    /// The view epoch the entry was made at.
    epoch: u64,
    /// The view's first element.
    elements: *mut u8,
    /// The view's element count, below 2^31 (`Value::int_key_offset`); zero for an empty entry.
    length: usize,
    kind: TypedArrayKind,
    /// Whether the view's buffer is owned alone and mutable, so stores write `elements`.
    writable: bool,
}

impl TypedViewEntry {
    const EMPTY: Self = Self {
        view: 0,
        epoch: u64::MAX,
        elements: std::ptr::null_mut(),
        length: 0,
        kind: TypedArrayKind::Uint8,
        writable: false,
    };
}

impl TypedViewCache {
    #[inline(always)]
    fn entry(&self, view: Value) -> &TypedViewEntry {
        &self.0[view.to_bits() as usize % TYPED_VIEW_ENTRIES]
    }
}

impl<H: Host> Vm<H> {
    /// The element of the typed array `object` at `key` from its cached entry
    /// (`TypedViewCache`), when `object` has a current one of kind `KIND` (or any kind) and
    /// `key` is an int32 inside it. `None` otherwise; the caller fills the entry
    /// (`fill_typed_view`) on its slower path.
    #[inline(always)]
    pub(in crate::vm) fn typed_cached_element<const KIND: usize>(&self, object: Value, key: Value) -> Option<Value> {
        let entry = self.typed_views.entry(object);
        if entry.view != object.to_bits() || entry.epoch != crate::heap::view_epoch() {
            return None;
        }
        let kind = typed_kind::<KIND>(entry.kind)?;
        let index = key.int_key_offset();
        if index >= entry.length {
            return None;
        }
        // SAFETY: the entry is current, so `elements` holds `length` elements of `kind`
        // (`fill_typed_view`), and `index` is below `length`.
        unsafe { read_element(kind, entry.elements.add(index * kind.width())) }
    }

    /// `typed_set_quick` through the cached entry of `object` (as `typed_cached_element`), of
    /// a writable view. False leaves the store to the caller's slower path.
    #[inline(always)]
    pub(in crate::vm) fn typed_cached_store<const KIND: usize>(&mut self, object: Value, key: Value, value: Value) -> bool {
        let entry = self.typed_views.entry(object);
        if entry.view != object.to_bits() || entry.epoch != crate::heap::view_epoch() || !entry.writable {
            return false;
        }
        let Some(kind) = typed_kind::<KIND>(entry.kind) else {
            return false;
        };
        let index = key.int_key_offset();
        if index >= entry.length {
            return false;
        }
        // SAFETY: the entry is current and writable, so `elements` holds `length` elements of
        // `kind` in a store this view's buffer owns alone (`fill_typed_view`).
        unsafe { write_quick_element(kind, entry.elements.add(index * kind.width()), value) }
    }

    /// Makes the cached entry of the typed array `object` (`TypedViewCache`) at the current
    /// view epoch: a fixed length view of Number elements inside an attached buffer whose
    /// store this heap owns, writable when the buffer owns it alone and is mutable. Any other
    /// object leaves the cache as it is.
    #[inline(never)]
    pub(in crate::vm) fn fill_typed_view(&mut self, object: Value) {
        let Some(Cell::TypedArray { object: view, buffer, length }) = self.heap.get(object) else {
            return;
        };
        let (kind, offset, buffer) = (view.native.kind(), view.offset, *buffer);
        let Some(length) = length.get() else {
            return;
        };
        if kind.is_bigint() || kind == TypedArrayKind::Float16 {
            return;
        }
        let Some(Cell::ArrayBuffer { bytes, detached: false, immutable, .. }) = self.heap.buffer_bytes_mut(buffer) else {
            return;
        };
        let Some((elements, writable)) = view_pointer(bytes, offset, length, kind.width(), *immutable) else {
            return;
        };
        let slot = object.to_bits() as usize % TYPED_VIEW_ENTRIES;
        self.typed_views.0[slot] = TypedViewEntry {
            view: object.to_bits(),
            epoch: crate::heap::view_epoch(),
            elements,
            length: length.min(i32::MAX as usize),
            kind,
            writable,
        };
    }
}
