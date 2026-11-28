//! A loop region's stores to fields and bindings: which store rows the region lowers, and how their values are admitted.

#[allow(unused_imports)] use quench_runtime::staged::{OperationOperationExt};
use super::*;
use std::collections::BTreeSet;
#[allow(unused_imports)] use quench_runtime::staged::glue_prelude::*;

/// Whether a loop region admits `instruction` as a named field store: every named store the
/// decoder makes (`SetField`, its `base` the receiver register or `None` for `this`) but a strict
/// store to `this`.
pub(super) fn admitted_store(instruction: WideInstruction) -> bool {
    matches!(crate::compiled::decoded_completed(instruction),
        Some(quench_runtime::staged::Operation::SetField { base, strict, .. }) if !(strict && base.is_none()))
}

/// Whether `instruction` stores to a binding, by its staged row (`Operation::StoreCapture`), or is a strict global reference.
pub(super) fn stores_binding(instruction: WideInstruction) -> bool {
    matches!(Operation::<(), ()>::decode(instruction, DecodeContext::EMPTY), Some(Operation::StoreCapture { .. } | Operation::GlobalWrite { initialize: false, .. }))
        || quench_runtime::staged::strict_reference_step(instruction)
}

/// A binding's store passes its value (`admit_stored`); a strict code's reference to a global (`ResolveName` then
/// `StoreResolvedName`, run as its call-out) has its base tagged, and its stored value (the first register read) passes so.
pub(super) fn admit_binding_store(admission: &mut Admission<'_>, instruction: WideInstruction, tagged: &BTreeSet<Register>,
    defined_registers: &BTreeSet<Register>) -> Option<()> {
    if !quench_runtime::staged::strict_reference_step(instruction) { return admit_stored(admission, instruction.register_a(), tagged, defined_registers); }
    (instruction.result_flags() == 0).then_some(())?;
    for register in instruction.written_registers() { admission.tagged(register)?; }
    let mut values = instruction.value_registers();
    if let Some(value) = values.next() { admit_stored(admission, value, tagged, defined_registers)?; }
    values.try_for_each(|base| admission.tagged(base))
}

/// A stored value's role: boxed where its register is tagged or holds an unchanged entry value
/// (any tagged value), else a number.
pub(super) fn admit_stored(admission: &mut Admission<'_>, register: Register, tagged: &BTreeSet<Register>,
    defined_registers: &BTreeSet<Register>) -> Option<()> {
    if tagged.contains(&register) || !defined_registers.contains(&register) { admission.tagged(register) }
    else { admission.number(register) }
}
