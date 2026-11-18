//! The VM callout of the reference operations of the b-jit's Wasm functions (`Step::Gc`): array
//! length, `ref.is_null`, `ref.as_non_null`, `ref.eq`, `ref.test` and `ref.cast`. The rows that
//! call it are `call_w_31_<flags>_1_0` (`quench-bjit/stencils/wasm_rows_callout.rs`), the same
//! kind-31 row the table callouts use, so no stencil is added.
//!
//! The callout's ABI, the one `wasm_table_ops_stage::table_op_callout` has:
//!
//!     extern "C" fn(context: usize, request: u64, a: u64, b: u64, c: u64, value: u64, out: *mut u64) -> i64
//!
//! - `request`: the operation's `GcCall` tag in bits 0..8 and the instruction's immediate in bits
//!   8..40 (a non-null check's tag, a cast's target, else unused).
//! - `a`: the reference operand's Wasm bits, which are its `Value` bits (a frame's reference
//!   register holds them, as a table get writes them). `b`: the second reference of `ref.eq`.
//! - `out`: the result word, written on success. A result that is i32 is wrapped by the row
//!   (`CALLOUT_WRAP`); a reference result is written whole.
//! - the status: 0 when the operation ran, else the status a trap leaves with (`finish_callout`).
//!
//! Only operations whose operands are references or i32 results are here. The GC field
//! operations (`struct.get`, `array.set`, ...) are not: the frame holds no Wasm types, and a
//! stored field value cannot tell an i32 from an `i31` reference (both are `Value::integer`), so
//! their frame words cannot be converted. Allocation is not here either: a collection could free
//! the references a stitched frame holds, as the frame roots none (`wasm_stitch_gc.rs`).
//!
//! The rules are the ones the interpreter runs (`staged::wasm_gc`, on `HeapStage`), so both tiers
//! share the checks, their order and their traps.

use super::{CallContext, Host, JsError, ProgramId, Vm, finish_callout};
use crate::staged::wasm_gc::{WasmGc, array_length, non_null, reference_cast};
use crate::wasm::reference::{NonNullCheck, ReferenceTarget};
use super::super::wasm_gc::stage::HeapStage;

/// The GC operation a callout runs: its tag is the request's low byte (`wasm_stitch_gc.rs`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum GcCall {
    ArrayLength = 0,
    IsNull = 1,
    AsNonNull = 2,
    RefEq = 3,
    RefTest = 4,
    RefCast = 5,
}

impl GcCall {
    /// The request tag of the operation.
    pub(super) const fn tag(self) -> u64 {
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
            _ => return None,
        })
    }
}

/// The result word of `call` over its operands, which the rules check (a trap or validation error
/// raised by a rule propagates). `declarations` are the module's, read only by the casts.
fn run<H: Host>(vm: &mut Vm<H>, program: ProgramId, call: GcCall, imm: u32, a: u64, b: u64) -> Result<u64, JsError> {
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
            let declarations = vm
                .programs
                .wasm_signatures(program)
                .map(|pool| pool.declarations.clone())
                .ok_or_else(|| JsError::validation("Wasm GC operation requires module declarations".into()))?;
            let target = ReferenceTarget::validated(imm);
            let stage = &mut HeapStage { vm };
            if call == GcCall::RefTest {
                Ok(WasmGc::admits(stage, a, target, &declarations))
            } else {
                reference_cast(stage, a, target, &declarations)
            }
        }
    }
}

/// The GC callout of the b-jit's rows (`CallContext` holds the VM and the program): decodes the
/// request, runs the operation on the VM's heap, and writes the result to `out`. A trap leaves
/// through `finish_callout`, as the table callout's does.
pub(super) unsafe extern "C" fn wasm_gc_callout<H: Host>(
    context: *mut CallContext,
    request: u64,
    a: u64,
    b: u64,
    _c: u64,
    _value: u64,
    out: *mut u64,
) -> i64 {
    // SAFETY: compiled code passes the context of its call, whose VM is suspended in it.
    let context = unsafe { &mut *context };
    let vm = unsafe { &mut *context.vm.cast::<Vm<H>>() };
    let program = context.program;
    let imm = ((request >> 8) & 0xffff_ffff) as u32;
    let result = GcCall::from_tag(request & 0xff)
        .ok_or_else(|| JsError::validation("invalid Wasm GC callout".into()))
        .and_then(|call| run(vm, program, call, imm, a, b));
    match result {
        Ok(word) => {
            // SAFETY: the row gives `out` a frame slot of its result, or a scratch word.
            unsafe { *out = word };
            0
        }
        Err(error) => finish_callout(context, Ok(Err(error))),
    }
}
