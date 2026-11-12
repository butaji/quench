//! Calls of natives a body knows by identity: the callee register was defined by a field read
//! of a Stable global whose member was a native function when the body was compiled
//! (`GlobalRead::NativeMember`). The call guards that identity, a fact that may vary per
//! execution, and runs the native's operation inline; any other callee or operand takes the
//! call's helper.
//!
//! A method of a string or an array is known the same way: the callee register was defined by
//! a field read the VM resolved to the realm's native method (`Vm::method_native`), and the call
//! guards the callee's identity at run time, as the receiver varies per execution.
//!
//! A Math operation is SSA code on the call's operands: arguments read as unboxed Numbers (each
//! proved once, leaving at the call where one is not a Number), the result defined as a
//! Float64 (or Int32) register. Nothing is written back, boxed or reloaded: the operation runs
//! no guest code and allocates nothing. A builtin of strings, arrays and `Number.isInteger`
//! (`builtin_calls`) runs its staged row, its result a tagged Boolean, Number or string, or an
//! int32.

use super::*;
use crate::staged::math::FloatUnary;
use crate::vm::Native;
use crate::vm::global_facts::GlobalRead;
use super::builtin_calls::Builtin;

/// A native a call runs inline: a Math function of Numbers, the wrapping product of two
/// arguments' ToInt32 (`Math.imul`), or the machine operation the native's own definition
/// applies (`staged::math`); or a builtin on its staged row (`Builtin`).
#[derive(Clone, Copy)]
pub(super) enum Inline {
    Imul,
    Unary(FloatUnary),
    /// `Math.max` (true) or `Math.min` of two Numbers.
    Extreme(bool),
    Builtin(Builtin),
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
            .or_else(|| Builtin::of(native).map(Self::Builtin))
    }

    /// The arguments the inline operation reads; a call passing another count takes the native.
    const fn arguments(self) -> u16 {
        match self { Self::Imul | Self::Extreme(_) => BINARY, Self::Unary(_) => UNARY, Self::Builtin(builtin) => builtin.arguments() }
    }

    /// Whether the operation may change an array's elements, which a loop's hoisted views
    /// and the Stable facts its code folded do not survive.
    const fn mutates_arrays(self) -> bool {
        matches!(self, Self::Builtin(builtin) if builtin.mutates_arrays())
    }

    /// The operation on `numbers`: a Float64 value; `Imul` is lowered by the caller.
    fn emit(self, builder: &mut FunctionBuilder<'_>, numbers: &[ir::Value]) -> ir::Value {
        let mut stage = super::super::object_slots::Emit { builder, property_values: None };
        match (self, numbers) {
            (Self::Unary(operation), &[value]) => crate::staged::math::unary(&mut stage, operation, value),
            (Self::Extreme(maximum), &[left, right]) => crate::staged::math::extreme(&mut stage, maximum, left, right),
            _ => unreachable!("an inline Math operation reads its own argument count"),
        }
    }
}

/// The inline operation of each call of a unit, by PC (`intrinsic_calls`).
/// Each with the function it held and the exact shape of the prototype holding it.
pub(super) type Intrinsics = Vec<Option<(Value, Inline, u32)>>;

/// Whether the call at `pc` runs inline and changes no array, so a loop holds it natively and
/// the Stable facts its code folded survive it.
pub(super) fn keeps_arrays(intrinsics: &Intrinsics, pc: usize) -> bool {
    intrinsics.get(pc).copied().flatten().is_some_and(|(_, inline, _)| !inline.mutates_arrays())
}

/// The calls of `unit` that run inline, by PC: a call's callee register was defined by a field
/// read that was a native function when the body was compiled (`GlobalRead::NativeMember`: a
/// member of a Stable global, or a method of the string or array prototype), the call passes
/// that function's argument count, and no direct callee is bound to it. Each with the function it held, whose identity the call guards.
pub(super) fn intrinsic_calls(unit: &super::super::lane_unit::Unit, global_reads: &crate::vm::global_facts::GlobalReads,
    direct: impl Fn(usize) -> bool) -> Intrinsics {
    unit.instructions.iter().enumerate().map(|(pc, instruction)| Some((pc, instruction)).filter(|(pc, instruction)| instruction.op().class().eq(Class::Call)
        && !direct(*pc) && !instruction.returns_from_frame()
        && !instruction.direct_eval() && !instruction.parameter_eval() && instruction.result_flags() == 0)
        .and_then(|(pc, instruction)| {
            let callee = instruction.register_b();
            let defined = (0..pc).rev().find(|at| unit.instructions[*at].written_registers().any(|register| register == callee))?;
            let GlobalRead::NativeMember { value, native, holder } = global_reads.get(&defined)? else { return None };
            let inline = Inline::of(*native)?;
            (crate::bytecode::ImmediateLayout::argument_count(instruction.imm()) == inline.arguments())
                .then_some((*value, inline, *holder))
        })).collect()
}

impl Lowering<'_, '_> {
    /// The call at `pc` where its callee is a known native that runs inline (`intrinsic_calls`),
    /// run as SSA code: guarded once on the callee's identity (the call's interpreter step runs
    /// where it differs) and on each operand its operation reads (a Number, a flat string, a
    /// dense array). False where the call is not one.
    pub(super) fn intrinsic_call(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> bool {
        let Some(&Some((callee, inline, holder))) = self.intrinsics.get(pc) else { return false };
        // The callee's identity is held by its holder, a dependency that retires this code
        // before the function can die and its id be reused.
        if !self.division().folds(BindingTime::Stable) { return false; }
        let exit = self.guard_exit(state, pc as u32);
        self.hold_holder(state, exit, holder);
        let held = self.tagged(state, instruction.register_b());
        let same = self.is_identity(held, callee);
        emit_js_guard(&mut self.builder, same, exit);
        match inline {
            Inline::Builtin(builtin) => self.builtin_call(state, instruction, builtin, exit),
            _ => self.math_call(state, instruction, inline, exit),
        }
        true
    }

    /// The Math call `inline` of Numbers, its result an unboxed Number (an int32 for `imul`).
    fn math_call(&mut self, state: &mut State, instruction: WideInstruction, inline: Inline, exit: ir::Block) {
        let window = crate::bytecode::ImmediateLayout::call_window_base(instruction.imm());
        let numbers: Vec<ir::Value> = (window..window + inline.arguments()).map(|register| self.number(state, register, exit)).collect();
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
    }
}
