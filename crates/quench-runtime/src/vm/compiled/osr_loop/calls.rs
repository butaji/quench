//! How the OSR inliner re-reads a call's callee when the loop writes its receiver.
use super::*;

/// How a call's callee is read again from a receiver the loop writes.
pub(super) enum Reroot {
    /// The receiver keeps its register; the callee read is its entry snapshot.
    None,
    /// The callee is read through the receiver's own field path (`this.m.am(…)`), and native
    /// code checks that read each iteration.
    Splice(FieldPath),
    /// The receiver's own field is read through another held path; `receiver` holds it now.
    Rerooted { path: FieldPath, receiver: Register },
    /// The receiver's own read at `own` takes a fresh register, holding its field's value.
    Fresh { path: FieldPath, own: usize },
}

impl Reroot {
    /// Applies this decision: the callee's path, the receiver register and where the setup
    /// resumes; `fresh` allocates a `Fresh` decision's register and rewrites its read.
    pub(super) fn apply(
        self,
        callee_path: &mut Option<FieldPath>,
        receiver: &mut Register,
        resume_offset: &mut usize,
        fresh: impl FnOnce(usize) -> Option<Register>,
    ) -> Option<bool> {
        Some(match self {
            Self::None => false,
            Self::Splice(path) => { *callee_path = Some(path); true }
            Self::Rerooted { path, receiver: held } => { *callee_path = Some(path); *receiver = held; false }
            Self::Fresh { path, own } => {
                *receiver = fresh(own)?;
                *callee_path = Some(path);
                *resume_offset = own;
                false
            }
        })
    }
}

/// Reroots the callee read of a call whose receiver register `receiver` the loop writes: the
/// receiver's own read is spliced into the callee's path, or given a register of its own, or,
/// with no field path to splice, the callee's read stands at its PC (`Reroot::Dynamic`).
#[allow(clippy::too_many_arguments)]
pub(super) fn reroot_receiver(
    caller: &[WideInstruction],
    pc: u32,
    offset: usize,
    read_offset: usize,
    receiver: Register,
    sites: &[crate::bytecode::FieldSite],
    stored_name: &dyn Fn(crate::bytecode::Atom) -> bool,
    callee_path: Option<&FieldPath>,
    captured: Option<&FieldPath>,
    defines: &dyn Fn(Register) -> bool,
    mask: &dyn Fn(u32) -> Option<RegisterSet>,
) -> Option<Reroot> {
    if defines(receiver) && receivers::stored(caller, read_offset, receiver, sites, stored_name) {
        let own = read_offset.checked_sub(1)?;
        let receiver_path = receivers::field_path_unrooted(caller[own], sites)?;
        let path = callee_path.filter(|path| path.root == FieldReceiver::Register(receiver))?;
        return Some(Reroot::Splice(splice(receiver_path, path)));
    }
    if let Some((root, held)) = receivers::rerooted_with(caller, read_offset, receiver, defines, callee_path, captured) {
        let path = FieldPath { root, ..callee_path?.clone() };
        return Some(Reroot::Rerooted { path, receiver: held });
    }
    if !defines(receiver) {
        return Some(Reroot::None);
    }
    let path = callee_path?;
    let own = read_offset.checked_sub(1)?;
    receivers::field_path_unrooted(caller[own], sites)?;
    Some(Reroot::Fresh { path: fresh_receiver(caller, pc, offset, read_offset, receiver, sites, path, mask)?, own })
}

/// The receiver's own read that takes a fresh register: its field's value, when the loop
/// defines the receiver register and no instruction between that read and the call rewrites it.
#[allow(clippy::too_many_arguments)]
fn fresh_receiver(
    caller: &[WideInstruction],
    pc: u32,
    offset: usize,
    read_offset: usize,
    receiver: Register,
    sites: &[crate::bytecode::FieldSite],
    path: &FieldPath,
    mask: &dyn Fn(u32) -> Option<RegisterSet>,
) -> Option<FieldPath> {
    let own = read_offset.checked_sub(1)?;
    let receiver_path = receivers::field_path_unrooted(caller[own], sites)?;
    (caller[own].defined_register() == Some(receiver)
        && receiver_path.root != FieldReceiver::Register(receiver)
        && path.root == FieldReceiver::Register(receiver)
        && path.steps.len() == 1
        && caller[read_offset + 1..offset].iter().all(|instruction| !mentions(*instruction, receiver, sites))
        && !crate::compile::liveness::register_in(mask(pc + 1)?, usize::from(receiver)))
    .then_some(())?;
    Some(splice(receiver_path, path))
}

/// `receiver_path`'s steps before `callee_path`'s: the callee read through the receiver's own
/// field (`this.m.am`).
fn splice(mut receiver_path: FieldPath, callee_path: &FieldPath) -> FieldPath {
    receiver_path.steps.extend(callee_path.steps.iter().copied());
    receiver_path
}
