//! Frozen reads: a by-name load or a capture load of a binding the closed world fixes with one immediate store
//! (`AotFacts::dominated_root_store`). The fact is the one home of that decision: each read asks it for its key and folds
//! the value it returns. No table of its own, and no check at the read, since the store dominates every read.

#[allow(unused_imports)] use quench_runtime::staged::{OperationOperationExt, OperationNamesExt};
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
#[allow(unused_imports)] use quench_runtime::staged::glue_prelude::*;

/// The frozen reads of a lane unit of function `function`, by unit PC: each instruction read in the function its origin names
/// (the caller's, or an inlined frame's callee); a `LoadName` no frozen fact folds is the lane's own read (`dynamic_load`).
pub(crate) fn frozen_unit_reads(program: &ResidualProgram, unit: &super::lane_unit::Unit, function: u32, facts: &quench_runtime::compile::aot_facts::AotFacts,
) -> quench_runtime::vm::global_facts::GlobalReads {
    unit.instructions.iter().enumerate().filter_map(|(pc, &instruction)| {
        let owner = match *unit.origins.get(pc)? {
            super::lane_unit::Origin::Caller(_) => function,
            super::lane_unit::Origin::Inlined { frame, .. } => unit.frames.get(frame)?.target.function,
        };
        let read = frozen_read(program, program.function_get(owner as usize)?, instruction, facts)
            .or_else(|| dynamic_load(instruction))?;
        Some((pc, read))
    }).collect()
}

/// A `LoadName` no frozen fact folds: the lane's own read at run time (`GlobalRead::Dynamic`,
/// `LaneHelper::GlobalGet`), which leaves where the global scope gives no plain value, as the
/// interpreter's read would answer it.
fn dynamic_load(instruction: WideInstruction) -> Option<quench_runtime::vm::global_facts::GlobalRead> {
    matches!(quench_runtime::staged::Operation::<(), ()>::decode_load_name(instruction, ()), Some(quench_runtime::staged::Operation::LoadName { .. }))
        .then_some(quench_runtime::vm::global_facts::GlobalRead::Dynamic)
}

/// The frozen read `instruction` of `function` makes: a by-name load of a global, or a capture load of a slot, whose key the
/// fact gives one value, with that value.
fn frozen_read(program: &ResidualProgram, function: &Function, instruction: WideInstruction, facts: &quench_runtime::compile::aot_facts::AotFacts,
) -> Option<quench_runtime::vm::global_facts::GlobalRead> {
    let key = if instruction.op().loads_name() {
        quench_runtime::typed_ir::FactKey::Global(instruction.atom_index())
    } else if matches!(crate::compiled::decoded_completed(instruction), Some(quench_runtime::staged::Operation::LoadCapture { .. })) {
        capture_key(program, function, instruction)?
    } else {
        return None;
    };
    let value = facts.dominated_root_store(key)?.value;
    Some(quench_runtime::vm::global_facts::GlobalRead::Frozen { value, ty: quench_runtime::typed_ir::Type::immediate(value) })
}

/// The slot a capture load reads, keyed as the census keys it: the environment slot of the function `depth` + 1 parents up
/// from `function`, the owner the scope chain names (`FactKey::Slot`).
fn capture_key(program: &ResidualProgram, function: &Function, instruction: WideInstruction) -> Option<quench_runtime::typed_ir::FactKey> {
    let mut owner = function.parent?;
    for _ in 0..quench_runtime::bytecode::ImmediateLayout::capture_depth(instruction.imm()) {
        owner = program.function(owner as usize).parent?;
    }
    let slot = quench_runtime::bytecode::ImmediateLayout::capture_slot(instruction.imm());
    Some(quench_runtime::typed_ir::FactKey::Slot { owner, slot })
}
