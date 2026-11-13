//! Small questions the region analysis asks of an instruction or an op: a register's
//! definitions, an indexed access's array, an admitted store, an instruction's registers, and
//! a call setup's purity.

use super::*;

/// Whether `register` has definitions in `instructions` and every one is a field read, which
/// may give an object.
pub(super) fn field_reads_alone_define(instructions: &[WideInstruction], register: Register) -> bool {
    let mut definitions = instructions.iter().filter(|candidate| candidate.defined_register() == Some(register)).peekable();
    definitions.peek().is_some() && definitions.all(|definition| definition.op().is_get_field())
}

/// The register an indexed access, or the coercibility check before one, reads as its array.
pub(super) fn array_read(instruction: WideInstruction) -> Option<Register> {
    match instruction.op().class() {
        Class::GetIndex => instruction.operand_b().register_index(),
        Class::SetIndex | Class::RequireObjectCoercible => Some(instruction.register_b()),
        _ => None,
    }
}

/// Whether a loop region admits `op` as a named field store: every one the opcode table
/// names (`Op::field_store`) but a strict store to `this`.
pub(super) fn admitted_store(op: Op) -> bool {
    op.field_store().is_some_and(|store| !(store.strict && store.receiver == crate::bytecode::StoreReceiver::This))
}

/// Whether `instruction` names `register` (a read or a write); an instruction `renamed`
/// cannot rename names every register.
pub(super) fn mentions(instruction: WideInstruction, register: Register, sites: &[FieldSite]) -> bool {
    // Only a GetField carries a field lookup immediate, so the op is checked first.
    if instruction.op().is_get_field() && matches!(instruction.field_lookup(), Some(FieldLookup::Site(_))) {
        return instruction.result_register() == register
            || field_base(instruction, sites).is_none_or(|base| base.register_index() == Some(register));
    }
    let named = std::cell::Cell::new(false);
    let rename = |name: Register| {
        named.set(named.get() || name == register);
        name
    };
    renamed(instruction, &rename, register, &|target| Some(target)).is_none() || named.get()
}

/// The ops a call's argument setup may run between reading its callee and the call: none
/// has an effect a second run could repeat, so leaving among them resumes at the read.
pub(super) fn pure_setup(op: Op) -> bool {
    op.traits().has(crate::bytecode::Traits::PURE_SETUP)
}

/// Whether a region calls: a call may write a field a hold would assume, so a call-bearing
/// region reads every field through its cache instead of holding it.
pub(super) fn call_bearing(instructions: &[WideInstruction]) -> bool {
    instructions.iter().any(|instruction| instruction.op().is_ordinary_call())
}

/// Whether a region may hold `path`'s value: no call writes a field under it, and no store
/// names one of its steps.
pub(super) fn holds(
    instructions: &[WideInstruction],
    path: &FieldPath,
    stored_names: &[crate::bytecode::Atom],
) -> bool {
    !call_bearing(instructions) && path.steps.iter().all(|(atom, _)| !stored_names.contains(atom))
}

/// The names `instructions` store into.
pub(super) fn stored_names(instructions: &[WideInstruction]) -> Vec<crate::bytecode::Atom> {
    instructions.iter()
        .filter(|instruction| admitted_store(instruction.op()))
        .map(|instruction| instruction.atom_index())
        .collect()
}
