//! Protocol misses of forwarded and kept calls, tier prologues and body entries (`protocol_family::ENTRY`).

use super::super::*;

/// Parameter codes of the call records (`Op::Call`, `Op::CallForwardingArguments`).
/// A call of a target that keeps its activation (`script_call_keeping_miss`).
pub(in crate::vm::dispatch_fast) const SCRIPT_CALL_KEEPING: u8 = 0;
/// A forwarding call (`forward_call_miss`).
pub(in crate::vm::dispatch_fast) const FORWARD_CALL: u8 = 1;
/// A Math call of Numbers (`call_math_miss`).
pub(in crate::vm::dispatch_fast) const CALL_MATH: u8 = 0x82;
/// An array callback call whose result nothing reads (`call_array_callback_miss::<true>`).
pub(in crate::vm::dispatch_fast) const CALL_ARRAY_CALLBACK_DISCARDING: u8 = 0x80;
/// An array callback call whose result is read (`call_array_callback_miss::<false>`).
pub(in crate::vm::dispatch_fast) const CALL_ARRAY_CALLBACK: u8 = 0x81;

/// The miss body of a record of this family with static parameters `parameters`
/// (`protocol_row::<H, OP, ACC, { protocol_family::ENTRY | parameters }>`).
pub(in crate::vm::dispatch_fast) const fn entry_miss_of<H: Host>(parameters: u8) -> Handler {
    match parameters {
        SCRIPT_CALL_KEEPING => super::super::script_call_keeping_miss::<H> as Handler,
        FORWARD_CALL => super::super::forward_call_miss::<H> as Handler,
        CALL_MATH => super::super::call_math_miss::<H> as Handler,
        CALL_ARRAY_CALLBACK_DISCARDING => super::super::call_array_callback_miss::<H, true> as Handler,
        CALL_ARRAY_CALLBACK => super::super::call_array_callback_miss::<H, false> as Handler,
        _ => general_step_in_lane::<H>,
    }
}
