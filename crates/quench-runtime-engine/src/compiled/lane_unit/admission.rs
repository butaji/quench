//! Which instructions of a callee the unit inlines, and which callees may restart at a miss.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use quench_runtime::bytecode::Traits;

/// The tier that asks whether an inlined callee's instruction may be inlined. The two answer
/// differently, by design: a loop region takes an instruction by its opcode's `INLINABLE` trait
/// alone (its own admission checks the operands), while the lane body also reads the instruction's
/// result flags and the decoded row.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum InlineScope {
    /// An OSR loop region's inlined callee (`osr_loop::inlining`).
    Loop,
    /// A lane body's inlined callee (`lane_unit::Planner::inline_body`).
    Lane,
}

/// Whether `instruction` of an inlined callee may be inlined by `scope`. A loop region takes the
/// opcode's `INLINABLE` trait; the lane runs `instruction` where it is a capture of its own
/// environment only, no fusion but a returning one, which splits into the computation and the
/// return (`osr_loop::inlined_returning`). A lane call is the plan's to admit: only one it inlines
/// in turn.
pub(crate) fn inlinable(instruction: WideInstruction, scope: InlineScope) -> bool {
    match scope {
        InlineScope::Loop => instruction.op().traits().has(Traits::INLINABLE),
        InlineScope::Lane => lane_inlinable(instruction),
    }
}

/// Whether the lane body runs `instruction` of an inlined callee (`inlinable`, `InlineScope::Lane`).
fn lane_inlinable(instruction: WideInstruction) -> bool {
    let plain = instruction.result_flags() == 0 || super::super::osr_loop::inlined_returning(instruction);
    inlinable_pure(instruction, plain).unwrap_or_else(|| match () {
        // Operations native code leaves at, exactly, through the inlined frame's recovery: a
        // global name's read, a construction and a throw, typically a callee's error path.
        _ if quench_runtime::staged::Operation::<(), ()>::decode_load_name(instruction, ()).is_some() || throws(instruction) => instruction.result_flags() == 0,
        _ if super::super::meaning_compiled::is_construct(instruction) => instruction.result_flags() == 0,
        _ if super::super::meaning_compiled::is_call(instruction) => returns_or_keeps(instruction) && !instruction.direct_eval() && !instruction.parameter_eval(),
        _ => plain && super::super::meaning_compiled::is_numeric_binary(instruction),
    })
}

/// Whether `instruction` is inlinable where it is a scalar, element, `this`, capture, field or unary
/// operation, given whether it is `plain`; `None` for the other instructions.
fn inlinable_pure(instruction: WideInstruction, plain: bool) -> Option<bool> {
    Some(match () {
        _ if instruction.op().traits().has(Traits::SCALAR_REGION.union(Traits::ELEMENT_ACCESS).union(Traits::THIS_LEAF)) => plain,
        // The callee's own captures, which its frame reads from its environment
        // (`Lowering::frame_captures`).
        _ if matches!(crate::compiled::decoded_completed(instruction), Some(quench_runtime::staged::Operation::LoadCapture { .. })) =>
            plain && ImmediateLayout::capture_depth(instruction.imm()) == 0,
        // A nested lookup's chain names its base by the function's registers; the lowering
        // renames it through the frame (`Lowering::get_field`), and its sink stores to `this`.
        _ if matches!(crate::compiled::decoded_completed(instruction), Some(quench_runtime::staged::Operation::GetField { .. })) => match instruction.field_lookup() {
            Some(FieldLookup::Atom { .. }) => plain,
            Some(FieldLookup::Site(_)) => plain || instruction.result_flags() == quench_runtime::bytecode::SET_THIS_REGISTER,
            None => false,
        },
        _ if matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::SetField { .. })) => plain,
        // A fused `typeof` test (`Lowering::typeof_test`), a `typeof` switch's index
        // (`Lowering::typeof_index`); any other unary leaves where the lowering has none.
        _ if super::super::meaning_compiled::is_unary(instruction) => plain,
        _ => return None,
    })
}

/// The shared lowering performs only guarded, non-guest operations for an inlinable pure
/// leaf. A miss may restart that call without replaying effects.
pub(crate) fn restartable(program: &quench_runtime::ResidualProgram, function: &Function, code: &[WideInstruction]) -> bool {
    if super::super::lane_region::restartable_numeric_function(program, function, code.iter().copied()) { return true; }
    code.iter().enumerate().all(|(pc, instruction)| match () {
        _ if matches!(crate::compiled::decoded_completed(instruction), Some(quench_runtime::staged::Operation::ElementWrite { .. })) => false,
        _ if super::super::meaning_compiled::is_call(*instruction) => false,
        _ if matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::SetField { .. })) => false,
        _ if matches!(crate::compiled::decoded_completed(instruction), Some(quench_runtime::staged::Operation::Number(quench_runtime::staged::NumberOperation::IncDec { .. }))) => true,
        // An IncDec with a numeric local target decodes apart: its result stays a local.
        _ if matches!(crate::compiled::decoded_completed(instruction), Some(quench_runtime::staged::Operation::IncDecLocal { .. })) => false,
        _ if matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::Jump | quench_runtime::staged::Operation::JumpIf { .. }
            | quench_runtime::staged::Operation::CompareJump { .. })) => instruction.jump_target() as usize > pc,
        _ => true, // `inlinable` owns the supported instruction/effect set.
    })
}

/// Whether a call keeps its result in its register or returns it: the two a unit inlines.
pub(crate) fn returns_or_keeps(instruction: WideInstruction) -> bool {
    matches!(instruction.result_flags(), 0 | quench_runtime::bytecode::RETURN_REGISTER)
}
