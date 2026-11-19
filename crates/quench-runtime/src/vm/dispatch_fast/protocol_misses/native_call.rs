//! Protocol misses of native, applied, spread and array calls (`protocol_family::NATIVE_CALL`).

use super::super::*;

/// The miss body of a record of this family with static parameters `parameters`
/// (`protocol_row::<H, OP, ACC, { protocol_family::NATIVE_CALL | parameters }>`).
pub(in crate::vm::dispatch_fast) const fn native_call_miss_of<H: Host>(parameters: u8) -> Handler {
    match parameters {
        _ => lane_general_step::<H>,
    }
}
