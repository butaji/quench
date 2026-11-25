//! The typed edge out of a compiled unit (TOD-2195): the fact its leave was missing, and where.

use crate::bytecode::WideInstruction;

/// The fact a compiled unit's leave was missing: the only reason code leaves a compiled unit for
/// the next lower tier (`FactMissing`). Classed for now by the leaving instruction; a guard that
/// names its own fact refines it (TOD-2195 step 4c).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ExitFact {
    /// An operand's number kind (int32, double) or primitive kind: arithmetic, compares, branches.
    NumberKind,
    /// A receiver's shape or layout: a named field read or store.
    Shape,
    /// A call's callee identity or admission: a call, a method call or a construction.
    Callee,
    /// An element access's array kind, bounds or holes.
    Element,
    /// A global binding's cell or the realm's globals.
    Global,
    /// Anything the leaving instruction does not class.
    Unclassified,
}

impl ExitFact {
    pub const NAMES: [&'static str; 6] = ["number-kind", "shape", "callee", "element", "global", "unclassified"];

    /// The fact a leave at `instruction` was missing, by the instruction's meaning.
    pub fn of(instruction: &WideInstruction) -> Self {
        use crate::staged::meaning::{means, Meaning as M};
        let any = |meanings: &[M]| meanings.iter().any(|m| means(instruction, *m));
        if any(&[M::Call, M::Construct, M::MethodCall]) {
            Self::Callee
        } else if any(&[M::NamedFieldAccess]) {
            Self::Shape
        } else if any(&[M::Binary, M::Unary, M::IncDec, M::ConditionalJump, M::NumericBinary, M::NumericUnary, M::NumericArithmetic]) {
            Self::NumberKind
        } else {
            Self::Unclassified
        }
    }
}

/// The typed edge out of a compiled unit: the fact that was missing and the site (`ExitSite::id`)
/// it left at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FactMissing {
    pub fact: ExitFact,
    pub site: u32,
}
