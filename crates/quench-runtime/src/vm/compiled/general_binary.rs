//! Binary operations the numeric lowering leaves to the exact step: a string operand, or an
//! operator outside the shared arithmetic stage (`%`). `general_binary` answers them as the
//! step does (`Vm::number_binary`, then `Vm::binary`) for number and string operands, which
//! run no guest code; a remainder of two integers is computed inline first. Any other pair
//! takes the step.
use super::{emit_js_payload_word, emit_lane_helper_call, Host, JsExactFallback, JsStepDispatch, PcLowering, ResidualProgram, Vm};
use crate::bytecode::{Constant, Operand, OperandKind, WideInstruction};
use crate::value::ValueLayout;
use crate::Value;
use cranelift_codegen::ir::{self, condcodes::IntCC, types, InstBuilder, MemFlagsData};
use cranelift_frontend::FunctionBuilder;
use oxc_ast::ast::BinaryOperator;

/// Where `general_binary`'s word holds each operand's constant index plus one (zero for an
/// operand passed as a value) above the operator.
const LEFT_CONSTANT: u32 = u8::BITS;
const RIGHT_CONSTANT: u32 = LEFT_CONSTANT + u32::BITS - u8::BITS;
const OPERAND_FIELD: u64 = (1 << (RIGHT_CONSTANT - LEFT_CONSTANT)) - 1;

/// Lowers an instruction no other lowering took: a general one where it has one, else the
/// exact step alone.
pub(super) fn lower_rest(
    builder: &mut FunctionBuilder<'_>,
    program: &ResidualProgram,
    (instruction, next): (WideInstruction, Option<ir::Block>),
    register_offset: &dyn Fn(u16) -> Option<i32>,
    (arguments, payload): (ir::Value, ir::Value),
    exact: &JsExactFallback,
) -> PcLowering {
    if let Some(next) = next && instruction.op() == crate::bytecode::Op::Binary
        && lower_binary(builder, program, (instruction, next), register_offset, (arguments, payload), exact) {
        return PcLowering::Native;
    }
    if lower_logical_not(builder, (instruction, next), register_offset, arguments, exact) {
        return PcLowering::Native;
    }
    super::emit_js_step_fallback(builder, exact.context, exact.callout, exact.callout_signature, exact.result_pointer,
        exact.stopped_block, exact.continue_block, exact.pc);
    PcLowering::Exact
}

/// Lowers the `!` at the exact fallback's PC, which only negates its operand's ToBoolean and
/// so runs no guest code: a returning one (`return !x`) completes the frame with it. Any value
/// `emit_js_tagged_truthiness` does not decide takes the exact step.
fn lower_logical_not(
    builder: &mut FunctionBuilder<'_>,
    (instruction, next): (WideInstruction, Option<ir::Block>),
    register_offset: &dyn Fn(u16) -> Option<i32>,
    arguments: ir::Value,
    exact: &JsExactFallback,
) -> bool {
    let returns = instruction.returns_from_frame();
    if instruction.op().immediate_role() != crate::bytecode::ImmediateRole::UnaryOperator
        || instruction.unary_operator() != oxc_ast::ast::UnaryOperator::LogicalNot as u32
        || instruction.result_flags() != if returns { crate::bytecode::RETURN_REGISTER } else { 0 }
        || instruction.numeric_local_target().is_some() {
        return false;
    }
    let (Some(source), Some(destination)) = (register_offset(instruction.register_b()), register_offset(instruction.result_register()))
        else { return false };
    let next = match (returns, next) {
        (true, _) => exact.register_return(builder, arguments, destination),
        (false, Some(next)) => next,
        (false, None) => return false,
    };
    let fallback = builder.create_block();
    let bits = builder.ins().load(types::I64, MemFlagsData::trusted(), arguments, source);
    let truthy = super::emit_js_tagged_truthiness(builder, bits, fallback);
    let (truth, falsity) = (builder.ins().iconst(types::I64, Value::TRUE.to_bits() as i64),
        builder.ins().iconst(types::I64, Value::FALSE.to_bits() as i64));
    let negated = builder.ins().select(truthy, falsity, truth);
    builder.ins().store(MemFlagsData::trusted(), negated, arguments, destination);
    builder.ins().jump(next, &[]);
    exact.emit(builder, fallback);
    true
}

/// Lowers the plain `Binary` at the exact fallback's PC; false where its result has no home,
/// an operand is no register or constant, or the operator may throw for primitives (`in`,
/// `instanceof`).
fn lower_binary(
    builder: &mut FunctionBuilder<'_>,
    program: &ResidualProgram,
    (instruction, next): (WideInstruction, ir::Block),
    register_offset: &dyn Fn(u16) -> Option<i32>,
    (arguments, payload): (ir::Value, ir::Value),
    exact: &JsExactFallback,
) -> bool {
    let operator = instruction.binary_operator();
    if instruction.result_flags() != 0 || instruction.returns_from_frame() || instruction.writes_current_this()
        || instruction.numeric_local_target().is_some() || operator > BinaryOperator::BitwiseAnd as u32 {
        return false;
    }
    let Some(destination) = register_offset(instruction.result_register()) else { return false };
    let (Some(left), Some(right)) = (binary_operand(program, register_offset, instruction.operand_b()),
        binary_operand(program, register_offset, instruction.operand_c())) else { return false };
    let value = |builder: &mut FunctionBuilder<'_>, (offset, _): (Option<i32>, u64)| match offset {
        Some(offset) => builder.ins().load(types::I64, MemFlagsData::trusted(), arguments, offset),
        None => builder.ins().iconst(types::I64, 0) };
    let (left_value, right_value) = (value(builder, left), value(builder, right));
    let (store, fallback) = (builder.create_block(), builder.create_block());
    let result = builder.append_block_param(store, types::I64);
    if operator == BinaryOperator::Remainder as u32 && left.0.is_some() && right.0.is_some() {
        emit_integer_remainder(builder, (left_value, right_value), store);
    }
    let word = u64::from(operator) | (left.1 << LEFT_CONSTANT) | (right.1 << RIGHT_CONSTANT);
    let word = builder.ins().iconst(types::I64, word as i64);
    let helper = emit_js_payload_word(builder, payload, std::mem::offset_of!(JsStepDispatch, binary));
    let answered = emit_lane_helper_call(builder, payload, helper, &[payload, left_value, right_value, word]);
    let hit = builder.ins().icmp_imm_u(IntCC::NotEqual, answered, Value::DELETED.to_bits() as i64);
    builder.ins().brif(hit, store, &[ir::BlockArg::Value(answered)], fallback, &[]);
    builder.switch_to_block(store);
    builder.ins().store(MemFlagsData::trusted(), result, arguments, destination);
    builder.ins().jump(next, &[]);
    exact.emit(builder, fallback);
    true
}

/// An operand's register home, or its constant index plus one for a number or string
/// constant `general_binary` reads itself.
fn binary_operand(program: &ResidualProgram, register_offset: &dyn Fn(u16) -> Option<i32>, operand: Operand)
    -> Option<(Option<i32>, u64)> {
    match operand.kind()? {
        OperandKind::Register => Some((Some(register_offset(operand.register_index()?)?), 0)),
        OperandKind::Constant => matches!(program.constants.get(usize::from(operand.payload()))?,
            Constant::Number(_) | Constant::String(_) | Constant::StringUnits(_))
            .then(|| (None, u64::from(operand.payload()) + 1)),
        _ => None,
    }
}

/// The remainder of two integers with a nonzero divisor, on to `store`: an integer, or -0 for
/// a zero remainder of a negative dividend (`numeric_integer_result`). Any other pair falls
/// through to the general path.
fn emit_integer_remainder(builder: &mut FunctionBuilder<'_>, (left, right): (ir::Value, ir::Value), store: ir::Block) {
    let shift = i64::from(ValueLayout::TAG_SHIFT);
    let int_tag = (ValueLayout::INT_TAG >> shift) as i64;
    let left_tag = builder.ins().ushr_imm_u(left, shift);
    let right_tag = builder.ins().ushr_imm_u(right, shift);
    let left_integer = builder.ins().icmp_imm_u(IntCC::Equal, left_tag, int_tag);
    let right_integer = builder.ins().icmp_imm_u(IntCC::Equal, right_tag, int_tag);
    let (dividend, divisor) = (builder.ins().ireduce(types::I32, left), builder.ins().ireduce(types::I32, right));
    let nonzero = builder.ins().icmp_imm_s(IntCC::NotEqual, divisor, 0);
    // `i32::MIN % -1` is 0, which Cranelift's `srem` gives without trapping.
    let integers = builder.ins().band(left_integer, right_integer);
    let admitted = builder.ins().band(integers, nonzero);
    let (compute, general) = (builder.create_block(), builder.create_block());
    builder.ins().brif(admitted, compute, &[], general, &[]);
    builder.switch_to_block(compute);
    let remainder = builder.ins().srem(dividend, divisor);
    let payload = builder.ins().uextend(types::I64, remainder);
    let integer = builder.ins().bor_imm_u(payload, ValueLayout::INT_TAG as i64);
    let zero = builder.ins().icmp_imm_s(IntCC::Equal, remainder, 0);
    let negative = builder.ins().icmp_imm_s(IntCC::SignedLessThan, dividend, 0);
    let negative_zero = builder.ins().band(zero, negative);
    let minus_zero = builder.ins().iconst(types::I64, Value::number(-0.0).to_bits() as i64);
    let value = builder.ins().select(negative_zero, minus_zero, integer);
    builder.ins().jump(store, &[ir::BlockArg::Value(value)]);
    builder.switch_to_block(general);
}

/// The binary operation `word` names (its operator, then each operand's constant index plus
/// one) of number and string operands, as the exact step computes it; `Value::DELETED` for
/// any other operand or an operation that throws.
///
/// # Safety
/// As `general_user_call`.
pub(super) unsafe extern "C" fn general_binary<H: Host>(vm: u64, residual: u64, payload: u64, left: u64, right: u64,
    word: u64) -> u64 {
    // SAFETY: as the caller promises.
    let (vm, residual, dispatch) = unsafe {
        (&mut *(vm as *mut Vm<H>), &*(residual as *const ResidualProgram), &mut *(payload as *mut JsStepDispatch))
    };
    let operand = |vm: &Vm<H>, value: u64, constant: u64| match constant {
        0 => Some(Value::from_bits(value)),
        index => vm.active_constant((index - 1) as usize),
    };
    let operator = (word & u64::from(u8::MAX)) as u32;
    let (Some(left), Some(right)) = (operand(vm, left, (word >> LEFT_CONSTANT) & OPERAND_FIELD),
        operand(vm, right, (word >> RIGHT_CONSTANT) & OPERAND_FIELD)) else { return Value::DELETED.to_bits() };
    let primitive = |value: Value| value.as_number().is_some() || vm.heap.is_string(value);
    if !primitive(left) || !primitive(right) { return Value::DELETED.to_bits(); }
    let value = match vm.number_binary(operator, left, right) {
        Some(value) => value,
        None => match vm.binary(residual, operator, left, right) { Ok(value) => value, Err(_) => return Value::DELETED.to_bits() },
    };
    // A concatenation may grow the heap's records; it never collects.
    dispatch.refresh_object_bases(vm);
    value.to_bits()
}
