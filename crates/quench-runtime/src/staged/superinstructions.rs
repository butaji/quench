//! The superinstruction rows: a `SuperConstArrayObject2` record makes a constant array, runs two
//! `Binary` operations and makes an object pair. Its step is the composition of the rows its parts
//! would run on their own (`Allocation::ConstArray`, `Binary`, `Allocation::ObjectPair`), each through
//! the frame method that row uses. Its four parts are the program's superinstruction table entry the
//! record names, which the decode reads from the caller's program.

use super::*;

/// One `Binary` part of a pair: the register it defines, its two operands and its operator.
#[derive(Clone, Copy)]
pub(crate) struct BinaryPart {
    destination: Register,
    left: Operand,
    right: Operand,
    operator: u32,
}

impl BinaryPart {
    /// The `Binary` part `instruction` is, over register or constant operands; `None` for any other record.
    fn decode(instruction: WideInstruction) -> Option<Self> {
        if !binary(instruction.op()) { return None; }
        let (left, right) = (instruction.operand_b(), instruction.operand_c());
        if !(super::generic::is_value_operand(left) && super::generic::is_value_operand(right)) { return None; }
        Some(Self { destination: instruction.result_register(), left, right, operator: instruction.binary_operator() })
    }
}

/// A `SuperConstArrayObject2` record's parts, as its step runs them: the constant array (into its
/// part's register), the two binary operations, then the object pair, which defines the record's
/// own `destination`.
#[derive(Clone, Copy)]
pub(crate) struct ConstArrayPair<P> {
    array: Register,
    array_allocation: Allocation,
    first: BinaryPart,
    second: BinaryPart,
    object: Allocation,
    destination: Register,
    proof: P,
}

impl<P> ConstArrayPair<P> {
    /// The pair `instruction` (a `SuperConstArrayObject2` record) denotes, given its four parts `code`
    /// (the superinstruction table entry it names): a constant array, two `Binary` parts and an object
    /// pair, in that order. `None` for any other parts.
    pub(crate) fn decode(instruction: WideInstruction, code: [WideInstruction; 4], proof: P) -> Option<Self> {
        let [array, first, second, object] = code;
        if !makes_const_array(array) || !makes_object_pair(object) { return None; }
        let (array_register, array_allocation) = Allocation::decode(array, &[])?;
        let (_, object_allocation) = Allocation::decode(object, &[])?;
        Some(Self {
            array: array_register,
            array_allocation,
            first: BinaryPart::decode(first)?,
            second: BinaryPart::decode(second)?,
            object: object_allocation,
            destination: instruction.result_register(),
            proof,
        })
    }
}

/// Run a pair (`Operation::SuperConstArrayObject2`): each part's row in order, stopping at the first
/// part the frame does not answer (`Step::Unresolved`), which the record's miss then runs from its start.
/// Nothing after that part has run, and the constant array is a fresh allocation the miss repeats.
#[inline(always)]
pub(super) fn step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
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
