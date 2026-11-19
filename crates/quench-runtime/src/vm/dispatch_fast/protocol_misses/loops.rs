//! Protocol misses of loop headers and numeric protocol records (`protocol_family::LOOP`).

use super::super::*;

/// `f64_sqrt_miss::<ACC_NONE>`.
pub(in crate::vm::dispatch_fast) const F64_SQRT_NONE: u8 = 0;
/// `f64_sqrt_miss::<ACC_A>`.
pub(in crate::vm::dispatch_fast) const F64_SQRT_A: u8 = 1;
/// `f64_sqrt_miss::<ACC_B>`.
pub(in crate::vm::dispatch_fast) const F64_SQRT_B: u8 = 2;
/// `f64_sqrt_miss::<ACC_C>`.
pub(in crate::vm::dispatch_fast) const F64_SQRT_C: u8 = 3;
/// `f64_sqrt_miss::<ACC_PAIR>`.
pub(in crate::vm::dispatch_fast) const F64_SQRT_PAIR: u8 = 4;

/// The miss body of a record of this family with static parameters `parameters`
/// (`protocol_row::<H, OP, ACC, { protocol_family::LOOP | parameters }>`).
pub(in crate::vm::dispatch_fast) const fn loop_miss_of<H: Host>(parameters: u8) -> Handler {
    match parameters {
        F64_SQRT_NONE => f64_sqrt_miss::<H, ACC_NONE> as Handler,
        F64_SQRT_A => f64_sqrt_miss::<H, ACC_A> as Handler,
        F64_SQRT_B => f64_sqrt_miss::<H, ACC_B> as Handler,
        F64_SQRT_C => f64_sqrt_miss::<H, ACC_C> as Handler,
        F64_SQRT_PAIR => f64_sqrt_miss::<H, ACC_PAIR> as Handler,
        _ => lane_general_step::<H>,
    }
}
