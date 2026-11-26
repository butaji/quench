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
    let mut kind = None;
    for pc in pcs {
        // SAFETY: `pc` names an instruction of `view`'s function, whose record it holds; a typed record's immediate is its kind.
        let record = unsafe { &*view.records.base().add(pc) };
        let Some(next) = (crate::dispatch_fast::is_typed_element_row(record.handler, true) || crate::dispatch_fast::is_typed_element_row(record.handler, false)).then(|| quench_runtime::staged::typed_elements::native_kind_at(unsafe { record.operand.imm }).kind) else { continue };
        if kind.is_some_and(|kind| kind != next) { return None; }
        kind = Some(next);
    }
    kind
}

/// The element accesses of `unit` whose records in `function`'s own code were switched to a typed kind
/// (`requicken_typed`), each as an observed access (`facts::ObservedRead`) of that kind: a read is typed by the
/// kind's element type, so an Int32Array read defines an int32, not a Number the body converts at every use; a
/// write types nothing (`Type::ANY`). Either proves its view for that kind alone (`Observed::kind_at`). An access
/// of any other base leaves at the view's proof, and a read of any other element at its type's.
pub(crate) fn typed_reads(vm: &Vm, function: u32, unit: &super::lane_unit::Unit) -> Vec<super::facts::ObservedRead> {
    let Some(view) = vm.programs.views_in_lane(vm.active_program).and_then(|views| views.get(function as usize))
        .and_then(quench_runtime::vm::program_store::derived_lane_view) else { return Vec::new() };
    unit.instructions.iter().zip(&unit.origins).enumerate().filter_map(|(at, (instruction, origin))| {
        let super::lane_unit::Origin::Caller(pc) = origin else { return None };
        let reads = match crate::compiled::performed(instruction) {
            Some(quench_runtime::staged::Operation::ElementRead { .. }) => true,
            Some(quench_runtime::staged::Operation::ElementWrite { .. }) => false,
            _ => return None,
        };
        // SAFETY: `pc` names an instruction of `view`'s function, whose record it holds; a typed record's immediate is its kind.
        let record = unsafe { &*view.records.base().add(*pc as usize) };
        crate::dispatch_fast::is_typed_element_row(record.handler, reads).then(|| quench_runtime::staged::typed_elements::native_kind_at(unsafe { record.operand.imm }))
            .map(|native| super::facts::ObservedRead { pc: at, ty: if reads { native.element_type() } else { quench_runtime::typed_ir::Type::ANY }, kind: Some(native.kind) })
    }).collect()
}
