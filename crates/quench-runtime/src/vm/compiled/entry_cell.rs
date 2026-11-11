//! The one entry word of a function's lane body, which native callers read.

use super::*;

/// The status of a call that entered no body: its callee's entry cell held the stub, so the
/// caller takes its helper path at the call.
pub(in crate::vm) const LANE_BODY_NOT_ENTERED: u32 = JS_GENERAL_RETURN_STATUS_BASE - 2;

/// What a function's native callers read to enter its lane body: the entry (the stub where no
/// body is published or it retired) and the space its recovery needs, per function, at a
/// stable address (`LaneView::entry_cell`).
#[repr(C)]
pub(in crate::vm) struct EntryCell {
    pub(in crate::vm) entry: std::cell::Cell<usize>,
    pub(in crate::vm) values: std::cell::Cell<usize>,
    pub(in crate::vm) activations: std::cell::Cell<usize>,
    /// The index of the published body among its view's pins (`LaneView::pinned_body`).
    pub(in crate::vm) pin: std::cell::Cell<usize>,
}

/// The entry no body backs: it enters nothing.
unsafe extern "C" fn lane_entry_stub(_: *mut CallContext, _: u64, _: u64, _: *const u64, _: *mut u64) -> u32 {
    LANE_BODY_NOT_ENTERED
}

impl EntryCell {
    pub(in crate::vm) fn stub() -> usize { lane_entry_stub as Entry as usize }

    pub(in crate::vm) fn new() -> Self {
        Self { entry: std::cell::Cell::new(Self::stub()), values: Default::default(), activations: Default::default(), pin: Default::default() }
    }

    /// Publishes `method`'s entry and recovery space; it takes the cell back when it retires.
    pub(in crate::vm) fn publish(&self, method: &PublishedLaneMethod, pin: usize) {
        let Some(body) = method.lease() else { return };
        let space = body.recovery_space();
        self.values.set(space.values);
        self.activations.set(space.activations.max(1));
        self.pin.set(pin);
        self.entry.set(body.entry_address());
        method.on_retire(&self.entry, body.entry_address(), Self::stub());
    }

    pub(in crate::vm) fn withdraw(&self) { self.entry.set(Self::stub()); }
}
