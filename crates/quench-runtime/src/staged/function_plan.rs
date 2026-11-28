//! The plan of a function (the K-PLAN contract): for every PC, what the interpreter's own
//! definition says the instruction does, in the one form every tier projects. A PC holds its
//! staged operation where the catalog names a decoder (`Op::staged`), else the interpreter's
//! generic step a tier calls out to (`Op::generic_step`), else one exit of a closed set. The
//! plan is total: no PC is "unsupported", so admission is "the plan lowers" and a tier that
//! cannot specialize a PC calls the step out instead of refusing the function.
//!
//! Only the operation is stored. Its binding times (`Operation::operand_times`) and effect row
//! (`Operation::effects`) are functions of it, read where a tier asks.

#[cfg(not(stencil_build))]
#[allow(unused_imports)]
use crate::staged::allocation::AllocationAllocationExt;
#[cfg(not(stencil_build))]
#[allow(unused_imports)] use crate::staged::{OperationOperationExt};
use super::decode::DecodeContext;
use super::{Allocation, Operation};
use crate::bytecode::{GenericStep, JsDenotation, Register, WideInstruction};
#[cfg(not(stencil_build))]
#[allow(unused_imports)]
use crate::staged::glue_prelude::*;

/// A plan operation's word and proof are its registers and a proof slot to come (K-PROOF):
/// the plan names what an instruction reads, never a copy of a fact.
pub(crate) type PlanOperation = Operation<Register, ()>;

/// Where an instruction with neither a staged operation nor a generic step leaves its tier.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Exit {
    /// The activation suspends (`await`, `yield`, `yield*`): a resumable frame continues it.
    Suspend,
    /// The interpreter's hand path at this exact PC, through the frame's miss edge.
    Miss,
}

/// What the plan holds for one PC.
#[derive(Clone)]
pub(crate) enum PlanOp {
    /// The instruction does nothing but continue (its denotation is `JsDenotation::Noop`).
    Continue,
    Staged(PlanOperation),
    Generic(GenericStep),
    Exit(Exit),
}

/// The plan of one function version, one operation per PC.
pub(crate) struct FunctionPlan {
    ops: Box<[PlanOp]>,
}

/// The plan operation of the instruction at `pc` of `instructions`.
fn plan_op(instructions: &[WideInstruction], pc: usize, context: DecodeContext<'_>) -> PlanOp {
    let instruction = instructions[pc];
    let op = instruction.op();
    if op.js_denotation() == JsDenotation::Noop {
        return PlanOp::Continue;
    }
    // A suspension resumes only in a frame that continues it, whatever row it has.
    if op.suspends() {
        return PlanOp::Exit(Exit::Suspend);
    }
    let staged = Operation::decode(instruction, context).or_else(|| {
        let (destination, allocation) = Allocation::decode(instruction, &instructions[pc + 1..])?;
        Some(Operation::Allocate { destination, allocation, proof: () })
    });
    match (staged, op.generic_step()) {
        (Some(operation), _) => PlanOp::Staged(operation),
        (None, Some(step)) => PlanOp::Generic(step),
        (None, None) => PlanOp::Exit(Exit::Miss),
    }
}

impl FunctionPlan {
    /// The plan of a function's `instructions`, decoding operands through `context`.
    pub(crate) fn of(instructions: &[WideInstruction], context: DecodeContext<'_>) -> Self {
        Self { ops: (0..instructions.len()).map(|pc| plan_op(instructions, pc, context)).collect() }
    }

    /// The operation at `pc`.
    pub(crate) fn at(&self, pc: usize) -> &PlanOp {
        &self.ops[pc]
    }

    /// The PCs whose instruction only the interpreter's hand path runs: the plan's measure of
    /// semantics no tier can derive yet (the catalog completeness of task P2).
    pub(crate) fn misses(&self) -> impl Iterator<Item = usize> + '_ {
        self.ops.iter().enumerate().filter(|(_, op)| matches!(op, PlanOp::Exit(Exit::Miss))).map(|(pc, _)| pc)
    }
}

#[cfg(test)]
#[path = "function_plan_tests.rs"]
mod tests;
