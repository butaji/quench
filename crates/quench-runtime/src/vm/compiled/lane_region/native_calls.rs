//! Calls of natives a body knows by identity: the callee register was defined by a field read
//! of a Stable global whose member was a native function when the body was compiled
//! (`GlobalRead::NativeMember`). The call guards that identity, a fact that may vary per
//! execution, and runs the native's operation inline; any other callee or operand takes the
//! call's helper.

use super::*;
use crate::vm::global_facts::GlobalRead;

/// `Math.imul` multiplies its first two arguments.
const IMUL_ARGUMENTS: u16 = 2;

impl Lowering<'_, '_> {
    /// The call at `pc`, before its helper at `generic`: `Math.imul` of two Numbers is the
    /// wrapping product of their ToInt32 in the call's register, continuing at `completed` as a completed
    /// call does. The caller wrote back its registers.
    pub(super) fn native_call(&mut self, pc: usize, instruction: WideInstruction, generic: ir::Block, completed: ir::Block) {
        let passed = crate::bytecode::ImmediateLayout::argument_count(instruction.imm());
        let Some(callee) = self.native_callee(pc, instruction).filter(|_| passed == IMUL_ARGUMENTS && !instruction.returns_from_frame()) else {
            self.ins().jump(generic, &[]);
            return;
        };
        let base = self.registers_base;
        let window = crate::bytecode::ImmediateLayout::call_window_base(instruction.imm());
        let load = |lowering: &mut Self, register: Register|
            lowering.ins().load(types::I64, MemFlagsData::trusted(), base, register_offset(register));
        let (held, left, right) = (load(self, instruction.register_b()), load(self, window), load(self, window + 1));
        let same = self.ins().icmp_imm_u(IntCC::Equal, held, callee.to_bits() as i64);
        emit_js_guard(&mut self.builder, same, generic);
        // ToInt32 of each Number argument; any other value takes the native's own coercion.
        let [left, right] = [left, right].map(|bits| {
            let number = super::emit_js_tagged_number(&mut self.builder, bits, generic);
            super::emit_number_to_int32(&mut self.builder, number, generic)
        });
        let product = self.ins().imul(left, right);
        let product = self.ins().uextend(types::I64, product);
        let tagged = self.ins().bor_imm_u(product, crate::value::ValueLayout::INT_TAG as i64);
        self.ins().store(MemFlagsData::trusted(), tagged, base, register_offset(instruction.result_register()));
        self.ins().jump(completed, &[]);
    }

    /// The function the callee register of the call at `pc` held when the body was compiled,
    /// where its nearest definition is a native member's read and the native is one this
    /// lowering runs inline.
    fn native_callee(&self, pc: usize, instruction: WideInstruction) -> Option<Value> {
        let callee = instruction.register_b();
        let defined = (0..pc).rev().find(|at| self.unit.instructions[*at].written_registers().any(|register| register == callee))?;
        match self.global_reads.get(&defined)? {
            GlobalRead::NativeMember { value, native: crate::vm::Native::MathImul } => Some(*value),
            _ => None,
        }
    }
}
