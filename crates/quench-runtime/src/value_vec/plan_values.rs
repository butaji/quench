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
            if held != SlotType::Tagged && held != class {
                self.widen_owner_type(owner, class);
            }
        }
    }

    #[inline(never)]
    fn widen_owner_type(&mut self, owner: u32, incoming: SlotType) -> bool {
        self.join_owner_type(owner, || incoming)
    }
}
