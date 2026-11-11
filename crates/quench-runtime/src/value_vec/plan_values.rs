use super::*;

impl ValueArena {
    /// Joins the classes of a new object's initial `values` into the slots of `shape`: the
    /// shape's owner chain read once, then one load and compare per slot.
    #[inline(always)]
    pub(crate) fn note_plan_values(&mut self, shape: u32, values: &[Value]) {
        if !self.tracking { return; }
        let chain = self.shape_chains[shape as usize] as usize;
        for (slot, value) in values.iter().copied().enumerate() {
            let owner = self.chain_owners[chain + slot];
            let (held, class) = (self.owner_types[owner as usize], SlotType::of(value));
            if held != SlotType::Tagged && held != class {
                self.widen_owner_type(owner, class);
            }
        }
    }

    #[inline(never)]
    fn widen_owner_type(&mut self, owner: u32, incoming: SlotType) -> bool {
        self.join_owner_type(owner, incoming)
    }
}
