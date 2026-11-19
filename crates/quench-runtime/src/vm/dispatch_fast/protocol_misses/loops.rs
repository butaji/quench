//! Protocol misses of loop headers and numeric protocol records (`protocol_family::LOOP`).

use super::super::*;

/// The miss body of a record of this family with static parameters `parameters`
/// (`protocol_row::<H, OP, ACC, { protocol_family::LOOP | parameters }>`).
pub(in crate::vm::dispatch_fast) const fn loop_miss_of<H: Host>(parameters: u8) -> Handler {
    match parameters {
        _ => lane_general_step::<H>,
    }
}
