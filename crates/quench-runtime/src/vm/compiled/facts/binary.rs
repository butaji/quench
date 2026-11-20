//! The facts a binary operator gives: a string or BigInt operand takes the interpreter's own
//! definition, any other proves its operands Numbers or leaves.

use super::{ranges, Flow, boolean_binary, constant_number, operand_type, prove_number, set};
use crate::bytecode::{Constant, WideInstruction};
use crate::typed_ir::Type;
use super::super::super::operations::{NumberArithmetic, NumberBitwise, NumberRelation};

/// A binary operator one operand of which is a string or BigInt, which runs by the
/// interpreter's definition (`lane_region::binary_by_definition`), and its result's type.
pub(in crate::vm) fn heap_primitive_binary(operator: u32, left: Type, right: Type) -> Option<Type> {
    let is = |ty: Type, kind| ty != Type::NEVER && ty.is_subtype(Type::language(kind));
    let heap = |ty| is(ty, crate::ValueType::String) || is(ty, crate::ValueType::BigInt);
    (heap(left) || heap(right)).then(|| primitive_result(operator, left, right))
}

/// `+` with a string makes a string, arithmetic on two BigInts a BigInt (or throws), a
/// relation a Boolean; anything else may be any value.
fn primitive_result(operator: u32, left: Type, right: Type) -> Type {
    let [string, bigint] = [crate::ValueType::String, crate::ValueType::BigInt].map(Type::language);
    if operator == oxc_ast::ast::BinaryOperator::Addition as u32 && (left.is_subtype(string) || right.is_subtype(string)) { return string; }
    if bigint_operator(operator) && left.is_subtype(bigint) && right.is_subtype(bigint) { return bigint; }
    let relation = boolean_binary(operator) || NumberRelation::from_operator(operator).is_some();
    if relation { Type::language(crate::ValueType::Boolean) } else { Type::ANY }
}

/// Whether two BigInts give a BigInt by `operator`: arithmetic and bitwise ones but `>>>`.
fn bigint_operator(operator: u32) -> bool {
    (NumberArithmetic::from_operator(operator).is_some() || NumberBitwise::from_operator(operator).is_some())
        && operator != oxc_ast::ast::BinaryOperator::ShiftRightZeroFill as u32
}

/// Whether `instruction` takes its operator as data (`Binary`), so any operand type, rather
/// than one fixed Number operator (`NumericAdd`).
pub(in crate::vm) fn generic_operator(instruction: WideInstruction) -> bool {
    instruction.op().immediate_role() == crate::bytecode::ImmediateRole::BinaryOperator
}

/// Whether `instruction` may run by the interpreter's definition once a string or BigInt operand
/// takes it (`heap_primitive_binary`): a generic operator, or the `+` row (`NumericAdd`), whose
/// operands a string makes a concatenation of. Its Number form proves Number operands, which a
/// string operand must not.
pub(in crate::vm) fn heap_operator(instruction: WideInstruction) -> bool {
    generic_operator(instruction) || instruction.op().immediate_role() == crate::bytecode::ImmediateRole::AdditionOperator
}

/// `transfer` of a numeric binary operator.
pub(super) fn numeric_binary(types: &mut [Type], instruction: WideInstruction, constants: &[Constant],
    written: &dyn Fn(WideInstruction) -> super::Writes) -> Flow {
    let operand = |operand| operand_type(constants, operand, |register| types.get(usize::from(register)).copied().unwrap_or(Type::ANY));
    let heap = heap_primitive_binary(instruction.binary_operator(), operand(instruction.operand_b()), operand(instruction.operand_c()))
        .filter(|_| heap_operator(instruction) && !instruction.returns_from_frame());
    let result = heap.or_else(|| number_result(types, instruction, constants, written));
    if instruction.returns_from_frame() { return Flow::Leaves; }
    if let Some(result) = result { set(types, instruction.result_register(), result); }
    Flow::Next
}

/// The result type of a numeric operator on Numbers, whose operands it proves Numbers
/// (`prove_number`) where the operator leaves for any other.
fn number_result(types: &mut [Type], instruction: WideInstruction, constants: &[Constant],
    written: &dyn Fn(WideInstruction) -> super::Writes) -> Option<Type> {
    let (op, operator) = (instruction.op(), instruction.binary_operator());
    let operands = [instruction.operand_b(), instruction.operand_c()];
    if let Some(bitwise) = NumberBitwise::from_js_instruction(op, operator) {
        // ToInt32 of any Number runs natively; anything else leaves.
        prove_number(types, operands);
        let range = ranges::bitwise(&ranges::lookup(types, constants), bitwise, operands);
        return Some(if bitwise.result_is_int32(constant_number(constants, operands[1])) { Type::int(range) } else { Type::NUMBER });
    }
    if NumberArithmetic::from_js_instruction(op, operator).is_some() {
        prove_number(types, operands);
        return Some(ranges::arithmetic_type(&ranges::lookup(types, constants), instruction));
    }
    if NumberRelation::from_operator(operator).is_some() {
        // An equality decides operands of any type (`lane_region::relation`) and proves
        // nothing of them; so does a relation with an operand known to hold no Number (a
        // string constant), which the lane decides by the interpreter's rule. Every other
        // relation compares Numbers.
        let numberless = operands.iter().filter_map(|operand| operand.register_index())
            .any(|register| super::numberless(types.get(usize::from(register)).copied().unwrap_or(Type::ANY)));
        if super::super::super::operations::equality_operator(operator).is_none() && !numberless { prove_number(types, operands); }
        return Some(Type::language(crate::ValueType::Boolean));
    }
    for register in written(instruction) { set(types, register, Type::ANY); }
    boolean_binary(operator).then(|| Type::language(crate::ValueType::Boolean))
}
