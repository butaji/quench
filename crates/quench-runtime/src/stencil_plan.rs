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

    const fn profitable(self) -> bool {
        self.removed_dispatches + self.removed_materializations > self.added_transfers
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct AddChainSelection {
    pub bindings: F64x3Bindings,
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
        assert!(
            select_add_chain(
                Instruction::binary_operator(
                    3,
                    crate::ops::BinaryOp::Subtract,
                    1,
                    2,
                ),
                add(5, 3, 4),
                &BTreeSet::new(),
            )
            .is_none()
        );
    }
}
