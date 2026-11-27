use super::*;

impl ValueArena {
    /// Joins the classes of a new object's initial `values` into the slots of `shape`: the
    /// shape's owner chain read once, then one load and compare per slot.
    #[inline(always)]
    pub(crate) fn note_plan_values(&mut self, shape: u32, values: &[Value]) {
        if !self.tracking { return; }
        debug_assert!(values.len() <= self.shape_length(shape));
        // SAFETY: a plan's shape holds `values.len()` slots, so its owner chain does; every
        // owner names an `owner_types` entry (`slot_owner`).
        let chain = unsafe { *self.shape_chains.get_unchecked(shape as usize) } as usize;
        for (slot, value) in values.iter().copied().enumerate() {
            let owner = unsafe { *self.chain_owners.get_unchecked(chain + slot) };
            let (held, class) = (unsafe { *self.owner_types.get_unchecked(owner as usize) }, SlotType::of(value));
            if held.join(class) != held {
                self.widen_owner_type(owner, class);
            }
        }
    }

    /// `plan_values_admitted` of the first `len` of the inline values `values`, taken by value
    /// so a caller keeps them in registers: no array lives in memory for the slice.
    #[inline(always)]
    pub(crate) fn inline_plan_values_admitted(&self, shape: u32, values: [Value; INLINE_PROPERTY_COUNT], len: usize) -> bool {
        !self.tracking || self.tracked_inline_values_admitted(shape, values, len)
    }

    #[inline(never)]
    fn tracked_inline_values_admitted(&self, shape: u32, values: [Value; INLINE_PROPERTY_COUNT], len: usize) -> bool {
        self.plan_values_admitted(shape, &values[..len])
    }

    /// Whether `note_plan_values` of `values` into `shape` would widen nothing, read only, so a
    /// caller that may not call out of line can leave before any effect.
    #[inline(always)]
    pub(crate) fn plan_values_admitted(&self, shape: u32, values: &[Value]) -> bool {
        if !self.tracking { return true; }
        // SAFETY: as `note_plan_values`.
        let chain = unsafe { *self.shape_chains.get_unchecked(shape as usize) } as usize;
        for (slot, value) in values.iter().copied().enumerate() {
            let owner = unsafe { *self.chain_owners.get_unchecked(chain + slot) };
            let held = unsafe { *self.owner_types.get_unchecked(owner as usize) };
            if held.join(SlotType::of(value)) != held {
                return false;
            }
        }
        true
    }

    #[inline(never)]
    fn widen_owner_type(&mut self, owner: u32, incoming: SlotType) -> bool {
        self.join_owner_type(owner, || incoming)
    }

    /// `note_slot_value` of a slot of a live exact shape, whose owner is read unchecked, with the type its
    /// slot's words have afterwards (`word_type`), from the one owner lookup: a store encodes its value as
    /// that type (`encode_slot`).
    ///
    /// # Safety
    /// `shape` is a shape of this arena that holds `slot`.
    #[inline(always)] pub(crate) unsafe fn note_exact_slot_word(&mut self, shape: u32, slot: usize, value: Value) -> (bool, SlotType) {
        if !self.tracking { return (false, SlotType::Tagged); }
        // SAFETY: the caller's contract; a shape's chain holds an owner per slot.
        let owner = unsafe { *self.chain_owners.get_unchecked(*self.shape_chains.get_unchecked(shape as usize) as usize + slot) };
        let widened = self.join_owner_type(owner, || SlotType::of(value));
        // SAFETY: owners name `owner_types` entries (`slot_owner`).
        let ty = if self.float { unsafe { *self.owner_types.get_unchecked(owner as usize) } } else { SlotType::Tagged };
        (widened, ty)
    }

    /// The type a store of `value` encodes as at `slot` of the live exact `shape`
    /// (`note_exact_slot_word`), where the slot's owner already holds `value`'s class; `None`
    /// where the store would widen it, for the joining store to take. One owner lookup and one
    /// class test (`SlotType::admits`), no join.
    ///
    /// # Safety
    /// `shape` is a shape of this arena that holds `slot`.
    #[inline(always)] pub(crate) unsafe fn held_slot_word(&self, shape: u32, slot: usize, value: Value) -> Option<SlotType> {
        if !self.tracking { return Some(SlotType::Tagged); }
        // SAFETY: the caller's contract; a shape's chain holds an owner per slot.
        let owner = unsafe { *self.chain_owners.get_unchecked(*self.shape_chains.get_unchecked(shape as usize) as usize + slot) };
        // SAFETY: owners name `owner_types` entries (`slot_owner`).
        let held = unsafe { *self.owner_types.get_unchecked(owner as usize) };
        if !held.admits(value) { return None; }
        Some(if self.float { held } else { SlotType::Tagged })
    }

    /// The base addresses of the owner tables a tracked store reads: `shape_chains`, `chain_owners`
    /// and `owner_types`, the three `Table` bases of `Stage::table`.
    #[inline(always)]
    pub(crate) fn owner_table_bases(&self) -> [u64; 3] {
        [self.shape_chains.as_ptr() as u64, self.chain_owners.as_ptr() as u64, self.owner_types.as_ptr() as u64]
    }

    /// Joins `value`'s class into the representation `owner` names (`slot_owner`, valid while
    /// the shapes are); whether it widened.
    #[inline(always)] pub(crate) fn join_slot_owner(&mut self, owner: u32, value: Value) -> bool {
        self.tracking && self.join_owner_type(owner, || SlotType::of(value))
    }
}
