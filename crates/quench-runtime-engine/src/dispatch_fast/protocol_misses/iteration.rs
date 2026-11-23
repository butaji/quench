//! Protocol misses of iterator and generator steps (`protocol_family::ITERATION`).

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::super::*;

/// `iterator_step_miss`.
pub(crate) const STEP: u8 = 0;
/// `iterator_step_reads_miss::<false>`: a for-of step over an array whose values the loop reads
/// emitted.
pub(crate) const READS_EMITTED: u8 = 1;
/// `iterator_step_reads_miss::<true>`: a for-of step over an array whose values the loop reads
/// threaded.
pub(crate) const READS_THREADED: u8 = 2;
/// `iterator_step_binding_miss`: a destructuring step.
pub(crate) const BINDING: u8 = 3;

/// The miss body of a record of this family with static parameters `parameters`
/// (`protocol_row!(OP, ACC, { protocol_family::ITERATION | parameters })`).
pub(crate) const fn iteration_miss_of(parameters: u8) -> Handler {
    match parameters {
        STEP => iterator_step_miss as Handler,
        READS_EMITTED => iterator_step_reads_miss::<false> as Handler,
        READS_THREADED => iterator_step_reads_miss::<true> as Handler,
        BINDING => iterator_step_binding_miss as Handler,
        _ => general_step_in_lane,
    }
}
