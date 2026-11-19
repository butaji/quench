//! Protocol misses of generator resumes and suspensions (`protocol_family::GENERATOR`).

use super::super::*;

/// The miss body of a record of this family with static parameters `parameters`
/// (`protocol_row::<H, OP, ACC, { protocol_family::GENERATOR | parameters }>`).
pub(in crate::vm::dispatch_fast) const fn generator_miss_of<H: Host>(parameters: u8) -> Handler {
    match parameters {
        _ => lane_general_step::<H>,
    }
}
