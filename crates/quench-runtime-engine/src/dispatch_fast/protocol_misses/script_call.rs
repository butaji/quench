//! Protocol misses of script calls (`protocol_family::SCRIPT_CALL`).

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::super::*;

// Parameter codes (the low byte of `protocol_family::SCRIPT_CALL | code`). The argument count of
// `script_call_miss` is in bits 0-2 (`ANY_ARGC` is 5), `PLAIN` is bit 3, and `RETURNS` (a returning
// call whose completion the row admits statically, `specialize_script_calls`) is bit 5.

/// `script_call_miss::<0, false>`.
pub(crate) const ARGC_0: u8 = 0;
/// `script_call_miss::<1, false>`.
pub(crate) const ARGC_1: u8 = 1;
/// `script_call_miss::<2, false>`.
pub(crate) const ARGC_2: u8 = 2;
/// `script_call_miss::<3, false>`.
pub(crate) const ARGC_3: u8 = 3;
/// `script_call_miss::<4, false>`.
pub(crate) const ARGC_4: u8 = 4;
/// `script_call_miss::<ANY_ARGC, false>`.
pub(crate) const ARGC_ANY: u8 = 5;
/// The `PLAIN` bit of `script_call_miss`.
pub(crate) const PLAIN: u8 = 8;
/// `script_call_miss::<0, true>`.
pub(crate) const PLAIN_ARGC_0: u8 = PLAIN | ARGC_0;
/// `script_call_miss::<1, true>`.
pub(crate) const PLAIN_ARGC_1: u8 = PLAIN | ARGC_1;
/// `script_call_miss::<2, true>`.
pub(crate) const PLAIN_ARGC_2: u8 = PLAIN | ARGC_2;
/// `script_call_miss::<3, true>`.
pub(crate) const PLAIN_ARGC_3: u8 = PLAIN | ARGC_3;
/// `script_call_miss::<4, true>`.
pub(crate) const PLAIN_ARGC_4: u8 = PLAIN | ARGC_4;
/// `script_call_miss::<ANY_ARGC, true>`.
pub(crate) const PLAIN_ARGC_ANY: u8 = PLAIN | ARGC_ANY;
/// The `RETURNS` bit of `script_call_miss`: the call returns its callee's completion, which the
/// row admits (`concrete_admits_returning_call`). Its miss is the same as without it.
pub(crate) const RETURNS: u8 = 32;
/// `script_call_full_miss`.
pub(crate) const FULL: u8 = 16;
/// `script_call_environment_miss`.
pub(crate) const ENVIRONMENT: u8 = 17;
/// `script_call_settles_miss`.
pub(crate) const SETTLES: u8 = 18;
/// `script_call_cross_miss`.
pub(crate) const CROSS: u8 = 19;
/// `script_call_cross_plain_miss::<0>`.
pub(crate) const CROSS_PLAIN_0: u8 = 20;
/// `script_call_cross_plain_miss::<1>`.
pub(crate) const CROSS_PLAIN_1: u8 = 21;
/// `script_call_cross_plain_miss::<2>`.
pub(crate) const CROSS_PLAIN_2: u8 = 22;
/// `script_call_cross_plain_miss::<3>`.
pub(crate) const CROSS_PLAIN_3: u8 = 23;
/// `script_call_cross_plain_miss::<4>`.
pub(crate) const CROSS_PLAIN_4: u8 = 24;

/// The miss body of a record of this family with static parameters `parameters`
/// (`protocol_row!(OP, ACC, { protocol_family::SCRIPT_CALL | parameters })`).
pub(crate) const fn script_call_miss_of(parameters: u8) -> Handler {
    match parameters & !RETURNS {
        ARGC_0 => script_call_miss::<0, false> as Handler,
        ARGC_1 => script_call_miss::<1, false> as Handler,
        ARGC_2 => script_call_miss::<2, false> as Handler,
        ARGC_3 => script_call_miss::<3, false> as Handler,
        ARGC_4 => script_call_miss::<4, false> as Handler,
        ARGC_ANY => script_call_miss::<ANY_ARGC, false> as Handler,
        PLAIN_ARGC_0 => script_call_miss::<0, true> as Handler,
        PLAIN_ARGC_1 => script_call_miss::<1, true> as Handler,
        PLAIN_ARGC_2 => script_call_miss::<2, true> as Handler,
        PLAIN_ARGC_3 => script_call_miss::<3, true> as Handler,
        PLAIN_ARGC_4 => script_call_miss::<4, true> as Handler,
        PLAIN_ARGC_ANY => script_call_miss::<ANY_ARGC, true> as Handler,
        FULL => script_call_full_miss as Handler,
        ENVIRONMENT => script_call_environment_miss as Handler,
        SETTLES => script_call_settles_miss as Handler,
        CROSS => script_call_cross_miss as Handler,
        CROSS_PLAIN_0 => script_call_cross_plain_miss::<0> as Handler,
        CROSS_PLAIN_1 => script_call_cross_plain_miss::<1> as Handler,
        CROSS_PLAIN_2 => script_call_cross_plain_miss::<2> as Handler,
        CROSS_PLAIN_3 => script_call_cross_plain_miss::<3> as Handler,
        CROSS_PLAIN_4 => script_call_cross_plain_miss::<4> as Handler,
        _ => general_step_in_lane,
    }
}
