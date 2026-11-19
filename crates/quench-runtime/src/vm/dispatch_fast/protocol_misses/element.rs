//! Protocol misses of keyed reads and writes, deletes and named reads past their rows (`protocol_family::ELEMENT`).

use super::super::*;

/// The miss body of a record of this family with static parameters `parameters`
/// (`protocol_row::<H, OP, ACC, { protocol_family::ELEMENT | parameters }>`).
pub(in crate::vm::dispatch_fast) const fn element_miss_of<H: Host>(parameters: u8) -> Handler {
    match parameters {
        _ => lane_general_step::<H>,
    }
}
