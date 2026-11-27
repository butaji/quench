//! Wasm GC operations as one semantics: the checks of a struct or array access, of an array's bulk
//! operations, and of a reference's null check and cast, with their order and traps, written once
//! over `WasmGc`. The interpreter runs them on its heap (`HeapStage`, `vm/wasm_gc/stage.rs`); a
//! specializer runs them over its own cells. Allocation and the descriptor operations stay with the
//! stage that owns the heap.

use super::machine::MachineStage;
use super::wasm::Width;
use crate::WasmTrap;
use crate::WasmTypes;
use crate::bytecode::Op;
use crate::wasm::gc::{ArraySegmentKind, GcField, GcFieldAccess};
use crate::wasm::integer::{I64BinaryOperator as Binary, I64UnaryOperator as Unary};
use crate::wasm::reference::ReferenceTarget;

/// The aggregate an access names: a struct, whose field validation proved present, or an array,
/// whose element past the end traps.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Aggregate {
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

/// What a Wasm GC instruction is, for the tiers (`Step::Gc`): the groups whose rules and
/// primitives differ. The call targets (`ref.func`, an indirect target) are `Step::Call`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GcOp {
    /// `struct.get[_s|_u]`, `struct.set`, `array.get[_s|_u]`, `array.set`: `field_read` or
    /// `field_write`.
    Field,
    /// `array.len`: `array_length`.
    ArrayLength,
    /// `ref.is_null` and `ref.as_non_null` (`non_null`).
    Null,
    /// `ref.eq`: equal reference bits.
    Equal,
    /// `ref.test` and `ref.cast` (`reference_cast`), without descriptors.
    Test,
    /// Everything that can allocate a cell, so make a collection due: `struct.new*`,
    /// `array.new*`, `array.init_*`, `array.fill`, `array.copy`, `ref.i31`, the extern
    /// conversions, the descriptor operations. A tier runs these only where it roots the
    /// references its frame holds.
    Allocating,
}

/// The array constructor's initial contents: `array.new` repeats a value, `array.new_default`
/// fills defaults, `array.new_fixed` takes a window of values.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ArrayConstruction {
    Repeated,
    Default,
    Fixed,
}

/// What a Wasm GC value opcode does, with the fields its rule and operands need (`GcStep::of`).
/// Each step's group is the coarse `GcOp` the tiers match (`GcStep::group`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GcStep {
    /// `struct.get`, `struct.get_s`, `struct.get_u`: `signed` is `None` for a plain read, else the
    /// sign of a packed field's read.
    StructGet { signed: Option<bool> },
    /// `struct.set`.
    StructSet,
    /// `array.get`, `array.get_s`, `array.get_u`, as `StructGet`.
    ArrayGet { signed: Option<bool> },
    /// `array.set`.
    ArraySet,
    /// `array.len`.
    ArrayLen,
    /// `array.new`, `array.new_default`, `array.new_fixed`.
    ArrayNew { construction: ArrayConstruction },
    /// `array.new_data`, `array.new_elem` (`init` false), `array.init_data`, `array.init_elem`
    /// (`init` true).
    ArraySegment { kind: ArraySegmentKind, init: bool },
    /// `array.fill`.
    ArrayFill,
    /// `array.copy`.
    ArrayCopy,
    /// `struct.new`, `struct.new_default`, `struct.new_desc`, `struct.new_default_desc`.
    StructNew { default: bool, descriptor: bool },
    /// `ref.get_desc`.
    GetDescriptor,
    /// `ref.test` and `ref.cast` (`cast`), of a reference or of its descriptor (`descriptor`).
    Test { cast: bool, descriptor: bool },
    /// `ref.is_null`.
    IsNull,
    /// `ref.as_non_null`.
    AsNonNull,
    /// `ref.eq`.
    Equal,
    /// `ref.i31`.
    I31,
    /// The extern conversions (`any.convert_extern`, `extern.convert_any`).
    ExternConversion,
}

/// The one table of the Wasm GC value opcodes the value lane runs, each with its step (`GcStep::of`)
/// or `None` for a call target (`Call`, which the step does not own). `wasm_gc_table!` hands the
/// entries to a projector: `wasm_gc_step_of` makes `GcStep::of`'s match, `wasm_gc_op_of` makes
/// `GcStep::op`'s reverse match, `wasm_gc_names` the bare names the lane's handler table and
/// `wasm_gc_value_ops!` apply their macro to. The list is written
/// here once; nothing else names these opcodes as a set.
#[macro_export]
#[doc(hidden)]
macro_rules! wasm_gc_table {
    ($projector:ident $extra:ident) => {
        $crate::$projector! {
            $extra ;
            WasmStructGet => Some(GcStep::StructGet { signed: None }),
            WasmStructGetS => Some(GcStep::StructGet { signed: Some(true) }),
            WasmStructGetU => Some(GcStep::StructGet { signed: Some(false) }),
            WasmStructSet => Some(GcStep::StructSet),
            WasmArrayGet => Some(GcStep::ArrayGet { signed: None }),
            WasmArrayGetS => Some(GcStep::ArrayGet { signed: Some(true) }),
            WasmArrayGetU => Some(GcStep::ArrayGet { signed: Some(false) }),
            WasmArraySet => Some(GcStep::ArraySet),
            WasmStructGetRaw => Some(GcStep::StructGet { signed: None }),
            WasmStructSetRaw => Some(GcStep::StructSet),
            WasmArrayGetRaw => Some(GcStep::ArrayGet { signed: None }),
            WasmArraySetRaw => Some(GcStep::ArraySet),
            WasmArrayLen => Some(GcStep::ArrayLen),
            WasmRefIsNull => Some(GcStep::IsNull),
            WasmRefAsNonNull => Some(GcStep::AsNonNull),
            WasmRefEq => Some(GcStep::Equal),
            WasmRefTest => Some(GcStep::Test { cast: false, descriptor: false }),
            WasmRefCast => Some(GcStep::Test { cast: true, descriptor: false }),
            WasmI31 => Some(GcStep::I31),
            WasmExternalConversion => Some(GcStep::ExternConversion),
            WasmArrayNewData => Some(GcStep::ArraySegment { kind: ArraySegmentKind::Data, init: false }),
            WasmArrayNewElem => Some(GcStep::ArraySegment { kind: ArraySegmentKind::Element, init: false }),
            WasmArrayInitData => Some(GcStep::ArraySegment { kind: ArraySegmentKind::Data, init: true }),
            WasmArrayInitElem => Some(GcStep::ArraySegment { kind: ArraySegmentKind::Element, init: true }),
            WasmArrayFill => Some(GcStep::ArrayFill),
            WasmArrayCopy => Some(GcStep::ArrayCopy),
            WasmArrayNew => Some(GcStep::ArrayNew { construction: ArrayConstruction::Repeated }),
            WasmArrayNewDefault => Some(GcStep::ArrayNew { construction: ArrayConstruction::Default }),
            WasmArrayNewFixed => Some(GcStep::ArrayNew { construction: ArrayConstruction::Fixed }),
            WasmStructNew => Some(GcStep::StructNew { default: false, descriptor: false }),
            WasmStructNewDefault => Some(GcStep::StructNew { default: true, descriptor: false }),
            WasmStructNewDesc => Some(GcStep::StructNew { default: false, descriptor: true }),
            WasmStructNewDefaultDesc => Some(GcStep::StructNew { default: true, descriptor: true }),
            WasmRefGetDesc => Some(GcStep::GetDescriptor),
            WasmDescriptorTest => Some(GcStep::Test { cast: false, descriptor: true }),
            WasmDescriptorCast => Some(GcStep::Test { cast: true, descriptor: true }),
            WasmIndirectTarget => None,
            WasmRefFunc => None,
        }
    };
}

/// Projects `wasm_gc_table!`'s entries to the match of `GcStep::of`, over the opcode `$op`.
#[macro_export]
#[doc(hidden)]
macro_rules! wasm_gc_step_of {
    ($op:ident ; $($name:ident => $step:expr,)*) => {
        match $op {
            $(Op::$name => $step,)*
            _ => None,
        }
    };
}

/// Projects `wasm_gc_table!`'s entries to the reverse map `GcStep::op`: the opcode whose entry is
/// the step `$step`. Each value step has one entry, so the first match is the one.
#[macro_export]
#[doc(hidden)]
macro_rules! wasm_gc_op_of {
    ($step:ident ; $($name:ident => $entry:expr,)*) => {
        {
            $(if $entry == $step { return Op::$name; })*
            unreachable!("every GC value step has an opcode")
        }
    };
}

/// Projects `wasm_gc_table!`'s entries to the bare opcode names, applying `$then!` to them.
#[macro_export]
#[doc(hidden)]
macro_rules! wasm_gc_names {
    ($then:ident ; $($name:ident => $group:expr,)*) => {
        $then!($($name)*)
    };
}

/// Applies `$then!` to the names of every Wasm GC value opcode: the one list (`wasm_gc_table!`)
/// that `GcOp::of` and the lane's handlers both derive from.
#[macro_export]
#[doc(hidden)]
macro_rules! wasm_gc_value_ops {
    ($then:ident) => {
        $crate::wasm_gc_table!(wasm_gc_names $then)
    };
}

/// The access of a plain or packed read: `signed` as `GcStep::StructGet` and `ArrayGet` carry it.
const fn read_access(signed: Option<bool>) -> GcFieldAccess {
    match signed {
        None => GcFieldAccess::Read,
        Some(true) => GcFieldAccess::ReadSigned,
        Some(false) => GcFieldAccess::ReadUnsigned,
    }
}

/// The signedness of a read access, as `GcStep::StructGet` and `ArrayGet` carry it; a write has
/// none (`None`), and no field step asks for one.
const fn read_signed(access: GcFieldAccess) -> Option<bool> {
    match access {
        GcFieldAccess::Read | GcFieldAccess::Write => None,
        GcFieldAccess::ReadSigned => Some(true),
        GcFieldAccess::ReadUnsigned => Some(false),
    }
}

/// Whether `op` is a value step `Vm::wasm_gc_value_step` makes: a GC value operation
/// (`GcStep::of`), or an instance read, which names a function of the running instance (an
/// indirect call's target, `ref.func`).
#[cfg(not(stencil_build))]
#[inline(always)]
pub const fn gc_value_op(op: Op) -> bool {
    GcStep::of(op).is_some()
        || matches!(super::wasm_call::call_of(op), Some(super::wasm_call::Call::IndirectTarget | super::wasm_call::Call::RefFunc))
}

impl GcStep {
    /// The step of `op`, `None` for an opcode that is no GC value operation (a call target is none).
    #[inline(always)]
    pub const fn of(op: Op) -> Option<Self> {
        wasm_gc_table!(wasm_gc_step_of op)
    }

    /// The opcode of this step: the one whose table entry is this step (the reverse of `of`).
    pub(crate) fn op(self) -> Op {
        let step = Some(self);
        wasm_gc_table!(wasm_gc_op_of step)
    }

    /// The field step of an access of `aggregate` with `access`: a struct's or an array's read,
    /// packed or plain, or its write.
    pub(crate) const fn field_step(aggregate: Aggregate, access: GcFieldAccess) -> Self {
        match (aggregate, access) {
            (Aggregate::Struct, GcFieldAccess::Write) => Self::StructSet,
            (Aggregate::Array, GcFieldAccess::Write) => Self::ArraySet,
            (Aggregate::Struct, access) => Self::StructGet { signed: read_signed(access) },
            (Aggregate::Array, access) => Self::ArrayGet { signed: read_signed(access) },
        }
    }

    /// The group the tiers match: the coarse `GcOp` of this step.
    pub(crate) const fn group(self) -> GcOp {
        match self {
            Self::StructGet { .. } | Self::StructSet | Self::ArrayGet { .. } | Self::ArraySet => GcOp::Field,
            Self::ArrayLen => GcOp::ArrayLength,
            Self::IsNull | Self::AsNonNull => GcOp::Null,
            Self::Equal => GcOp::Equal,
            Self::Test { descriptor: false, .. } => GcOp::Test,
            Self::Test { descriptor: true, .. }
            | Self::ArrayNew { .. }
            | Self::ArraySegment { .. }
            | Self::ArrayFill
            | Self::ArrayCopy
            | Self::StructNew { .. }
            | Self::GetDescriptor
            | Self::I31
            | Self::ExternConversion => GcOp::Allocating,
        }
    }

    /// The aggregate and the access of a field step (`struct.get`/`set`, `array.get`/`set`), `None`
    /// for any other step.
    pub const fn field(self) -> Option<(Aggregate, GcFieldAccess)> {
        Some(match self {
            Self::StructGet { signed } => (Aggregate::Struct, read_access(signed)),
            Self::StructSet => (Aggregate::Struct, GcFieldAccess::Write),
            Self::ArrayGet { signed } => (Aggregate::Array, read_access(signed)),
            Self::ArraySet => (Aggregate::Array, GcFieldAccess::Write),
            _ => return None,
        })
    }
}

impl GcOp {
    /// The group of `op`, `None` for an opcode that is no GC operation (a call target is none).
    pub(crate) const fn of(op: Op) -> Option<Self> {
        match GcStep::of(op) {
            Some(step) => Some(step.group()),
            None => None,
        }
    }

    /// Whether the operation can allocate (`Allocating`); the others only read or write cells
    /// that exist, so no collection runs during them.
    pub(crate) const fn allocates(self) -> bool {
        matches!(self, Self::Allocating)
    }
}
