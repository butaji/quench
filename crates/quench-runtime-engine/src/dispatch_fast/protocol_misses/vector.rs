//! Protocol misses of vector (SIMD) records (`protocol_family::VECTOR`).

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::super::*;
#[allow(unused_imports)] use quench_runtime::staged::glue_prelude::*;

/// The miss body of a record of this family with static parameters `parameters`
/// (`protocol_row!(OP, ACC, { protocol_family::VECTOR | parameters })`).
pub(crate) const fn vector_miss_of(parameters: u8) -> Handler {
    match parameters {
        _ => general_step_in_lane,
    }
}
