//! A proved region's data: the plan of a counted loop whose body the lane runs on proved
//! records (`typed_ir::counted_loop`), the handlers its records held before the region proved
//! them, and its state. The lane's state machine (`dispatch_fast::region_state`) runs it; this
//! module only builds it, once per lane view.

use crate::typed_ir::counted_loop::LoopPlan;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum State {
    /// The region's records are the lane's own.
    Generic,
    /// Its units and update run their proved records.
    Proved,
    /// The header runs its generic test again: entries failed too often to pay their proof.
    Retired,
}

pub(super) struct Region {
    pub(super) plan: LoopPlan,
    /// The handler each unit's first record held when the region was last proved, then the update
    /// record's: what a demotion restores.
    pub(super) generic: Box<[std::cell::Cell<*const ()>]>,
    /// The header record's own handler, which it runs again once the region retires.
    pub(super) header: *const (),
    pub(super) state: std::cell::Cell<State>,
    pub(super) declines: std::cell::Cell<u8>,
}

#[cfg(test)]
impl Region {
    /// Whether the region is proved now, and how many entries failed its proof.
    pub(super) fn state(&self) -> (bool, u8) { (self.state.get() == State::Proved, self.declines.get()) }
}

/// The regions of `plans`, each with the header handler `header` names.
pub(super) fn build(plans: Vec<LoopPlan>, header: impl Fn(&LoopPlan) -> *const ()) -> Box<[Region]> {
    plans.into_iter().map(|plan| {
        let generic = std::iter::repeat_n(std::cell::Cell::new(std::ptr::null()), plan.units.len() + 1).collect();
        Region { header: header(&plan), plan, generic, state: std::cell::Cell::new(State::Generic), declines: std::cell::Cell::new(0) }
    }).collect()
}
