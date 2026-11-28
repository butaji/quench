//! The rules: the checks of each Wasm GC access, written once over `WasmGc`, with their order and traps.

use super::*;

impl Aggregate {
    /// The trap a null reference to this aggregate raises.
    const fn null_trap(self) -> WasmTrap {
        match self {
            Self::Struct => WasmTrap::NullStructReference,
            Self::Array => WasmTrap::NullArrayReference,
        }
    }
}

/// The flag of the negation of `flag`.
#[inline(always)]
fn negated<S: MachineStage>(stage: &mut S, flag: S::Word) -> S::Word {
    stage.operate_unary(Unary::EqualZero, Width::I32, flag)
}

/// The checks of a reference to an aggregate before its field `index` is read or written: a null
/// reference traps as its aggregate's null trap, then a struct's missing field is rejected or an
/// array's element past the end traps. A struct read a stage proves (`PROVEN_STRUCT_READS`) has no
/// missing-field test. The field's address, which the access then uses.
#[inline(always)]
fn locate<S: WasmGc>(
    stage: &mut S,
    aggregate: Aggregate,
    reference: S::Word,
    index: S::Word,
    field: GcField,
    offset: Option<u32>,
    write: bool,
) -> Result<S::Word, S::Exit> {
    let null = stage.is_null(reference);
    stage.trap_when(null, aggregate.null_trap())?;
    let slot = if write { stage.field_slot_for_write(reference, index) }
        else if aggregate == Aggregate::Array { stage.element_slot(reference, index, field) }
        else { stage.struct_slot(reference, index, offset) };
    if aggregate == Aggregate::Struct && !write && S::PROVEN_STRUCT_READS {
        // Validation proved the field present, and the stage's address needs no test.
        return Ok(slot);
    }
    let missing = stage.operate_unary(Unary::EqualZero, Width::I64, slot);
    match aggregate {
        Aggregate::Struct => stage.reject_when(missing, "invalid Wasm aggregate reference")?,
        Aggregate::Array => stage.trap_when(missing, WasmTrap::OutOfBoundsArray)?,
    }
    Ok(slot)
}

/// The field `index` of `reference` as a `struct.get` or `array.get` reads it: a signed packed read
/// sign-extends the packed bits (shifted to the top and back down arithmetically); the others return
/// the stored word. `offset` is a struct field's static payload offset where the lowering carried it.
#[inline(always)]
pub fn field_read<S: WasmGc>(
    stage: &mut S,
    aggregate: Aggregate,
    reference: S::Word,
    index: S::Word,
    field: GcField,
    access: GcFieldAccess,
    offset: Option<u32>,
) -> Result<S::Word, S::Exit> {
    let slot = locate(stage, aggregate, reference, index, field, offset, false)?;
    let stored = stage.read_slot(slot, field);
    let bits = match (access, field.packed_bits()) {
        (GcFieldAccess::ReadSigned, Some(bits)) => bits,
        _ => return Ok(stored),
    };
    let shift = stage.constant(Width::I64, i64::from(u64::BITS - bits));
    let high = stage.operate(Binary::ShiftLeft, Width::I64, stored, shift);
    Ok(stage.operate(Binary::ShiftRightSigned, Width::I64, high, shift))
}

/// The field `index` of `reference` as a `struct.set` or `array.set` writes it (`offset` as `field_read`).
#[inline(always)]
pub fn field_write<S: WasmGc>(
    stage: &mut S,
    aggregate: Aggregate,
    reference: S::Word,
    index: S::Word,
    field: GcField,
    value: S::Word,
    offset: Option<u32>,
) -> Result<(), S::Exit> {
    let slot = locate(stage, aggregate, reference, index, field, offset, true)?;
    stage.write_slot(slot, field, value);
    Ok(())
}

/// The length of the array `reference`: a null traps, and a reference that is no array traps
/// past the end, like an element access does.
#[inline(always)]
pub fn array_length<S: WasmGc>(stage: &mut S, reference: S::Word) -> Result<S::Word, S::Exit> {
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
pub fn array_fill<S: WasmGc>(
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
pub fn array_copy<S: WasmGc>(
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
pub fn non_null<S: WasmGc>(stage: &mut S, reference: S::Word, trap: WasmTrap) -> Result<S::Word, S::Exit> {
    let null = stage.is_null(reference);
    stage.trap_when(null, trap)?;
    Ok(reference)
}

/// `ref.cast`: the reference, or a cast failure where `target` does not admit it.
#[inline(always)]
pub fn reference_cast<S: WasmGc>(
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
