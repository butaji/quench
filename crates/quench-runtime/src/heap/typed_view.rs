//! A typed array's native record: the prefix of its view record (`NativeView`) that generated
//! code reads in place, and the table (`TypedViewTable`) that names each live prefix by its
//! legacy slot, so a typed array's value reaches its record with one load, as a direct array's
//! does through its record table.
//!
//! The prefix is the one home of what native code needs of a view: the kind, the first
//! element's address and the element count. The address and count are published by the VM
//! (`Vm::publish_typed_view`) for the buffer as it is, and stay true until a buffer event
//! (`buffer_relocations`) changes where its bytes are or who shares them; the stamp is the
//! count of such events at publication, so a stale prefix is one the proof refuses.

use super::buffer_bytes::buffer_relocations;
use super::cell::{TYPED_ARRAYS, TypedArrayKind};
use crate::staged::record_heads::NativeViewHead;

/// The kind byte of a view that is no typed array (a DataView): no native element has it.
pub const NO_KIND: u8 = u8::MAX;
const _: () = assert!(TypedArrayKind::COUNT < NO_KIND as usize);

/// What native code reads of a view record, first in `BufferViewObject` (`#[repr(C)]`), so a
/// table entry addresses it. `detached` is set while the view cannot be addressed in place:
/// before its first publication (its stamp is no count), when its buffer is detached, shrank below it, is shared
/// atomically or holds no native element.
#[repr(C)]
#[derive(Debug)]
pub struct NativeView {
    /// What native code reads in place (`staged::record_heads::NativeViewHead`).
    head: NativeViewHead,
}

impl std::ops::Deref for NativeView {
    type Target = NativeViewHead;
    #[inline(always)]
    fn deref(&self) -> &NativeViewHead {
        &self.head
    }
}

impl std::ops::DerefMut for NativeView {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut NativeViewHead {
        &mut self.head
    }
}

impl Default for NativeView {
    fn default() -> Self {
        Self::unpublished(NO_KIND)
    }
}

/// A copy is a record no table names: it is published again when it is allocated.
impl Clone for NativeView {
    fn clone(&self) -> Self {
        Self::unpublished(self.kind)
    }
}

impl NativeView {
    fn unpublished(kind: u8) -> Self {
        Self { head: NativeViewHead { data: 0, stamp: u64::MAX, length: 0, kind, detached: 1, unshared: 0, home: std::ptr::null_mut() } }
    }

    /// Publishes the view's elements: `length` of them from `data`, over bytes `unshared` or not.
    /// The unpublished view of a typed array of `kind`.
    pub(crate) fn typed(kind: TypedArrayKind) -> Self {
        Self::unpublished(kind as u8)
    }

    /// The kind of a typed array's view; a DataView's has none.
    #[inline]
    pub(crate) fn kind(&self) -> TypedArrayKind {
        TYPED_ARRAYS[usize::from(self.kind)].kind
    }

    pub(crate) fn publish(&mut self, data: *mut u8, length: u32, unshared: bool) {
        (self.data, self.length, self.unshared) = (data.addr(), length, u8::from(unshared));
        self.detached = 0;
        self.stamp = buffer_relocations();
    }

    /// Withdraws the view's elements: native code leaves for the VM until it is published again.
    pub(crate) fn retract(&mut self) {
        self.detached = 1;
        self.stamp = buffer_relocations();
    }

    /// Whether the view was withdrawn since the last event that could have made it addressable,
    /// so publishing it again would withdraw it again.
    pub(crate) fn is_retracted(&self) -> bool {
        self.detached == 1 && self.stamp == buffer_relocations()
    }

    /// Names this record by the table entry that holds it.
    fn bind(&mut self, entry: *mut usize) {
        self.home = entry;
    }
}

/// A prefix leaves its table entry when its record goes, unless the slot's next typed array
/// already took the entry.
impl Drop for NativeView {
    fn drop(&mut self) {
        let record = std::ptr::from_mut(self).addr();
        // SAFETY: `home` is an entry of the heap's table, which outlives the heap's slots.
        if !self.home.is_null() && unsafe { *self.home } == record {
            unsafe { *self.home = 0 };
        }
    }
}

/// Entries a page of `TypedViewTable` holds, a power of two so a slot is a page and an entry by shift and mask.
const PAGE_ENTRIES: usize = 1 << PAGE_SHIFT;
const PAGE_SHIFT: u32 = crate::staged::layout_offsets::TYPED_PAGE_SHIFT;

/// The view records of a heap's typed arrays, by legacy slot, in zeroed pages of `PAGE_ENTRIES`
/// words behind a directory of page addresses: zero where the slot is no typed array. Its
/// cost is one word per slot up to the highest slot a typed array has taken, in whole pages,
/// and one directory word per page; both grow only where a typed array is allocated, which
/// moves the directory (native code reads its base and length again after a callout) and never
/// a page, so a record's entry stays where its `home` names it.
#[derive(Default)]
pub(super) struct TypedViewTable {
    pages: Vec<Box<[usize]>>,
    directory: Vec<usize>,
}

impl TypedViewTable {
    /// The directory's base and its page count.
    #[inline]
    pub(super) fn directory(&self) -> (*const usize, usize) {
        (self.directory.as_ptr(), self.directory.len())
    }

    /// Names the record `view` of the typed array in legacy slot `slot`.
    pub(super) fn register(&mut self, slot: u32, view: &mut NativeView) {
        let page = slot as usize >> PAGE_SHIFT;
        while self.pages.len() <= page {
            let mut entries = vec![0_usize; PAGE_ENTRIES].into_boxed_slice();
            self.directory.push(entries.as_mut_ptr().addr());
            self.pages.push(entries);
        }
        let entry = &mut self.pages[page][slot as usize & (PAGE_ENTRIES - 1)];
        view.bind(std::ptr::from_mut(entry));
        *entry = std::ptr::from_mut(view).addr();
    }
}

/// The native reader's view of a typed array's record: the table entry holds the record's
/// address (`entry_size` bytes), a slot's page is `slot >> page_shift` of the directory and its
/// entry `slot & page_mask` of that page, and these are the byte offsets of the prefix's fields
/// from the record. A consumer resolves a typed array's value to its entry by the slot index, proves the
/// record's kind, that its `stamp` is the current `buffer_relocations` and that it is not
/// `detached`, and reads `data` and `length`; a store also requires the bytes `unshared`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeTypedArrayLayout {
    pub entry_size: usize,
    pub page_shift: u32,
    pub page_mask: usize,
    pub kind_offset: usize,
    pub detached_offset: usize,
    pub unshared_offset: usize,
    pub data_offset: usize,
    pub length_offset: usize,
    pub stamp_offset: usize,
}

/// Layout facts for code generators which read a typed array's elements in place.
pub const NATIVE_TYPED_ARRAY_LAYOUT: NativeTypedArrayLayout = NativeTypedArrayLayout {
    entry_size: std::mem::size_of::<usize>(),
    page_shift: PAGE_SHIFT,
    page_mask: PAGE_ENTRIES - 1,
    kind_offset: super::cell::NATIVE_VIEW_OFFSET + std::mem::offset_of!(NativeView, head) + std::mem::offset_of!(NativeViewHead, kind),
    detached_offset: super::cell::NATIVE_VIEW_OFFSET + std::mem::offset_of!(NativeView, head) + std::mem::offset_of!(NativeViewHead, detached),
    unshared_offset: super::cell::NATIVE_VIEW_OFFSET + std::mem::offset_of!(NativeView, head) + std::mem::offset_of!(NativeViewHead, unshared),
    data_offset: super::cell::NATIVE_VIEW_OFFSET + std::mem::offset_of!(NativeView, head) + std::mem::offset_of!(NativeViewHead, data),
    length_offset: super::cell::NATIVE_VIEW_OFFSET + std::mem::offset_of!(NativeView, head) + std::mem::offset_of!(NativeViewHead, length),
    stamp_offset: super::cell::NATIVE_VIEW_OFFSET + std::mem::offset_of!(NativeView, head) + std::mem::offset_of!(NativeViewHead, stamp),
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_layout_names_the_prefix_fields_of_a_boxed_view_record() {
        let record = super::super::cell::BufferViewObject {
            native: NativeView::default(),
            object: super::super::Object::new(crate::value::Value::NULL),
            offset: 0,
        };
        let layout = NATIVE_TYPED_ARRAY_LAYOUT;
        let at = |offset: usize| (std::ptr::from_ref(&record).addr() + offset) as *const u8;
        assert_eq!(std::ptr::from_ref(&record.native).addr(), std::ptr::from_ref(&record).addr(), "the prefix is first");
        assert_eq!(at(layout.kind_offset), std::ptr::from_ref(&record.native.kind));
        assert_eq!(at(layout.detached_offset), std::ptr::from_ref(&record.native.detached));
        assert_eq!(at(layout.unshared_offset), std::ptr::from_ref(&record.native.unshared));
        assert_eq!(at(layout.data_offset), std::ptr::from_ref(&record.native.data).cast());
        assert_eq!(at(layout.length_offset), std::ptr::from_ref(&record.native.length).cast());
        assert_eq!(at(layout.stamp_offset), std::ptr::from_ref(&record.native.stamp).cast());
    }

    #[test]
    fn a_typed_array_registered_by_slot_leaves_its_entry_when_its_record_goes() {
        let mut table = TypedViewTable::default();
        assert_eq!(table.directory().1, 0);
        let mut view = Box::new(NativeView::typed(TypedArrayKind::Float64));
        let slot = PAGE_ENTRIES as u32 + 7;
        table.register(slot, &mut view);
        assert_eq!(table.directory().1, 2, "the pages up to the slot's");
        let entry = || unsafe { (table.directory().0.add(1).read() as *const usize).add(7).read() };
        assert_eq!(entry(), std::ptr::from_ref(&*view).addr());
        assert_eq!(view.kind(), TypedArrayKind::Float64);
        let copy = view.clone();
        drop(copy);
        assert_ne!(entry(), 0, "an unregistered copy leaves the entry");
        drop(view);
        assert_eq!(entry(), 0);
    }

    #[test]
    fn a_view_is_addressable_only_from_its_publication_until_it_is_retracted() {
        let mut bytes = [0_u8; 16];
        let mut view = NativeView::default();
        assert_eq!(view.detached, 1, "a new view is unpublished");
        view.publish(bytes.as_mut_ptr(), 2, true);
        assert_eq!((view.detached, view.unshared, view.length, view.data), (0, 1, 2, bytes.as_ptr().addr()));
        assert_eq!(view.stamp, buffer_relocations());
        view.retract();
        assert_eq!(view.detached, 1);
    }
}
