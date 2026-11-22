//! Protocol misses of vector regions (`protocol_family::VECTOR`).

use super::super::*;

/// The miss body of a record of this family with static parameters `parameters`
/// (`protocol_row::<OP, ACC, { protocol_family::VECTOR | parameters }>`).
pub(crate) const fn vector_miss_of(parameters: u8) -> Handler {
    match parameters {
        _ => general_step_in_lane,
    }
}
