//! Environment slots: the dense and sparse slot storage of an environment record.

use super::*;

impl EnvironmentSlots {
    /// Dense backing storage for a cache that already owns the environment's lifetime.
    /// The pointer expires when the storage is replaced or collected.
    #[inline(always)]
    pub fn dense_storage(&self) -> Option<*const usize> {
        Some(self.dense()?.0.storage().as_ptr().cast_const())
    }

    /// # Safety
    /// A non-null pointer came from `dense_storage`, whose owner and dense layout remain
    /// live. The returned descriptor must stay within that lifetime; its elements obey
    /// normal aliasing rules. Only the scoped read/write/bridge operations below use it.
    #[inline(always)]
    pub(super) unsafe fn borrow_storage(storage: *const usize) -> Option<std::mem::ManuallyDrop<Self>> {
        let header = std::ptr::NonNull::new(storage.cast_mut())?;
        // SAFETY: the caller keeps the original storage live. Rewrap the descriptor
        // immediately so neither wrapper can destroy the original owner's elements.
        let slots = unsafe { super::thin_slice::ThinBoxSlice::borrow_storage(header) };
        Some(std::mem::ManuallyDrop::new(Self(std::mem::ManuallyDrop::into_inner(slots))))
    }

    /// Read through a live dense-storage cache by `staged::environment::dense_capture`.
    /// The dense storage of the parent environment of the environment whose dense storage is
    /// at `storage`, as `link` recorded it in collection `epoch`; `None` when no link of that
    /// collection was recorded.
    ///
    /// # Safety
    /// `storage` came from `dense_storage` of a live heap environment in collection `epoch`
    /// (never a storage laid out in place, which has no prefix of its own).
    #[inline(always)]
    pub(crate) unsafe fn linked_parent(storage: *const usize, epoch: u64) -> Option<*const usize> {
        type Slice = super::thin_slice::ThinBoxSlice<EnvironmentSlot, ENVIRONMENT_PREFIX_WORDS>;
        // SAFETY: as the caller guarantees, the prefix words precede the header; an empty
        // storage's are the shared empty header's zeros, which name no collection.
        unsafe {
            let prefix = Slice::prefix(std::ptr::NonNull::new_unchecked(storage.cast_mut()));
            if *prefix != epoch.wrapping_add(1) as usize {
                return None;
            }
            let parent = *prefix.add(1) as *const usize;
            (!parent.is_null()).then_some(parent)
        }
    }

    /// Records `parent`, the dense storage of the parent environment of the environment whose
    /// dense storage is at `storage`, for collection `epoch` (`linked_parent`). Within one
    /// collection a live environment's storage never moves, and each collection names a new
    /// epoch, so a link is read only while both storages it joins stand.
    ///
    /// # Safety
    /// As `linked_parent`, for both storages.
    #[inline(always)]
    pub unsafe fn link(storage: *const usize, parent: *const usize, epoch: u64) {
        type Slice = super::thin_slice::ThinBoxSlice<EnvironmentSlot, ENVIRONMENT_PREFIX_WORDS>;
        // SAFETY: as the caller guarantees; an empty storage shares the static header, whose
        // prefix is never written.
        unsafe {
            if *storage & !super::thin_slice::MARK_BIT == 0 {
                return;
            }
            let prefix = Slice::prefix(std::ptr::NonNull::new_unchecked(storage.cast_mut()));
            *prefix.add(1) = parent as usize;
            *prefix = epoch.wrapping_add(1) as usize;
        }
    }

    /// # Safety
    /// `storage` is null or live as for `dense_storage`; no element is mutably borrowed.
    #[inline(always)]
    pub unsafe fn read_storage(storage: *const usize, slot: u16) -> Option<Value> {
        // SAFETY: as guaranteed by the caller; each slot is a `Value` (`repr(transparent)`).
        unsafe { let (values, len) = Self::borrow_storage(storage)?.0.run(); crate::staged::environment::concrete_dense_capture(values.cast(), len, slot) }
    }

    /// Write through a cache whose caller has already admitted the store and taken its barrier:
    /// `staged::environment::dense_capture_store`, as the native tiers store a capture.
    /// # Safety
    /// As `read_storage`, with exclusive access to the element for this operation and the
    /// environment's write authorization/barrier satisfied. No reference escapes the write.
    #[inline(always)]
    pub unsafe fn write_storage(storage: *const usize, slot: usize, value: Value) -> Option<()> {
        // SAFETY: as guaranteed by the caller; the view's values are live for this write only.
        let (values, len) = unsafe { Self::native_storage_view(storage) }?;
        // SAFETY: the caller's exclusive access and authorization, over the view's `len` values.
        unsafe { crate::staged::environment::concrete_dense_capture_store(values.cast_mut(), len, u16::try_from(slot).ok()?, value) }
            .then_some(())
    }

    /// `read_storage` and `write_storage` of one slot under one borrow and bounds test: `update`
    /// maps the slot's ordinary value to the one it stores, or declines and stores nothing.
    /// The old value and the stored one.
    /// # Safety
    /// As `write_storage`.
    #[inline(always)]
    pub(crate) unsafe fn update_storage(
        storage: *const usize,
        slot: usize,
        update: impl FnOnce(Value) -> Option<Value>,
    ) -> Option<(Value, Value)> {
        // SAFETY: as guaranteed by the caller, for this update only.
        let mut slots = unsafe { Self::borrow_storage(storage) }?;
        let place = slots.dense_plain_mut(slot)?;
        let old = *place;
        *place = update(old)?;
        Some((old, *place))
    }

    /// Native entry's values/count projection of the same cached storage.
    /// # Safety
    /// As `read_storage`; the returned pointer expires with the original storage.
    #[inline(always)]
    pub unsafe fn native_storage_view(storage: *const usize) -> Option<(*const Value, usize)> {
        // SAFETY: the original owner, not this temporary descriptor, owns the result.
        unsafe { Self::borrow_storage(storage) }?.native_dense_view()
    }

    /// A dense slot view for native reads. The caller must keep the environment live and
    /// discard the pointer at a safepoint, where collection or mutation can replace storage.
    #[inline]
    pub fn native_dense_view(&self) -> Option<(*const Value, usize)> {
        let dense = self.dense()?;
        const { assert!(std::mem::size_of::<EnvironmentSlot>() == std::mem::size_of::<Value>()) };
        const { assert!(std::mem::align_of::<EnvironmentSlot>() == std::mem::align_of::<Value>()) };
        Some((dense.0.as_ptr().cast::<Value>(), dense.0.len()))
    }

    /// The logical length and default owner of sparse storage.
    #[inline(always)]
    pub(super) fn sparse(&self) -> Option<(usize, Value)> {
        if !self.0.marked() {
            return None;
        }
        let length = self.0.first()?.0.as_environment_index()?;
        let owner = self.0.get(1)?.0.shared_environment_owner()?;
        Some((length as usize, owner))
    }

    /// The listed sparse entry for `slot`, as its storage position.
    pub(super) fn sparse_position(&self, slot: usize) -> Option<usize> {
        let slot = u32::try_from(slot).ok()?;
        let entries = self.0.len().saturating_sub(SPARSE_ENTRIES_START) / SPARSE_ENTRY_WORDS;
        let position = |entry: usize| SPARSE_ENTRIES_START + entry * SPARSE_ENTRY_WORDS;
        let (mut low, mut high) = (0, entries);
        while low < high {
            let middle = (low + high) / 2;
            match self.0[position(middle)].0.cmp_environment_index(slot) {
                std::cmp::Ordering::Equal => return Some(position(middle) + 1),
                std::cmp::Ordering::Less => low = middle + 1,
                std::cmp::Ordering::Greater => high = middle,
            }
        }
        None
    }

    pub fn len(&self) -> usize {
        self.sparse().map_or(self.0.len(), |(length, _)| length)
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// What `slot` holds: a value this environment owns, or the owner it shares the slot with.
    #[inline(always)]
    pub(crate) fn state(&self, slot: usize) -> Option<EnvironmentSlotState> {
        if self.0.marked() {
            return self.sparse_state(slot);
        }
        Some(self.0.get(slot)?.state())
    }
    /// `state` of sparse storage, kept out of the dense read's line.
    #[inline(never)]
    pub(super) fn sparse_state(&self, slot: usize) -> Option<EnvironmentSlotState> {
        let (length, owner) = self.sparse()?;
        if slot >= length {
            return None;
        }
        Some(match self.sparse_position(slot) {
            Some(position) => self.0[position].state(),
            None => EnvironmentSlotState::Shared(owner),
        })
    }
    /// This storage when it is dense, which a caller may then read with `dense_plain`.
    #[inline(always)]
    pub fn dense(&self) -> Option<&Self> {
        (!self.0.marked()).then_some(self)
    }
    /// A dense slot's ordinary value (storage `dense` returned): `None` past the storage or
    /// for any internal marker it holds (`Value::is_internal_marker`: a shared slot, an
    /// uninitialized binding), which the caller resolves the long way. One tag test answers
    /// both the shared-slot and the initialization check.
    #[inline(always)]
    pub fn dense_plain(&self, slot: usize) -> Option<Value> {
        debug_assert!(!self.0.marked());
        let value = self.0.get(slot)?.0;
        (!value.is_internal_marker()).then_some(value)
    }
    /// `dense` for writing.
    #[inline(always)]
    pub(crate) fn dense_mut(&mut self) -> Option<&mut Self> {
        (!self.0.marked()).then_some(self)
    }
    /// `dense_plain`'s slot to write: an ordinary value's place, which a store replaces.
    #[inline(always)]
    pub(crate) fn dense_plain_mut(&mut self, slot: usize) -> Option<&mut Value> {
        debug_assert!(!self.0.marked());
        let value = &mut self.0.get_mut(slot)?.0;
        (!value.is_internal_marker()).then_some(value)
    }
    /// The value of a slot this environment owns; a shared slot lives in its owner.
    #[inline(always)]
    pub(crate) fn owned(&self, slot: usize) -> Option<Value> {
        match self.state(slot)? {
            EnvironmentSlotState::Owned(value) => Some(value),
            EnvironmentSlotState::Shared(_) => None,
        }
    }
    pub(crate) fn owned_mut(&mut self, slot: usize) -> Option<&mut Value> {
        let position = match self.sparse() {
            None => slot,
            Some((length, _)) if slot < length => self.sparse_position(slot)?,
            Some(_) => return None,
        };
        self.0.get_mut(position)?.owned_mut()
    }
    /// Appends the heap values `roots` yields (owned heap values, and the owner of each
    /// shared slot) to `out`. A dense storage takes a branch-free compaction over its words:
    /// a loop's per-iteration copy repeats every binding of its function, which the filtered
    /// iterator decoded at ~20 Ir a slot.
    #[inline]
    pub(crate) fn extend_heap_roots(&self, out: &mut Vec<Value>) {
        if self.0.marked() {
            out.extend(self.roots().filter(|value| value.is_heap()));
            return;
        }
        let slots: &[EnvironmentSlot] = &self.0;
        out.reserve(slots.len());
        let start = out.len();
        let mut kept = 0;
        for slot in slots {
            let (value, heap) = slot.0.environment_slot_edge();
            // SAFETY: `kept` never passes the slots read so far, so every write lands within
            // the `slots.len()` values reserved past `start`.
            unsafe { out.as_mut_ptr().add(start + kept).write(value) };
            kept += usize::from(heap);
        }
        // SAFETY: the first `kept` values past `start` were written above.
        unsafe { out.set_len(start + kept) };
    }

    pub(crate) fn roots(&self) -> impl Iterator<Item = Value> + '_ {
        // A sparse storage's index words are not values; its owner and slots are.
        let sparse = self.0.marked();
        self.0
            .iter()
            .filter(move |slot| !sparse || slot.0.as_environment_index().is_none())
            .map(|slot| match slot.state() {
                EnvironmentSlotState::Owned(value) | EnvironmentSlotState::Shared(value) => value,
            })
    }

    /// The owner a sparse storage's unlisted slots share.
    pub(crate) fn default_owner(&self) -> Option<Value> {
        self.sparse().map(|(_, owner)| owner)
    }

    /// Clears every slot whose value no code reads, as `use_of` tells: an unread slot becomes
    /// an uninitialized binding (`Value::DELETED`) this environment owns, and the value it owned
    /// or owner it shared may be swept. A slot read only for whether it is initialized keeps
    /// that state: an initialized value it owns becomes `undefined`, and a shared slot stays
    /// shared with its owner, which the caller keeps. A sparse storage's unlisted slots stay
    /// shared with its default owner, which the caller keeps too.
    pub(crate) fn clear_unread(&mut self, use_of: impl Fn(usize) -> SlotUse) {
        let clear = |slot: &mut EnvironmentSlot, index: usize| match use_of(index) {
            SlotUse::Value => {}
            SlotUse::Unread => *slot = EnvironmentSlot::owned(Value::DELETED),
            SlotUse::Initialization => {
                if matches!(slot.state(), EnvironmentSlotState::Owned(value) if !value.is_deleted()) {
                    *slot = EnvironmentSlot::owned(Value::UNDEFINED);
                }
            }
        };
        if self.0.marked() {
            let entries = self.0.len().saturating_sub(SPARSE_ENTRIES_START) / SPARSE_ENTRY_WORDS;
            for entry in 0..entries {
                let position = SPARSE_ENTRIES_START + entry * SPARSE_ENTRY_WORDS;
                if let Some(index) = self.0[position].0.as_environment_index() {
                    clear(&mut self.0[position + 1], index as usize);
                }
            }
        } else {
            for (index, slot) in self.0.iter_mut().enumerate() {
                clear(slot, index);
            }
        }
    }

    /// The indexes a sparse storage lists, ascending; none for dense storage.
    pub(crate) fn sparse_indexes(&self) -> impl Iterator<Item = usize> + '_ {
        let entries = if self.0.marked() {
            self.0.len().saturating_sub(SPARSE_ENTRIES_START) / SPARSE_ENTRY_WORDS
        } else {
            0
        };
        (0..entries).filter_map(move |entry| {
            self.0[SPARSE_ENTRIES_START + entry * SPARSE_ENTRY_WORDS]
                .0
                .as_environment_index()
                .map(|index| index as usize)
        })
    }

    /// `dense` stored sparsely around the owner most of its slots share, when it reaches
    /// `SPARSE_CLONE_MIN_SLOTS` and the sparse words are fewer; a copy that owns most of its
    /// slots stays dense, whose reads need no search.
    pub(super) fn compacted(dense: super::thin_slice::ThinBoxSlice<EnvironmentSlot, ENVIRONMENT_PREFIX_WORDS>) -> Option<Self> {
        let len = dense.len();
        if len < SPARSE_CLONE_MIN_SLOTS {
            return Some(Self(dense));
        }
        let mut owners: Vec<(Value, usize)> = Vec::new();
        for slot in dense.iter() {
            if let EnvironmentSlotState::Shared(owner) = slot.state() {
                match owners.iter_mut().find(|(candidate, _)| *candidate == owner) {
                    Some((_, count)) => *count += 1,
                    None => owners.push((owner, 1)),
                }
            }
        }
        let Some((owner, sharing)) = owners.into_iter().max_by_key(|(_, count)| *count) else {
            return Some(Self(dense));
        };
        let listed = len - sharing;
        if SPARSE_ENTRIES_START + listed * SPARSE_ENTRY_WORDS >= len {
            return Some(Self(dense));
        }
        let shared = Value::shared_environment_slot(owner);
        let mut words = Vec::with_capacity(SPARSE_ENTRIES_START + listed * SPARSE_ENTRY_WORDS);
        words.push(EnvironmentSlot(Value::environment_index(u32::try_from(len).ok()?)));
        words.push(EnvironmentSlot(shared));
        for (index, slot) in dense.iter().enumerate() {
            if slot.0 != shared {
                words.push(EnvironmentSlot(Value::environment_index(u32::try_from(index).ok()?)));
                words.push(*slot);
            }
        }
        let mut storage = super::thin_slice::ThinBoxSlice::from_exact_iter(words.into_iter());
        storage.mark();
        Some(Self(storage))
    }

    /// The slots of a per-iteration copy of `environment`, which holds these slots: a `reset`
    /// slot is `undefined`, a `fresh` slot owns its current value (`owned_value` reads a
    /// shared slot from its owner), and every other slot shares with the slot's current
    /// owner. The copy's default owner is the one most slots share, so a copy of a copy
    /// stays sparse, and a sparse source is copied through the slots it lists and the
    /// fresh and reset ones alone. `fresh` and `reset` are sorted.
    pub(crate) fn iteration_copy(
        &self,
        environment: Value,
        fresh: &[u16],
        reset: crate::bytecode::Resets<'_>,
        owned_value: impl Fn(Value, usize) -> Option<Value>,
    ) -> Option<Self> {
        let len = self.len();
        let copy_slot = |index: usize| -> Option<EnvironmentSlot> {
            let slot = u16::try_from(index).ok()?;
            if reset.contains(slot) {
                return Some(EnvironmentSlot::owned(Value::UNDEFINED));
            }
            let fresh = fresh.binary_search(&slot).is_ok();
            Some(match self.state(index)? {
                EnvironmentSlotState::Owned(value) if fresh => EnvironmentSlot::owned(value),
                EnvironmentSlotState::Owned(_) => EnvironmentSlot::shared(environment),
                EnvironmentSlotState::Shared(owner) if fresh => {
                    EnvironmentSlot::owned(owned_value(owner, index)?)
                }
                EnvironmentSlotState::Shared(owner) => EnvironmentSlot::shared(owner),
            })
        };
        let Some((_, owner)) = self.sparse() else {
            // Every slot shares with its current owner, then the fresh and reset ones own: one
            // copy of the words, then a select per word (a shared word names its owner already).
            let shared = EnvironmentSlot::shared(environment);
            let mut copy = super::thin_slice::ThinBoxSlice::from_slice(&self.0);
            for slot in copy.iter_mut() {
                if !slot.0.is_shared_environment_slot() {
                    *slot = shared;
                }
            }
            for &slot in fresh {
                let index = usize::from(slot);
                let Some(source) = self.0.get(index) else {
                    continue;
                };
                copy[index] = EnvironmentSlot::owned(match source.state() {
                    EnvironmentSlotState::Owned(value) => value,
                    EnvironmentSlotState::Shared(owner) => owned_value(owner, index)?,
                });
            }
            for slot in reset.iter() {
                if let Some(target) = copy.get_mut(usize::from(slot)) {
                    *target = EnvironmentSlot::owned(Value::UNDEFINED);
                }
            }
            return Self::compacted(copy);
        };
        // Every slot the source does not list shares with `owner`, as the copy's do unless
        // they are fresh or reset.
        // Each list is ascending (sparse entries by position, `fresh` and `reset` sorted), so
        // their union is merged, never sorted.
        let mut indexes = merge_ascending(
            merge_ascending(self.sparse_indexes(), fresh.iter().map(|&slot| usize::from(slot)))
                .into_iter(),
            reset.iter().map(usize::from),
        );
        indexes.truncate(indexes.partition_point(|&index| index < len));
        let shared = Value::shared_environment_slot(owner);
        let mut listed = Vec::with_capacity(indexes.len());
        for index in indexes {
            let slot = copy_slot(index)?;
            if slot.0 != shared {
                listed.push((index, slot));
            }
        }
        if SPARSE_ENTRIES_START + listed.len() * SPARSE_ENTRY_WORDS >= len {
            return super::thin_slice::ThinBoxSlice::try_from_fn(len, copy_slot).map(Self);
        }
        let mut words = Vec::with_capacity(SPARSE_ENTRIES_START + listed.len() * SPARSE_ENTRY_WORDS);
        words.push(EnvironmentSlot(Value::environment_index(u32::try_from(len).ok()?)));
        words.push(EnvironmentSlot(shared));
        for (index, slot) in listed {
            words.push(EnvironmentSlot(Value::environment_index(u32::try_from(index).ok()?)));
            words.push(slot);
        }
        let mut storage = super::thin_slice::ThinBoxSlice::from_exact_iter(words.into_iter());
        storage.mark();
        Some(Self(storage))
    }

    /// `len` slots, each `DELETED` but slot `k` of each bit `k` of `captured`, which holds
    /// `values[k]`: one fill of the storage, then one write per captured slot.
    ///
    /// # Safety
    /// `values` holds a readable value at every index `captured` names, each below `len`,
    /// and none of them shares a slot (`Value::shared_environment_owner`).
    pub unsafe fn from_captured(len: usize, captured: u64, values: *const Value) -> Self {
        let mut slots = super::thin_slice::ThinBoxSlice::filled(len, EnvironmentSlot(Value::DELETED));
        let mut remaining = captured;
        while remaining != 0 {
            let slot = remaining.trailing_zeros() as usize;
            debug_assert!(slot < len);
            // SAFETY: as the caller guarantees.
            slots[slot] = EnvironmentSlot::owned(unsafe { *values.add(slot) });
            remaining &= remaining - 1;
        }
        Self(slots)
    }

    pub(crate) fn from_values(values: impl ExactSizeIterator<Item = Value>) -> Self {
        Self(super::thin_slice::ThinBoxSlice::from_exact_iter(
            values.map(EnvironmentSlot::owned),
        ))
    }
}

impl From<Vec<Value>> for EnvironmentSlots {
    fn from(values: Vec<Value>) -> Self {
        Self::from_values(values.into_iter())
    }
}

impl From<Box<[Value]>> for EnvironmentSlots {
    fn from(values: Box<[Value]>) -> Self {
        Self(super::thin_slice::ThinBoxSlice::from_exact_iter(
            values.iter().copied().map(EnvironmentSlot::owned),
        ))
    }
}
