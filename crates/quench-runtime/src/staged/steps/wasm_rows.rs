//! The WebAssembly rows of the staged meaning: int, float, state, vector and jump steps.

use super::*;

#[cfg(not(stencil_build))]
/// `step` of a Wasm integer row (`RowStep::WasmInt`) on the interpreter's stage and frame; any
/// other operation is unresolved.
#[inline(always)]
pub fn step_wasm_int<F: Frame<Concrete>>(stage: &mut Concrete, frame: &mut F, operation: Operation<u64, ()>) -> Step<u64, F::Control> {
    match operation {
        Operation::WasmInt(operation) => super::super::wasm_int::step(stage, frame, operation),
        _ => Step::Unresolved,
    }
}

#[cfg(not(stencil_build))]
/// `step` of a Wasm float row (`RowStep::WasmFloat`) on the interpreter's stage and frame; any
/// other operation is unresolved.
#[inline(always)]
pub fn step_wasm_float<F: Frame<Concrete>>(stage: &mut Concrete, frame: &mut F, operation: Operation<u64, ()>) -> Step<u64, F::Control> {
    match operation {
        Operation::WasmFloat(operation) => super::super::wasm_float::step(stage, frame, operation),
        _ => Step::Unresolved,
    }
}

#[cfg(not(stencil_build))]
/// `step` of an instance-state row (`RowStep::WasmState`) on the interpreter's stage and frame.
#[inline(always)]
pub fn step_wasm_state<F: Frame<Concrete>>(stage: &mut Concrete, frame: &mut F, operation: Operation<u64, ()>) -> Step<u64, F::Control> {
    match operation {
        Operation::WasmState(operation) => super::super::wasm_state::step(stage, frame, operation),
        _ => Step::Unresolved,
    }
}

#[cfg(not(stencil_build))]
/// `step` of a Wasm vector row (`RowStep::WasmVector`) on the interpreter's stage and frame; any
/// other operation is unresolved.
#[inline(always)]
pub fn step_wasm_vector<F: Frame<Concrete>>(stage: &mut Concrete, frame: &mut F, operation: Operation<u64, ()>) -> Step<u64, F::Control> {
    match operation {
        Operation::WasmVector(operation) => super::super::wasm_simd::step(stage, frame, operation),
        _ => Step::Unresolved,
    }
}

#[cfg(not(stencil_build))]
/// `step` of a Wasm i32 jump row (`RowStep::WasmJump`).
#[inline(always)]
pub fn step_wasm_jump_of<const KIND: u16, S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    match operation {
        Operation::WasmJump(jump) => if const { super::super::op_kind::admits(KIND, &[super::super::op_kind::WASM_JUMP]) } { super::super::wasm_jump::step(stage, frame, jump) } else { unreachable!() },
        Operation::BranchTable(table) => if const { super::super::op_kind::admits(KIND, &[super::super::op_kind::BRANCH_TABLE]) } { super::super::wasm_jump::branch_table(stage, frame, table) } else { unreachable!() },
        _ => if const { super::super::op_kind::outside(KIND, &[super::super::op_kind::WASM_JUMP, super::super::op_kind::BRANCH_TABLE]) } { Step::Unresolved } else { unreachable!() },
    }
}

/// `step_wasm_jump_of` for any operation.
#[cfg(not(stencil_build))]
#[inline(always)]
pub fn step_wasm_jump<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    step_wasm_jump_of::<{ super::super::op_kind::ANY }, S, F>(stage, frame, operation)
}
