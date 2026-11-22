//! The VM callout of the reference and field operations of the b-jit's Wasm functions
//! (`Step::Gc`): array length, `ref.is_null`, `ref.as_non_null`, `ref.eq`, `ref.test`, `ref.cast`,
//! and the struct and array field accesses (`struct.get`, `array.set`, ...). The rows that call it
//! are `call_w_21_<flags>_1_0` (`quench-bjit/stencils/wasm_rows_callout.rs`), the same kind-21 (`CALLOUT`) row
//! the table callouts use, so no stencil is added.
//!
//! The callout's ABI, the one `wasm_table_ops_stage::table_op_callout` has:
//!
//!     extern "C" fn(context: usize, request: u64, a: u64, b: u64, c: u64, value: u64, out: *mut u64) -> i64
//!
//! - `request`: the operation's `GcCall` tag in bits 0..8 and the instruction's immediate in bits
//!   8..40 (a non-null check's tag, a cast's target, a field's tag, else unused).
//! - `a`: the reference operand's Wasm bits, which are its `Value` bits (a frame's reference
//!   register holds them, as a table get writes them). `b`: the second reference of `ref.eq`, an
//!   array's element index. `c`: a field write's data word.
//! - `out`: the result word, written on success. A result that is i32 is wrapped by the row
//!   (`CALLOUT_WRAP`); a reference or a 64-bit result is written whole.
//! - the status: 0 when the operation ran, else the status a trap leaves with (`finish_callout`).
//!
//! The field words are the Wasm bits of the field's value class (`GcFieldValue`): an i32
//! zero-extended, an i64 number's bits, a reference's Value bits. The interpreter's heap works on
//! Value bits, so the callout converts at its boundary (`stored_bits`, `value_of_bits`, the u-jit's
//! conversions). Allocation is not here: `wasm_gc_allocate.rs` owns it.
//!
//! The rules are the ones the interpreter runs (`staged::wasm_gc`, on `HeapStage`), so both tiers
//! share the checks, their order and their traps.

use super::{CallContext, JsError, ProgramId, Vm, finish_callout};
use quench_runtime::staged::wasm_gc::{Aggregate, WasmGc, array_length, field_read, field_write, non_null};
use quench_runtime::wasm::gc::{GcField, GcFieldAccess, GcFieldValue};
use quench_runtime::wasm::reference::{NonNullCheck, ReferenceTarget};
use quench_runtime::vm::wasm_gc::stage::HeapStage;

/// The GC operation a callout runs: its tag is the request's low byte (`wasm_stitch_gc.rs`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum GcCall {
    ArrayLength = 0,
    IsNull = 1,
    AsNonNull = 2,
    RefEq = 3,
    RefTest = 4,
    RefCast = 5,
    /// A struct field read (`struct.get`, unsigned or plain), its index in the field's tag.
    StructGet = 6,
    /// A struct field read that sign-extends a packed field (`struct.get_s`).
    StructGetSigned = 7,
    /// An array element read (`array.get`, unsigned or plain), its index in register `b`.
    ArrayGet = 8,
    /// An array element read that sign-extends a packed element (`array.get_s`).
    ArrayGetSigned = 9,
    /// A struct field write (`struct.set`), its data in `c`.
    StructSet = 10,
    /// An array element write (`array.set`), its index in `b` and its data in `c`.
    ArraySet = 11,
}

impl GcCall {
    /// The request tag of the operation.
    pub(crate) const fn tag(self) -> u64 {
        self as u64
    }

    fn from_tag(tag: u64) -> Option<Self> {
        Some(match tag {
            0 => Self::ArrayLength,
            1 => Self::IsNull,
            2 => Self::AsNonNull,
            3 => Self::RefEq,
            4 => Self::RefTest,
            5 => Self::RefCast,
            6 => Self::StructGet,
            7 => Self::StructGetSigned,
            8 => Self::ArrayGet,
            9 => Self::ArrayGetSigned,
            10 => Self::StructSet,
            11 => Self::ArraySet,
            _ => return None,
        })
    }
}

/// The field a request's immediate names (its tag), which the stitcher planned.
fn field_of(imm: u32) -> Result<GcField, JsError> {
    GcField::from_tag(imm).ok_or_else(|| JsError::validation("invalid Wasm field operand".into()))
}

/// The Wasm bits of the field word `word` that `field_read` returned: a narrow field's i32
/// zero-extended (a packed one's sign-extended integer is truncated to its 32 bits), else the
/// word itself, the field's bits as stored.
fn read_bits(field: GcField, word: u64) -> u64 {
    if field.value() == GcFieldValue::Narrow { word as u32 as u64 } else { word }
}

/// The result word of `call` over its operands, which the rules check (a trap or validation error
/// raised by a rule propagates). The casts read `program`'s declarations.
fn run(vm: &mut Vm, program: ProgramId, call: GcCall, imm: u32, a: u64, b: u64, c: u64) -> Result<u64, JsError> {
    match call {
        GcCall::ArrayLength => array_length(&mut HeapStage { vm }, a),
        GcCall::IsNull => Ok(WasmGc::is_null(&mut HeapStage { vm }, a)),
        GcCall::AsNonNull => {
            let check = NonNullCheck::from_tag(imm)
                .ok_or_else(|| JsError::validation("invalid Wasm non-null check".into()))?;
            non_null(&mut HeapStage { vm }, a, check.trap())
        }
        GcCall::RefEq => Ok(u64::from(a == b)),
        GcCall::RefTest | GcCall::RefCast => {
            vm.wasm_ref_test_or_cast(program, ReferenceTarget::validated(imm), a, call == GcCall::RefCast)
        }
        GcCall::StructGet | GcCall::StructGetSigned | GcCall::ArrayGet | GcCall::ArrayGetSigned => {
            let field = field_of(imm)?;
            let structure = matches!(call, GcCall::StructGet | GcCall::StructGetSigned);
            let signed = matches!(call, GcCall::StructGetSigned | GcCall::ArrayGetSigned);
            let (aggregate, index) = if structure {
                (Aggregate::Struct, u64::from(field.index()))
            } else {
                (Aggregate::Array, b)
            };
            let access = if signed { GcFieldAccess::ReadSigned } else { GcFieldAccess::Read };
            let word = field_read(&mut HeapStage { vm: &mut *vm }, aggregate, a, index, field, access, None)?;
            Ok(read_bits(field, word))
        }
        GcCall::StructSet | GcCall::ArraySet => {
            let field = field_of(imm)?;
            let (aggregate, index) = if call == GcCall::StructSet {
                (Aggregate::Struct, u64::from(field.index()))
            } else {
                (Aggregate::Array, b)
            };
            field_write(&mut HeapStage { vm: &mut *vm }, aggregate, a, index, field, c, None)?;
            Ok(0)
        }
    }
}

/// The GC callout of the b-jit's rows (`CallContext` holds the VM and the program): decodes the
/// request, runs the operation on the VM's heap, and writes the result to `out`. A trap leaves
/// through `finish_callout`, as the table callout's does.
pub(crate) unsafe extern "C" fn wasm_gc_callout(
    context: *mut CallContext,
    request: u64,
    a: u64,
    b: u64,
    c: u64,
    _value: u64,
    out: *mut u64,
) -> i64 {
    // SAFETY: compiled code passes the context of its call, whose VM is suspended in it.
    let context = unsafe { &mut *context };
    let vm = unsafe { &mut *context.vm.cast::<Vm>() };
    let program = context.program;
    let imm = ((request >> 8) & 0xffff_ffff) as u32;
    let result = GcCall::from_tag(request & 0xff)
        .ok_or_else(|| JsError::validation("invalid Wasm GC callout".into()))
        .and_then(|call| run(vm, program, call, imm, a, b, c));
    match result {
        Ok(word) => {
            // SAFETY: the row gives `out` a frame slot of its result, or a scratch word.
            unsafe { *out = word };
            0
        }
        Err(error) => finish_callout(context, Ok(Err(error))),
    }
}
