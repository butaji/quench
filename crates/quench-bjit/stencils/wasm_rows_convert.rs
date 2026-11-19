//! The Wasm float rows that need a float constant, as stencils with no constant pool: the
//! float-to-integer conversions (trapping and saturating), the unsigned 64-bit integer-to-float
//! conversions, and copysign. LLVM loads every float bound or mask those rows use from a
//! RIP-relative constant-pool entry, which a row cannot relocate (`wasm_families.rs`). The bit forms
//! of those constants and their conversions are the staged helpers of `staged::wasm_float`
//! (`saturated`, `unsigned_to_f64`, ...), which turn an integer immediate into a float, so LLVM
//! emits a `movabs` and a `movq`.
//!
//! `convert_row` runs the staged rule, `staged::wasm_float::convert`, which holds the trap order
//! (the NaN trap, then the overflow trap) once, over `ConstantFree` (`wasm_stage.rs`): the stage
//! of the primitives that touch a float constant, which compute the same values from bits.
//!
//! `copysign_row` runs the staged rule `staged::wasm_float::float_binary`, whose CopySign primitive
//! is the bit operation of `staged::wasm_float::copysign_bits` on `ConstantFree`.
//!
//! A row continues (`Out::Next`) or leaves through exit 0 with the trap's status in the first
//! pinned register, as `wasm_rows.rs` rows do.

use super::wasm_frame::{float_of, trap};
pub(crate) use super::wasm_stage::ConstantFree;
use crate::staged::wasm_float::{convert, conversion_operator, float_binary};
use crate::wasm::float::F64BinaryOperator as FloatBinary;
use crate::words::{HOLE_A, HOLE_B, Out, S, slot};

/// The `CopySign` tag of `F64BinaryOperator` (`FLOAT_BINARY_NEEDS_CONSTANTS`'s only entry).
pub(crate) const COPYSIGN_TAG: u32 = 6;
const _: () = assert!(matches!(FloatBinary::from_tag(COPYSIGN_TAG), Some(FloatBinary::CopySign)));

/// `D = op A` for the conversion tag `OP` (`ScalarConversionOperator`), any tag: the staged
/// `staged::wasm_float::convert` over `ConstantFree`, so the NaN trap comes before the overflow
/// trap and both leave with `WasmTrap`'s compiled status, as the interpreter's rule does.
#[inline(always)]
pub(crate) unsafe fn convert_row<const OP: u32>(s: S) -> Out {
    let operator = const { conversion_operator(OP) };
    unsafe {
        let value = *(slot::<HOLE_A>(&s) as *const u64);
        match convert(&mut ConstantFree, operator, value) {
            Ok(word) => {
                *(slot::<0>(&s) as *mut u64) = word;
                Out::Next(s)
            }
            Err(error) => trap(s, error),
        }
    }
}

/// `D = copysign(A, B)` at width `W` (`W32` is f32): the staged `float_binary` rule over
/// `ConstantFree`, which returns CopySign's word unchanged (no NaN rule).
#[inline(always)]
pub(crate) unsafe fn copysign_row<const W: u8>(s: S) -> Out {
    unsafe {
        let (left, right) = (*(slot::<HOLE_A>(&s) as *const u64), *(slot::<HOLE_B>(&s) as *const u64));
        let word = float_binary(&mut ConstantFree, FloatBinary::CopySign, const { float_of(W) }, left, right);
        *(slot::<0>(&s) as *mut u64) = word;
    }
    Out::Next(s)
}
