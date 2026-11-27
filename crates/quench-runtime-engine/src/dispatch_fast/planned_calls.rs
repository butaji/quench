//! The planned call row (`planned_call_row`): a method call whose receiver's class union is closed (`PlannedSite`,
//! `compile::call_plan`) is dispatched on the receiver's class header, not on the call's cache. A receiver is an
//! instance of one planned class, and its layout id names the class: the arm whose class header
//! (`ProgramStore::class_header`) holds that id is the call's arm. Each arm's target is resolved at the edge that
//! records its class's header (`resolve_planned_class`), from the method the class's prototype holds, and the program
//! store keeps it (`PlannedTarget`). No receiver reaches a site before its class's header is recorded, so the row
//! reads a resolved target and has no miss path of its own.
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use quench_runtime::vm::ProgramId;
use quench_runtime::vm::engine_types::{PlannedArm, PlannedSite};

/// The arm of `site` a receiver of layout id `layout` is. The arms before the last are compared in the plan's order;
/// the last arm is the default, taken without a compare. The receiver is an instance of one of the site's classes
/// (the plan's closed union), and its class's header is recorded, so when no earlier arm's header names it, it is
/// the last arm's class. A header of 0 is a class no construction has recorded, which matches nothing.
#[inline(always)]
fn planned_arm(site: &PlannedSite, layout: u32) -> &PlannedArm {
    let (last, others) = site.arms.split_last().expect("a planned site names at least one class");
    for arm in others {
        // SAFETY: a header cell lives in its program's class table, which the program's entry owns with this view.
        let header = unsafe { (*arm.header).get() };
        if header != 0 && header == layout {
            return arm;
        }
    }
    last
}

/// Resolves the planned arms of `class` (a constructor's function id) in `program` at the edge that records its
/// header: `this`, the instance just constructed, names the prototype that holds each arm's method. Each arm's
/// method is a proven field of that prototype (`prototype_layout`, no other writes), so the closure it reads is the
/// arm's for the program's life, and its target is the closure's own resolution (`resolve_call_target_in_lane`), whose
/// function and entry the plan's form fixes: a framed method call, which activates its body in its frame's
/// registers (`Framed::Activates`). Each check below is the edge's fact, made once per class; a violated fact stops
/// the run rather than leave a header that a row would enter with an unresolved target.
fn resolve_planned_class(vm: &mut Vm, program: ProgramId, class: u32, this: Value) {
    vm.programs.register_planned_targets(program);
    let arms = vm.programs.planned_targets_of_class(program, class);
    if arms.is_empty() {
        return;
    }
    let prototype = vm.object_data(this).map(|object| object.proto()).expect("a constructed instance is an object");
    let Some(code) = vm.programs.residual(program) else { return };
    // SAFETY: the program store keeps a program's residual alive with the program, which outlives this resolution.
    let code: &quench_runtime::bytecode::ResidualProgram = unsafe { &*std::ptr::from_ref(code) };
    for (function, method, slot) in arms {
        let closure = vm.own_property(prototype, method)
            .expect("a planned arm's method is an own data property of its class's prototype (`prototype_layout`)");
        let target = vm.resolve_call_target_in_lane(code, closure, true)
            .expect("a planned arm's method is a script closure that resolves (`resolve_call_target_in_lane`)");
        assert_eq!(target.function, function, "a planned arm's method is the arm's function");
        assert!(target.entry == CallEntry::framed(Framed::Activates), "a framed method call's target activates its body");
        // SAFETY: the slot is the program store's and lives with the program; a row reads it only after this edge
        // records the class's header, so no reader holds it while it is set.
        unsafe { (*slot).target.set(target) };
    }
    vm.programs.planned_target_set(program);
}

/// The layer entry of `quench_runtime::vm::layer_hooks::planned_class_recorded`: resolves the planned arms of a class
/// whose header was just recorded.
#[unsafe(no_mangle)]
fn quench_layer_planned_class_recorded(vm: &mut Vm, program: ProgramId, class: u32, this: Value) {
    resolve_planned_class(vm, program, class, this);
}
const _: quench_runtime::vm::layer_hooks::signatures::planned_class_recorded = quench_layer_planned_class_recorded;

script_handler! {
    /// A method call at a planned site, entered through its receiver's class arm. The receiver's method is the arm's
    /// function by the plan's fact (a method slot with no other writes, `Miss::None`), so the record's callee is not
    /// compared. The arm's target was resolved when its class's header was recorded (`resolve_planned_class`), so the
    /// call activates it directly; the activation itself declines only a call whose arguments or window do not fit,
    /// which takes the call's full row.
    #[inline(never)]
    pub(crate) fn planned_call_row<>(cx, r, ip, i, view) {
        if !unsafe { (*cx).script_calls } {
            return exit(ip);
        }
        let vm = unsafe { vm(cx) };
        // SAFETY: `mode` names the site of the view this record belongs to (`specialize_script_calls`).
        let site = unsafe { &*(i.mode as usize as *const PlannedSite) };
        // A receiver that is no object is outside the plan's union, which the facts exclude.
        let Some(layout) = vm.object_data(r.get(i.c)).map(|object| object.layout_id()) else {
            sibling!(script_call_full_miss, r, ip, Carried::script_in(cx, view))
        };
        let arm = planned_arm(site, layout);
        // SAFETY: the arm's slot was resolved at the edge that recorded its class's header, which precedes any receiver
        // of the class reaching this site; the program store keeps the slot with the program.
        let target = unsafe { &*(*arm.target).target.as_ptr() };
        match activate_unproven_in_lane::<ANY_ARGC>(cx, r, ip, i, target) {
            Some(resumed) => resume!(cx, resumed),
            None => sibling!(script_call_full_miss, r, ip, Carried::script_in(cx, view)),
        }
    }
}
