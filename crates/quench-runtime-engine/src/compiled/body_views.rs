//! The element view a compiled body's loop arrays take (`lane_region::lower_body`'s `RegionPlan::views`), read from the
//! interpreter's element records: a typed array read in a loop holds its view, not a dense proof that leaves at every entry.
use quench_runtime::vm::Vm;

/// The view a body's loop arrays take: the typed kind the element records of `function`'s own code were switched to
/// (`quickened_typed_kind`), else dense. A typed array read in a loop then holds its view, not a dense proof that leaves.
pub(crate) fn body_views(vm: &Vm, function: u32, unit: &super::lane_unit::Unit) -> quench_runtime::vm::engine_types::ElementViews {
    let view = vm.programs.views_in_lane(vm.active_program).and_then(|views| views.get(function as usize)).and_then(quench_runtime::vm::program_store::derived_lane_view);
    let sites = unit.instructions.iter().zip(&unit.origins).filter_map(|(instruction, origin)| match (crate::compiled::performed(instruction), origin) {
        (Some(quench_runtime::staged::Operation::ElementRead { .. } | quench_runtime::staged::Operation::ElementWrite { .. }), super::lane_unit::Origin::Caller(pc)) => Some(*pc as usize),
        _ => None,
    });
    view.and_then(|view| quickened_typed_kind(view, sites))
        .map_or(quench_runtime::vm::engine_types::ElementViews::Dense, |kind| quench_runtime::vm::engine_types::ElementViews::Typed(Some(kind)))
}

/// The typed kind the element records of `view` at `pcs` were switched to (`requicken_typed`), where they agree: the
/// view a compiled body's loop arrays take (`lane_region::lower_body`). A record no run reached yet (a loop not entered
/// so far) names no kind. `None` where none is typed, or two kinds are.
fn quickened_typed_kind(view: &crate::dispatch_fast::LaneView, pcs: impl Iterator<Item = usize>) -> Option<quench_runtime::tier_surface::TypedArrayKind> {
    let typed = [crate::dispatch_fast::element_typed_read_row() as *const (), crate::dispatch_fast::element_typed_write_row() as *const ()];
    let mut kind = None;
    for pc in pcs {
        // SAFETY: `pc` names an instruction of `view`'s function, whose record it holds; a typed record's immediate is its kind.
        let record = unsafe { &*view.records.base().add(pc) };
        let Some(next) = typed.contains(&record.handler).then(|| quench_runtime::staged::typed_elements::native_kind_at(unsafe { record.operand.imm }).kind) else { continue };
        if kind.is_some_and(|kind| kind != next) { return None; }
        kind = Some(next);
    }
    kind
}
