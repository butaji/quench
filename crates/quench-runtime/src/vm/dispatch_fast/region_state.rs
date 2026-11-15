//! The state machine of a proved region: a counted loop whose body is straight-line work the
//! lane runs on records that assume facts the loop's entry proved (`int_region`,
//! `field_region`; the plan is `typed_ir::counted_loop`).
//!
//! A region is `Generic` (its records are the lane's own) or `Proved` (its units and update are
//! requickened to their proved records, which read int32 words unchecked and write only their
//! payloads). The one transition point is the loop's header, whose test record runs
//! `lane_region_enter` for as long as the region lives: the loop's fused update and jump
//! continue past that record, so it runs once per entry of the loop, and there the entry
//! facts are proved once. Nothing reaches the body but through the header: the view clears the
//! lane entries of the body's PCs (`install`), a straight-line body holds no jump target, and a
//! record before the header that the lane fuses with the test would enter past it, so such a
//! loop has no region. A lane that left mid-body continues in the general path until the next
//! entry. A failed proof demotes the region; failures past `REGION_DECLINES` retire it to the
//! generic header for good (the bound a `LoopSite` puts on declined entries).

use super::*;
use super::field_region::field_holds;
use super::int_region::{proved_handler, proved_update};
use crate::bytecode::Operand;
use crate::compile::liveness::RegisterMasks;
use crate::staged::{Concrete, Domain, NumberOperation, Operation, Step};
use crate::typed_ir::counted_loop::{plan_loops, EXIT_AFTER_UPDATE};
use crate::typed_ir::loop_ints::all_int;
use crate::vm::compiled::tier_policy::REGION_DECLINES;
use crate::vm::proved_region::{build, Region, State};

/// Whether the record of `op`, which falls into a loop's header, is one the lane fuses with the
/// test that follows (`specialize_load_branch_pairs`): the fused record runs the test itself.
fn fuses_with_test<H: Host>(op: Op) -> bool {
    let mut fused = false;
    for accumulator in [ACC_NONE, ACC_C] {
        for zero in [false, true] {
            fused |= load_jump_handler::<H>(op, accumulator, zero, false).is_some();
        }
    }
    fused
}

/// Gives each counted loop whose body is proved-region work its state machine: the header's
/// record runs `lane_region_enter`, the update record names the region and its limit register,
/// and the lane no longer enters the body's PCs from the general path. `first` is the view's
/// first record, in its final place; a view under a loop tier or of Wasm has no regions.
pub(super) fn install<'live, H: Host>(
    first: Ip, code: &[WideInstruction], constants: &[Value], live: &dyn Fn() -> Option<&'live std::rc::Rc<RegisterMasks>>,
    targets: &InstructionSet, (loop_tier, wasm): (bool, bool), entries: &mut [u64],
) -> Box<[Region]> {
    // An aggregate profile counts each generic record (`specialize_proved_arrays`).
    if loop_tier || wasm || cfg!(feature = "profile-aggregate") {
        return Box::default();
    }
    let plans = plan_loops(
        code, constants, live().map(|masks| &**masks), &|pc| targets.contains(pc),
        &|header| header > 0 && fuses_with_test::<H>(code[header - 1].op()),
    );
    // SAFETY: a plan's records lie inside the view, and the lane has not started.
    let regions = build(plans, |plan| unsafe { (*first.add(plan.counted.header)).handler });
    for region in &*regions {
        let counted = &region.plan.counted;
        unsafe { link::<H>(first, region) };
        for pc in counted.header + 1..=counted.update + 1 {
            entries[pc / u64::BITS as usize] &= !(1 << (pc % u64::BITS as usize));
        }
    }
    regions
}

/// Links `region`'s header and update records to it.
///
/// # Safety
/// `first` is the view's first record, and the region's records lie inside the view.
unsafe fn link<H: Host>(first: Ip, region: &Region) {
    let counted = &region.plan.counted;
    let header = unsafe { first.add(counted.header).cast_mut() };
    let update = unsafe { first.add(counted.update).cast_mut() };
    unsafe {
        (*update).c = counted.limit;
        (*update).operand = LaneOperand { int_region: region };
        (*header).call_link.set((counted.update - counted.header) as u16);
        (*header).handler = lane_region_enter::<H> as Handler as *const ();
    }
}

/// Requickens `record` to `proved` and keeps the handler it held in `generic`, or restores it.
///
/// # Safety
/// `record` is a record of the running view.
unsafe fn requicken_region(record: *mut LaneInstruction, generic: &std::cell::Cell<*const ()>, proved: Option<Handler>) {
    match proved {
        Some(handler) => unsafe {
            generic.set((*record).handler);
            (*record).handler = handler as *const ();
        },
        None => unsafe { (*record).handler = generic.get() },
    }
}

impl Region {
    /// Whether the registers the body keeps as int32 words are int32s in `r`, giving the
    /// registers it defines first the int tag it writes under.
    #[inline(always)]
    fn admits(&self, r: Registers) -> bool {
        if !all_int(self.plan.must.iter().map(|&register| r.get(register))) { return false; }
        for &register in &*self.plan.fresh { r.set(register, Value::integer(0)); }
        true
    }

    /// Whether every field access of the body holds its proved facts now (`field_holds`).
    ///
    /// # Safety
    /// `header` is the header record of this region's view, and `cx` its running context.
    unsafe fn holds_fields<H: Host>(&self, cx: *mut LaneContext<H>, r: Registers, header: Ip) -> bool {
        for planned in &*self.plan.units {
            if let Some(access) = &planned.unit.field
                && !unsafe { field_holds(cx, r, header.add(planned.at), access) }
            {
                return false;
            }
        }
        true
    }

    /// Requickens the region's records to their proved handlers, or back.
    ///
    /// # Safety
    /// `header` is the header record of this region's view.
    unsafe fn swap<H: Host>(&self, header: Ip, proved: bool) {
        for (generic, planned) in self.generic.iter().zip(&*self.plan.units) {
            let handler = proved.then(|| proved_handler::<H>(planned.unit.kind));
            unsafe { requicken_region(header.add(planned.at).cast_mut(), generic, handler) };
        }
        let counted = &self.plan.counted;
        let handler = proved.then(|| proved_update::<H>(counted.decrement));
        unsafe { requicken_region(header.add(counted.update - counted.header).cast_mut(), &self.generic[self.plan.units.len()], handler) };
        self.state.set(if proved { State::Proved } else { State::Generic });
    }

    /// A failed entry: the proved records are demoted, and the header retires once declines
    /// pass the bound.
    ///
    /// # Safety
    /// As `swap`.
    unsafe fn decline<H: Host>(&self, header: Ip) {
        if self.state.get() == State::Proved {
            unsafe { self.swap::<H>(header, false) };
        }
        let declines = self.declines.get().saturating_add(1);
        self.declines.set(declines);
        if declines >= REGION_DECLINES {
            self.state.set(State::Retired);
            unsafe { (*header.cast_mut()).handler = self.header };
        }
    }
}

script_handler! {
    /// The test record of a counted loop with a region, run where control enters the loop:
    /// the entry facts are proved once (`Region::admits`, `Region::holds_fields`), the body
    /// requickened to its proved records, and the test decided here (the proved update decides
    /// the later ones). Where the facts do not hold the region is demoted and the generic test runs.
    fn lane_region_enter<>(cx, r, ip, i, view) {
        // SAFETY: the view links the header to its update record, which names the region.
        let (region, update) = unsafe {
            let update = ip.add(usize::from(i.call_link.get()));
            (&*(*update).operand.int_region, update)
        };
        if !region.admits(r) || !unsafe { region.holds_fields(cx, r, ip) } {
            sibling!(lane_region_decline::<H>, r, ip, Carried::script(cx))
        }
        if region.state.get() != State::Proved {
            // SAFETY: `ip` is the header of the region's view.
            unsafe { region.swap::<H>(ip, true) };
        }
        let mut frame = lane_frame::LaneFrame::<{ lane_frame::ACC_PROVED }>::new(r, Carried::script(cx));
        let test = NumberOperation::Relation {
            left: Operand::register(i.b), right: Operand::register(i.c), relation: region.plan.counted.relation, domain: Domain::Int32,
        };
        let Step::Value(holds) = crate::staged::step(&mut Concrete::new(std::ptr::null()), &mut frame, Operation::Number(test))
            else { unreachable!("a proved test is a value") };
        // SAFETY: the update record is followed by its back edge and the loop's exit.
        let next = if holds != 0 { following(ip) } else { unsafe { update.add(EXIT_AFTER_UPDATE) } };
        next!(cx, r, next, Carried::script(cx))
    }
}

script_handler! {
    /// A region entry whose facts did not hold: the region is demoted (`Region::decline`) and
    /// the header's generic test runs. Out of line, so the entry keeps no state for it.
    #[inline(never)]
    fn lane_region_decline<>(cx, r, ip, i, view) {
        // SAFETY: as `lane_region_enter`.
        let region = unsafe { &*(*ip.add(usize::from(i.call_link.get()))).operand.int_region };
        unsafe { region.decline::<H>(ip) };
        // SAFETY: the region saved the header's own handler, which has the shared signature.
        let handler: Handler = unsafe { std::mem::transmute::<*const (), Handler>(region.header) };
        sibling!(handler, r, ip, Carried::script(cx))
    }
}
