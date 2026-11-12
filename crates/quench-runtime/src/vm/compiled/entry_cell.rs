//! The one entry word of a function's lane body, which native callers read.

use super::*;
use crate::vm::dispatch_fast::LaneRecoverySpace;

/// The status of a call that entered no body: its callee's entry cell held the stub, so the
/// caller takes its helper path at the call.
pub(in crate::vm) const LANE_BODY_NOT_ENTERED: u32 = JS_GENERAL_RETURN_STATUS_BASE - 2;

/// What a function's native callers read to enter its lane body: the entry (the stub where no
/// body is published or it retired) and the body's own recovery space, which it is stored in
/// once, per function, at a stable address (`LaneView::entry_cell`).
#[repr(C)]
pub(in crate::vm) struct EntryCell {
    pub(in crate::vm) entry: std::cell::Cell<usize>,
    /// The address of the published body's `LaneMethod::recovery_space` (`NO_RECOVERY` for the stub).
    pub(in crate::vm) recovery: std::cell::Cell<usize>,
    /// The address of the published body (`LaneMethod`), which the view keeps until it drops
    /// (`LaneView::pinned`): a leave inside a call through the cell recovers through it.
    pub(in crate::vm) body: std::cell::Cell<usize>,
    /// Runs of the body its native callers completed (`Lowering::direct_finish` counts each),
    /// which no handler entry ends: the tier machine's evidence a body runs to its return
    /// (`LaneView::body_completion_evidence`).
    pub(in crate::vm) completions: std::cell::Cell<u64>,
}

/// The entry no body backs: it enters nothing.
unsafe extern "C" fn lane_entry_stub(_: *mut CallContext, _: u64, _: u64, _: *const u64) -> BodyReturn {
    BodyReturn { status: LANE_BODY_NOT_ENTERED, value: 0 }
}

/// The recovery space the stub's cell names: no body, nothing to recover.
static NO_RECOVERY: LaneRecoverySpace = LaneRecoverySpace { values: 0, activations: 0 };

impl EntryCell {
    pub(in crate::vm) fn stub() -> usize { lane_entry_stub as BodyEntry as usize }

    pub(in crate::vm) fn new() -> Self {
        Self { entry: std::cell::Cell::new(Self::stub()), recovery: std::cell::Cell::new(std::ptr::addr_of!(NO_RECOVERY).addr()), body: Default::default(), completions: Default::default() }
    }

    /// Publishes `method`'s entry and recovery space; it takes the cell back when it retires.
    pub(in crate::vm) fn publish(&self, method: &PublishedLaneMethod) {
        let Some(body) = method.lease() else { return };
        reserve_lane_scratch(body.scratch_width);
        self.recovery.set(std::ptr::addr_of!(body.recovery_space).addr());
        self.body.set(Rc::as_ptr(&body).addr());
        self.completions.set(0);
        self.entry.set(body.entry_address());
        method.on_retire(&self.entry, body.entry_address(), Self::stub());
    }

    pub(in crate::vm) fn withdraw(&self) {
        self.entry.set(Self::stub());
        self.recovery.set(std::ptr::addr_of!(NO_RECOVERY).addr());
        self.body.set(0);
    }
}

/// Bits of a capture word that hold the dense slots' address; the slot count sits above them
/// (a user-space address fits 48 bits).
pub(super) const CAPTURE_ADDRESS_BITS: u32 = 48;
/// The mask of those bits.
pub(super) const CAPTURE_ADDRESS_MASK: i64 = (1 << CAPTURE_ADDRESS_BITS) - 1;

/// The one word a body's entry takes its captures in: the dense slots' address and count.
pub(in crate::vm) fn capture_word(values: *const Value, len: usize) -> u64 {
    debug_assert!(values.addr() >> CAPTURE_ADDRESS_BITS == 0 && len >> (u64::BITS - CAPTURE_ADDRESS_BITS) == 0);
    values.addr() as u64 | (len as u64) << CAPTURE_ADDRESS_BITS
}

/// Grows the scratch window to `width` registers for a body native code may call: an
/// inlined leave inside it fills its window, which the run that entered the outermost
/// body sized for that body alone.
pub(super) fn reserve_lane_scratch(width: usize) {
    let frame = super::lane_frame();
    // SAFETY: as `lane_frame`; native code reads the window's address from the dispatch
    // record at each use, so the record follows a resize.
    unsafe {
        if (*frame).scratch.len() < width {
            (*frame).scratch.resize(width, Value::UNDEFINED);
            (*frame).dispatch.inlined = (*frame).scratch.as_mut_ptr().addr();
        }
    }
}
