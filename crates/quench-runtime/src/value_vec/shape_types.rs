//! Slot types of shapes: a slot's representation widens when a value outgrows it, and the runs
//! of a shape recode to the wider form (`ValueArena::recode_run`).

use super::*;

impl ValueArena {
    /// Joins `value`'s class into `slot` of `shape`'s representation; whether it widened. Only a
    /// widening is out of line (`widen_owner`): an untracked store is the test alone.
    #[inline(always)] pub(crate) fn note_slot_value(&mut self, shape: u32, slot: usize, value: Value) -> bool {
        self.tracking && { let owner = self.slot_owner(shape, slot); self.join_slot_owner(owner, value) }
    }

    #[inline(never)]
    pub(super) fn widen_owner(&mut self, owner: u32, class: SlotType) -> bool {
        self.slot_readers.retire(owner);
        let held = std::mem::replace(&mut self.owner_types[owner as usize], class);
        if self.float && (held == SlotType::Number) != (class == SlotType::Number) {
            self.widened.push(owner);
        }
        true
    }

    /// Whether an owner changed the encoding of its words since the heap last recoded them.
    #[inline(always)] pub(crate) fn has_widened(&self) -> bool {
        !self.widened.is_empty()
    }

    /// The owners whose words wait for recoding (`recode_owned_run`), sorted, each once.
    pub(crate) fn take_widened(&mut self) -> Vec<u32> {
        let mut owners = std::mem::take(&mut self.widened);
        owners.sort_unstable();
        owners.dedup();
        owners
    }

    /// Recodes the `len` slots of the run at `start`, the values of an object of `shape`, whose
    /// owners are in `owners` (sorted), to the encodings the owners hold now. A word is decoded as
    /// any owner's, then encoded as its own, so a recoding of a word already in its encoding
    /// changes nothing.
    pub(crate) fn recode_owned_run(&mut self, (start, shape, len): (usize, u32, usize), owners: &[u32]) {
        if shape as usize >= self.shape_chains.len() { return; }
        for slot in 0..len {
            if owners.binary_search(&self.slot_owner(shape, slot)).is_ok() {
                self.recode_word(start + slot, self.word_type(shape, slot));
            }
        }
    }

    /// `recode_owned_run` of every slot, for a run written in the encoding of tagged values
    /// (`with_values`, `young_run_padded`), which the heap recodes once the values are admitted.
    #[inline(always)] pub(crate) fn recode_run(&mut self, start: usize, shape: u32, len: usize) {
        if self.float { self.recode_run_words(start, shape, len); }
    }

    #[inline(never)]
    pub(super) fn recode_run_words(&mut self, start: usize, shape: u32, len: usize) {
        for slot in 0..len {
            self.recode_word(start + slot, self.word_type(shape, slot));
        }
    }

    pub(super) fn recode_word(&mut self, at: usize, ty: SlotType) {
        self.values[at] = encode_slot(ty, decode_slot(SlotType::Number, self.values[at]));
    }

    /// `len` values of the run at `start` as values (`decode_slot`), for storage that holds them so.
    pub(super) fn decoded_run(&self, start: usize, len: usize) -> Vec<Value> {
        self.values[start..start + len].iter().map(|word| decode_slot(read_type(), *word)).collect()
    }

    /// Admits the possible writes of a no-reentry raw slot capability before exposing it.
    pub(crate) fn note_slot_type(&mut self, shape: u32, slot: usize, ty: crate::typed_ir::Type) -> bool {
        if !self.tracking { return false; }
        let owner = self.slot_owner(shape, slot);
        self.join_owner_type(owner, || SlotType::from_type(ty))
    }

    /// Joins `incoming` into the slot type `owner` holds; whether it widened.
    #[inline(always)]
    pub(super) fn join_owner_type(&mut self, owner: u32, incoming: impl FnOnce() -> SlotType) -> bool {
        debug_assert!((owner as usize) < self.owner_types.len());
        // SAFETY: owners come from `chain_owners`, which only names shapes `owner_types` covers.
        let held = unsafe { *self.owner_types.get_unchecked(owner as usize) };
        if held == SlotType::Tagged { return false; }
        let class = held.join(incoming());
        class != held && self.widen_owner(owner, class)
    }
}
