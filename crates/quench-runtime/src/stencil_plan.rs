//! Bounded, disposable selection over canonical residual instructions.
//!
//! This module owns physical bindings and cost decisions, never JavaScript
//! semantics. Selected plans refer back to immutable residual operations.

use crate::ir::{Instruction, Opcode, Register};
pub(crate) use crate::stencil_value_graph::{BlockValueGraph, ValueDefinition, ValueId};
use std::collections::BTreeSet;

pub(crate) const MAX_BLOCK_VALUES: usize = 8;
pub(crate) type DiscardedRegisters = [Option<Register>; MAX_BLOCK_VALUES];

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

    pub(crate) fn numeric_producers(count: usize) -> Self {
        let count = u8::try_from(count).unwrap_or(u8::MAX);
        Self {
            removed_dispatches: count,
            removed_materializations: count,
            added_transfers: 2,
        }
    }

    pub(crate) fn constant_fold(count: usize) -> Self {
        let count = u8::try_from(count).unwrap_or(u8::MAX);
        Self {
            removed_dispatches: count.saturating_add(1),
            removed_materializations: count.saturating_add(1),
            added_transfers: 1,
        }
    }

    pub(crate) fn property_producers(count: usize) -> Self {
        let count = u8::try_from(count).unwrap_or(u8::MAX);
        Self {
            removed_dispatches: count,
            removed_materializations: count,
            added_transfers: 1,
        }
    }

    pub(crate) const fn profitable(self) -> bool {
        self.removed_dispatches + self.removed_materializations > self.added_transfers
    }

    const fn rank(self) -> u8 {
        self.removed_dispatches
            .saturating_add(self.removed_materializations)
            .saturating_sub(self.added_transfers)
    }
}

pub(crate) trait RankedSelection {
    fn rank(&self) -> (u8, u8);
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
    AddChain {
        sources: [NumericSource; 3],
        bindings: F64x3Bindings,
    },
    Folded { bits: u64 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LocalBinarySelection {
    pub inputs: LocalNumericInputs,
    pub output: Register,
    pub operation: Instruction,
    pub span: u8,
    pub discarded: DiscardedRegisters,
    pub cost: FusionCost,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LocalPropertySelection {
    pub receiver_slot: u16,
    pub output: Register,
    pub operation: Instruction,
    pub span: u8,
    pub discarded: DiscardedRegisters,
    pub cost: FusionCost,
}

impl RankedSelection for LocalBinarySelection {
    fn rank(&self) -> (u8, u8) {
        (self.cost.rank(), self.span)
    }
}

impl RankedSelection for LocalPropertySelection {
    fn rank(&self) -> (u8, u8) {
        (self.cost.rank(), self.span)
    }
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
    if !(2..=MAX_BLOCK_VALUES).contains(&producers.len()) || duplicate_definitions(producers) {
        return None;
    }
    let operator = numeric_operation(operation)?;
    let inputs = operation_sources(producers, operation)?;
    let overwritten = operation.a;
    let lost_live_value = producers
        .iter()
        .any(|producer| producer.output != overwritten && live_after.contains(&producer.output));
    let folded = fold_numeric_sources(inputs, operator);
    let cost = folded.map_or_else(
        || FusionCost::numeric_producers(producers.len()),
        |_| FusionCost::constant_fold(producers.len()),
    );
    if lost_live_value || !cost.profitable() {
        return None;
    }
    Some(LocalBinarySelection {
        inputs: folded.map_or(LocalNumericInputs::Sources(inputs), |bits| {
            LocalNumericInputs::Folded { bits }
        }),
        output: operation.a,
        operation,
        span: u8::try_from(producers.len() + 1).ok()?,
        discarded: discarded_registers(producers, operation.a),
        cost,
    })
}

pub(crate) fn fold_numeric_sources(
    inputs: [NumericSource; 2],
    operator: crate::ops::BinaryOp,
) -> Option<u64> {
    let [NumericSource::Constant(lhs), NumericSource::Constant(rhs)] = inputs else {
        return None;
    };
    let lhs = f64::from_bits(lhs);
    let rhs = f64::from_bits(rhs);
    use crate::ops::BinaryOp::{Add, Divide, Multiply, Subtract};
    let value = match operator {
        Add => lhs + rhs,
        Subtract => lhs - rhs,
        Multiply => lhs * rhs,
        Divide => lhs / rhs,
        _ => return None,
    };
    Some(value.to_bits())
}

pub(crate) fn numeric_operation(instruction: Instruction) -> Option<crate::ops::BinaryOp> {
    use crate::ops::BinaryOp::{Add, Divide, Multiply, Subtract};
    let operator = match instruction.opcode {
        Opcode::Add | Opcode::Sub | Opcode::Mul | Opcode::Div if instruction.flags == 0 => {
            instruction.opcode.numeric_operator()?
        }
        Opcode::Binary => crate::ir::compact_binary_operator(instruction.flags)?,
        _ => return None,
    };
    matches!(operator, Add | Subtract | Multiply | Divide).then_some(operator)
}

pub(crate) fn select_source_add_const(
    producer: NumericProducer,
    operation: Instruction,
    constant_bits: u64,
    live_after: &BTreeSet<Register>,
) -> Option<LocalBinarySelection> {
    if operation.opcode != Opcode::AddConst
        || operation.b != producer.output
        || (producer.output != operation.a && live_after.contains(&producer.output))
        || !FusionCost::LOCAL_CONSTANT.profitable()
    {
        return None;
    }
    let inputs = add_const_inputs(producer.definition, operation, constant_bits)?;
    Some(LocalBinarySelection {
        inputs,
        output: operation.a,
        operation,
        span: 2,
        discarded: discarded_registers(&[producer], operation.a),
        cost: FusionCost::LOCAL_CONSTANT,
    })
}

fn add_const_inputs(
    definition: NumericDefinition,
    operation: Instruction,
    constant_bits: u64,
) -> Option<LocalNumericInputs> {
    match definition {
        NumericDefinition::Source(NumericSource::Local(slot)) if operation.flags == 0 => {
            Some(LocalNumericInputs::SlotConstant {
                slot,
                bits: constant_bits,
            })
        }
        NumericDefinition::Source(NumericSource::Constant(source_bits)) => {
            let (lhs, rhs) = if operation.add_const_is_left() {
                (constant_bits, source_bits)
            } else {
                (source_bits, constant_bits)
            };
            let bits = fold_numeric_sources(
                [NumericSource::Constant(lhs), NumericSource::Constant(rhs)],
                crate::ops::BinaryOp::Add,
            )?;
            Some(LocalNumericInputs::Folded { bits })
        }
        _ => None,
    }
}

fn discarded_registers(producers: &[NumericProducer], output: Register) -> DiscardedRegisters {
    let mut discarded = [None; MAX_BLOCK_VALUES];
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
