//! The receivers of a loop's inlined calls the loop itself writes, read right before the
//! callee's read (`inline_region`).

#[allow(unused_imports)]
use crate::ext_traits::*;
use std::collections::BTreeMap;
use super::{field_path, FieldPath, FieldReceiver, FieldSite, Register, RegisterSet, WideInstruction};
#[allow(unused_imports)] use quench_runtime::staged::glue_prelude::*;

/// Whether the receiver is read through a name the loop stores, so it changes in the loop:
/// the callee's read stays, and native code checks the callee after it (`InlineGuard::AtRead`).
pub(crate) fn stored(caller: &[WideInstruction], read: usize, receiver: Register, sites: &[FieldSite],
    stored: &dyn Fn(quench_runtime::bytecode::Atom) -> bool) -> bool {
    read.checked_sub(1).is_some_and(|own| caller[own].defined_register() == Some(receiver)
        && field_path(caller[own], sites, &BTreeMap::new())
            .is_some_and(|path| path.steps.iter().any(|(atom, _)| stored(*atom))))
}

/// The root a receiver the loop writes reads as, and the register holding it there: the
/// capture it holds (`captured`), or the register it is copied from right before the callee's
/// read when the loop never writes that one (`p.m()` with `p` held outside the loop).
pub(crate) fn rerooted(caller: &[WideInstruction], read: usize, receiver: Register, defines: &dyn Fn(Register) -> bool,
    callee: Option<&FieldPath>, captured: &BTreeMap<Register, FieldPath>) -> Option<(FieldReceiver, Register)> {
    callee.is_some_and(|path| path.root == FieldReceiver::Register(receiver)).then_some(())?;
    if let Some(path) = captured.get(&receiver) {
        return Some((path.root, receiver));
    }
    let copy = caller[read.checked_sub(1)?];
    (matches!(crate::compiled::decoded(copy), Some(quench_runtime::staged::Operation::Move { .. }))
        && copy.defined_register() == Some(receiver) && !defines(copy.register_b()))
        .then(|| (FieldReceiver::Register(copy.register_b()), copy.register_b()))
}

/// The registers whose every definition in `instructions` reads one depth-0 capture slot,
/// none live at the header: each holds that slot's value at every read, rooted at the slot
/// (`FieldReceiver::Capture`). A region stores no capture, so the value is its entry's.
pub(crate) fn captured(instructions: &[WideInstruction], header_live: RegisterSet) -> BTreeMap<Register, FieldPath> {
    let mut slots = BTreeMap::<Register, Option<u16>>::new();
    for instruction in instructions {
        let Some(register) = instruction.defined_register() else { continue };
        let captures = matches!(crate::compiled::decoded(instruction),
            Some(quench_runtime::staged::Operation::LoadCapture { .. } | quench_runtime::staged::Operation::StoreCapture { .. }));
        let slot = (captures && instruction.capture_depth() == 0 && instruction.result_flags() == 0)
            .then(|| instruction.capture_slot());
        slots.entry(register).and_modify(|held| if *held != slot { *held = None }).or_insert(slot);
    }
    slots.into_iter()
        .filter(|(register, _)| !quench_runtime::compile::liveness::register_in(header_live, usize::from(*register)))
        .filter_map(|(register, slot)| Some((register, FieldPath { root: FieldReceiver::Capture(slot?), steps: Vec::new() })))
        .collect()
}

#[quench_macros::ext(FieldReceiverOps1)]
impl FieldReceiver {
}
