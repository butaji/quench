//! Which instructions of a callee the unit inlines, and which callees may restart at a miss.

use super::*;
use crate::bytecode::Traits;

/// Whether the lane-body lowering runs `instruction` of an inlined callee: a capture of its
/// own environment only, no fusion but a returning one, which splits into the
/// computation and the return (`osr_loop::inlined_returning`). A call is the plan's to admit:
/// only one it inlines in turn.
pub(super) fn inlinable(instruction: WideInstruction) -> bool {
    let plain = instruction.result_flags() == 0 || super::super::osr_loop::inlined_returning(instruction);
    inlinable_pure(instruction, plain).unwrap_or_else(|| match instruction.op() {
        // Operations native code leaves at, exactly, through the inlined frame's recovery: a
        // global name's read, a construction and a throw, typically a callee's error path.
        opcode if opcode.is_load_name() || opcode.is_construct() || opcode.is_throw() => instruction.result_flags() == 0,
        opcode if opcode.is_call() => returns_or_keeps(instruction) && !instruction.direct_eval() && !instruction.parameter_eval(),
        _ => plain && instruction.op().js_denotation() == crate::bytecode::JsDenotation::NumericBinary,
    })
}

/// Whether `instruction` is inlinable where it is a scalar, element, `this`, capture, field or unary
/// operation, given whether it is `plain`; `None` for the other instructions.
fn inlinable_pure(instruction: WideInstruction, plain: bool) -> Option<bool> {
    Some(match instruction.op() {
        _ if instruction.op().traits().has(Traits::SCALAR_REGION.union(Traits::ELEMENT_ACCESS).union(Traits::THIS_LEAF)) => plain,
        // The callee's own captures, which its frame reads from its environment
        // (`Lowering::frame_captures`).
        opcode if opcode.is_load_capture() => plain && ImmediateLayout::capture_depth(instruction.imm()) == 0,
        // A nested lookup's chain names its base by the function's registers; the lowering
        // renames it through the frame (`Lowering::get_field`), and its sink stores to `this`.
        opcode if opcode.is_get_field() => match instruction.field_lookup() {
            Some(FieldLookup::Atom { .. }) => plain,
            Some(FieldLookup::Site(_)) => plain || instruction.result_flags() == crate::bytecode::SET_THIS_REGISTER,
            None => false,
        },
        _ if instruction.op().is_field_store() => plain,
        // A fused `typeof` test (`Lowering::typeof_test`), a `typeof` switch's index
        // (`Lowering::typeof_index`); any other unary leaves where the lowering has none.
        opcode if opcode.is_unary() => plain,
        _ => return None,
    })
}

/// The shared lowering performs only guarded, non-guest operations for an inlinable pure
/// leaf. A miss may restart that call without replaying effects.
pub(super) fn restartable(program: &crate::ResidualProgram, function: &Function, code: &[WideInstruction]) -> bool {
    if super::super::lane_region::restartable_numeric_function(program, function, code) { return true; }
    code.iter().enumerate().all(|(pc, instruction)| match instruction.op() {
        opcode if opcode.is_set_index() || opcode.is_call() => false,
        _ if instruction.op().is_field_store() => false,
        opcode if opcode.is_inc_dec() => instruction.numeric_local_target().is_none(),
        opcode if opcode.is_jump() || opcode.is_jump_false() || opcode.is_jump_true() || opcode.is_jump_binary_false() => instruction.jump_target() as usize > pc,
        _ => true, // `inlinable` owns the supported instruction/effect set.
    })
}

/// Whether a call keeps its result in its register or returns it: the two a unit inlines.
pub(super) fn returns_or_keeps(instruction: WideInstruction) -> bool {
    matches!(instruction.result_flags(), 0 | crate::bytecode::RETURN_REGISTER)
}
