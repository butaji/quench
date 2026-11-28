//! The unit a lane body specializes: one function with the callees its call sites' call plans
//! name spliced in, transitively, under the growth cap (`tier_policy::INLINE_GROWTH_PERCENT`).
//!
//! A call whose call plan names its callee, to a callee whose body the lane-body lowering runs
//! entirely and every call of which is spliced in in turn, is replaced by a call marker and the
//! callee's renamed instructions. Its registers
//! follow the unit's; its `this` is the call's receiver register; its `Return` moves the
//! value to the call's result and jumps past the call. Each PC of the unit names its origin,
//! so a leave inside a callee makes the activations of its frame and the frames around it,
//! outermost first, through the lane's own call path, and continues at the innermost one's
//! record (`Vm::body_inlined_activation_in_lane`).

#[allow(unused_imports)] use quench_runtime::staged::{OperationOperationRecordsExt, OperationNamesExt};
#[allow(unused_imports)]
use crate::ext_traits::*;
use quench_runtime::bytecode::{Constant, FieldBase, FieldLayout, FieldLookup, ImmediateLayout, ImmediateRole, InstructionField, Op, Operand, OperandKind, Register, RETURN_REGISTER, WideInstruction};
use quench_runtime::bytecode::Function;
use quench_runtime::compile::call_graph::CallGraph;
use quench_runtime::compile::call_plan::{tail_site, CallPlan};
use quench_runtime::staged::Operation;
use quench_runtime::value::Value;
#[path = "lane_unit/admission.rs"] pub(crate) mod admission;
pub(crate) use admission::{inlinable, InlineScope};
use admission::{restartable, returns_or_keeps};
use super::admission::{admit_callee, CalleeUse};
#[cfg(test)] #[path = "lane_unit/static_hints.rs"] pub(crate) mod static_hints;
#[cfg(test)] pub(crate) use static_hints::static_call_targets;

pub(crate) use quench_runtime::vm::engine_types::CalleeBinding;

pub(crate) use quench_runtime::vm::engine_types::CallRecovery;

pub(crate) use quench_runtime::vm::engine_types::CallTarget;

#[quench_macros::ext(CallTargetOps1)]
impl CallTarget {
    /// The forwarding native this site called the target through.
    pub(crate) fn through(self) -> Option<Value> {
        match self.callee { CalleeBinding::Forwarded { through, .. } => Some(through), _ => None }
    }
}

/// The unit PC a self tail call jumps to: the root's first instruction, which the unit reaches
/// through a jump at PC 0, so the entry is entered once and PC 1 is the loop header.
const SELF_TAIL_HEADER: u32 = 1;

/// Where a unit PC comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Origin {
    /// The caller's instruction at this PC.
    Caller(u32),
    /// The instruction at `pc` of inlined `frame`.
    Inlined { frame: usize, pc: u32 },
}

pub(crate) use quench_runtime::vm::engine_types::InlinedCall;

#[quench_macros::ext(InlinedCallOps1)]
impl InlinedCall {
    /// The physical parameter homes initialized by the passed arguments.
    pub(crate) fn passed_registers(self, callee: &Function) -> impl Iterator<Item = (Register, Register)> + '_ {
        (0..self.arguments.min(callee.params)).filter_map(move |argument|
            callee.parameter_registers.map(|base| base + argument)
                .or_else(|| callee.promoted_register(argument))
                .map(|register| (self.base + register, self.window + argument)))
    }

    /// The parent's register 0: the caller's, or the enclosing callee's.
    pub(crate) fn parent_base(&self, frames: &[InlinedCall]) -> Register {
        self.parent.map_or(0, |parent| frames[parent].base)
    }
}

#[derive(Clone)]
pub(crate) struct Unit {
    pub(crate) instructions: Vec<WideInstruction>,
    pub(crate) origins: Vec<Origin>,
    pub(crate) frames: Vec<InlinedCall>,
    /// The unit PC of each frame's call marker, where the call's guards and bindings run.
    pub(crate) markers: Vec<usize>,
    pub(crate) caller_registers: u16,
    pub(crate) registers: u16,
    /// The caller PC past its last instruction, where falling off the unit resumes.
    pub(crate) end: u32,
}

#[path = "lane_unit/unit_methods.rs"]
mod unit_methods;
#[path = "lane_unit/analysis.rs"]
mod analysis;
pub(crate) use analysis::*;
#[path = "lane_unit/plan.rs"]
mod plan;
pub(crate) use plan::*;
#[path = "lane_unit/emit.rs"]
mod emit;
#[cfg(test)]
#[path = "lane_unit/tests.rs"]
pub(crate) mod tests;
pub(crate) use emit::*;
use super::tier_policy::inline_plan;
#[allow(unused_imports)] use quench_runtime::staged::glue_prelude::*;

