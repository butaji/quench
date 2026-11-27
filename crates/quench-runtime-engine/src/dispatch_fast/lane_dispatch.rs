//! The lane dispatch loop, in a file of its own: every lane entry inlines it, so a line
//! shift above it in `dispatch_fast.rs` re-codegened ~190 s of the quench crate (rustc
//! hashes absolute spans). Keep widely inlined hubs out of large, often edited files.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;

/// Dispatch lane records from `ip` until a handler exits; the record it exits at.
#[inline(always)]
pub(crate) fn dispatch_in_lane(cx: *mut LaneContext, mut ip: Ip) -> Ip {
    loop {
        #[cfg(feature = "profile-lane-exits")]
        {
            // The record's opcode from its packed instruction: the census needs the mix, and
            // the lane record keeps only the handler.
            let function = running_function_in_lane(cx);
            let code = unsafe { &*(*cx).code };
            let pc = unsafe { record_pc(cx, ip) as usize };
            if let Some(body) = code.function_get(function as usize)
                && let Some(packed) = body.code.get(pc)
            {
                quench_runtime::vm::lane_exits::dispatch(body.widened(*packed).op());
            }
        }
        // SAFETY: handlers have returned, so no other access is live.
        let (view, registers, acc) = unsafe {
            let acc = std::mem::replace(&mut (*cx).resumed_acc, Slot::of(Value::UNDEFINED));
            ((*cx).memory, (*cx).registers, acc)
        };
        let next = handler(ip)(
            registers,
            ip,
            view.bytes,
            view.reach,
            quench_runtime::value::IntegerEncoding::TAG,
            acc,
        );
        let (ip_next, left) = exited(next);
        if left {
            // A record of an inlined view leaves it first: the general path runs the function's own
            // record, in the frames an inlined callee's makes (`inlined_views::leave`).
            if super::inlined_views::may_be_inlined(ip_next) {
                return super::inlined_views::leave_at_exit(cx, ip_next);
            }
            return ip_next;
        }
        ip = ip_next;
    }
}
