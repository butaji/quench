//! Protocol misses of forwarded and kept calls, tier prologues and body entries (`protocol_family::ENTRY`).

use super::super::*;

/// Parameter codes of the call records (`Op::Call`, `Op::CallForwardingArguments`).
/// A call of a target that keeps its activation (`script_call_keeping_miss`).
pub(in crate::dispatch_fast) const SCRIPT_CALL_KEEPING: u8 = 0;
/// A forwarding call (`forward_call_miss`).
pub(in crate::dispatch_fast) const FORWARD_CALL: u8 = 1;
/// A Math call of Numbers (`call_math_miss`).
pub(in crate::dispatch_fast) const CALL_MATH: u8 = 0x82;
/// An array callback call whose result nothing reads (`call_array_callback_miss::<true>`).
pub(in crate::dispatch_fast) const CALL_ARRAY_CALLBACK_DISCARDING: u8 = 0x80;
/// An array callback call whose result is read (`call_array_callback_miss::<false>`).
pub(in crate::dispatch_fast) const CALL_ARRAY_CALLBACK: u8 = 0x81;
/// A frameless activation's tier prologue (`tier_prologue_in_lane`, the `TierPrologue` record).
pub(in crate::dispatch_fast) const TIER_PROLOGUE: u8 = 2;
/// A specialized body's prologue (`body_entry_in_lane`, the `BodyEntry` record).
pub(in crate::dispatch_fast) const BODY_ENTRY: u8 = 3;

/// The miss body of a record of this family with static parameters `parameters`
/// (`protocol_row::<OP, ACC, { protocol_family::ENTRY | parameters }>`).
pub(in crate::dispatch_fast) const fn entry_miss_of(parameters: u8) -> Handler {
    match parameters {
        SCRIPT_CALL_KEEPING => super::super::script_call_keeping_miss as Handler,
        FORWARD_CALL => super::super::forward_call_miss as Handler,
        CALL_MATH => super::super::call_math_miss as Handler,
        CALL_ARRAY_CALLBACK_DISCARDING => super::super::call_array_callback_miss::<true> as Handler,
        CALL_ARRAY_CALLBACK => super::super::call_array_callback_miss::<false> as Handler,
        TIER_PROLOGUE => super::super::tier_prologue_in_lane as Handler,
        BODY_ENTRY => super::super::body_entry_in_lane as Handler,
        _ => general_step_in_lane,
    }
}
