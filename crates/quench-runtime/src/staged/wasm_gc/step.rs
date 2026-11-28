//! The step vocabulary: what each Wasm GC opcode is (`GcStep`), and the group the tiers match
//! (`GcOp`), both derived from the one opcode table (`table.rs`).

use super::*;
use crate::wasm_gc_table;

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
        || matches!(crate::staged::wasm_call::call_of(op), Some(crate::staged::wasm_call::Call::IndirectTarget | crate::staged::wasm_call::Call::RefFunc))
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
