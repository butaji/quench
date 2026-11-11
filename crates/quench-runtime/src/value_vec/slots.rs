//! A store's join into the slot representations it writes, for sites that already hold the
//! shape: `store_exact_slot` of a matched exact shape reads the owner unchecked.

use super::ValueArena;
use crate::Value;
use crate::typed_ir::SlotType;

impl ValueArena {
    /// `note_slot_value` of a slot of a live exact shape, whose owner is read unchecked.
    ///
    /// # Safety
    /// `shape` is a shape of this arena that holds `slot`.
    #[inline(always)] pub(crate) unsafe fn note_exact_slot_value(&mut self, shape: u32, slot: usize, value: Value) -> bool {
        // SAFETY: the caller's contract; a shape's chain holds an owner per slot.
        self.tracking && { let owner = unsafe { *self.chain_owners.get_unchecked(*self.shape_chains.get_unchecked(shape as usize) as usize + slot) }; self.join_slot_owner(owner, value) }
    }

    /// Joins `value`'s class into the representation `owner` names (`slot_owner`, valid while
    /// the shapes are); whether it widened.
    #[inline(always)] pub(crate) fn join_slot_owner(&mut self, owner: u32, value: Value) -> bool {
        self.tracking && self.join_owner_type(owner, || SlotType::of(value))
    }
}
