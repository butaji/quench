//! The entry protocol: how a driver hands a body-ABI entry its arguments and reads its result,
//! written once.
//!
//! Every driver of a body-ABI entry (a lane body, an OSR loop region, a bare stencil region, a
//! numeric image, and later the ahead image) builds an `EntryDescriptor` from the arguments it
//! already holds, and the protocol enters it: `enter` runs it through the body ABI's trampoline
//! (`enter_body`), and `exit` reads the result with `LaneExit::read`, the one exit decoder. A
//! numeric entry keeps its own status-0 convention, which is not a leave protocol, so its result
//! is returned as the body wrote it (`numeric_result`).

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::lane_body_abi::enter_body;
use super::*;

/// Which driver built an entry descriptor, which says what its result means.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EntryKind {
    /// A lane body, run from its entry cell or its call site (`run_lane_body`).
    Body,
    /// An OSR loop region, entered at the header of the loop it runs (`osr_lane::enter`).
    Region,
    /// A bare stencil region that reads no heap row, run on the register file (`run_bare`).
    Stencil,
    /// A numeric image or JIT body over its argument window (`run_numeric_entry`), whose status 0 is success.
    Numeric,
}

/// What one body-ABI entry takes, built by its driver: the lane body ABI's words (receiver,
/// capture word, registers) and the entry word they are passed to. The drivers (the lane body,
/// the OSR region, the bare stencil, the numeric image, and later the ahead image) each build
/// one from the arguments they hold, and `enter` runs it; `exit` (or `numeric_result`) reads
/// the return.
pub(crate) struct EntryDescriptor {
    /// The driver that built the descriptor, which says how its result is read.
    pub(crate) kind: EntryKind,
    /// The receiver word the entry takes (`THIS`): a lane body's, an OSR region's and a body
    /// entry's (`Completion::Body`), which the entry reads as its argument. 0 where the entry has none
    /// (a bare stencil region, a numeric entry). Remaining difference: the AOT trampoline
    /// (`ahead_entry.rs`) is entered as a native function, whose ABI (`native_entry_signature`) has
    /// no receiver word, so it reads the receiver from the window header its caller wrote and passes
    /// it to the body as the argument. Changing that would change the call rows' emitted entry
    /// protocol, so it is left as is.
    pub(crate) receiver: u64,
    /// The capture word the entry takes: the activation's captures, or a resume stub's address
    /// for a region entered mid-run; 0 where neither applies.
    pub(crate) captures: u64,
    /// The register file, or the argument window, the entry reads.
    pub(crate) registers: *const u64,
    /// The entry's code, as the body ABI types it.
    pub(crate) entry: BodyEntry,
}

/// Enters `descriptor` through the body ABI's trampoline, on `context`.
///
/// # Safety
/// As `enter_body`, for `descriptor.entry` and the registers it names.
pub(crate) unsafe fn enter(context: &mut CallContext, descriptor: &EntryDescriptor) -> BodyReturn {
    // SAFETY: as the caller promises.
    unsafe { enter_body(context, descriptor.receiver, descriptor.captures, descriptor.registers, descriptor.entry) }
}

/// Reads the return of a non-numeric entry as its run's exit: the one reader of a status and
/// result word (`LaneExit::read`). `returned_pc` names the record that completes the
/// activation, read only then.
pub(crate) fn exit(descriptor: &EntryDescriptor, returned: BodyReturn, returned_pc: impl FnOnce() -> usize) -> LaneExit {
    debug_assert!(descriptor.kind != EntryKind::Numeric, "a numeric entry's result is read by `numeric_result`, not as a leave");
    LaneExit::read(returned.status, returned.value, returned_pc)
}

/// The result of a numeric entry, returned as its body wrote it: status 0 is success and the
/// word is the value. Numeric semantics are not a leave, so no `LaneExit` is made.
pub(crate) fn numeric_result(descriptor: &EntryDescriptor, returned: BodyReturn) -> BodyReturn {
    debug_assert!(descriptor.kind == EntryKind::Numeric, "only a numeric entry's result is returned untouched");
    returned
}
