//! Protocol misses of script calls (`protocol_family::SCRIPT_CALL`).

use super::super::*;

/// The miss body of a record of this family with static parameters `parameters`
/// (`protocol_row::<H, OP, ACC, { protocol_family::SCRIPT_CALL | parameters }>`).
pub(in crate::vm::dispatch_fast) const fn script_call_miss_of<H: Host>(parameters: u8) -> Handler {
    match parameters {
        _ => lane_general_step::<H>,
    }
}
