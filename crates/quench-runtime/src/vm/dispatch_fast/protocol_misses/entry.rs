//! Protocol misses of forwarded and kept calls, tier prologues and body entries (`protocol_family::ENTRY`).

use super::super::*;

/// The miss body of a record of this family with static parameters `parameters`
/// (`protocol_row::<H, OP, ACC, { protocol_family::ENTRY | parameters }>`).
pub(in crate::vm::dispatch_fast) const fn entry_miss_of<H: Host>(parameters: u8) -> Handler {
    match parameters {
        _ => lane_general_step::<H>,
    }
}
