//! Protocol misses of native, applied, spread and array calls (`protocol_family::NATIVE_CALL`).

use super::super::*;

/// `native_call_miss`: a `Call` record whose callee is a native.
pub(in crate::vm::dispatch_fast) const NATIVE: u8 = 0;
/// `call_array_miss`: a `CallArray` record.
pub(in crate::vm::dispatch_fast) const CALL_ARRAY: u8 = 1;
/// `call_spread_miss`: a `CallSpread` record.
pub(in crate::vm::dispatch_fast) const CALL_SPREAD: u8 = 2;

/// The miss body of a record of this family with static parameters `parameters`
/// (`protocol_row::<OP, ACC, { protocol_family::NATIVE_CALL | parameters }>`).
pub(in crate::vm::dispatch_fast) const fn native_call_miss_of(parameters: u8) -> Handler {
    match parameters {
        NATIVE => native_call_miss as Handler,
        CALL_ARRAY => call_array_miss as Handler,
        CALL_SPREAD => call_spread_miss as Handler,
        _ => general_step_in_lane,
    }
}
