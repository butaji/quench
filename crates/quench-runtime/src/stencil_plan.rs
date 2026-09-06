//! Bounded, disposable selection over canonical residual instructions.
//!
//! This module owns physical bindings and cost decisions, never JavaScript
//! semantics. Selected plans refer back to immutable residual operations.

use crate::ir::{Instruction, Opcode, Register};
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct F64x3Bindings {
    pub inputs: [Register; 3],
    pub output: Register,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct FusionCost {
    removed_dispatches: u8,
    removed_materializations: u8,
    added_transfers: u8,
}

impl FusionCost {
    const ADD_CHAIN: Self = Self {
        removed_dispatches: 1,
        removed_materializations: 1,
        added_transfers: 0,
    };

    const LOCAL_CONSTANT: Self = Self {
        removed_dispatches: 1,
        removed_materializations: 1,
        added_transfers: 1,
    };

    fn numeric_producers(count: usize) -> Self {
        let count = u8::try_from(count).unwrap_or(u8::MAX);
        Self {
            removed_dispatches: count,
            removed_materializations: count,
            added_transfers: 2,
        }
    }

    const fn profitable(self) -> bool {
        self.removed_dispatches + self.removed_materializations > self.added_transfers
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct AddChainSelection {
    pub bindings: F64x3Bindings,
    pub cost: FusionCost,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum NumericSource {
    Local(u16),
    Constant(u64),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct NumericProducer {
    pub output: Register,
    pub definition: NumericDefinition,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum NumericDefinition {
    Source(NumericSource),
    Alias(Register),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LocalNumericInputs {
    Sources([NumericSource; 2]),
    SlotConstant { slot: u16, bits: u64 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LocalBinarySelection {
    pub inputs: LocalNumericInputs,
    pub output: Register,
    pub operation: Instruction,
    pub span: u8,
    pub discarded: [Option<Register>; 3],
    pub cost: FusionCost,
}

pub(crate) fn select_add_chain(
    first: Instruction,
    second: Instruction,
    live_after: &BTreeSet<Register>,
) -> Option<AddChainSelection> {
    let bindings = add_chain_bindings(first, second)?;
    if live_after.contains(&first.a) || !FusionCost::ADD_CHAIN.profitable() {
        return None;
    }
    Some(AddChainSelection {
        bindings,
        cost: FusionCost::ADD_CHAIN,
    })
}

pub(crate) fn select_local_binary(
    producers: &[NumericProducer],
    operation: Instruction,
    live_after: &BTreeSet<Register>,
) -> Option<LocalBinarySelection> {
    if !(2..=3).contains(&producers.len())
        || duplicate_definitions(producers)
        || numeric_operation(operation).is_none()
    {
        return None;
    }
    let inputs = operation_sources(producers, operation)?;
    if inputs
        .iter()
        .all(|source| matches!(source, NumericSource::Constant(_)))
    {
        return None;
    }
    let overwritten = operation.a;
    let lost_live_value = producers
        .iter()
        .any(|producer| producer.output != overwritten && live_after.contains(&producer.output));
    let cost = FusionCost::numeric_producers(producers.len());
    if lost_live_value || !cost.profitable() {
        return None;
    }
    Some(LocalBinarySelection {
        inputs: LocalNumericInputs::Sources(inputs),
        output: operation.a,
        operation,
        span: u8::try_from(producers.len() + 1).ok()?,
        discarded: discarded_registers(producers, operation.a),
        cost,
    })
}

fn numeric_operation(instruction: Instruction) -> Option<crate::ops::BinaryOp> {
    use crate::ops::BinaryOp::{Add, Divide, Multiply, Subtract};
    if instruction.opcode != Opcode::Binary && instruction.flags != 0 {
        return None;
    }
    let operator = instruction
        .opcode
        .numeric_operator()
        .or_else(|| crate::ir::compact_binary_operator(instruction.flags))?;
    matches!(operator, Add | Subtract | Multiply | Divide).then_some(operator)
}

pub(crate) fn select_local_add_const(
    load: Instruction,
    operation: Instruction,
    constant_bits: u64,
    live_after: &BTreeSet<Register>,
) -> Option<LocalBinarySelection> {
    if load.opcode != Opcode::LoadLocal
        || operation.opcode != Opcode::AddConst
        || operation.flags != 0
        || operation.b != load.a
        || (load.a != operation.a && live_after.contains(&load.a))
        || !FusionCost::LOCAL_CONSTANT.profitable()
    {
        return None;
    }
    Some(LocalBinarySelection {
        inputs: LocalNumericInputs::SlotConstant {
            slot: load.b,
            bits: constant_bits,
        },
        output: operation.a,
        operation,
        span: 2,
        discarded: discarded_registers(
            &[NumericProducer {
                output: load.a,
                definition: NumericDefinition::Source(NumericSource::Local(load.b)),
            }],
            operation.a,
        ),
        cost: FusionCost::LOCAL_CONSTANT,
    })
}

fn discarded_registers(producers: &[NumericProducer], output: Register) -> [Option<Register>; 3] {
    let mut discarded = [None; 3];
    let mut length = 0;
    for producer in producers.iter().map(|producer| producer.output) {
        if producer != output && !discarded.contains(&Some(producer)) {
            discarded[length] = Some(producer);
            length += 1;
        }
    }
    discarded
}

fn operation_sources(
    producers: &[NumericProducer],
    operation: Instruction,
) -> Option<[NumericSource; 2]> {
    Some([
        resolve_source(producers, operation.b)?,
        resolve_source(producers, operation.c)?,
    ])
}

fn resolve_source(producers: &[NumericProducer], mut register: Register) -> Option<NumericSource> {
    let mut end = producers.len();
    for _ in 0..producers.len() {
        let index = producers[..end]
            .iter()
            .rposition(|producer| producer.output == register)?;
        match producers[index].definition {
            NumericDefinition::Source(source) => return Some(source),
            NumericDefinition::Alias(input) => {
                register = input;
                end = index;
            }
        }
    }
    None
}

fn duplicate_definitions(producers: &[NumericProducer]) -> bool {
    producers.iter().enumerate().any(|(index, producer)| {
        producers[..index]
            .iter()
            .any(|prior| prior.output == producer.output)
    })
}

fn add_chain_bindings(first: Instruction, second: Instruction) -> Option<F64x3Bindings> {
    let is_numeric_add = |instruction: Instruction| {
        instruction.opcode == Opcode::Add
            && instruction.flags == 0
            && instruction
                .opcode
                .has_guard(crate::facts::OperationGuard::Number)
    };
    if !is_numeric_add(first) || !is_numeric_add(second) {
        return None;
    }
    if second.b != first.a || second.c == first.a {
        return None;
    }
    Some(F64x3Bindings {
        inputs: [first.b, first.c, second.c],
        output: second.a,
    })
}

#[cfg(test)]
#[path = "stencil_plan_tests.rs"]
mod tests;
