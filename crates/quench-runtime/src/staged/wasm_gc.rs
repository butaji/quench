//! Wasm GC operations as one semantics: the checks of a struct or array access, of an array's bulk
//! operations, and of a reference's null check and cast, with their order and traps, written once
//! over `WasmGc`. The interpreter runs them on its heap (`HeapStage`, `vm/wasm_gc/stage.rs`); a
//! specializer runs them over its own cells. Allocation and the descriptor operations stay with the
//! stage that owns the heap.

use super::machine::MachineStage;
use super::wasm::Width;
use crate::WasmTrap;
use crate::WasmTypes;
use crate::wasm::gc::{GcField, GcFieldAccess};
use crate::wasm::integer::{I64BinaryOperator as Binary, I64UnaryOperator as Unary};
use crate::wasm::reference::ReferenceTarget;

/// The aggregate an access names: a struct, whose field validation proved present, or an array,
/// whose element past the end traps.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Aggregate {
    Struct,
    Array,
}

impl Aggregate {
    /// The trap a null reference to this aggregate raises.
    const fn null_trap(self) -> WasmTrap {
        match self {
            Self::Struct => WasmTrap::NullStructReference,
            Self::Array => WasmTrap::NullArrayReference,
        }
    }
}

/// The primitives of a GC heap the rules run on. A reference is its bits, an integer its
/// zero-extended word and a flag 0 or 1. The rules hold every check, its order and its trap; a
/// stage answers the primitives only. A primitive a rule names has been checked by the rule first.
pub(crate) trait WasmGc: MachineStage {
    /// The flag that `reference` is null.
    fn is_null(&mut self, reference: Self::Word) -> Self::Word;
    /// The flag that `reference` is a GC object: a cell with fields, neither null nor an i31.
    fn is_object(&mut self, reference: Self::Word) -> Self::Word;
    /// The flag that `reference` is an object whose field `index` exists.
    fn field_exists(&mut self, reference: Self::Word, index: Self::Word) -> Self::Word;
    /// The number of fields of the object `reference`.
    fn field_count(&mut self, reference: Self::Word) -> Self::Word;
    /// The stored word of field `index`: the integer of a packed field, the value's bits otherwise.
    fn read_field(&mut self, reference: Self::Word, index: Self::Word, field: GcField) -> Self::Word;
    /// Stores the value bits `value` at field `index`, in the field's storage.
    fn write_field(&mut self, reference: Self::Word, index: Self::Word, field: GcField, value: Self::Word);
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

/// The flag of the negation of `flag`.
#[inline(always)]
fn negated<S: MachineStage>(stage: &mut S, flag: S::Word) -> S::Word {
    stage.operate_unary(Unary::EqualZero, Width::I32, flag)
}

/// The checks of a reference to an aggregate before its field `index` is read or written: a null
/// reference traps as its aggregate's null trap, then a struct's missing field is rejected or an
/// array's element past the end traps.
#[inline(always)]
fn locate<S: WasmGc>(stage: &mut S, aggregate: Aggregate, reference: S::Word, index: S::Word) -> Result<(), S::Exit> {
    let null = stage.is_null(reference);
    stage.trap_when(null, aggregate.null_trap())?;
    let exists = stage.field_exists(reference, index);
    let missing = negated(stage, exists);
    match aggregate {
        Aggregate::Struct => stage.reject_when(missing, "invalid Wasm aggregate reference"),
        Aggregate::Array => stage.trap_when(missing, WasmTrap::OutOfBoundsArray),
    }
}

/// The field `index` of `reference` as a `struct.get` or `array.get` reads it: a signed packed read
/// sign-extends the packed bits (shifted to the top and back down arithmetically); the others return
/// the stored word.
#[inline(always)]
pub(crate) fn field_read<S: WasmGc>(
    stage: &mut S,
    aggregate: Aggregate,
    reference: S::Word,
    index: S::Word,
    field: GcField,
    access: GcFieldAccess,
) -> Result<S::Word, S::Exit> {
    locate(stage, aggregate, reference, index)?;
    let stored = stage.read_field(reference, index, field);
    let bits = match (access, field.packed_bits()) {
        (GcFieldAccess::ReadSigned, Some(bits)) => bits,
        _ => return Ok(stored),
    };
    let shift = stage.constant(Width::I64, i64::from(u64::BITS - bits));
    let high = stage.operate(Binary::ShiftLeft, Width::I64, stored, shift);
    Ok(stage.operate(Binary::ShiftRightSigned, Width::I64, high, shift))
}

/// The field `index` of `reference` as a `struct.set` or `array.set` writes it.
#[inline(always)]
pub(crate) fn field_write<S: WasmGc>(
    stage: &mut S,
    aggregate: Aggregate,
    reference: S::Word,
    index: S::Word,
    field: GcField,
    value: S::Word,
) -> Result<(), S::Exit> {
    locate(stage, aggregate, reference, index)?;
    stage.write_field(reference, index, field, value);
    Ok(())
}

/// The length of the array `reference`: a null traps, and a reference that is no array traps
/// past the end, like an element access does.
#[inline(always)]
pub(crate) fn array_length<S: WasmGc>(stage: &mut S, reference: S::Word) -> Result<S::Word, S::Exit> {
    let null = stage.is_null(reference);
    stage.trap_when(null, WasmTrap::NullArrayReference)?;
    let object = stage.is_object(reference);
    let missing = negated(stage, object);
    stage.trap_when(missing, WasmTrap::OutOfBoundsArray)?;
    Ok(stage.field_count(reference))
}

/// The array operand of a bulk operation: a null traps, and a reference that is no array is
/// rejected, since validation proved it one.
fn array_operand<S: WasmGc>(stage: &mut S, reference: S::Word) -> Result<(), S::Exit> {
    let null = stage.is_null(reference);
    stage.trap_when(null, WasmTrap::NullArrayReference)?;
    let object = stage.is_object(reference);
    let missing = negated(stage, object);
    stage.reject_when(missing, "invalid Wasm array reference")
}

/// `array.fill`: `extent` elements of `destination` from `output` become `value`. The destination
/// must be mutable, then its range must lie within it.
#[inline(always)]
pub(crate) fn array_fill<S: WasmGc>(
    stage: &mut S,
    destination: S::Word,
    output: S::Word,
    value: S::Word,
    extent: S::Word,
) -> Result<(), S::Exit> {
    array_operand(stage, destination)?;
    let mutable = stage.array_mutable(destination);
    let immutable = negated(stage, mutable);
    stage.reject_when(immutable, "immutable Wasm array fill destination")?;
    let exceeds = stage.fields_exceed(destination, output, extent);
    stage.trap_when(exceeds, WasmTrap::OutOfBoundsArray)?;
    stage.fill_fields(destination, output, extent, value);
    Ok(())
}

/// `array.copy`: `extent` elements of `source` from `input` go to `destination` from `output`. Both
/// arrays are checked for null and kind, then the destination's mutability and the storage
/// compatibility; both complete ranges, including empty ones, are checked before any write.
#[inline(always)]
pub(crate) fn array_copy<S: WasmGc>(
    stage: &mut S,
    destination: S::Word,
    output: S::Word,
    source: S::Word,
    input: S::Word,
    extent: S::Word,
) -> Result<(), S::Exit> {
    array_operand(stage, destination)?;
    array_operand(stage, source)?;
    let mutable = stage.array_mutable(destination);
    let admitted = stage.storage_admits(source, destination);
    let accepted = stage.operate(Binary::And, Width::I32, mutable, admitted);
    let rejected = negated(stage, accepted);
    stage.reject_when(rejected, "incompatible Wasm array copy storage")?;
    let exceeds = stage.fields_exceed(destination, output, extent);
    stage.trap_when(exceeds, WasmTrap::OutOfBoundsArray)?;
    let exceeds = stage.fields_exceed(source, input, extent);
    stage.trap_when(exceeds, WasmTrap::OutOfBoundsArray)?;
    stage.copy_fields(destination, output, source, input, extent);
    Ok(())
}

/// `ref.as_non_null`: the reference, or `trap` where it is null.
#[inline(always)]
pub(crate) fn non_null<S: WasmGc>(stage: &mut S, reference: S::Word, trap: WasmTrap) -> Result<S::Word, S::Exit> {
    let null = stage.is_null(reference);
    stage.trap_when(null, trap)?;
    Ok(reference)
}

/// `ref.cast`: the reference, or a cast failure where `target` does not admit it.
#[inline(always)]
pub(crate) fn reference_cast<S: WasmGc>(
    stage: &mut S,
    reference: S::Word,
    target: ReferenceTarget,
    declarations: &WasmTypes,
) -> Result<S::Word, S::Exit> {
    let admitted = stage.admits(reference, target, declarations);
    let refused = negated(stage, admitted);
    stage.trap_when(refused, WasmTrap::CastFailure)?;
    Ok(reference)
}
