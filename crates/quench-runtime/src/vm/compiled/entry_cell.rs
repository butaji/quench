//! The one entry word of a function's lane body, which native callers read.

use super::*;
use crate::vm::dispatch_fast::LaneRecoverySpace;

/// The status of a call that entered no body: its callee's entry cell held the stub, so the
/// caller takes its helper path at the call.
pub(in crate::vm) const LANE_BODY_NOT_ENTERED: u32 = JS_GENERAL_RETURN_STATUS_BASE - 2;

/// What a function's native callers read to enter its lane body: the entry (the stub where no
/// body is published or it retired) and the body's own recovery space, which it is stored in
/// once, per function, at a stable address (`LaneView::entry_cell`).
///
/// One 64-byte line, whose offsets are the designed cell's (`staged::call::ENTRY_CELL`): `entry` 0,
/// `validity` 24, `view` 32, `env` 40, `capture` 48, `counters` 56. `recovery` and `body` sit at 8
/// and 16 until `recovery` is derived from `body`, when `body` moves to 8 and `facts` takes 16.
/// Writers: `entry`, `recovery` and `body` by `publish`/`withdraw`; `counters` only on cold paths
/// (completion evidence); nothing yet reads or writes the other words.
#[repr(C, align(64))]
pub(in crate::vm) struct EntryCell {
    pub(in crate::vm) entry: std::cell::Cell<usize>,
    /// The address of the published body's `LaneMethod::recovery_space` (`NO_RECOVERY` for the stub).
    pub(in crate::vm) recovery: std::cell::Cell<usize>,
    /// The address of the published body (`LaneMethod`), which the view keeps until it drops
    /// (`LaneView::pinned`): a leave inside a call through the cell recovers through it.
    pub(in crate::vm) body: std::cell::Cell<usize>,
    /// What the body's native callers compare to admit a call (the layout epoch the body was
    /// published under).
    pub(in crate::vm) validity: std::cell::Cell<u64>,
    pub(in crate::vm) view: std::cell::Cell<usize>,
    pub(in crate::vm) env: std::cell::Cell<usize>,
    pub(in crate::vm) capture: std::cell::Cell<usize>,
    /// Leaves (low half) and completions (high half) of this function's body since it
    /// specialized (`EVIDENCE`): the one home of its run statistics, written only on paths that
    /// already do work (a handler's run end, a leave's materialization), never by a call or a return.
    pub(in crate::vm) counters: std::cell::Cell<u64>,
}

const _: () = {
    use crate::staged::call::ENTRY_CELL;
    assert!(std::mem::size_of::<EntryCell>() == 64);
    assert!(std::mem::offset_of!(EntryCell, entry) as i32 == ENTRY_CELL.entry_offset);
    assert!(std::mem::offset_of!(EntryCell, validity) as i32 == ENTRY_CELL.validity_offset);
    assert!(std::mem::offset_of!(EntryCell, view) as i32 == ENTRY_CELL.view_offset);
    assert!(std::mem::offset_of!(EntryCell, env) as i32 == ENTRY_CELL.env_offset);
    assert!(std::mem::offset_of!(EntryCell, capture) as i32 == ENTRY_CELL.capture_offset);
    assert!(std::mem::offset_of!(EntryCell, counters) as i32 == ENTRY_CELL.counters_offset);
};

/// The halves of a cell's counters word.
pub(in crate::vm) const EVIDENCE: Evidence = Evidence { half_bits: u32::BITS };

/// The layout of the counters word: `half_bits` wide leaves, then completions.
pub(in crate::vm) struct Evidence {
    half_bits: u32,
}

impl Evidence {
    const fn half_mask(&self) -> u64 { (1 << self.half_bits) - 1 }
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
        Self { entry: std::cell::Cell::new(Self::stub()), recovery: std::cell::Cell::new(std::ptr::addr_of!(NO_RECOVERY).addr()), body: Default::default(), validity: Default::default(), view: Default::default(), env: Default::default(), capture: Default::default(), counters: Default::default() }
    }

    /// Publishes `method`'s entry and recovery space; it takes the cell back when it retires.
    pub(in crate::vm) fn publish(&self, method: &PublishedLaneMethod) {
        let Some(body) = method.lease() else { return };
        self.recovery.set(std::ptr::addr_of!(body.recovery_space).addr());
        self.body.set(Rc::as_ptr(&body).addr());
        self.entry.set(body.entry_address());
        method.on_retire(&self.entry, body.entry_address(), Self::stub());
    }

    /// Starts the statistics of a newly specialized body.
    pub(in crate::vm) fn reset_evidence(&self) { self.counters.set(0) }

    /// Adds `runs` to one half (`shift` 0: leaves, `half_bits`: completions), saturating.
    fn add_half(&self, shift: u32, runs: u64) -> u64 {
        let word = self.counters.get();
        let half = ((word >> shift) & EVIDENCE.half_mask()).saturating_add(runs).min(EVIDENCE.half_mask());
        self.counters.set(word & !(EVIDENCE.half_mask() << shift) | half << shift);
        half
    }

    /// Counts a leave; returns the leaves so far.
    pub(in crate::vm) fn note_leave(&self) -> u64 { self.add_half(0, 1) }

    /// Counts `runs` completions (a handler's run end counts one, a native caller's credit many);
    /// returns the completions so far.
    pub(in crate::vm) fn note_completions(&self, runs: u64) -> u64 { self.add_half(EVIDENCE.half_bits, runs) }

    /// The body's completions and leaves since it specialized: the tier model's one reader.
    pub(in crate::vm) fn evidence(&self) -> (u64, u64) {
        let word = self.counters.get();
        (word >> EVIDENCE.half_bits, word & EVIDENCE.half_mask())
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

/// Grows the scratch window to `width` registers for a body a run may enter: an inlined leave
/// inside it fills its window. A run finds the window sized by its body's publication.
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
