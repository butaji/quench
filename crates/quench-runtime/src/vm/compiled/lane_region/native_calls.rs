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
use crate::staged::math::FloatUnary;
use crate::vm::Native;
use crate::vm::global_facts::GlobalRead;

/// A Math function of Numbers a call runs inline: the wrapping product of two arguments'
/// ToInt32 (`Math.imul`), or the machine operation the native's own definition applies
/// (`staged::math`).
#[derive(Clone, Copy)]
pub(super) enum Inline {
    Imul,
    Unary(FloatUnary),
    /// `Math.max` (true) or `Math.min` of two Numbers.
    Extreme(bool),
}

/// The argument counts of the inline operations.
const UNARY: u16 = 1;
const BINARY: u16 = 2;

/// `operation` of the binary64 value, as machine code (`staged::math::float_unary`).
pub(in crate::vm::compiled) fn emit_float_unary(builder: &mut FunctionBuilder<'_>, operation: FloatUnary, value: ir::Value) -> ir::Value {
    let mut ins = |build: &dyn for<'a, 'b> Fn(cranelift_frontend::FuncInstBuilder<'a, 'b>) -> ir::Value| build(builder.ins());
    match operation {
        FloatUnary::Floor => ins(&|i| i.floor(value)),
        FloatUnary::Ceil => ins(&|i| i.ceil(value)),
        FloatUnary::Trunc => ins(&|i| i.trunc(value)),
        FloatUnary::Abs => ins(&|i| i.fabs(value)),
        FloatUnary::Sqrt => ins(&|i| i.sqrt(value)),
        FloatUnary::Fround => { let single = ins(&|i| i.fdemote(types::F32, value)); ins(&|i| i.fpromote(types::F64, single)) }
        FloatUnary::Round => {
            let floor = ins(&|i| i.floor(value));
            let excess = ins(&|i| i.fsub(value, floor));
            let half = ins(&|i| i.f64const(crate::staged::math::ROUND_HALF));
            let up = ins(&|i| i.fcmp(FloatCC::GreaterThanOrEqual, excess, half));
            let one = ins(&|i| i.f64const(1.0));
            let zero = ins(&|i| i.f64const(0.0));
            let step = ins(&|i| i.select(up, one, zero));
            let rounded = ins(&|i| i.fadd(floor, step));
            // A negative that rounds to zero is `-0`.
            let is_zero = ins(&|i| i.fcmp(FloatCC::Equal, rounded, zero));
            let negative = ins(&|i| i.fcmp(FloatCC::LessThan, value, zero));
            let negative_zero = ins(&|i| i.band(is_zero, negative));
            let minus_zero = ins(&|i| i.f64const(-0.0));
            ins(&|i| i.select(negative_zero, minus_zero, rounded))
        }
    }
}

/// The larger (`maximum`) or smaller of two binary64 values (`staged::math::float_extreme`).
pub(in crate::vm::compiled) fn emit_float_extreme(builder: &mut FunctionBuilder<'_>, maximum: bool, left: ir::Value, right: ir::Value) -> ir::Value {
    let ins = builder.ins();
    if maximum { ins.fmax(left, right) } else { ins.fmin(left, right) }
}

/// The wrapping product of two int32 values (`staged::math::int_multiply`).
pub(in crate::vm::compiled) fn emit_int_multiply(builder: &mut FunctionBuilder<'_>, left: ir::Value, right: ir::Value) -> ir::Value {
    builder.ins().imul(left, right)
}

impl Inline {
    /// The operation `native` runs inline, if any.
    fn of(native: Native) -> Option<Self> {
        const INLINE: [(Native, Inline); 10] = [
            (Native::MathImul, Inline::Imul), (Native::MathFloor, Inline::Unary(FloatUnary::Floor)),
            (Native::MathCeil, Inline::Unary(FloatUnary::Ceil)), (Native::MathTrunc, Inline::Unary(FloatUnary::Trunc)),
            (Native::MathAbs, Inline::Unary(FloatUnary::Abs)), (Native::MathSqrt, Inline::Unary(FloatUnary::Sqrt)),
            (Native::MathFround, Inline::Unary(FloatUnary::Fround)), (Native::MathMax, Inline::Extreme(true)),
            (Native::MathMin, Inline::Extreme(false)), (Native::MathRound, Inline::Unary(FloatUnary::Round)),
        ];
        INLINE.iter().find(|(known, _)| *known == native).map(|(_, inline)| *inline)
    }

    /// The arguments the inline operation reads; a call passing another count takes the native.
    const fn arguments(self) -> u16 {
        match self { Self::Imul | Self::Extreme(_) => BINARY, Self::Unary(_) => UNARY }
    }

    /// The operation on `numbers`: a Float64 value; `Imul` is lowered by the caller.
    fn emit(self, builder: &mut FunctionBuilder<'_>, numbers: &[ir::Value]) -> ir::Value {
        let mut stage = super::super::object_slots::Emit { builder, property_values: None };
        match (self, numbers) {
            (Self::Unary(operation), &[value]) => crate::staged::math::unary(&mut stage, operation, value),
            (Self::Extreme(maximum), &[left, right]) => crate::staged::math::extreme(&mut stage, maximum, left, right),
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
        let same = self.ins().icmp_imm_u(IntCC::Equal, held, callee.to_bits() as i64);
        emit_js_guard(&mut self.builder, same, exit);
        let window = crate::bytecode::ImmediateLayout::call_window_base(instruction.imm());
        let numbers: Vec<ir::Value> = (window..window + passed).map(|register| self.number(state, register, exit)).collect();
        let result = instruction.result_register();
        if let Inline::Imul = inline {
            let [left, right] = [numbers[0], numbers[1]].map(|number| self.number_to_int32(number, exit));
            let mut stage = super::super::object_slots::Emit { builder: &mut self.builder, property_values: None };
            let product = crate::staged::math::multiply(&mut stage, left, right);
            self.set_int(state, result, product);
        } else {
            let number = inline.emit(&mut self.builder, &numbers);
            self.set_number(state, result, number);
        }
        true
    }
}
