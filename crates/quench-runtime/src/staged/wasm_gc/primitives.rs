//! The rule primitives: the `WasmGc` trait a stage implements, and the aggregate an access names.

use super::*;

/// The aggregate an access names: a struct, whose field validation proved present, or an array,
/// whose element past the end traps.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Aggregate {
    Struct,
    Array,
}

/// The primitives of a GC heap the rules run on. A reference is its bits, an integer its
/// zero-extended word and a flag 0 or 1. The rules hold every check, its order and its trap; a
/// stage answers the primitives only. A primitive a rule names has been checked by the rule first.
pub trait WasmGc: MachineStage {
    /// Whether `struct_slot` answers every struct read, validation having proved each field present, so
    /// the rule skips the missing-field test of a struct read. Stages keep the test by default.
    const PROVEN_STRUCT_READS: bool = false;
    /// The flag that `reference` is null.
    fn is_null(&mut self, reference: Self::Word) -> Self::Word;
    /// The flag that `reference` is a GC object: a cell with fields, neither null nor an i31.
    fn is_object(&mut self, reference: Self::Word) -> Self::Word;
    /// The address of field `index` of `reference`'s storage, 0 where `reference` is no object or
    /// has no field `index`: one lookup that both checks the field and locates it, as a compiled
    /// access computes the address it bounds-checks. Valid until the next allocation.
    fn field_slot(&mut self, reference: Self::Word, index: Self::Word) -> Self::Word;
    /// `field_slot` for a struct field read, at the field's static payload `offset` where the lowering
    /// carried it. A stage computes the address inline from `offset` when it can; the default is `field_slot`.
    fn struct_slot(&mut self, reference: Self::Word, index: Self::Word, _offset: Option<u32>) -> Self::Word {
        self.field_slot(reference, index)
    }
    /// `field_slot` for an array element read, which a stage may compute inline at the element's stride.
    fn element_slot(&mut self, reference: Self::Word, index: Self::Word, _field: GcField) -> Self::Word { self.field_slot(reference, index) }
    /// `field_slot` for a store into the field: the same address, past the write barrier a store
    /// into the object requires. Reads never take it, so a read of a field is barrier-free.
    fn field_slot_for_write(&mut self, reference: Self::Word, index: Self::Word) -> Self::Word;
    /// The number of fields of the object `reference`.
    fn field_count(&mut self, reference: Self::Word) -> Self::Word;
    /// The Wasm bits stored at the field address `slot` (`field_slot`, checked nonzero), typed by
    /// the field's static storage: a packed or 32-bit field's bits zero-extended, a 64-bit
    /// number's bits, a reference's bits. No box, no tag: the field's type says what they are.
    fn read_slot(&mut self, slot: Self::Word, field: GcField) -> Self::Word;
    /// Stores the Wasm bits `value` (as `read_slot` gives them) at the field address `slot`, in
    /// the field's storage.
    fn write_slot(&mut self, slot: Self::Word, field: GcField, value: Self::Word);
    /// The flag that the array `reference` has mutable elements.
    fn array_mutable(&mut self, reference: Self::Word) -> Self::Word;
    /// The flag that the elements of array `source` are storable in array `destination`.
    fn storage_admits(&mut self, source: Self::Word, destination: Self::Word) -> Self::Word;
    /// The flag that `extent` fields from `index` pass the end of the object `reference`'s fields.
    fn fields_exceed(&mut self, reference: Self::Word, index: Self::Word, extent: Self::Word) -> Self::Word;
    /// Stores the value bits `value` in `extent` elements of the array `reference` from `index`,
    /// each in the element storage.
    fn fill_fields(&mut self, reference: Self::Word, index: Self::Word, extent: Self::Word, value: Self::Word);
    /// Copies `extent` stored fields of `source` from `input` into `destination` from `output`.
    fn copy_fields(
        &mut self,
        destination: Self::Word,
        output: Self::Word,
        source: Self::Word,
        input: Self::Word,
        extent: Self::Word,
    );
    /// The flag that `reference` passes a cast to `target`, a type of `declarations`.
    fn admits(&mut self, reference: Self::Word, target: ReferenceTarget, declarations: &WasmTypes) -> Self::Word;
    /// Continues where `flag` is 0; where it is 1, rejects with `reason`: a validation failure of
    /// the module, which the rules never reach for a validated one.
    fn reject_when(&mut self, flag: Self::Word, reason: &'static str) -> Result<(), Self::Exit>;
}
