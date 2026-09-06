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
    prototype_depth: u32,
    _prototype_padding: u32,
    prototype_links: [PrototypeGuardLink; 4],
}

#[derive(Clone, Copy)]
#[repr(C)]
pub(crate) struct PrototypeGuardLink {
    /// Slot in the preceding owner. It is checked before either pointer below
    /// is dereferenced, so a broken chain cannot expose a stale descendant.
    slot: *const crate::register_file::SlotWord,
    expected_word: u64,
    /// Layout cell owned by the exact object encoded in `expected_word`.
    layout: *const u32,
    expected_layout: u32,
    _padding: u32,
}

impl PrototypeGuardLink {
    pub(crate) fn new(
        slot: *const crate::register_file::SlotWord,
        expected_word: u64,
        layout: *const u32,
        expected_layout: u32,
    ) -> Self {
        Self {
            slot,
            expected_word,
            layout,
            expected_layout,
            _padding: 0,
        }
    }

    pub(crate) const EMPTY: Self = Self {
        slot: std::ptr::null(),
        expected_word: 0,
        layout: std::ptr::null(),
        expected_layout: 0,
        _padding: 0,
    };
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
            prototype_depth: access.prototype_depth,
            _prototype_padding: 0,
            prototype_links: access.prototype_links,
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
    prototype_depth: u32,
    prototype_links: [PrototypeGuardLink; 4],
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
            prototype_depth: 0,
            prototype_links: [PrototypeGuardLink::EMPTY; 4],
        }
    }

    pub(crate) fn with_prototype_chain(
        self,
        receiver_layout: (*const u32, u32),
        links: &[PrototypeGuardLink],
    ) -> Option<Self> {
        if links.is_empty() || links.len() > 4 {
            return None;
        }
        let mut prototype_links = [PrototypeGuardLink::EMPTY; 4];
        prototype_links[..links.len()].copy_from_slice(links);
        Some(Self {
            layout: receiver_layout.0,
            expected_layout: receiver_layout.1,
            prototype_depth: u32::try_from(links.len()).ok()?,
            prototype_links,
            ..self
        })
    }

    pub(crate) fn region_key(self) -> crate::stencil_fact::RegionKey {
        if self.prototype_depth == 0 {
            crate::stencil_select::property_region_key()
        } else {
            crate::stencil_select::prototype_property_region_key()
        }
    }

    pub(crate) fn accepts_non_owning_store(self) -> bool {
        unsafe { self.slot.as_ref() }
            .and_then(crate::register_file::SlotWord::plain_non_owning_bits)
            .is_some()
    }
}

const _: () = {
    assert!(std::mem::align_of::<NativePropertyReadContext>() == 8);
    assert!(std::mem::offset_of!(NativePropertyReadContext, layout) == 0);
    assert!(std::mem::offset_of!(NativePropertyReadContext, expected_layout) == 8);
    assert!(std::mem::offset_of!(NativePropertyReadContext, descriptor_state) == 16);
    assert!(std::mem::offset_of!(NativePropertyReadContext, deleted_state) == 24);
    assert!(std::mem::offset_of!(NativePropertyReadContext, slot) == 32);
    assert!(std::mem::offset_of!(NativePropertyReadContext, result) == 40);
    assert!(std::mem::offset_of!(NativePropertyReadContext, prototype_depth) == 48);
    assert!(std::mem::offset_of!(NativePropertyReadContext, prototype_links) == 56);
    assert!(std::mem::size_of::<PrototypeGuardLink>() == 32);
    assert!(std::mem::size_of::<NativePropertyReadContext>() == 184);
    assert!(std::mem::size_of::<NativePropertyWriteContext>() == 48);
    assert!(std::mem::align_of::<NativePropertyWriteContext>() == 8);
    assert!(std::mem::offset_of!(NativePropertyWriteContext, layout) == 0);
    assert!(std::mem::offset_of!(NativePropertyWriteContext, expected_layout) == 8);
    assert!(std::mem::offset_of!(NativePropertyWriteContext, descriptor_state) == 16);
    assert!(std::mem::offset_of!(NativePropertyWriteContext, deleted_state) == 24);
    assert!(std::mem::offset_of!(NativePropertyWriteContext, slot) == 32);
    assert!(std::mem::offset_of!(NativePropertyWriteContext, value) == 40);
};
