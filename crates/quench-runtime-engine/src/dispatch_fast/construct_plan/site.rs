//! The construct site a lane construction runs at, and the class it constructs under (`Vm::construct_site_class`).
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;

/// The class of the construction of `callee` at the record `ip` of the running lane: the class the site's
/// program, function and PC name (`Vm::construct_site_class`), or the callee's own body where the site has no fact
/// (`Vm::ordinary_class`). A plan for `callee` serves only constructions of this class.
///
/// # Safety
/// `cx` is the running lane's context and `ip` a record of its view.
#[inline(always)]
pub(crate) unsafe fn site_class_in_lane(cx: *mut LaneContext, ip: Ip, callee: Value) -> u32 {
    // SAFETY: the caller's contract.
    let pc = unsafe { record_pc(cx, ip) };
    let (program, function) = (unsafe { (*cx).program }, running_function_in_lane(cx));
    let vm = unsafe { vm(cx) };
    vm.construct_site_class(program, function, pc, vm.ordinary_class(callee))
}

/// `planned_construction_in_lane` for the construction the lane's record `i` at `ip` makes, as its site names its class.
///
/// # Safety
/// `cx` is the running lane's context and `ip` the record of `i`.
#[inline(always)]
pub(crate) unsafe fn planned_site_in_lane(vm: &mut Vm, cx: *mut LaneContext, ip: Ip, r: Registers, i: &LaneInstruction) -> Option<Value> {
    let callee = r.get(i.b);
    // SAFETY: the caller's contract.
    let class = unsafe { site_class_in_lane(cx, ip, callee) };
    planned_construction_in_lane(vm, callee, class, r, i)
}

/// `planned_inline_quick_in_lane` for the construction the lane's record `i` at `ip` makes, as its site names its class.
///
/// # Safety
/// As `planned_site_in_lane`.
#[inline(always)]
pub(crate) unsafe fn planned_quick_site_in_lane(vm: &mut Vm, cx: *mut LaneContext, ip: Ip, r: Registers, i: &LaneInstruction) -> Option<Value> {
    let callee = r.get(i.b);
    // SAFETY: the caller's contract.
    let class = unsafe { site_class_in_lane(cx, ip, callee) };
    planned_inline_quick_in_lane(vm, callee, class, r, i)
}

/// `initialize_fields_in_lane` for the construction the lane's record `i` at `ip` makes by `function`, as its site
/// names its class.
///
/// # Safety
/// As `planned_site_in_lane`.
pub(crate) unsafe fn initialize_site_in_lane(vm: &mut Vm, cx: *mut LaneContext, ip: Ip, r: Registers, i: &LaneInstruction, function: u32) -> Option<(Value, bool)> {
    let callee = r.get(i.b);
    // SAFETY: the caller's contract; the context's program outlives the lane run.
    let class = unsafe { site_class_in_lane(cx, ip, callee) };
    initialize_fields_in_lane(vm, unsafe { &*(*cx).code }, callee, class, callee, function, construct_argument_in_lane(r, i))
}

/// `Vm::inline_construct_receiver` of `callee`, a construction at the lane's record `ip`, as its site names its class.
///
/// # Safety
/// As `planned_site_in_lane`.
pub(crate) unsafe fn inline_receiver_site_in_lane(vm: &mut Vm, cx: *mut LaneContext, ip: Ip, callee: Value) -> Option<Value> {
    // SAFETY: the caller's contract.
    let class = unsafe { site_class_in_lane(cx, ip, callee) };
    vm.inline_construct_receiver(callee, class)
}
