//! Reads that meet an accessor: answered without an activation where the getter's call runs
//! no guest code.
use super::*;

/// A read whose site caches an accessor (`Vm::cached_field_getter`) whose getter is a pure
/// native read (`Vm::pure_native_getter_value`) or a frameless read target, answered as the
/// getter's call would return (`lane_call_read`): `[[Get]]` calls it on the base with no
/// arguments, and neither runs guest code. `None` leaves the read to the general path, which
/// calls the getter.
///
/// # Safety
/// `cx` is the running lane's context.
#[inline(never)]
pub(super) unsafe fn lane_getter_read<H: Host>(
    cx: *mut LaneContext<H>,
    object: Value,
    atom: crate::bytecode::Atom,
    site: u16,
) -> Option<Value> {
    let getter = {
        let vm = unsafe { vm(cx) };
        vm.cached_field_getter(object, atom, vm.field_cache_index(site))?
    };
    unsafe { lane_getter_answer(cx, getter, object) }
}

/// What `getter` returns for a read of `object` where its call runs no guest code: a pure
/// native read, or a frameless read target (`lane_getter_read`).
///
/// # Safety
/// `cx` is the running lane's context.
#[inline(never)]
unsafe fn lane_getter_answer<H: Host>(cx: *mut LaneContext<H>, getter: Value, object: Value) -> Option<Value> {
    if let Some(value) = (unsafe { vm(cx) }).pure_native_getter_value(getter, object) {
        return Some(value);
    }
    let target = lane_script_target(cx, getter)?;
    if target.entry == CallEntry::Reads {
        // A getter only the general path has called has not run its read in the lane yet: it
        // takes the kind its first run would.
        // SAFETY: as `lane_call_read`; the context's program outlives the lane run.
        let (entry, code) = unsafe { ((*target.view).records(), &*(*cx).code) };
        if unsafe { (*entry).handler } == lane_returning_field_first::<H, NOT_LEAF> as *const () {
            requicken::<H>(entry, returning_field_first_kind::<H, NOT_LEAF>(unsafe { vm(cx) }, code, unsafe { &*entry }));
        }
    } else if !target.entry.answered_by_read() {
        return None;
    }
    let this = lane_direct_call_this(unsafe { vm(cx) }, &target, object)?;
    unsafe { lane_call_read(cx, &target, ReadArguments::NONE, this) }
}

/// A lane read of `base` at the record `i` (at `ip`) that met `getter`: its value where a read
/// answers the getter's call (`lane_getter_answer`), else the getter's activation
/// (`lane_activate_getter`), whose completion is the read's value.
pub(super) fn lane_read_getter<H: Host>(
    cx: *mut LaneContext<H>,
    r: Registers,
    ip: Ip,
    i: &LaneInstruction,
    getter: Value,
    base: Value,
) -> Result<(Registers, Ip), Ip> {
    if let Some(value) = unsafe { lane_getter_answer(cx, getter, base) } {
        r.set(i.a, value);
        return Ok((r, following(ip)));
    }
    lane_activate_getter(cx, r, ip, getter, base)
}
