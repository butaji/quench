//! Field and call reads in a lane: the returning field hit, call reads, captured reads, and answered calls.
use super::*;

/// The cached read of a single-base returning `GetField` of `KIND` (`RETURNING_FIELD_OWN`,
/// `_POLYMORPHIC`, `_HOLDER`, `_INHERITED`) at `cache`: a hit, which runs no guest code.
#[inline(always)]
pub(in super::super) fn returning_field_hit<const KIND: u8>(
    vm: &mut Vm,
    code: &ResidualProgram,
    object: Value,
    cache: u16,
) -> Option<Value> {
    match KIND {
        RETURNING_FIELD_OWN => vm.primary_own_field_hit(code, object, cache),
        RETURNING_FIELD_POLYMORPHIC => vm.inline_polymorphic_field_hit(code, object, cache),
        RETURNING_FIELD_HOLDER => vm.primary_holder_field_hit(code, object, cache),
        _ => vm.inherited_field_hit(code, object, cache),
    }
}

/// The value a frameless read target (`CallEntry::reads`, `CallEntry::frameless(Frameless::ForwardsRead)`)
/// returns for a call passing `arguments.get(k)` as parameter `k` (`None` where not passed) on
/// `this`, where the reads its records have quickened to hit. Answering it at the call
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
    // A forwarding body's answer is its method's leaf read on the field it reads, so both
    // kinds reach one leaf read, which the call handler then holds once.
    let (target, arguments, this) = if target.entry == CallEntry::frameless(Frameless::ForwardsRead) {
        // SAFETY: as for `ForwardRead::of`.
        let (callee, receiver) = unsafe { ForwardRead::of(target) }?.method(vm, code, arguments, this)?;
        // SAFETY: as `Vm::script_call_entry`; a read resolves no target and collects nothing.
        let inner = unsafe { &*vm.script_call_entry(callee)? };
        if !inner.entry.reads() {
            return None;
        }
        let inner_this = direct_call_this_in_lane(vm, inner, receiver)?;
        // The call passes the parameters in order, so its argument `k` is parameter `k`.
        (inner, arguments.within(target.params), inner_this)
    } else {
        (target, arguments, this)
    };
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
/// `call_read_in_lane` of the call at `site`, memoized when it answers (`note_read_in_lane`). Out
/// of line: the read's calls take stack temporaries, which a handler inlining them would hold
/// on its common path.
///
/// # Safety
/// As `call_read_in_lane`.
#[inline(never)]
pub(in super::super) unsafe fn answered_call_in_lane(
    cx: *mut LaneContext,
    site: Ip,
    target: &ScriptCallTarget,
    arguments: ReadArguments,
    this: Value,
) -> Option<Value> {
    // SAFETY: the caller's contract.
    let value = unsafe { read_answer_in_lane(cx, target, arguments, this) };
    // A mutating operation must never enter the pure read memo.
    if value.is_some() && target.entry != CallEntry::frameless(Frameless::AppendsCapture) {
        note_read_in_lane(cx, site, target, arguments, this);
    }
    value
}

/// The value `target`'s read answers a call with, or what a capture append returns, without the
/// site's memo: what a call whose record has no memo form (a method call's) is answered by.
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
