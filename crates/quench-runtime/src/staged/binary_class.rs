//! The class of a `Binary` operator as the Number operations name it (`vm::operations`): the arithmetic, the
//! bitwise, the remainder and the relation operators, and the rest (`in`, `instanceof`, string or BigInt operators).
//! A tier that makes a Number row reads its operator's class here; the staged `Operation::Binary` keeps the raw operator
//! for the VM's callout.

use crate::vm::operations::{NumberArithmetic, NumberBitwise, NumberRelation};

/// The class of one `Binary` operator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinaryClass {
    Arithmetic(NumberArithmetic),
    Bitwise(NumberBitwise),
    /// `%`, the staged Number remainder.
    Remainder,
    /// A relation as a value (`<`, `==`, ...): its own staged row, not an arithmetic one.
    Relation(NumberRelation),
    /// An operator no Number operation names.
    Other,
}

impl BinaryClass {
    /// The class of the binary operator `operator` (the `BinaryOperator` discriminant).
    pub(crate) fn of_operator(operator: u32) -> Self {
        if let Some(operation) = NumberArithmetic::from_operator(operator) {
            Self::Arithmetic(operation)
        } else if operator == crate::vm::operations::binary_operator::Remainder {
            Self::Remainder
        } else if let Some(operation) = NumberBitwise::from_operator(operator) {
            Self::Bitwise(operation)
        } else if let Some(relation) = NumberRelation::from_operator(operator) {
            Self::Relation(relation)
        } else {
            Self::Other
        }
    }
}
