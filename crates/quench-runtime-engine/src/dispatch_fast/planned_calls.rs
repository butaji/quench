//! The planned call row (`planned_call_row`): a method call whose receiver's class union is closed (`PlannedSite`,
//! `compile::call_plan`) is dispatched on the receiver's class header, not on the call's cache. A receiver is an
//! instance of one planned class, and its layout id names the class: the arm whose class header
//! (`ProgramStore::class_header`) holds that id is the call's arm, and the last class of the union is the default arm.
//! Each arm's target is resolved at the class-header edge (`resolve_planned_class`), from the prototype the class's
//! instances hold, and the program store keeps it (`PlannedTarget`). The derivation admits a site only where every
//! arm's function enters as a framed method call (`planned_arm_admits`), so the edge and the row cannot fail.
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use quench_runtime::bytecode::ResidualProgram;
use quench_runtime::vm::ProgramId;
use quench_runtime::vm::engine_types::{PlannedArm, PlannedSite, FIXED_ARGUMENT_COPIES};

/// The arm of `site` a receiver of layout id `layout` is: the first arm whose class header names it, else the default
/// arm. A header of 0 is a class no construction has recorded, which matches nothing.
#[inline(always)]
fn planned_arm(site: &PlannedSite, layout: u32) -> &PlannedArm {
    site.arms.iter().find(|arm| {
        // SAFETY: a header cell lives in its program's class table, which the program's entry owns with this view.
        let header = unsafe { (*arm.header).get() };
        header != 0 && header == layout
    }).unwrap_or(&site.default)
}

/// Whether the arm of function `function` of `code` enters as a framed method call with `argc` arguments: the
/// derivation's admission of a planned site (`ProgramStore::planned_call_sites`), decided by the resolver's own forms
/// (`body_entry`, `call_plan`). A framed call form takes `Framed::Activates` for a body that is not async, rests or
/// keeps its arguments, runs in its registers and constructs nothing, so its resolution (`resolve_call_target_in_lane`)
/// is that entry. The call passes exactly the parameters the body takes, within the fixed copies, so the activation
/// fills its window and takes no keeping path.
pub(crate) fn planned_arm_admits(code: &ResidualProgram, function: u32, argc: u16) -> bool {
    let Some(body) = code.function_get(function as usize) else { return false };
    let plan = body.call_plan(code);
    body.params == argc
        && argc <= FIXED_ARGUMENT_COPIES
        && !body.is_async
        && !body.rest
        && !plan.keeps_arguments
        && plan.registers_only
        && body_entry(body, &code.constants, false, false) == CallEntry::framed(Framed::Activates)
        && !body.code.iter().any(|packed| quench_runtime::staged::constructs(body.widened(*packed).op()))
}

/// Resolves the planned arms of `class` (a constructor's function id) in `program` at the class-header edge: `prototype`
/// is the prototype its instances hold, which holds each arm's method as an own data property (`prototype_layout`, no
/// other writes). Each arm's target is the closure's own resolution (`resolve_call_target_in_lane`), which the derivation
/// proved a framed activation of the arm's function. The slot is written here, once per class, before any receiver of
/// the class reaches a planned row.
fn resolve_planned_class(vm: &mut Vm, program: ProgramId, class: u32, prototype: Value) {
    vm.programs.register_planned_targets(program, planned_arm_admits);
    let arms = vm.programs.planned_targets_of_class(program, class);
    let Some(code) = vm.programs.residual(program) else { return };
    // SAFETY: the program store keeps a program's residual alive with the program, which outlives this resolution.
    let code: &ResidualProgram = unsafe { &*std::ptr::from_ref(code) };
    // The class's layout, recorded by the edge before this resolution (`record_class_edge`): the method vector's key.
    let layout = vm.programs.class_header(program, class).map_or(0, std::cell::Cell::get);
    for (function, method, slot) in arms {
        let Some(closure) = vm.own_property(prototype, method) else {
            debug_assert!(false, "a planned arm's method is an own data property of its class's prototype");
            continue;
        };
        let Some(target) = vm.resolve_call_target_in_lane(code, closure) else {
            debug_assert!(false, "a planned arm's method is a script closure that resolves");
            continue;
        };
        debug_assert_eq!(target.body.function, function, "a planned arm's method is the arm's function");
        debug_assert!(target.body.entry == CallEntry::framed(Framed::Activates), "a framed method call's target activates its body");
        // The class's method vector entry for the method, where the method has a selector (`class_method_record`).
        if let (true, Some(selector)) = (layout != 0, vm.programs.selectors(program).and_then(|selectors| selectors.get(method))) {
            if let Some(record) = vm.class_method_record(code, program, class, method, function, closure, target) {
                vm.programs.set_class_method(program, layout, selector, record);
            }
        }
        // SAFETY: the slot is the program store's and lives with the program; a row reads it only after this edge
        // records the class's header, so no reader holds it while it is set.
        unsafe { (*slot).target.set(target) };
    }
    vm.programs.planned_target_set(program);
}

/// The layer entry of `quench_runtime::vm::layer_hooks::planned_class_recorded`: resolves the planned arms of a class
/// whose header was just recorded, or restored.
#[unsafe(no_mangle)]
fn quench_layer_planned_class_recorded(vm: &mut Vm, program: ProgramId, class: u32, prototype: Value) {
    resolve_planned_class(vm, program, class, prototype);
}
const _: quench_runtime::vm::layer_hooks::signatures::planned_class_recorded = quench_layer_planned_class_recorded;

script_handler! {
    /// A method call at a planned site, entered through its receiver's class arm. The receiver's method is the arm's
    /// function by the plan's fact (a method slot with no other writes, `Miss::None`), so the record's callee is not
    /// compared. The arm's target was resolved at the class-header edge, so the call activates it directly; the
    /// activation declines only a receiver whose binding reads the heap, which takes the call's full row, and grows the
    /// lane stack where it has no room, as a normal call does.
    #[inline(never)]
    pub(crate) fn planned_call_row<>(cx, r, ip, i, view) {
        if !unsafe { (*cx).script_calls } {
            return exit(ip);
        }
        let vm = unsafe { vm(cx) };
        // SAFETY: `mode` names the site of the view this record belongs to (`specialize_script_calls`).
        let site = unsafe { &*(i.mode as usize as *const PlannedSite) };
        // The receiver is an instance of one of the site's classes (the plan's closed union), so it is an object.
        let receiver = vm.heap_reads().object(r.get(i.c));
        debug_assert!(receiver.is_some(), "a planned site's receiver is an instance of its plan's classes");
        // SAFETY: as above.
        let layout = unsafe { receiver.unwrap_unchecked() }.layout_id();
        let arm = planned_arm(site, layout);
        // SAFETY: the arm's slot was resolved at the class-header edge that recorded its class, which precedes any receiver
        // of the class reaching this site; the program store keeps the slot with the program.
        let target = unsafe { &*(*arm.target).target.as_ptr() };
        match activate_unproven_in_lane::<ANY_ARGC>(cx, r, ip, i, target) {
            Some(resumed) => resume!(cx, resumed),
            None => sibling!(script_call_full_miss, r, ip, Carried::script_in(cx, view)),
        }
    }
}
