use super::*;

/// A string element read whose unit its next instruction compares with a one-unit constant
/// (`Lowering::unit_compare`): the register the read would define, the unit it read, and the
/// constant unit and sense of the comparison.
#[derive(Clone, Copy)]
pub(super) struct PendingUnit {
    pub(super) pc: usize,
    pub(super) register: Register,
    pub(super) unit: ir::Value,
    pub(super) constant: u16,
    pub(super) negated: bool,
}

/// The register whose value `register` copies at the comparison at `pc`: following the moves and
/// nops back to `header` within one block, so copies share the comparison's SSA value.
pub(super) fn move_root(instructions: &[WideInstruction], leaders: &BitSet, (header, pc): (usize, usize), mut register: Register) -> Register {
    for at in (header..pc).rev() {
        if at + 1 < pc && leaders.contains(at + 1) { break; }
        let instruction = instructions[at];
        match () {
            _ if matches!(super::super::decoded(instruction), Some(quench_runtime::staged::Operation::Move { .. })) && instruction.result_flags() == 0 => {
                if instruction.register_a() == register { register = instruction.register_b(); }
            }
            _ if matches!(super::super::decoded(instruction), Some(quench_runtime::staged::Operation::Nop)) => {},
            _ => break,
        }
    }
    register
}

/// Whether `instruction` can change no heap value natively: anything else it would do (a
/// non-Number operand, a missed cache) leaves the body first.
pub(super) fn reads_only(constants: &[quench_runtime::bytecode::Constant], instruction: WideInstruction) -> bool {
    reads_slot(constants, instruction).unwrap_or_else(|| match () {
        // ToNumeric of a Number is the Number; any other operand leaves (`Lowering::instruction`). A
        // dense element is read natively, any other leaving (`emit_dense_element_read`); a null or
        // undefined operand leaves before any effect (`staged::require_object_coercible`).
        _ if matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::Number(quench_runtime::staged::NumberOperation::ToNumeric { .. }))) || matches!(crate::compiled::performed(instruction), Some(quench_runtime::staged::Operation::ElementRead { .. })) || matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::RequireObjectCoercible { .. })) => !instruction.returns_from_frame(),
        // A Number operator of a Number; any other operand leaves before any effect (`Lowering::unary`).
        // A dense switch's case slot and its branch table read one frame register; an operand that
        // is no int leaves before any effect (`Lowering::int_case_index`, `Lowering::branch_table`).
        _ if matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::Number(quench_runtime::staged::NumberOperation::IntCase { .. }))) => instruction.result_flags() == 0,
        _ if instruction.op().control_flow_layout() == quench_runtime::bytecode::ControlFlowLayout::BranchTable => instruction.result_flags() == 0,
        _ if instruction.op().immediate_role() == quench_runtime::bytecode::ImmediateRole::UnaryOperator
            && crate::compiled::number_unary(instruction, instruction.unary_operator()).is_some() =>
            instruction.result_flags() == 0 && !instruction.writes_current_this(),
        _ => quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::NumericBinary) && !instruction.returns_from_frame(),
    })
}

/// Whether `instruction` reads only a frame slot, a constant, a capture or a field, where it is
/// one; `None` for the other instructions.
pub(super) fn reads_slot(constants: &[quench_runtime::bytecode::Constant], instruction: WideInstruction) -> Option<bool> {
    Some(match () {
        _ if instruction.op().traits().has(Traits::FRAME_ONLY) => true,
        _ if quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::LoadConst) => constants.get(instruction.constant_index()).and_then(super::super::facts::immediate).is_some(),
        _ if quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::IncDec) => instruction.numeric_local_target().is_none(),
        _ if matches!(crate::compiled::performed(instruction), Some(quench_runtime::staged::Operation::LoadCapture { .. })) => instruction.result_flags() == 0 && instruction.capture_depth() == 0,
        _ if matches!(crate::compiled::performed(instruction), Some(quench_runtime::staged::Operation::GetField { .. })) => !instruction.writes_current_this() && !instruction.returns_from_frame(),
        _ => return None,
    })
}

/// Whether `instruction` is a store natively: of its atom, into a slot the object already
/// has, so it changes no shape, layout or other property; anything else leaves first.
pub(super) fn stores_own(instruction: WideInstruction) -> bool {
    matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::SetField { .. })) && !instruction.returns_from_frame()
}

/// Whether `instruction` is an element store natively: into a record array, whose elements
/// no named field read, capture or object shape observes (`Lowering::element_write`); anything
/// else leaves first.
pub(super) fn stores_element(instruction: WideInstruction) -> bool {
    matches!(crate::compiled::performed(instruction), Some(quench_runtime::staged::Operation::ElementWrite { .. })) && !instruction.returns_from_frame()
}

/// Whether an array view (`LocalProofs::arrays`) survives `instruction`: it reads no heap
/// value but an element, or replaces one in place (`Lowering::element_write` retires the views
/// itself on its general path), so no array's storage changes.
pub(super) fn retains_array_views(constants: &[quench_runtime::bytecode::Constant], instruction: WideInstruction) -> bool {
    match () {
        _ if matches!(crate::compiled::performed(instruction), Some(quench_runtime::staged::Operation::GetField { .. } | quench_runtime::staged::Operation::LoadCapture { .. })) => false,
        _ if matches!(crate::compiled::performed(instruction), Some(quench_runtime::staged::Operation::ElementWrite { .. })) => true,
        _ => reads_only(constants, instruction),
    }
}

/// Whether a loop may hold `instruction` natively: a read, an own-field or element store, or a
/// call lowered as an intrinsic the loop holds (`native_calls::holds_in_loop`).
pub(super) fn stays_native(constants: &[Constant], instruction: WideInstruction, intrinsic: bool) -> bool {
    intrinsic || reads_only(constants, instruction) || stores_own(instruction) || stores_element(instruction)
}

/// Whether an allocation makes a holder of named or indexed properties (an object or array literal), whose stores may be
/// fresh (`LocalProofs::fresh`). A closure, regexp or template is no such holder here.
pub(super) fn makes_holder(allocation: quench_runtime::staged::Allocation) -> bool {
    use quench_runtime::staged::Allocation;
    matches!(allocation, Allocation::Empty | Allocation::Array { .. } | Allocation::ConstArray { .. } | Allocation::ArrayOf { .. } | Allocation::ObjectPair { .. } | Allocation::ObjectLiteral { .. })
}

