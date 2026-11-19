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
use crate::vm::number::{MathFunction, math_function};
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
const UNARY_ARGUMENTS: u16 = 1;
const BINARY_ARGUMENTS: u16 = 2;

impl Inline {
    /// The operation `native` runs inline, if any: the machine rows and the Number-only Math
    /// functions of `number::math_function` that native code lowers, else a builtin's row.
    fn of(native: Native) -> Option<Self> {
        match math_function(native) {
            Some(MathFunction::Row(operation)) => Some(Self::Unary(operation)),
            Some(MathFunction::Extreme(maximum)) => Some(Self::Extreme(maximum)),
            Some(MathFunction::Imul) => Some(Self::Imul),
            _ => None,
        }
        .or_else(|| Builtin::of(native).map(Self::Builtin))
    }

    /// The arguments the inline operation reads; a call passing another count takes the native.
    const fn arguments(self) -> u16 {
        match self { Self::Imul | Self::Extreme(_) => BINARY_ARGUMENTS, Self::Unary(_) => UNARY_ARGUMENTS, Self::Builtin(builtin) => builtin.arguments() }
    }

    /// Whether the operation may change an array's elements, which a loop's hoisted views
    /// and the Stable facts its code folded do not survive.
    const fn mutates_arrays(self) -> bool {
        matches!(self, Self::Builtin(builtin) if builtin.mutates_arrays())
    }

    /// The operation on `numbers`: a Float64 value; `Imul` is lowered by the caller.
    fn emit(self, builder: &mut FunctionBuilder<'_>, numbers: &[ir::Value]) -> ir::Value {
        let mut stage = super::super::object_slots::emit(builder);
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

/// Whether a lane body runs a call of `native` inline and the call changes no array: a loop
/// whose calls are all such runs natively in a body (`loop_body::offer_body`).
pub(in crate::vm) fn inlines_keeping_arrays(native: Native) -> bool {
    Inline::of(native).is_some_and(|inline| !inline.mutates_arrays())
}

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
    unit.instructions.iter().enumerate().map(|(pc, instruction)| Some((pc, instruction)).filter(|(pc, instruction)| instruction.op().is_call()
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
        if !self.holds_holders() { return false; }
        let exit = self.guard_exit(state, pc as u32);
        self.hold_holder_any(state, exit, holder);
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
            let mut stage = super::super::object_slots::emit(&mut self.builder);
            let product = crate::staged::math::multiply(&mut stage, left, right);
            self.set_int(state, result, product);
        } else {
            let number = inline.emit(&mut self.builder, &numbers);
            self.set_number(state, result, number);
        }
    }
}
