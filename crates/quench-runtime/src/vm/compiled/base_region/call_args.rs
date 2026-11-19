//! The arguments a call passes as Numbers, planned into its direct row (`CallRecord::proven_args`): the facts analysis
//! (`facts::analyze_text`) run over the enclosing function's bytecode from pc 0, every register starting unknown except the
//! declared Number parameters (`declared::entry_numbers`), which the entry guards hold as Numbers and the function never
//! writes. A call's argument is proven where its register holds a Number at the call on every path. The analysis is an
//! invariant of the bytecode, as `exact.rs` relies on: a region or resume stub enters only in states it covers.

use super::*;
use super::super::facts::{self, Observed, Seeds, Text};
use super::super::lane_region::{leaders, writes};
use super::call_plan::{CallForm, CallShape};
use super::link::INT32_BIT;
use crate::bytecode::{Constant, Function, Register, WideInstruction};
use crate::typed_ir::Type;

/// Per PC of `function`, the mask of its call's arguments proven Numbers (bit `k`: argument `k`) and int32s (bit
/// `INT32_BIT + k`, which implies bit `k`), zero where no call is
/// or no argument is proven; `None` where the analysis models less than the function does (`exact::exact_pcs`'s conditions).
pub(super) fn proven_arguments(function: &Function, constants: &[Constant], numbers: &[Register]) -> Option<Box<[u64]>> {
    if !function.handlers.is_empty() || function.is_generator || function.is_async || function.has_dynamic_scope_access() || function.arguments_slot.is_some() {
        return None;
    }
    let instructions: Vec<WideInstruction> = function.instructions().collect();
    let blocks = leaders(&instructions);
    let mut initial = vec![Type::ANY; usize::from(function.registers)];
    for &register in numbers {
        initial[usize::from(register)] = Type::NUMBER;
    }
    let text = Text { entry: 0, instructions: &instructions, leaders: &blocks, constants, written: &writes,
        own_shape: &|_| None, native_calls: true, raises: &[] };
    let seeds = Seeds { contracts: &[], observed: Observed::NONE, entering: &[], model: None, signless: &[] };
    let mut masks = vec![0u64; instructions.len()];
    facts::analyze_text(&text, &initial, seeds).replay(&text, seeds, |pc, point| {
        let instruction = instructions[pc];
        if CallForm::of(instruction) != Some(CallForm::Plain) {
            return;
        }
        let shape = CallShape::of(pc as u32, instruction, CallForm::Plain);
        masks[pc] = (0..u32::from(shape.argc).min(INT32_BIT)).fold(0u64, |mask, k| {
            let register = shape.window + k as Register;
            let number = if point.holds_number(register) { 1u64 << k } else { 0 };
            let int32 = if point.holds_int32(register) { 1u64 << (INT32_BIT + k) } else { 0 };
            mask | number | int32
        });
    });
    Some(masks.into_boxed_slice())
}
