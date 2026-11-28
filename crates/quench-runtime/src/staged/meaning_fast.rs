//! Opcode meanings the dispatch tiers ask of a record, where no one decoded variant covers the
//! tier's test exactly. Each classification reads the record's decoded operation (`Operation::decode`,
//! the shared staged semantics) or a catalog trait, never the opcode's name.

#[cfg(not(stencil_build))]
#[allow(unused_imports)]
use crate::staged::allocation::AllocationAllocationExt;
#[cfg(not(stencil_build))]
#[allow(unused_imports)] use crate::staged::{OperationOperationExt};
use crate::bytecode::{Register, Traits, WideInstruction};
use super::protocol_literals::LiteralStep;
use super::protocol_owned::OwnedVarStep;
use super::protocol_values::ValueStep;
use super::{DecodeContext, Operation, Protocol};

/// The staged operation of `instruction`, with no constants or sites resolved (`Operation::decode`).
#[inline(always)]
fn decoded(instruction: WideInstruction) -> Option<Operation<(), ()>> {
    Operation::decode(instruction, DecodeContext::EMPTY)
}

/// The index of the function a closure record makes (`MakeClosure`).
#[inline(always)]
pub(crate) fn closure_function(instruction: WideInstruction) -> Option<u32> {
    // A closure record decodes under its allocation's proof (`Decoder::Literal`), not as an operation.
    match super::Allocation::decode(instruction, &[])? {
        (_, super::Allocation::Closure { function }) => Some(function),
        _ => None,
    }
}

/// Whether `instruction` copies the running frame's environment (`CloneEnv`).
#[inline(always)]
pub(crate) fn clone_env(instruction: WideInstruction) -> bool {
    matches!(decoded(instruction), Some(Operation::Protocol { step: Protocol::Value(ValueStep::CloneEnv { .. }), .. }))
}

/// The activation-variable slot of a local load, a local store or a TDZ reset (`LoadLocal`,
/// `StoreLocal`, `InitializeTdz`).
#[inline(always)]
pub(crate) fn owned_local_slot(instruction: WideInstruction) -> Option<usize> {
    match decoded(instruction)? {
        Operation::Protocol {
            step: Protocol::Owned(OwnedVarStep::LoadLocal { slot } | OwnedVarStep::StoreLocal { slot, .. }
                | OwnedVarStep::InitializeTdz { slot }),
            ..
        } => Some(slot),
        _ => None,
    }
}

/// The slot of a store to a `var` binding (`StoreVarBinding`).
#[inline(always)]
pub(crate) fn var_binding_store_slot(instruction: WideInstruction) -> Option<usize> {
    match decoded(instruction)? {
        Operation::Protocol { step: Protocol::Owned(OwnedVarStep::StoreVarBinding { slot }), .. } => Some(slot),
        _ => None,
    }
}

/// Whether `instruction` resets a local to its TDZ (`InitializeTdz`).
#[inline(always)]
pub(crate) fn initialize_tdz(instruction: WideInstruction) -> bool {
    matches!(decoded(instruction), Some(Operation::Protocol { step: Protocol::Owned(OwnedVarStep::InitializeTdz { .. }), .. }))
}

/// Whether `instruction` reads an element of the `arguments` binding (`ArgumentGet`).
#[inline(always)]
pub(crate) fn argument_get(instruction: WideInstruction) -> bool {
    matches!(decoded(instruction), Some(Operation::Protocol { step: Protocol::Owned(OwnedVarStep::ArgumentGet { .. }), .. }))
}

/// Whether `instruction` reads a local binding whole (`LoadLocal`) or an element of `arguments`
/// (`ArgumentGet`): the two owned reads a tier forwards from an arguments slot.
#[inline(always)]
pub(crate) fn local_load_or_argument_get(instruction: WideInstruction) -> bool {
    matches!(decoded(instruction), Some(Operation::Protocol {
        step: Protocol::Owned(OwnedVarStep::LoadLocal { .. } | OwnedVarStep::ArgumentGet { .. }), ..
    }))
}

/// The register an iterator result check reads (`RequireIteratorResult`, its second operand).
#[inline(always)]
pub fn require_iterator_result_operand(instruction: WideInstruction) -> Option<Register> {
    match decoded(instruction)? {
        // The record decodes to its own row; its protocol form names the register as its second operand.
        Operation::RequireIteratorResult { register } => Some(register),
        Operation::Protocol { step: Protocol::Value(ValueStep::RequireIteratorResult), operands, .. } => operands[1],
        _ => None,
    }
}

/// Whether `instruction` selects among a run of string constants (`StringCaseIndex`) or builds an
/// array of constants (`MakeConstArray`): the two records whose immediate names a constant run.
#[inline(always)]
pub(crate) fn case_or_constant_array(instruction: WideInstruction) -> bool {
    matches!(decoded(instruction), Some(Operation::Protocol {
        step: Protocol::Value(ValueStep::StringCaseIndex { .. }) | Protocol::Literal(LiteralStep::ConstArray { .. }), ..
    }))
}

/// Whether `instruction` is a literal that returns its object from the frame, by the catalog's
/// returning traits (`RETURNING_OBJECT`, `RETURNING_OBJECT_LITERAL`, `RETURNING_LITERAL`).
#[inline(always)]
pub(crate) fn returning_literal(instruction: WideInstruction) -> bool {
    let traits = instruction.op().traits();
    traits.has(Traits::RETURNING_OBJECT) || traits.has(Traits::RETURNING_OBJECT_LITERAL) || traits.has(Traits::RETURNING_LITERAL)
}
