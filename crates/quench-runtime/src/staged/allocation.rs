//! The allocation rows whose operands the instruction alone fixes: an object literal over a
//! register window, a closure, a literal template and a string concatenation. Each makes a
//! heap value, so the frame's heap decides how (`Frame::allocate_value`): the interpreter
//! allocates in place, a specializer calls out to an allocator that leaves for a due
//! collection first.

use super::Operation;
use crate::bytecode::{Op, Register, RegisterWindow, WideInstruction};

/// What an allocation row makes. Every operand is static: the registers it reads are named by
/// a window, never passed as words, so each tier reads them from its own register homes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Allocation {
    /// An ordinary object of literal `site`'s shape holding the registers of `window`.
    ObjectLiteral { site: u32, window: RegisterWindow },
    /// The closure of function `function` over the activation's environment, which the first
    /// closure promotes (`Vm::promote_frame_environment`).
    Closure { function: u32 },
    /// The value literal template `template` builds from its steps.
    Template { template: u32 },
    /// ToString of each register of `window`, concatenated in order.
    Concat { window: RegisterWindow },
}

impl Allocation {
    /// The register `instruction` defines and the allocation it makes; `None` for an opcode
    /// that makes no such value.
    pub(crate) fn decode(instruction: WideInstruction) -> Option<(Register, Self)> {
        let allocation = match instruction.op() {
            Op::MakeObjectLiteral => Self::ObjectLiteral {
                site: u32::try_from(instruction.object_site_index()).ok()?, window: instruction.register_window() },
            Op::MakeClosure => Self::Closure { function: instruction.closure_function_index() },
            Op::MakeLiteralTemplate => Self::Template {
                template: u32::try_from(instruction.literal_template_index()).ok()? },
            Op::StringConcat => Self::Concat { window: instruction.register_window() },
            _ => return None,
        };
        Some((instruction.result_register(), allocation))
    }
}

impl<W, P> Operation<W, P> {
    /// The allocation operation `instruction` denotes under `proof`, the one the frame's heap
    /// proves (an exit for a due collection). The array and pair literals carry word operands
    /// a tier reads itself, so it builds those.
    pub(crate) fn decode_allocation(instruction: WideInstruction, proof: P) -> Option<Self> {
        if instruction.op() == Op::MakeObject {
            return Some(Self::MakeObject { destination: instruction.result_register(), proof });
        }
        let (destination, allocation) = Allocation::decode(instruction)?;
        Some(Self::Allocate { destination, allocation, proof })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each allocation opcode decodes to its row with the operands the catalog fixes, and
    /// no other opcode decodes.
    #[test]
    fn allocation_opcodes_decode_to_their_rows() {
        let literal = WideInstruction::new(Op::MakeObjectLiteral, 4, 1, 3, 9);
        assert_eq!(Allocation::decode(literal), Some((4, Allocation::ObjectLiteral {
            site: 9, window: RegisterWindow { base: 1, count: 3 } })));
        let closure = WideInstruction::new(Op::MakeClosure, 2, 0, 0, 7);
        assert_eq!(Allocation::decode(closure), Some((2, Allocation::Closure { function: 7 })));
        let template = WideInstruction::new(Op::MakeLiteralTemplate, 5, 0, 0, 3);
        assert_eq!(Allocation::decode(template), Some((5, Allocation::Template { template: 3 })));
        let concat = WideInstruction::new(Op::StringConcat, 6, 2, 2, 0);
        assert_eq!(Allocation::decode(concat), Some((6, Allocation::Concat {
            window: RegisterWindow { base: 2, count: 2 } })));
        assert_eq!(Allocation::decode(WideInstruction::new(Op::Move, 1, 2, 0, 0)), None);
    }

    #[test]
    fn an_empty_object_decodes_to_its_own_row_and_the_rest_to_allocate() {
        let make = |op| Operation::<u64, ()>::decode_allocation(WideInstruction::new(op, 3, 1, 1, 0), ());
        assert!(matches!(make(Op::MakeObject), Some(Operation::MakeObject { destination: 3, .. })));
        assert!(matches!(make(Op::StringConcat), Some(Operation::Allocate { destination: 3, .. })));
        assert!(make(Op::Jump).is_none());
    }
}
