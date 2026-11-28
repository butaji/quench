use super::*;

/// The unit PCs occupied by a call's alternatives, including each admission marker.
/// Returning calls continue at an existing completion; they add no synthetic `Return`.
pub(super) fn call_length(alternatives: &[InlinedPlan]) -> u32 {
    alternatives.iter().map(|callee| 1 + callee.plan.length(false)).sum()
}

/// The canonical caller completion a terminal call may use without replaying the call.
pub(super) fn terminal_return(code: &[WideInstruction]) -> Option<usize> {
    code.len().checked_sub(1).filter(|pc|
        matches!(Operation::<u64, ()>::of(code[*pc]), Some(Operation::Return { .. })) && code[*pc].result_flags() == 0)
}

/// Whether the staged decode makes `instruction` a throw of its first operand (`Throw`).
pub(super) fn throws(instruction: WideInstruction) -> bool {
    matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::Throw { .. }))
}

/// Whether an inlined callee's `instruction` renames to the unit's registers
/// (`renamed`): its operands are registers or constants. The plan decides it, so a
/// unit it plans always emits.
pub(super) fn renamable(instruction: WideInstruction) -> bool {
    match instruction.op() {
        _ if quench_runtime::staged::Operation::<u64, ()>::is_unit_rewritten(instruction) => true,
        _ if site_field_read(instruction) => true,
        _ => {
            let plain = if inlined_returning(instruction) {
                WideInstruction::new(instruction.op(), instruction.result_register(), instruction.b(), instruction.c(), instruction.imm())
            } else {
                instruction
            };
            renamed(plain, &|register| register, 0, &|target| Some(target)).is_some()
        }
    }
}

/// Whether `instruction` reads a field through its lookup site (`FieldLookup::Site`, a nested
/// lookup's), returning or not: the site keeps its base, which the lowering renames, so the record
/// keeps its operands and only its result register renames.
pub(super) fn site_field_read(instruction: WideInstruction) -> bool {
    matches!(crate::compiled::decoded_completed(instruction), Some(quench_runtime::staged::Operation::GetField { .. }))
        && matches!(instruction.field_lookup(), Some(FieldLookup::Site(_)))
}

/// The unit PCs an inlined callee instruction takes: a completion of the call (a `Return`, or
/// an instruction that computes and returns) adds the result's move and the jump past the call.
pub(super) fn completion_length(instruction: WideInstruction) -> u32 {
    if matches!(Operation::<u64, ()>::of(instruction), Some(Operation::Return { .. })) {
        2
    } else if inlined_returning(instruction) {
        3
    } else {
        1
    }
}

pub(super) fn decoded(function: &Function) -> Vec<WideInstruction> {
    function.code.iter().map(|packed| function.widened(*packed)).collect()
}

