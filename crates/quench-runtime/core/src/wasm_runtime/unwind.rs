//! One unwind walk; catch rules stay distinct.
//!
//! Traps, tagged Native exceptions, and Dynamic throws share the frame walk.
//! Matching is data: a trap does not match `try_table`; a tagged throw does;
//! a Dynamic throw matches Dynamic `catch`. Conversion to Dynamic happens only
//! at a layer boundary.

use crate::dynamic::Dynamic;

/// Native instruction failure. Not a tagged exception and not a Dynamic throw.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Trap {
    IntegerDivideByZero,
    IntegerOverflow,
    Unreachable,
    OutOfBoundsMemory,
    OutOfBoundsTable,
    UninitializedElement,
    UndefinedElement,
    IndirectCallMismatch,
    InvalidConversion,
    CallStackExhausted,
    NullReference,
    NullI31,
    CastFailure,
    OutOfBounds,
    NullArray,
    NullStruct,
    NullExn,
    NullFunc,
    NullDescriptor,
    DescriptorCast,
    UnalignedAtomic,
    ExpectedShared,
    Unimplemented,
}

impl Trap {
    pub fn message(self) -> &'static str {
        match self {
            Self::IntegerDivideByZero => "integer divide by zero",
            Self::IntegerOverflow => "integer overflow",
            Self::Unreachable => "unreachable",
            Self::OutOfBoundsMemory => "out of bounds memory access",
            Self::OutOfBoundsTable => "out of bounds table access",
            Self::UninitializedElement => "uninitialized element",
            Self::UndefinedElement => "undefined element",
            Self::IndirectCallMismatch => "indirect call type mismatch",
            Self::InvalidConversion => "invalid conversion to integer",
            Self::CallStackExhausted => "call stack exhausted",
            Self::NullReference => "null reference",
            Self::NullI31 => "null i31 reference",
            Self::CastFailure => "cast failure",
            Self::OutOfBounds => "out of bounds",
            Self::NullArray => "null array reference",
            Self::NullStruct => "null structure reference",
            Self::NullExn => "null exception reference",
            Self::NullFunc => "null function reference",
            Self::NullDescriptor => "null descriptor reference",
            Self::DescriptorCast => "descriptor cast failure",
            Self::UnalignedAtomic => "unaligned atomic",
            Self::ExpectedShared => "expected shared memory",
            Self::Unimplemented => "unimplemented",
        }
    }
}

/// Which catch sites may bind this failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CatchRule {
    /// Never matches Native `try_table` or Dynamic `catch`.
    Trap,
    /// Matches Native `try_table` by tag.
    Exception,
    /// Matches Dynamic `catch`.
    Throw,
}

/// Shared unwind payload. The walk is one; the rule is this tag.
#[derive(Clone, Debug, PartialEq)]
pub enum Failure {
    Trap(Trap),
    Exception {
        tag: u32,
        args: Vec<crate::slot::Slot>,
    },
    Throw(Dynamic),
}

impl Failure {
    pub fn catch_rule(&self) -> CatchRule {
        match self {
            Self::Trap(_) => CatchRule::Trap,
            Self::Exception { .. } => CatchRule::Exception,
            Self::Throw(_) => CatchRule::Throw,
        }
    }

    pub fn message(&self) -> &'static str {
        match self {
            Self::Trap(trap) => trap.message(),
            Self::Exception { .. } => "exception",
            Self::Throw(_) => "throw",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CatchRule, Failure, Trap};

    #[test]
    fn catch_rules_are_distinct() {
        assert_eq!(
            Failure::Trap(Trap::Unreachable).catch_rule(),
            CatchRule::Trap
        );
        assert_eq!(
            Failure::Exception {
                tag: 0,
                args: Vec::new(),
            }
            .catch_rule(),
            CatchRule::Exception
        );
    }
}
