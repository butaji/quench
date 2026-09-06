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

    const LOCAL_BINARY: Self = Self {
        removed_dispatches: 2,
        removed_materializations: 2,
        added_transfers: 2,
    };

    const LOCAL_CONSTANT: Self = Self {
        removed_dispatches: 1,
        removed_materializations: 1,
        added_transfers: 1,
    };

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
    pub source: NumericSource,
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
    pub discarded: [Option<Register>; 2],
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
    producers: [NumericProducer; 2],
    operation: Instruction,
    live_after: &BTreeSet<Register>,
) -> Option<LocalBinarySelection> {
    if producers[0].output == producers[1].output || numeric_operation(operation).is_none() {
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
    if lost_live_value || !FusionCost::LOCAL_BINARY.profitable() {
        return None;
    }
    Some(LocalBinarySelection {
        inputs: LocalNumericInputs::Sources(inputs),
        output: operation.a,
        operation,
        span: 3,
        discarded: discarded_registers(producers.map(|producer| producer.output), operation.a),
        cost: FusionCost::LOCAL_BINARY,
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
        discarded: discarded_registers([load.a, load.a], operation.a),
        cost: FusionCost::LOCAL_CONSTANT,
    })
}

fn discarded_registers(producers: [Register; 2], output: Register) -> [Option<Register>; 2] {
    let mut discarded = [None; 2];
    let mut length = 0;
    for producer in producers {
        if producer != output && !discarded.contains(&Some(producer)) {
            discarded[length] = Some(producer);
            length += 1;
        }
    }
    discarded
}

fn operation_sources(
    producers: [NumericProducer; 2],
    operation: Instruction,
) -> Option<[NumericSource; 2]> {
    let source_for = |register| {
        producers
            .iter()
            .find(|producer| producer.output == register)
            .map(|producer| producer.source)
    };
    Some([source_for(operation.b)?, source_for(operation.c)?])
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
mod tests {
    use super::*;

    fn add(dst: Register, lhs: Register, rhs: Register) -> Instruction {
        Instruction::add(dst, lhs, rhs)
    }

    fn local(output: Register, slot: u16) -> NumericProducer {
        NumericProducer {
            output,
            source: NumericSource::Local(slot),
        }
    }

    fn constant(output: Register, value: f64) -> NumericProducer {
        NumericProducer {
            output,
            source: NumericSource::Constant(value.to_bits()),
        }
    }

    #[test]
    fn add_chain_selection_derives_fixed_bindings() {
        let selected = select_add_chain(add(3, 1, 2), add(5, 3, 4), &BTreeSet::new()).unwrap();
        assert_eq!(selected.bindings.inputs, [1, 2, 4]);
        assert_eq!(selected.bindings.output, 5);
        assert!(selected.cost.profitable());
    }

    #[test]
    fn add_chain_selection_rejects_live_intermediate_and_alias() {
        let live = BTreeSet::from([3]);
        assert!(select_add_chain(add(3, 1, 2), add(5, 3, 4), &live).is_none());
        assert!(select_add_chain(add(3, 1, 2), add(5, 3, 3), &BTreeSet::new()).is_none());
    }

    #[test]
    fn add_chain_selection_rejects_noncanonical_operations() {
        let mut guarded = add(3, 1, 2);
        guarded.flags = 1;
        assert!(select_add_chain(guarded, add(5, 3, 4), &BTreeSet::new()).is_none());
        assert!(select_add_chain(
            Instruction::binary_operator(3, crate::ops::BinaryOp::Subtract, 1, 2,),
            add(5, 3, 4),
            &BTreeSet::new(),
        )
        .is_none());
    }

    #[test]
    fn local_binary_selection_forwards_slots_and_removes_materialization() {
        let selected = select_local_binary(
            [local(4, 9), local(7, 3)],
            Instruction::add(1, 7, 4),
            &BTreeSet::new(),
        )
        .unwrap();
        assert_eq!(
            selected.inputs,
            LocalNumericInputs::Sources([NumericSource::Local(3), NumericSource::Local(9)])
        );
        assert_eq!(selected.output, 1);
        assert_eq!(selected.span, 3);
        assert_eq!(selected.discarded, [Some(4), Some(7)]);
        assert!(selected.cost.profitable());
    }

    #[test]
    fn local_binary_selection_rejects_live_or_unrelated_loads() {
        let loads = [local(4, 9), local(7, 3)];
        assert!(
            select_local_binary(loads, Instruction::add(1, 7, 4), &BTreeSet::from([4])).is_none()
        );
        assert!(select_local_binary(loads, Instruction::add(1, 7, 8), &BTreeSet::new()).is_none());
    }

    #[test]
    fn local_binary_selection_numbers_repeated_slot_once() {
        let selected = select_local_binary(
            [local(4, 9), local(7, 9)],
            Instruction::add(1, 4, 7),
            &BTreeSet::new(),
        )
        .unwrap();
        assert_eq!(
            selected.inputs,
            LocalNumericInputs::Sources([NumericSource::Local(9), NumericSource::Local(9)])
        );
        assert_eq!(selected.discarded, [Some(4), Some(7)]);
    }

    #[test]
    fn local_binary_selection_propagates_constant_and_preserves_order() {
        let selected = select_local_binary(
            [constant(4, 2.5), local(7, 3)],
            Instruction::binary_operator(1, crate::ops::BinaryOp::Subtract, 4, 7),
            &BTreeSet::new(),
        )
        .unwrap();
        assert_eq!(
            selected.inputs,
            LocalNumericInputs::Sources([
                NumericSource::Constant(2.5_f64.to_bits()),
                NumericSource::Local(3),
            ])
        );
    }

    #[test]
    fn local_binary_selection_rejects_constant_only_work() {
        let selected = select_local_binary(
            [constant(4, 2.5), constant(7, 1.5)],
            Instruction::add(1, 4, 7),
            &BTreeSet::new(),
        );
        assert!(selected.is_none());
    }

    #[test]
    fn local_constant_selection_preserves_bits_and_operand_order() {
        let bits = (-0.0_f64).to_bits();
        let selected = select_local_add_const(
            Instruction::load_local(4, 9),
            Instruction::add_const(1, 4, 7),
            bits,
            &BTreeSet::new(),
        )
        .unwrap();
        assert_eq!(
            selected.inputs,
            LocalNumericInputs::SlotConstant { slot: 9, bits }
        );
        assert_eq!(selected.span, 2);
        assert_eq!(selected.discarded, [Some(4), None]);
    }

    #[test]
    fn local_constant_selection_rejects_left_or_live_source() {
        let load = Instruction::load_local(4, 9);
        assert!(select_local_add_const(
            load,
            Instruction::add_const_left(1, 4, 7),
            1.0_f64.to_bits(),
            &BTreeSet::new(),
        )
        .is_none());
        assert!(select_local_add_const(
            load,
            Instruction::add_const(1, 4, 7),
            1.0_f64.to_bits(),
            &BTreeSet::from([4]),
        )
        .is_none());
    }
}
