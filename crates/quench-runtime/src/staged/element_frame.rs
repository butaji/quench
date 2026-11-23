//! A frame's element callouts (`ElementFrame`): what the element rows read back of the tier's
//! proofs of an array (its kept view, its record table, its key's int32) and the VM's own steps
//! behind the dense arm. Every default declines, leaving the row to the host path.

use super::element_rows::ElementView;
use super::elements::DenseView;
use super::{value, PropertyFrame, Stage};
use crate::bytecode::{Operand, Register};

/// The element frame's contract for a stencil. The only method a stencil frame must override is
/// `array_records`; every other element method refuses by default (`None`, the row's miss edge):
/// `kept_view`, `array_store_gate`, `index_set_host` and `proved_view`/`keep_view`. `index_key`,
/// `index_offset` and `element_probed` carry the stage logic every frame shares (an int32 guard and
/// the fast-then-host join), so they are not overrides. `index_get_host` is declared on `PropertyFrame`,
/// which this trait extends, and refuses there (`None`); `stored` is the same fast-then-slow join as
/// `element_probed`. Neither is redeclared here, since a second declaration would make the call ambiguous.
pub trait ElementFrame<S: Stage>: PropertyFrame<S> {
    /// The dense view of the array in base register `base` that the frame's array storage proofs
    /// hold, its readable elements, or its writable ones where `writable` (a store replacing in
    /// place). A frame that proves no storage answers `None`, and the access takes its own proof.
    #[inline(always)]
    fn proved_view(&mut self, _stage: &mut S, _base: Register, _writable: bool) -> Option<DenseView<S::Word>> { None }

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

    /// `index_key` as one unsigned compare of the word past the int32 tag
    /// (`Value::int_key_offset`): the tag, sign and range test at once, the offset the index. A
    /// negative int32, which no backing holds, leaves here rather than at the bounds test.
    #[inline(always)]
    fn index_offset(&mut self, stage: &mut S, key: Operand, miss: S::Miss) -> Option<S::Word> {
        let bits = self.key_word(stage, key)?;
        let offset = stage.add_imm(bits, crate::value::ValueLayout::INT_TAG.wrapping_neg());
        let over = stage.ugt_imm(offset, u64::from(i32::MAX as u32));
        let index = stage.not(over);
        stage.guard(index, miss)?;
        Some(offset)
    }

    /// Whether a read with no kept view reads every backing of a direct array in place
    /// (`elements::any_backing_element`: wide, narrow and double), as the interpreter's frame
    /// does; a frame that keeps views proves the wide one alone (`prove_view`).
    #[inline(always)]
    fn reads_any_backing(&self) -> bool { false }

    /// Whether a typed element a kept view reads is its `Value` word (`typed_read_value`) rather
    /// than its Number's `f64` word (`typed_read`): a frame whose registers hold values (the
    /// interpreter's lane) takes the value, so an int32 element stays an int32; a tier that keeps
    /// the Number unboxed takes the `f64`.
    #[inline(always)]
    fn typed_values(&self) -> bool { false }

    /// The view of the array `base` an earlier access proved, or its loop's entering edge did for
    /// a typed array, which no storage change has retired; a view proved writable answers a read too.
    #[inline(always)]
    fn kept_view(&mut self, _stage: &mut S, _base: S::Word, _writable: bool) -> Option<ElementView<S::Word>> { None }

    /// Records the dense view an element access proved of the array `base`.
    #[inline(always)]
    fn keep_view(&mut self, _stage: &mut S, _base: S::Word, _writable: bool, _view: DenseView<S::Word>) {}

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
