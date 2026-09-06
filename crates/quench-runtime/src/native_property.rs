//! Typed physical boundary for guarded own-data property reads.
//!
//! Semantic lookup and IC installation stay in the ordinary property runtime.
//! This context carries only the live facts needed to revalidate and load one
//! admitted slot without rescanning names or descriptor metadata.

#[repr(C)]
pub(crate) struct NativePropertyReadContext {
    layout: *const u32,
    expected_layout: u32,
    _padding: u32,
    descriptor_state: *const u8,
    deleted_state: *const u8,
    slot: *const crate::register_file::SlotWord,
    result: u64,
}

#[repr(C)]
pub(crate) struct NativePropertyWriteContext {
    layout: *const u32,
    expected_layout: u32,
    _padding: u32,
    descriptor_state: *const u8,
    deleted_state: *const u8,
    slot: *const crate::register_file::SlotWord,
    value: u64,
}

impl NativePropertyReadContext {
    pub(crate) fn new(access: GuardedPropertySlot) -> Self {
        Self {
            layout: access.layout,
            expected_layout: access.expected_layout,
            _padding: 0,
            descriptor_state: access.descriptor_state,
            deleted_state: access.deleted_state,
            slot: access.slot,
            result: 0,
        }
    }

    pub(crate) fn result(&self, status: u32) -> Option<u64> {
        (status == 1).then_some(self.result)
    }
}

impl NativePropertyWriteContext {
    pub(crate) fn new(access: GuardedPropertySlot, value: u64) -> Self {
        Self {
            layout: access.layout,
            expected_layout: access.expected_layout,
            _padding: 0,
            descriptor_state: access.descriptor_state,
            deleted_state: access.deleted_state,
            slot: access.slot,
            value,
        }
    }
}

/// A synchronous borrow-free view of one guarded slot.
///
/// The owning `ObjectData` must remain alive during entry. Structural mutation
/// invalidates `layout` before native code can dereference `slot`; callers must
/// never retain this value across JavaScript reentry.
#[derive(Clone, Copy)]
pub(crate) struct GuardedPropertySlot {
    layout: *const u32,
    expected_layout: u32,
    descriptor_state: *const u8,
    deleted_state: *const u8,
    slot: *const crate::register_file::SlotWord,
}

impl GuardedPropertySlot {
    pub(super) fn new(
        layout: *const u32,
        expected_layout: u32,
        descriptor_state: *const u8,
        deleted_state: *const u8,
        slot: *const crate::register_file::SlotWord,
    ) -> Self {
        Self {
            layout,
            expected_layout,
            descriptor_state,
            deleted_state,
            slot,
        }
    }

    pub(crate) fn accepts_non_owning_store(self) -> bool {
        unsafe { self.slot.as_ref() }
            .and_then(crate::register_file::SlotWord::plain_non_owning_bits)
            .is_some()
    }
}

const _: () = {
    assert!(std::mem::size_of::<NativePropertyReadContext>() == 48);
    assert!(std::mem::align_of::<NativePropertyReadContext>() == 8);
    assert!(std::mem::offset_of!(NativePropertyReadContext, layout) == 0);
    assert!(std::mem::offset_of!(NativePropertyReadContext, expected_layout) == 8);
    assert!(std::mem::offset_of!(NativePropertyReadContext, descriptor_state) == 16);
    assert!(std::mem::offset_of!(NativePropertyReadContext, deleted_state) == 24);
    assert!(std::mem::offset_of!(NativePropertyReadContext, slot) == 32);
    assert!(std::mem::offset_of!(NativePropertyReadContext, result) == 40);
    assert!(std::mem::size_of::<NativePropertyWriteContext>() == 48);
    assert!(std::mem::align_of::<NativePropertyWriteContext>() == 8);
    assert!(std::mem::offset_of!(NativePropertyWriteContext, layout) == 0);
    assert!(std::mem::offset_of!(NativePropertyWriteContext, expected_layout) == 8);
    assert!(std::mem::offset_of!(NativePropertyWriteContext, descriptor_state) == 16);
    assert!(std::mem::offset_of!(NativePropertyWriteContext, deleted_state) == 24);
    assert!(std::mem::offset_of!(NativePropertyWriteContext, slot) == 32);
    assert!(std::mem::offset_of!(NativePropertyWriteContext, value) == 40);
};
