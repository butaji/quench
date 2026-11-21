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
    fn name_probe_miss<KIND: u8>(cx, r, ip, i, view) {
        materialize_running_leaf_in_lane(cx);
        let vm = unsafe { vm(cx) };
        vm.scope_names.probe = Some(crate::vm::name_scope::NameProbe {
            program: unsafe { (*cx).program },
            function: running_function_in_lane(cx),
            pc: unsafe { ip.offset_from((*cx).base) } as u32,
        });
        exit(ip)
    }
}

/// Quickens the record of the by-name read at `pc` of `view`, which `Vm::settle_name_probe`
/// settled: a hinted global read where the name lands on the global object, else an exit.
pub(in crate::vm) fn settle_name_record(view: &LaneView, pc: usize, op: Op, lands: bool) {
    let handler = match (lands, crate::staged::name_load(op)) {
        (false, _) => exit_lane as Handler,
        (true, Some(load)) => name_row::<FLAG_NAME_GLOBAL_SCOPED>(load),
        (true, None) => name_row::<FLAG_NAME_GLOBAL_SCOPED>(crate::staged::NameLoad::Typeof),
    };
    // SAFETY: `pc` is a record of the view, whose records live while its program does.
    requicken(unsafe { view.records().add(pc) }, handler);
}

/// The miss arm of a by-name read of kind `KIND` at `FLAG_PROBE`: its scope is left to the general path's read.
pub(super) const fn probe_handler<const KIND: u8>() -> Handler {
    name_probe_miss::<KIND>
}
