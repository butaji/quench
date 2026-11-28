//! Field and call reads in a lane: call reads, captured reads, and answered calls.
use super::*;

/// The value a frameless read target (`CallEntry::reads`) returns for a call passing `arguments.get(k)` as
/// parameter `k` (`None` where not passed) on `this`, where the reads its records have quickened to hit. Answering it at the call
/// leaves nothing for an activation to observe: each hit runs no guest code and allocates
/// nothing. `None` for any other record state or a miss, which the call activates to run.
///
/// # Safety
/// `cx` is the running lane's context and `target` a target of its program.
#[inline(always)]
pub(in super::super) unsafe fn call_read_in_lane(
    cx: *mut LaneContext,
    target: &ScriptCallTarget,
    arguments: ReadArguments,
    this: Value,
) -> Option<Value> {
    // SAFETY: the context's program outlives the lane run.
    let (vm, code) = unsafe { (vm(cx), &*(*cx).code) };
    match target.entry {
        // The receiver the call binds (`direct_call_this_in_lane`), which is what it returns.
        _ if target.entry == CallEntry::frameless(Frameless::ReadsThis) => Some(this),
        // SAFETY: a capture-read target of the running program.
        _ if target.entry == CallEntry::frameless(Frameless::ReadsCapture) => unsafe { read_capture_in_lane(vm, target) },
        // SAFETY: as for `LeafRead::of`.
        _ => unsafe { LeafRead::of(target) }?.run(vm, code, arguments, this),
    }
}

/// The capture a `CallEntry::frameless(Frameless::ReadsCapture)` target's closure environment holds, read as its
/// frameless activation's `LoadCapture` would (`load_capture_walk_miss`); an uninitialized
/// binding misses.
///
/// # Safety
/// `target` is a `CallEntry::frameless(Frameless::ReadsCapture)` target of the running program, whose view holds
/// its `LoadCapture` record first.
#[inline(never)]
pub(in super::super) unsafe fn read_capture_in_lane(vm: &mut Vm, target: &ScriptCallTarget) -> Option<Value> {
    // SAFETY: the program store keeps a derived view alive with its program.
    let imm = unsafe { &*(*target.view).records() }.imm();
    let (depth, slot) = (ImmediateLayout::capture_depth(imm), ImmediateLayout::capture_slot(imm));
    let slots = capture_reads(target.capture);
    if depth == 0 && !slots.is_null() {
        // SAFETY: as `ScriptCallTarget::capture`.
        return unsafe { quench_runtime::tier_surface::EnvironmentSlots::read_storage(slots, slot) };
    }
    let (slots, root) = vm.walked_capture_slots(target.env, false, depth)?;
    // SAFETY: as `Vm::walked_capture_slots`; no collection runs in the lane.
    vm.owned_capture(unsafe { &*slots }, root, slot)
}

/// The values a call that a read answers passed (`call_read_in_lane`): parameter `k` is the
/// call window's register `base + k` while `k < passed`, else unpassed. A concrete value,
/// not a closure, so the reads it feeds are one copy whatever forwarded them.
/// `read_answer_in_lane` of a call's read, out of line: the read's calls take stack temporaries,
/// which a handler inlining them would hold on its common path.
///
/// # Safety
/// As `call_read_in_lane`.
#[inline(never)]
pub(in super::super) unsafe fn answered_call_in_lane(
    cx: *mut LaneContext,
    target: &ScriptCallTarget,
    arguments: ReadArguments,
    this: Value,
) -> Option<Value> {
    // SAFETY: the caller's contract.
    unsafe { read_answer_in_lane(cx, target, arguments, this) }
}

/// The value `target`'s read answers a call with, or what a capture append returns.
///
/// # Safety
/// As `call_read_in_lane`.
#[inline(always)]
pub(in super::super) unsafe fn read_answer_in_lane(
    cx: *mut LaneContext,
    target: &ScriptCallTarget,
    arguments: ReadArguments,
    this: Value,
) -> Option<Value> {
    if target.entry == CallEntry::frameless(Frameless::AppendsCapture) {
        return unsafe { capture_append::run(cx, target, arguments) };
    }
    unsafe { call_read_in_lane(cx, target, arguments, this) }
}

/// Every bound own-field read (`own_formed_handler`), of `this` and of a register.
pub(in super::super) fn own_bound_handlers() -> [*const (); OWN_FORMS.len() + OWN_FORMS.len()] {
    if cfg!(not(feature = "jit")) { return [std::ptr::null(); OWN_FORMS.len() + OWN_FORMS.len()]; }
    macro_rules! row { ($site:expr) => { generated_row!(FIELD_LEAF_READ, ACC_NONE, $site) as *const () } }
    own_sites!(row)
}

/// The object a read body's field record names: `this`, or the parameter its activation
/// would hold in that register; `None` for an unpassed one, whose `undefined` no read hits.
#[inline(always)]
pub(in super::super) fn read_base_in_lane(
    base: quench_runtime::bytecode::FieldBase,
    arguments: ReadArguments,
    this: Value,
) -> Option<Value> {
    match base.register_index() {
        Some(register) => arguments.get(register),
        // A local base (`FieldBase::local`) names no activation register; the call runs.
        None if base.local_index().is_some() => None,
        None => Some(this),
    }
}
