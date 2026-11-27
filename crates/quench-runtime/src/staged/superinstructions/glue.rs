//! Runtime glue of `superinstructions`: the decode of a superinstruction's parts from the
//! instructions a record names. The moved types take these as extension traits, since an
//! inherent impl of a moved type cannot live in the runtime crate.
#![allow(unused_imports)]
use super::*;
use crate::bytecode::{Register, WideInstruction};
use crate::staged::generic::is_value_operand;
use crate::staged::{Allocation, binary, makes_const_array, makes_object_pair};

/// The decode of a `BinaryPart` from one `Binary` instruction over register or constant operands.
pub trait BinaryPartSuperinstructionsExt: Sized {
    /// The `Binary` part `instruction` is, over register or constant operands; `None` for any other record.
    fn decode(instruction: WideInstruction) -> Option<Self>;
}

impl BinaryPartSuperinstructionsExt for BinaryPart {
    fn decode(instruction: WideInstruction) -> Option<Self> {
        if !binary(instruction.op()) { return None; }
        let (left, right) = (instruction.operand_b(), instruction.operand_c());
        if !(is_value_operand(left) && is_value_operand(right)) { return None; }
        Some(Self { destination: instruction.result_register(), left, right, operator: instruction.binary_operator() })
    }
}

/// The decode of a `ConstArrayPair` from a `SuperConstArrayObject2` record and its four parts.
pub trait ConstArrayPairSuperinstructionsExt<P>: Sized {
    /// The pair `instruction` (a `SuperConstArrayObject2` record) denotes, given its four parts `code`
    /// (the superinstruction table entry it names): a constant array, two `Binary` parts and an object
    /// pair, in that order. `None` for any other parts.
    fn decode(instruction: WideInstruction, code: [WideInstruction; 4], proof: P) -> Option<Self>;
}

impl<P> ConstArrayPairSuperinstructionsExt<P> for ConstArrayPair<P> {
    fn decode(instruction: WideInstruction, code: [WideInstruction; 4], proof: P) -> Option<Self> {
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
