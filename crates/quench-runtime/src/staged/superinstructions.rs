//! The superinstruction rows: a `SuperConstArrayObject2` record makes a constant array, runs two
//! `Binary` operations and makes an object pair. Its step is the composition of the rows its parts
//! would run on their own (`Allocation::ConstArray`, `Binary`, `Allocation::ObjectPair`), each through
//! the frame method that row uses. Its four parts are the program's superinstruction table entry the
//! record names, which the decode reads from the caller's program (`superinstructions/glue.rs`).

use super::*;

/// One `Binary` part of a pair: the register it defines, its two operands and its operator.
#[derive(Clone, Copy)]
pub struct BinaryPart {
    pub destination: Register,
    pub left: Operand,
    pub right: Operand,
    pub operator: u32,
}

/// A `SuperConstArrayObject2` record's parts, as its step runs them: the constant array (into its
/// part's register), the two binary operations, then the object pair, which defines the record's
/// own `destination`.
#[derive(Clone, Copy)]
pub struct ConstArrayPair<P> {
    pub array: Register,
    pub array_allocation: Allocation,
    pub first: BinaryPart,
    pub second: BinaryPart,
    pub object: Allocation,
    pub destination: Register,
    pub proof: P,
}

/// Run a pair (`Operation::SuperConstArrayObject2`): each part's row in order, stopping at the first
/// part the frame does not answer (`Step::Unresolved`), which the record's miss then runs from its start.
/// Nothing after that part has run, and the constant array is a fresh allocation the miss repeats.
#[inline(always)]
pub fn step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    pair: ConstArrayPair<S::Proof>) -> Step<S::Word, F::Control> {
    let ConstArrayPair { array, array_allocation, first, second, object, destination, proof } = pair;
    match super::heap::heap_step(stage, frame, Operation::Allocate { destination: array, allocation: array_allocation, proof }) {
        Step::Value(_) => {}
        other => return other,
    }
    for part in [first, second] {
        let operation = Operation::Binary { destination: part.destination, left: part.left, right: part.right, operator: part.operator };
        match super::generic::step(stage, frame, operation) {
            Step::Value(_) => {}
            other => return other,
        }
    }
    super::heap::heap_step(stage, frame, Operation::Allocate { destination, allocation: object, proof })
}

#[cfg(not(stencil_build))]
mod glue;
#[cfg(not(stencil_build))]
pub use glue::*;
