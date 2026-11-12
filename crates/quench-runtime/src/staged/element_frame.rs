//! A frame's element callouts (`ElementFrame`): what the element rows read back of the tier's
//! proofs of an array (its kept view, its record table, its key's int32) and the VM's own steps
//! behind the dense arm. Every default declines, leaving the row to the host path.

use super::elements::ElementView;
use super::{value, PropertyFrame, Stage};
use crate::bytecode::Operand;

pub(crate) trait ElementFrame<S: Stage>: PropertyFrame<S> {
    /// The VM's store of `value` to `base[key]` where no in-place replacement answers it: an
    /// added element, a typed array, a keyed property, a setter; a failure leaves at `miss`.
    #[inline(always)]
    fn index_set_host(&mut self, _stage: &mut S, _base: S::Word, _key: S::Word, _value: S::Word, _miss: S::Miss) -> Option<()> { None }

    /// The base of the array record table, which an element access addresses.
    #[inline(always)]
    fn array_records(&mut self, _stage: &mut S) -> Option<S::Word> { None }

    /// The tagged word of an element access's key operand; a register's by default.
    #[inline(always)]
    fn key_word(&mut self, stage: &mut S, key: Operand) -> Option<S::Word> {
        Some(self.tagged(stage, key.register_index()?))
    }

    /// The int32 index of an element access's key operand, as its low half; a key that is no
    /// int32 leaves at `miss`. A frame that proved the key's representation reads it back.
    #[inline(always)]
    fn index_key(&mut self, stage: &mut S, key: Operand, miss: S::Miss) -> Option<S::Word> {
        let bits = self.key_word(stage, key)?;
        let int = value::is_int(stage, bits);
        stage.guard(int, miss)?;
        Some(stage.and_imm(bits, u64::from(u32::MAX)))
    }

    /// The view of the array `base` an earlier access proved, which no storage change has
    /// retired; a view proved writable answers a read too.
    #[inline(always)]
    fn kept_view(&mut self, _stage: &mut S, _base: S::Word, _writable: bool) -> Option<ElementView<S::Word>> { None }

    /// Records the view an element access proved of the array `base`.
    #[inline(always)]
    fn keep_view(&mut self, _stage: &mut S, _base: S::Word, _writable: bool, _view: ElementView<S::Word>) {}

    /// Continues where a store of `value` into the array `base` needs no barrier log, for a
    /// value whose type the frame did not prove immediate; a frame with no gate leaves at `miss`.
    #[inline(always)]
    fn array_store_gate(&mut self, _stage: &mut S, _base: S::Word, _value: S::Word, _miss: S::Miss) -> Option<()> { None }

    /// What `fast` reads from the proved view, which leaves at the edge it is given (a hole, or
    /// an index past the backing); where it misses, `host` instead, and past the join only what
    /// both prove holds.
    #[inline(always)]
    fn element_probed(&mut self, stage: &mut S, miss: S::Miss,
        fast: impl FnOnce(&mut S, &mut Self, S::Miss) -> Option<S::Word>,
        host: impl FnOnce(&mut S, &mut Self, S::Miss) -> Option<S::Word>) -> Option<S::Word> {
        match fast(stage, self, miss) {
            Some(word) => Some(word),
            None => host(stage, self, miss),
        }
    }
}
