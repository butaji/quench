//! The lane dispatch loop, in a file of its own: every lane entry inlines it, so a line
//! shift above it in `dispatch_fast.rs` re-codegened ~190 s of the quench crate (rustc
//! hashes absolute spans). Keep widely inlined hubs out of large, often edited files.

use super::*;

/// Dispatch lane records from `ip` until a handler exits; the record it exits at.
#[inline(always)]
pub(super) fn dispatch_in_lane<H: Host>(cx: *mut LaneContext<H>, mut ip: Ip) -> Ip {
    loop {
        #[cfg(feature = "profile-lane-exits")]
        {
            // The record's opcode from its packed instruction: the census needs the mix, and
            // the lane record keeps only the handler.
            let function = running_function_in_lane(cx);
            let code = unsafe { &*(*cx).code };
            let pc = unsafe { ip.offset_from((*cx).base) as usize };
            if let Some(body) = code.function_get(function as usize)
                && let Some(packed) = body.code.get(pc)
            {
                super::super::lane_exits::dispatch(body.widened(*packed).op());
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
            crate::value::IntegerEncoding::TAG,
            acc,
        );
        let (ip_next, left) = exited(next);
        if left {
            return ip_next;
        }
        ip = ip_next;
    }
}
