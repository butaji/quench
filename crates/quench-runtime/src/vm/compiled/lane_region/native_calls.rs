//! Calls of natives a body knows by identity: the callee register was defined by a field read
//! of a Stable global whose member was a native function when the body was compiled
//! (`GlobalRead::NativeMember`). The call guards that identity, a fact that may vary per
//! execution, and runs the native's operation inline; any other callee or operand takes the
//! call's helper.

use super::*;
use crate::vm::Native;
use crate::vm::global_facts::GlobalRead;

/// A Math function of Numbers a call runs inline: the machine operation the native's own
/// definition (`number::math_unary_operation`, `number::math_numbers`) applies.
#[derive(Clone, Copy)]
enum Inline {
    /// The wrapping product of two arguments' ToInt32.
    Imul,
    Floor, Ceil, Trunc, Abs, Sqrt,
    /// The nearest binary32, as a double.
    Fround,
    /// The larger or smaller of two Numbers: NaN if either is, and `-0` below `+0`.
    Max, Min,
}

/// The argument counts of the inline operations.
const UNARY: u16 = 1;
const BINARY: u16 = 2;

impl Inline {
    /// The operation `native` runs inline, if any.
    fn of(native: Native) -> Option<Self> {
        Some(match native {
            Native::MathImul => Self::Imul,
            Native::MathFloor => Self::Floor,
            Native::MathCeil => Self::Ceil,
            Native::MathTrunc => Self::Trunc,
            Native::MathAbs => Self::Abs,
            Native::MathSqrt => Self::Sqrt,
            Native::MathFround => Self::Fround,
            Native::MathMax => Self::Max,
            Native::MathMin => Self::Min,
            _ => return None,
        })
    }

    /// The arguments the inline operation reads; a call passing another count takes the native.
    const fn arguments(self) -> u16 {
        match self { Self::Imul | Self::Max | Self::Min => BINARY, _ => UNARY }
    }

    /// The tagged result of the operation on the tagged `arguments`, each a Number, else `miss`.
    fn emit(self, builder: &mut FunctionBuilder<'_>, arguments: &[ir::Value], miss: ir::Block) -> ir::Value {
        let numbers: Vec<ir::Value> = arguments.iter().map(|bits| super::emit_js_tagged_number(builder, *bits, miss)).collect();
        let number = match (self, numbers.as_slice()) {
            (Self::Imul, &[left, right]) => {
                let [left, right] = [left, right].map(|number| super::emit_number_to_int32(builder, number, miss));
                let product = builder.ins().imul(left, right);
                let product = builder.ins().uextend(types::I64, product);
                return builder.ins().bor_imm_u(product, crate::value::ValueLayout::INT_TAG as i64);
            }
            (Self::Max, &[left, right]) => builder.ins().fmax(left, right),
            (Self::Min, &[left, right]) => builder.ins().fmin(left, right),
            (Self::Floor, &[value]) => builder.ins().floor(value),
            (Self::Ceil, &[value]) => builder.ins().ceil(value),
            (Self::Trunc, &[value]) => builder.ins().trunc(value),
            (Self::Abs, &[value]) => builder.ins().fabs(value),
            (Self::Sqrt, &[value]) => builder.ins().sqrt(value),
            (Self::Fround, &[value]) => { let single = builder.ins().fdemote(types::F32, value); builder.ins().fpromote(types::F64, single) }
            _ => unreachable!("an inline operation reads its own argument count"),
        };
        super::emit_js_box_number(builder, number)
    }
}

impl Lowering<'_, '_> {
    /// The call at `pc`, before its helper at `generic`: a known Math function of Numbers
    /// (`Inline`) leaves its value in the call's register and continues at `completed`, as a
    /// completed call does. The caller wrote back its registers.
    pub(super) fn native_call(&mut self, pc: usize, instruction: WideInstruction, generic: ir::Block, completed: ir::Block) {
        let passed = crate::bytecode::ImmediateLayout::argument_count(instruction.imm());
        let Some((callee, inline)) = self.native_callee(pc, instruction)
            .filter(|(_, inline)| passed == inline.arguments() && !instruction.returns_from_frame()) else {
            self.ins().jump(generic, &[]);
            return;
        };
        let base = self.registers_base;
        let window = crate::bytecode::ImmediateLayout::call_window_base(instruction.imm());
        let held = self.ins().load(types::I64, MemFlagsData::trusted(), base, register_offset(instruction.register_b()));
        let same = self.ins().icmp_imm_u(IntCC::Equal, held, callee.to_bits() as i64);
        emit_js_guard(&mut self.builder, same, generic);
        let arguments: Vec<ir::Value> = (window..window + passed)
            .map(|register| self.ins().load(types::I64, MemFlagsData::trusted(), base, register_offset(register))).collect();
        let value = inline.emit(&mut self.builder, &arguments, generic);
        self.ins().store(MemFlagsData::trusted(), value, base, register_offset(instruction.result_register()));
        self.ins().jump(completed, &[]);
    }

    /// The function the callee register of the call at `pc` held when the body was compiled,
    /// and its inline operation, where the register's nearest definition is a native
    /// member's read of one.
    fn native_callee(&self, pc: usize, instruction: WideInstruction) -> Option<(Value, Inline)> {
        let callee = instruction.register_b();
        let defined = (0..pc).rev().find(|at| self.unit.instructions[*at].written_registers().any(|register| register == callee))?;
        let GlobalRead::NativeMember { value, native } = self.global_reads.get(&defined)? else { return None };
        Some((*value, Inline::of(*native)?))
    }
}
