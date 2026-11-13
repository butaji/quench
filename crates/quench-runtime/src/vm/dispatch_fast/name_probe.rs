//! By-name reads whose scope the compiler left open: the probe that asks the general path
//! where the name lands, and the record it quickens to.
use super::*;

script_handler! {
    /// A `LoadName` whose scope the compiler left open (an eval's code, which runs in its
    /// caller's scope, or a function with an eval of its own): the lane cannot search the
    /// scope from an activation without a frame, so the read exits to the general path, which
    /// decides whether the name lands on the global object and quickens the record
    /// accordingly (`Vm::settle_name_probe`).
    #[inline(never)]
    fn lane_load_name_probe<KIND: u8>(cx, r, ip, i, view) {
        lane_materialize_running_leaf(cx);
        let vm = unsafe { vm(cx) };
        vm.scope_names.probe = Some(crate::vm::name_scope::NameProbe {
            program: unsafe { (*cx).program },
            function: lane_running_function(cx),
            pc: unsafe { ip.offset_from((*cx).base) } as u32,
        });
        exit(ip)
    }
}

/// Quickens the record of the by-name read at `pc` of `view`, which `Vm::settle_name_probe`
/// settled: a hinted global read where the name lands on the global object, else an exit.
pub(in crate::vm) fn settle_name_record<H: Host>(view: &LaneView, pc: usize, op: Op, lands: bool) {
    let handler = match (lands, op.class()) {
        (false, _) => lane_exit::<H> as Handler,
        (true, Class::LoadName) => lane_load_global_name::<H, LOAD_NAME, true> as Handler,
        (true, Class::LoadNameCall) => lane_load_global_name::<H, LOAD_NAME_CALL, true> as Handler,
        (true, _) => lane_load_global_name::<H, LOAD_NAME_TYPEOF, true> as Handler,
    };
    // SAFETY: `pc` is a record of the view, whose records live while its program does.
    requicken::<H>(unsafe { view.records().add(pc) }, handler);
}

/// The handler of a by-name read of kind `KIND` whose scope is left to the general path's read.
pub(super) const fn probe_handler<H: Host, const KIND: u8>() -> Handler {
    lane_load_name_probe::<H, KIND>
}
