//! The activation-variable (local, var binding, TDZ, argument) opcodes as staged call-out rows (`Operation::Protocol`): each is one
//! definition every tier runs, and its step is the interpreter's own helper over the running
//! activation (`Vm::owner_step`), which the host makes (`Vm::call_out` -> `Vm::owned_var_step`).
use super::Protocol;
use crate::bytecode::Register;
#[cfg(not(stencil_build))]
use crate::bytecode::{InstructionField, Op, WideInstruction};

/// An activation-variable (local, var binding, TDZ, argument) step the host makes by the interpreter's own definition.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum OwnedVarStep {
    /// The binding of local `slot`, defined into the result register (`LoadLocal`).
    LoadLocal { slot: usize },
    /// Store the first operand to the `var` binding of local `slot` (`StoreVarBinding`); defines nothing.
    StoreVarBinding { slot: usize },
    /// Store the first operand to local `slot` under the function-name, initialization and constant
    /// rules, defining it into the optional register (`StoreLocal`).
    StoreLocal { slot: usize, initialize: Option<bool> },
    /// Reset local `slot` to its TDZ (`InitializeTdz`); defines nothing.
    InitializeTdz { slot: usize },
    /// The element of the `arguments` binding of local `slot` at the second operand's key (`ArgumentGet`).
    ArgumentGet { slot: usize },
}

#[cfg(not(stencil_build))]
/// The step `instruction` denotes among these opcodes, with the register it defines and its (at
/// most two) operand registers, which `Vm::call_out` reads; `None` for any other opcode.
#[inline(always)]
pub(super) fn decode(instruction: WideInstruction) -> Option<(Protocol, Option<Register>, [Option<Register>; 2])> {
    let (step, destination, operands) = match instruction.op() {
        Op::LoadLocal => (
            OwnedVarStep::LoadLocal { slot: instruction.local_slot() },
            Some(instruction.result_register()),
            [None, None],
        ),
        Op::StoreVarBinding => (
            OwnedVarStep::StoreVarBinding { slot: instruction.local_slot() },
            None,
            [Some(instruction.register_a()), None],
        ),
        Op::StoreLocal => (
            OwnedVarStep::StoreLocal {
                slot: instruction.local_slot(),
                initialize: instruction.boolean_field(InstructionField::C),
            },
            instruction.optional_register_b(),
            [Some(instruction.register_a()), None],
        ),
        Op::InitializeTdz => (OwnedVarStep::InitializeTdz { slot: instruction.local_slot() }, None, [None, None]),
        Op::ArgumentGet => (
            OwnedVarStep::ArgumentGet { slot: instruction.local_slot() },
            Some(instruction.result_register()),
            [None, Some(instruction.register_b())],
        ),
        _ => return None,
    };
    Some((Protocol::Owned(step), destination, operands))
}
