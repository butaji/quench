//! Operator classes of a loop body: the arithmetic, bitwise, comparison, and unary operators a region lowers, and the role each register plays.
use super::*;

/// The arithmetic and comparison a region lowers, by the operator's tag.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Arithmetic {
    Float(NumberArithmetic),
    Int32(quench_runtime::vm::operations::NumberBitwise),
    Remainder,
}

pub(super) fn arithmetic(instruction: WideInstruction, operator: u32) -> Option<Arithmetic> {
    if quench_runtime::staged::computes_binary(instruction.op()) && operator == quench_runtime::vm::operations::REMAINDER {
        return Some(Arithmetic::Remainder);
    }
    if let Some(operation) = NumberArithmetic::from_js_instruction(instruction.op(), operator) {
        return Some(Arithmetic::Float(operation));
    }
    quench_runtime::vm::operations::NumberBitwise::from_js_instruction(instruction.op(), operator)
        .map(Arithmetic::Int32)
}

/// Fused numeric mask truthiness. Other arithmetic/heap truthiness stays on its
/// existing path; both operands must satisfy the region's Number guards.
pub(super) fn bitwise_condition(instruction: WideInstruction) -> Option<NumberBitwise> {
    (quench_runtime::staged::compare_jump(instruction.op())).then_some(())?;
    NumberBitwise::from_operator(instruction.binary_operator_field())
        .filter(|operation| *operation == NumberBitwise::And)
}

pub(super) fn number_comparison(instruction: WideInstruction) -> Option<quench_runtime::vm::operations::NumberRelation> {
    (quench_runtime::staged::computes_binary(instruction.op()))
        .then(|| quench_runtime::vm::operations::NumberRelation::from_operator(instruction.binary_operator()))
        .flatten()
}

/// A unary operator a region lowers on a Number.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Unary {
    Number(NumberUnary),
    BitwiseNot,
}

pub(super) fn unary(instruction: WideInstruction, operator: u32) -> Option<Unary> {
    if let Some(operation) = NumberUnary::from_js_instruction(instruction.op(), operator) {
        return Some(Unary::Number(operation));
    }
    (quench_runtime::staged::computes_unary(instruction.op()) && operator == UnaryOperator::BitwiseNot as u32)
        .then_some(Unary::BitwiseNot)
}

/// What each register of a region holds: a scalar in a representation, or an indexed array whose
/// elements are held in `elements`, and whether the region stores into it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Role {
    Scalar(Representation),
    Array { elements: Representation, written: bool },
}

impl Role {
    /// The role that holds both `self` and `other`, or `None` when no single role does. Only
    /// Float64 and Tagged elements join; a Tagged scalar widens an array, and an array joins a
    /// Tagged scalar's register as a Tagged array.
    pub(super) fn join(self, other: Role) -> Option<Role> {
        use Representation::{Float64, Tagged};
        let boxed = |elements: Representation| matches!(elements, Float64 | Tagged);
        match (self, other) {
            (Role::Scalar(Float64), Role::Scalar(Float64)) => Some(self),
            (Role::Scalar(Tagged), Role::Scalar(Tagged)) => Some(self),
            (Role::Array { elements: a, written }, Role::Array { elements: b, written: more })
                if boxed(a) && boxed(b) =>
            {
                Some(Role::Array { elements: a.max(b), written: written || more })
            }
            (Role::Array { elements, written }, Role::Scalar(Tagged)) if boxed(elements) => {
                Some(Role::Array { elements: Tagged, written })
            }
            (Role::Scalar(Tagged), Role::Array { written, .. }) => Some(Role::Array { elements: Tagged, written }),
            _ => None,
        }
    }
}
