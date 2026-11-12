//! Calls of natives a body knows by identity: the callee register was defined by a field read
//! of a Stable global whose member was a native function when the body was compiled
//! (`GlobalRead::NativeMember`). The call guards that identity, a fact that may vary per
//! execution, and runs the native's operation inline; any other callee or operand takes the
//! call's helper.
//!
//! The operation is SSA code on the call's operands: arguments read as unboxed Numbers (each
//! proved once, leaving at the call where one is not a Number), the result defined as a
//! Float64 (or Int32) register. Nothing is written back, boxed or reloaded: the operation runs
//! no guest code and allocates nothing.

use super::*;
use crate::vm::Native;
use crate::vm::global_facts::GlobalRead;

/// A Math function of Numbers a call runs inline: the machine operation the native's own
/// definition (`number::math_unary_operation`, `number::math_numbers`) applies.
#[derive(Clone, Copy)]
pub(super) enum Inline {
    /// The wrapping product of two arguments' ToInt32.
    Imul,
    Floor, Ceil, Trunc, Abs, Sqrt,
    /// The nearest binary32, as a double.
    Fround,
    /// The larger or smaller of two Numbers: NaN if either is, and `-0` below `+0`.
    Max, Min,
    /// The nearest integer, halves toward `+Infinity`, `-0` for a negative that rounds to zero.
    Round,
}

/// The fraction at which `Math.round` rounds up.
const ROUND_HALF: f64 = 0.5;
/// The argument counts of the inline operations.
const UNARY: u16 = 1;
const BINARY: u16 = 2;

impl Inline {
    /// The operation `native` runs inline, if any.
    fn of(native: Native) -> Option<Self> {
        const INLINE: [(Native, Inline); 10] = [
            (Native::MathImul, Inline::Imul), (Native::MathFloor, Inline::Floor), (Native::MathCeil, Inline::Ceil),
            (Native::MathTrunc, Inline::Trunc), (Native::MathAbs, Inline::Abs), (Native::MathSqrt, Inline::Sqrt),
            (Native::MathFround, Inline::Fround), (Native::MathMax, Inline::Max), (Native::MathMin, Inline::Min),
            (Native::MathRound, Inline::Round),
        ];
        INLINE.iter().find(|(known, _)| *known == native).map(|(_, inline)| *inline)
    }

    /// The arguments the inline operation reads; a call passing another count takes the native.
    const fn arguments(self) -> u16 {
        match self { Self::Imul | Self::Max | Self::Min => BINARY, _ => UNARY }
    }

    /// The operation on `numbers`: a Float64 value, or for `Imul` an Int32 one.
    fn emit(self, builder: &mut FunctionBuilder<'_>, numbers: &[ir::Value]) -> ir::Value {
        match (self, numbers) {
            (Self::Max, &[left, right]) => builder.ins().fmax(left, right),
            (Self::Min, &[left, right]) => builder.ins().fmin(left, right),
            (Self::Floor, &[value]) => builder.ins().floor(value),
            (Self::Ceil, &[value]) => builder.ins().ceil(value),
            (Self::Trunc, &[value]) => builder.ins().trunc(value),
            (Self::Abs, &[value]) => builder.ins().fabs(value),
            (Self::Sqrt, &[value]) => builder.ins().sqrt(value),
            (Self::Fround, &[value]) => { let single = builder.ins().fdemote(types::F32, value); builder.ins().fpromote(types::F64, single) }
            (Self::Round, &[value]) => {
                let floor = builder.ins().floor(value);
                let excess = builder.ins().fsub(value, floor);
                let half = builder.ins().f64const(ROUND_HALF);
                let up = builder.ins().fcmp(FloatCC::GreaterThanOrEqual, excess, half);
                let one = builder.ins().f64const(1.0);
                let zero = builder.ins().f64const(0.0);
                let step = builder.ins().select(up, one, zero);
                let rounded = builder.ins().fadd(floor, step);
                // A negative that rounds to zero is `-0`.
                let is_zero = builder.ins().fcmp(FloatCC::Equal, rounded, zero);
                let negative = builder.ins().fcmp(FloatCC::LessThan, value, zero);
                let negative_zero = builder.ins().band(is_zero, negative);
                let minus_zero = builder.ins().f64const(-0.0);
                builder.ins().select(negative_zero, minus_zero, rounded)
            }
            _ => unreachable!("an inline operation reads its own argument count"),
        }
    }
}

/// The inline Math operation of each call of a unit, by PC (`intrinsic_calls`).
pub(super) type Intrinsics = Vec<Option<(Value, Inline)>>;

/// The calls of `unit` that run inline, by PC: a call's callee register was defined by a field
/// read of a Stable global whose member was a native Math function of Numbers when the body was
/// compiled (`GlobalRead::NativeMember`), the call passes that function's argument count, and no
/// direct callee is bound to it. Each with the function it held, whose identity the call guards.
pub(super) fn intrinsic_calls(unit: &super::super::lane_unit::Unit, global_reads: &crate::vm::global_facts::GlobalReads,
    direct: impl Fn(usize) -> bool) -> Intrinsics {
    unit.instructions.iter().enumerate().map(|(pc, instruction)| Some((pc, instruction)).filter(|(pc, instruction)| instruction.op().class().eq(Class::Call)
        && !direct(*pc) && !instruction.returns_from_frame()
        && !instruction.direct_eval() && !instruction.parameter_eval() && instruction.result_flags() == 0)
        .and_then(|(pc, instruction)| {
            let callee = instruction.register_b();
            let defined = (0..pc).rev().find(|at| unit.instructions[*at].written_registers().any(|register| register == callee))?;
            let GlobalRead::NativeMember { value, native } = global_reads.get(&defined)? else { return None };
            let inline = Inline::of(*native)?;
            (crate::bytecode::ImmediateLayout::argument_count(instruction.imm()) == inline.arguments())
                .then_some((*value, inline))
        })).collect()
}

impl Lowering<'_, '_> {
    /// The call at `pc` where its callee is a known Math function of Numbers (`intrinsic_calls`),
    /// run as SSA code: guarded once on the callee's identity (the call's interpreter step runs
    /// where it differs) and on each argument being a Number. False where the call is not one.
    pub(super) fn intrinsic_call(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> bool {
        let Some(&Some((callee, inline))) = self.intrinsics.get(pc) else { return false };
        let passed = inline.arguments();
        let exit = self.guard_exit(state, pc as u32);
        let held = self.tagged(state, instruction.register_b());
        let same = self.is_identity(held, callee);
        emit_js_guard(&mut self.builder, same, exit);
        let window = crate::bytecode::ImmediateLayout::call_window_base(instruction.imm());
        let numbers: Vec<ir::Value> = (window..window + passed).map(|register| self.number(state, register, exit)).collect();
        let result = instruction.result_register();
        if let Inline::Imul = inline {
            let [left, right] = [numbers[0], numbers[1]].map(|number| self.number_to_int32(number, exit));
            let product = self.ins().imul(left, right);
            self.set_int(state, result, product);
        } else {
            let number = inline.emit(&mut self.builder, &numbers);
            self.set_number(state, result, number);
        }
        true
    }
}
