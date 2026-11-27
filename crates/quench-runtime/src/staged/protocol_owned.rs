//! The activation-variable (local, var binding, TDZ, argument) opcodes as staged call-out rows (`Operation::Protocol`): each is one
//! definition every tier runs, and its step is the interpreter's own helper over the running
//! activation (`Vm::owner_step`), which the host makes (`Vm::call_out` -> `Vm::owned_var_step`).
//! The decode of a step from its record is in `protocol_owned/glue.rs`.
use crate::bytecode::Register;

/// An activation-variable (local, var binding, TDZ, argument) step the host makes by the interpreter's own definition.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OwnedVarStep {
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
mod glue;
#[cfg(not(stencil_build))]
pub use glue::*;
