//! The receivers of a loop's inlined calls the loop itself writes, read right before the
//! callee's read (`inline_region`).

use super::{field_path, FieldPath, FieldReceiver, FieldSite, Register, WideInstruction};

/// Whether the receiver is read through a name the loop stores, so it changes in the loop:
/// the callee's read stays, and native code checks the callee after it (`InlineGuard::AtRead`).
pub(super) fn stored(caller: &[WideInstruction], read: usize, receiver: Register, sites: &[FieldSite],
    stored: &dyn Fn(crate::bytecode::Atom) -> bool) -> bool {
    read.checked_sub(1).is_some_and(|own| caller[own].defined_register() == Some(receiver)
        && field_path(caller[own], sites, &std::collections::BTreeMap::new())
            .is_some_and(|path| path.steps.iter().any(|(atom, _)| stored(*atom))))
}

/// The register a receiver is copied from right before the callee's read, when the loop
/// never writes it: the receiver is that register (`p.m()` with `p` held outside the loop).
pub(super) fn copied(caller: &[WideInstruction], read: usize, receiver: Register, defines: &dyn Fn(Register) -> bool,
    callee: Option<&FieldPath>) -> Option<Register> {
    let copy = caller[read.checked_sub(1)?];
    (copy.op().copies_register() && copy.defined_register() == Some(receiver) && !defines(copy.register_b())
        && callee.is_some_and(|path| path.root == FieldReceiver::Register(receiver)))
    .then(|| copy.register_b())
}
