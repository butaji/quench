//! Protocol misses of private names and defined fields (`protocol_family::PRIVATE`).

use super::super::*;

/// The miss body of a record of this family with static parameters `parameters`
/// (`protocol_row::<H, OP, ACC, { protocol_family::PRIVATE | parameters }>`).
pub(in crate::vm::dispatch_fast) const fn private_miss_of<H: Host>(parameters: u8) -> Handler {
    match parameters {
        _ => lane_general_step::<H>,
    }
}
