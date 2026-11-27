//! The planned call row (`planned_call_row`): a method call whose receiver's class union is closed (`PlannedSite`,
//! `compile::call_plan`) is dispatched on the receiver's header, not on the call's cache. A receiver is an instance of
//! one planned class, and its layout id names it: the arm whose class header (`ProgramStore::class_header`) holds
//! that id is the call's arm. The arm's target is resolved by the first call through it and held while the script
//! target epoch (`ScriptTargets::epoch`) holds; an event that moves or un-proves a target advances the epoch.
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use quench_runtime::vm::engine_types::{PlannedArm, PlannedSite, PlannedTarget};

/// The arm of `site` whose class the receiver's layout id `receiver` names, in the plan's order. A header of 0 is a class
/// no construction has recorded, whose instances are unseen, so it matches nothing.
#[inline(always)]
fn planned_arm<'a>(vm: &Vm, site: &'a PlannedSite, receiver: Value) -> Option<&'a PlannedArm> {
    let layout = vm.object_data(receiver)?.layout_id();
    site.arms.iter().find(|arm| {
        // SAFETY: a header cell lives in its program's class table, which the program's entry owns with this view.
        let header = unsafe { (*arm.header).get() };
        header != 0 && header == layout
    })
}

/// The target `arm` holds at this epoch, resolved now where it holds none: the call's own resolution
/// (`linked_script_call_entry`) of an ordinary activation of the arm's function that the call's arguments fit. The
/// resolution proves the entry as `script_call_prove_miss` does (`Admission::UNPROVEN` cleared), once. None where
/// the call takes its full row.
fn resolve_arm(vm: &mut Vm, arm: &PlannedArm, r: Registers, i: &LaneInstruction) -> Option<*const ScriptCallTarget> {
    let target = vm.linked_script_call_entry(r.get(i.b), &i.call_link)?;
    // SAFETY: `linked_script_call_entry` returns an entry of the lane stack's cache for this generation.
    let entry = unsafe { &*target };
    let argc = ImmediateLayout::argument_count(i.imm());
    if entry.function != arm.function || entry.entry != CallEntry::framed(Framed::Activates)
        || argc > FIXED_ARGUMENT_COPIES || !quench_runtime::staged::call::concrete_fits(u64::from(entry.params), u64::from(argc)) {
        return None;
    }
    vm.lane_stack.script_targets.entries[link_index(i.call_link.get())].admission.0 &= !Admission::UNPROVEN;
    arm.target.set(PlannedTarget { epoch: vm.lane_stack.script_targets.epoch, target });
    Some(target)
}

script_handler! {
    /// A method call at a planned site, entered through its receiver's class arm. The receiver's method is the arm's
    /// function by the plan's fact (a method slot with no other writes, `Miss::None`), so the record's callee is not
    /// compared. A receiver of no planned class, and an arm with no held target, take the call's full row, which resolves
    /// the target and holds it for the arm's next call (`resolve_arm`).
    #[inline(never)]
    pub(crate) fn planned_call_row<>(cx, r, ip, i, view) {
        if !unsafe { (*cx).script_calls } {
            return exit(ip);
        }
        let vm = unsafe { vm(cx) };
        // SAFETY: `mode` names the site of the view this record belongs to (`specialize_script_calls`).
        let site = unsafe { &*(i.mode as usize as *const PlannedSite) };
        let Some(arm) = planned_arm(vm, site, r.get(i.c)) else {
            sibling!(script_call_full_miss, r, ip, Carried::script_in(cx, view))
        };
        let held = arm.target.get();
        let target = if !held.target.is_null() && held.epoch == vm.lane_stack.script_targets.epoch {
            held.target
        } else {
            match resolve_arm(vm, arm, r, i) {
                Some(target) => target,
                None => sibling!(script_call_full_miss, r, ip, Carried::script_in(cx, view)),
            }
        };
        // SAFETY: a held target is the entry the arm resolved at this epoch, which the epoch keeps in place.
        let target = unsafe { &*target };
        if target.entry != CallEntry::framed(Framed::Activates) {
            sibling!(script_call_full_miss, r, ip, Carried::script_in(cx, view))
        }
        match activate_unproven_in_lane::<ANY_ARGC>(cx, r, ip, i, target) {
            Some(resumed) => resume!(cx, resumed),
            None => sibling!(script_call_full_miss, r, ip, Carried::script_in(cx, view)),
        }
    }
}
