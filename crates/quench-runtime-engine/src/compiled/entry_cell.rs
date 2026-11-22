//! The one entry word of a function's lane body, which native callers read.

use super::*;
use crate::dispatch_fast::LaneRecoverySpace;

/// The status of a call that entered no body (`LaneStatus::NotEntered`): its callee's entry cell held the
/// stub, so the caller takes its helper path at the call.
pub(crate) use quench_bjit::call_record::LANE_BODY_NOT_ENTERED;

/// What a function's native callers read to enter its lane body: the entry (the stub where no
/// body is published or it retired) and the body's own recovery space, which it is stored in
/// once, per function, at a stable address (`LaneView::entry_cell`).
///
/// Two 64-byte lines whose every offset is the designed cell's (`staged::call::ENTRY_CELL`, asserted
/// below): `entry` 0, `body` 8, `facts` 16, `validity` 24, `recovery` 32, `counters` 40, `scalar` 48, `rows` 56,
/// `proven` 64, `numbers` 72, `uses_this` 80, `proven_entry` 88, `proven_body` 96.
/// Writers: `entry`, `scalar`, `recovery`, `body`, `rows`, `proven`, `numbers`, `uses_this`, `proven_entry` and `proven_body` by `publish`/`withdraw`;
/// `facts` once per function (`set_facts`); `counters` only on cold paths (completion evidence).
#[repr(C, align(64))]
pub(crate) struct EntryCell {
    pub(crate) entry: std::cell::Cell<usize>,
    /// The address of the published body (`LaneMethod`), which the view keeps until it drops
    /// (`LaneView::pinned`): a leave inside a call through the cell recovers through it.
    pub(crate) body: std::cell::Cell<usize>,
    /// The function's registers, room, parameters and entry kind (`CellFacts`), which a native
    /// call's admission reads.
    pub(crate) facts: std::cell::Cell<u64>,
    /// What the body's native callers compare to admit a call (the layout epoch the body was
    /// published under).
    pub(crate) validity: std::cell::Cell<u64>,
    /// The address of the published body's `LaneMethod::recovery_space` (`NO_RECOVERY` for the stub).
    pub(crate) recovery: std::cell::Cell<usize>,
    /// Leaves (low half) and completions (high half) of this function's body since it
    /// specialized (`EVIDENCE`): the one home of its run statistics, written only on paths that
    /// already do work (a handler's run end, a leave's materialization), never by a call or a return.
    pub(crate) counters: std::cell::Cell<u64>,
    /// The address of the published body's scalar entry (`LaneMethod::scalar_address`), which a native
    /// caller whose arguments are proved numbers calls instead of `entry`: it takes the numbers in
    /// registers and pushes no frame. The stub where the body has none or retired.
    pub(crate) scalar: std::cell::Cell<usize>,
    /// The address of the published b-jit body's row entry (`LaneMethod::rows_address`), which a
    /// b-jit direct call row calls instead of `entry`; 0 where the body has none or retired.
    pub(crate) rows: std::cell::Cell<usize>,
    /// The address of the published b-jit body's proven-arguments row entry (`RowEntries::proven`): `entry_rows` past
    /// its Number guards. A direct row calls it only where `numbers` is covered by the call's proven arguments.
    pub(crate) proven: std::cell::Cell<usize>,
    /// The parameter registers whose Number guards `proven` skips (`RowEntries::numbers`).
    pub(crate) numbers: std::cell::Cell<u64>,
    /// Whether the body reads its `this` from the context header (`RowEntries::uses_this`).
    pub(crate) uses_this: std::cell::Cell<u64>,
    /// The address of the published body's proven entry (`LaneMethod::proven_address`), which a direct call calls
    /// instead of `entry` where its arguments have the entry types the body names (`LaneMethod::entry_param_types`),
    /// and the body it belongs to is the one the call pinned (`proven_body`). Zero where the body has none or retired.
    pub(crate) proven_entry: std::cell::Cell<usize>,
    /// The address of the body `proven_entry` belongs to (`Rc::as_ptr`), so a call takes it only for the body whose
    /// parameter types its caller proved (`DirectCallee::body`).
    pub(crate) proven_body: std::cell::Cell<usize>,
}

/// A b-jit body's row entries in its code, as the cell publishes them: its row entry (`rows`), its proven-arguments
/// row entry (`proven`, past the Number guards `numbers` names) and whether it reads `this` from its context header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RowEntries {
    pub(crate) rows: usize,
    pub(crate) proven: usize,
    pub(crate) numbers: u64,
    pub(crate) uses_this: bool,
}

const _: () = {
    use quench_runtime::staged::call::ENTRY_CELL;
    assert!(std::mem::size_of::<EntryCell>() == 128);
    assert!(std::mem::offset_of!(EntryCell, entry) as i32 == ENTRY_CELL.entry_offset);
    assert!(std::mem::offset_of!(EntryCell, body) as i32 == ENTRY_CELL.body_offset);
    assert!(std::mem::offset_of!(EntryCell, facts) as i32 == ENTRY_CELL.facts_offset);
    assert!(std::mem::offset_of!(EntryCell, validity) as i32 == ENTRY_CELL.validity_offset);
    assert!(std::mem::offset_of!(EntryCell, recovery) as i32 == ENTRY_CELL.recovery_offset);
    assert!(std::mem::offset_of!(EntryCell, counters) as i32 == ENTRY_CELL.counters_offset);
    assert!(std::mem::offset_of!(EntryCell, scalar) as i32 == ENTRY_CELL.scalar_offset);
    assert!(std::mem::offset_of!(EntryCell, rows) as i32 == ENTRY_CELL.rows_offset);
    assert!(std::mem::offset_of!(EntryCell, proven) as i32 == ENTRY_CELL.proven_offset);
    assert!(std::mem::offset_of!(EntryCell, numbers) as i32 == ENTRY_CELL.numbers_offset);
    assert!(std::mem::offset_of!(EntryCell, uses_this) as i32 == ENTRY_CELL.uses_this_offset);
};

/// The halves of a cell's counters word.
pub(crate) const EVIDENCE: Evidence = Evidence { half_bits: u32::BITS };

/// The layout of the counters word: `half_bits` wide leaves, then completions.
pub(crate) struct Evidence {
    half_bits: u32,
}

impl Evidence {
    #[inline]
    const fn half_mask(&self) -> u64 { (1 << self.half_bits) - 1 }
}

/// The entry no body backs: it enters nothing. Its address is the cell's unpublished state
/// (`EntryCell::stub`), so it has exactly one instance: a tiny function is otherwise inlinable and
/// each codegen unit may get its own copy at its own address.
#[inline(never)]
unsafe extern "C" fn entry_stub_in_lane(_: *mut CallContext, _: u64, _: u64, _: *const u64) -> BodyReturn {
    BodyReturn { status: LANE_BODY_NOT_ENTERED, value: 0 }
}

/// A scalar entry: (context, numbers...) -> (status, result word), the status zero for a result.
pub(crate) type ScalarEntry = unsafe extern "C" fn(*mut CallContext) -> BodyReturn;

/// The scalar entry no body backs: it enters nothing, so its caller restarts the call. One
/// instance, as `entry_stub_in_lane`.
#[inline(never)]
unsafe extern "C" fn scalar_entry_stub(_: *mut CallContext) -> BodyReturn {
    BodyReturn { status: LANE_BODY_NOT_ENTERED, value: 0 }
}

/// The recovery space the stub's cell names: no body, nothing to recover.
static NO_RECOVERY: LaneRecoverySpace = LaneRecoverySpace { values: 0, activations: 0 };

/// One completion in `EntryCell::counters` (its high word): a run of the body that ended by
/// an exception through a native caller (`Lowering::direct_finish`), which no Rust code sees.
pub(crate) use quench_runtime::staged::layout_offsets::RAISED_RUN;

impl EntryCell {
    /// The runs of the body that ended by an exception through a native caller, for tests.
    #[cfg(test)]
    pub(crate) fn raised_runs(&self) -> u32 { (self.counters.get() >> u32::BITS) as u32 }

    #[inline(always)] pub(crate) fn stub() -> usize { entry_stub_in_lane as BodyEntry as usize }

    pub(crate) fn scalar_stub() -> usize { scalar_entry_stub as ScalarEntry as usize }

    pub(crate) fn new() -> Self {
        let cell = Self { entry: Default::default(), recovery: Default::default(), body: Default::default(), facts: Default::default(), validity: Default::default(), counters: Default::default(), scalar: Default::default(), rows: Default::default(), proven: Default::default(), numbers: Default::default(), uses_this: Default::default(),
            proven_entry: Default::default(), proven_body: Default::default() };
        cell.unpublish();
        cell
    }

    /// Writes the unpublished state: the stub entry and scalar entry, no rows or proofs, and no
    /// recovery space or body. A new cell and a withdrawn one are both in this state.
    fn unpublish(&self) {
        // The body the cell held no longer types its callers' calls (`LaneMethod::summary`).
        if self.body.get() != 0 {
            // SAFETY: a published cell's `body` is `Rc::as_ptr` of a body its view pins (`publish`), and the word is
            // zeroed when that body retires (`PublishedCode::retire`), so a nonzero word names a live body.
            unsafe { (*(self.body.get() as *const LaneMethod)).withdraw() }
        }
        self.entry.set(Self::stub());
        self.rows.set(0);
        self.proven.set(0);
        self.proven_entry.set(0);
        self.proven_body.set(0);
        self.scalar.set(Self::scalar_stub());
        self.proven_entry.set(0);
        self.proven_body.set(0);
        self.recovery.set(std::ptr::addr_of!(NO_RECOVERY).addr());
        self.body.set(0);
    }

    /// Publishes `method`'s entry and recovery space; it takes the cell back when it retires: the
    /// entry to the stub and the body word, which says whether the cell is published, to none.
    pub(crate) fn publish(&self, method: &PublishedLaneMethod) {
        let Some(body) = method.lease() else { return };
        self.recovery.set(std::ptr::addr_of!(body.recovery_space).addr());
        self.body.set(Rc::as_ptr(&body).addr());
        method.on_retire(&self.body, Rc::as_ptr(&body).addr(), 0);
        self.entry.set(body.entry_address());
        method.on_retire(&self.entry, body.entry_address(), Self::stub());
        self.rows.set(0);
        self.proven.set(0);
        self.numbers.set(0);
        self.uses_this.set(0);
        if let Some(entries) = body.row_entries() {
            self.rows.set(entries.rows);
            method.on_retire(&self.rows, entries.rows, 0);
            self.proven.set(entries.proven);
            method.on_retire(&self.proven, entries.proven, 0);
            self.numbers.set(entries.numbers);
            self.uses_this.set(u64::from(entries.uses_this));
        }
        self.scalar.set(Self::scalar_stub());
        if let Some(scalar) = body.scalar_address() {
            self.scalar.set(scalar);
            method.on_retire(&self.scalar, scalar, Self::scalar_stub());
        }
        self.proven_entry.set(0);
        self.proven_body.set(0);
        if let Some(proven) = body.proven_address() {
            let owner = Rc::as_ptr(&body).addr();
            self.proven_body.set(owner);
            method.on_retire(&self.proven_body, owner, 0);
            self.proven_entry.set(proven);
            method.on_retire(&self.proven_entry, proven, 0);
        }
    }

    /// Records the function's facts, which its native callers admit a call by.
    pub(crate) fn set_facts(&self, facts: quench_runtime::staged::call::CellFacts) { self.facts.set(facts.word()) }

    /// Starts the statistics of a newly specialized body.
    pub(crate) fn reset_evidence(&self) { self.counters.set(0) }

    /// Adds `runs` to one half (`shift` 0: leaves, `half_bits`: completions), saturating.
    #[inline]
    fn add_half(&self, shift: u32, runs: u64) -> u64 {
        let word = self.counters.get();
        let half = ((word >> shift) & EVIDENCE.half_mask()).saturating_add(runs).min(EVIDENCE.half_mask());
        self.counters.set(word & !(EVIDENCE.half_mask() << shift) | half << shift);
        half
    }

    /// Counts a leave; returns the leaves so far.
    pub(crate) fn note_leave(&self) -> u64 { self.add_half(0, 1) }

    /// Counts `runs` completions (a handler's run end counts one, a native caller's credit many);
    /// returns the completions so far.
    #[inline]
    pub(crate) fn note_completions(&self, runs: u64) -> u64 { self.add_half(EVIDENCE.half_bits, runs) }

    /// The body's completions and leaves since it specialized: the tier model's one reader.
    pub(crate) fn evidence(&self) -> (u64, u64) {
        let word = self.counters.get();
        (word >> EVIDENCE.half_bits, word & EVIDENCE.half_mask())
    }

    pub(crate) fn withdraw(&self) {
        self.unpublish();
    }
}

pub(super) use quench_bjit::shared_words::CAPTURE_ADDRESS_BITS;
/// The bit of the address a store tag takes: slots are aligned past it (`CAPTURE_STORES`).
const CAPTURE_STORES_BIT: u64 = crate::dispatch_fast::CAPTURE_STORES as u64;
/// The mask of the address bits.
pub(super) const CAPTURE_ADDRESS_MASK: i64 = ((1 << CAPTURE_ADDRESS_BITS) - 1) & !(CAPTURE_STORES_BIT as i64);

/// The one word a body's entry takes its captures in: the dense slots' address and count, and
/// whether they admit every direct store (`dispatch_fast::capture_stores`).
#[inline]
pub(crate) fn capture_word(values: *const Value, len: usize, stores: bool) -> u64 {
    debug_assert!(values.addr() >> CAPTURE_ADDRESS_BITS == 0 && len >> (u64::BITS - CAPTURE_ADDRESS_BITS) == 0);
    values.addr() as u64 | u64::from(stores) * CAPTURE_STORES_BIT | (len as u64) << CAPTURE_ADDRESS_BITS
}

/// Grows the scratch window to `width` registers for a body a run may enter: an inlined leave
/// inside it fills its window. A run finds the window sized by its body's publication.
pub(super) fn reserve_lane_scratch(width: usize) {
    let frame = super::frame_in_lane();
    // SAFETY: as `lane_frame`; native code reads the window's address from the dispatch
    // record at each use, so the record follows a resize.
    unsafe {
        if (*frame).scratch.len() < width {
            (*frame).scratch.resize(width, Value::UNDEFINED);
            (*frame).dispatch.inlined = (*frame).scratch.as_mut_ptr().addr();
        }
    }
}
