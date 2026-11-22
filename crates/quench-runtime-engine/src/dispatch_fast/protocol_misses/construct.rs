//! Protocol misses of constructions (`protocol_family::CONSTRUCT`).

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::super::*;

/// Parameter codes of the construction records (`Op::Construct`).
/// `new C(...)` of a frameless target, the first record the construction runs (`script_construct_miss`).
pub(crate) const SCRIPT_CONSTRUCT: u8 = 0;
/// A planned construction whose record completes its result in place (`planned_construct_miss`).
pub(crate) const PLANNED_CONSTRUCT: u8 = 1;
/// A planned construction whose result is returned (`planned_construct_returned_miss`).
pub(crate) const PLANNED_CONSTRUCT_RETURNED: u8 = 2;
/// A construction of a target that keeps its passed values (`script_construct_keeping_miss`).
pub(crate) const SCRIPT_CONSTRUCT_KEEPING: u8 = 3;

/// The miss body of a record of this family with static parameters `parameters`
/// (`protocol_row::<OP, ACC, { protocol_family::CONSTRUCT | parameters }>`).
pub(crate) const fn construct_miss_of(parameters: u8) -> Handler {
    match parameters {
        SCRIPT_CONSTRUCT => super::super::script_construct_miss as Handler,
        PLANNED_CONSTRUCT => super::super::planned_construct_miss as Handler,
        PLANNED_CONSTRUCT_RETURNED => super::super::planned_construct_returned_miss as Handler,
        SCRIPT_CONSTRUCT_KEEPING => super::super::script_construct_keeping_miss as Handler,
        _ => general_step_in_lane,
    }
}
