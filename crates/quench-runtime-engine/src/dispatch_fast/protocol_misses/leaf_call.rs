//! Protocol misses of leaf, Math and builtin native calls (`protocol_family::LEAF_CALL`).
//!
//! The parameters (low byte of `protocol_row`'s `MISS`) are a kind in the top two bits and a
//! payload in the rest: a leaf call's payload is its leaf form (`leaf << 3`) and argument count,
//! a builtin or Math call's is its row, a Reflect.get call's is none.

use super::super::*;
use super::super::leaf_calls::script_call_leaf_miss;
use super::super::native_calls::{builtin_miss_of, math_miss_of, reflect_get_miss};

/// The kind of a parameter byte: a leaf call (`leaf_parameters`).
const KIND_MASK: u8 = 0xc0;
const LEAF_KIND: u8 = 0;
/// A builtin native call of `builtin_miss_of`'s row.
pub(in crate::dispatch_fast) const BUILTIN_KIND: u8 = 1 << 6;
/// A Math call of `math_miss_of`'s row.
pub(in crate::dispatch_fast) const MATH_KIND: u8 = 2 << 6;
/// A `Reflect.get` call (`reflect_get_miss`).
pub(in crate::dispatch_fast) const REFLECT_GET_KIND: u8 = 3 << 6;
const PAYLOAD_MASK: u8 = !KIND_MASK;

/// The parameters of a leaf call of `leaf` form (`PLAIN_LEAF`..`THIS_LEAF`) and argument count
/// `argc` (at most `FIXED_ARGUMENT_COPIES`).
pub(in crate::dispatch_fast) const fn leaf_parameters(leaf: u8, argc: u8) -> u8 {
    LEAF_KIND | (leaf << 3) | argc
}

/// The leaf form and argument count of a leaf call's `parameters` (`leaf_parameters`); `None` for
/// another kind. The lane frame's leaf call (`LaneFrame::call_entry`) reads its form from here.
pub(in crate::dispatch_fast) const fn leaf_of(parameters: u8) -> Option<(u8, u16)> {
    if parameters & KIND_MASK != LEAF_KIND {
        return None;
    }
    let payload = parameters & PAYLOAD_MASK;
    Some((payload >> 3, (payload & 7) as u16))
}

/// The miss body of a record of this family with static parameters `parameters`
/// (`protocol_row::<OP, ACC, { protocol_family::LEAF_CALL | parameters }>`).
pub(in crate::dispatch_fast) const fn leaf_call_miss_of(parameters: u8) -> Handler {
    let payload = parameters & PAYLOAD_MASK;
    match parameters & KIND_MASK {
        BUILTIN_KIND => builtin_miss_of(payload),
        MATH_KIND => math_miss_of(payload),
        REFLECT_GET_KIND => reflect_get_miss,
        _ => leaf_miss_of_form(payload),
    }
}

/// The leaf call body of a leaf-form payload (`leaf_parameters`).
const fn leaf_miss_of_form(payload: u8) -> Handler {
    match (payload >> 3, payload & 7) {
        (PLAIN_LEAF, 0) => script_call_leaf_miss::<PLAIN_LEAF, 0> as Handler,
        (PLAIN_LEAF, 1) => script_call_leaf_miss::<PLAIN_LEAF, 1> as Handler,
        (PLAIN_LEAF, 2) => script_call_leaf_miss::<PLAIN_LEAF, 2> as Handler,
        (PLAIN_LEAF, 3) => script_call_leaf_miss::<PLAIN_LEAF, 3> as Handler,
        (PLAIN_LEAF, 4) => script_call_leaf_miss::<PLAIN_LEAF, 4> as Handler,
        (CAPTURE_LEAF, 0) => script_call_leaf_miss::<CAPTURE_LEAF, 0> as Handler,
        (CAPTURE_LEAF, 1) => script_call_leaf_miss::<CAPTURE_LEAF, 1> as Handler,
        (CAPTURE_LEAF, 2) => script_call_leaf_miss::<CAPTURE_LEAF, 2> as Handler,
        (CAPTURE_LEAF, 3) => script_call_leaf_miss::<CAPTURE_LEAF, 3> as Handler,
        (CAPTURE_LEAF, 4) => script_call_leaf_miss::<CAPTURE_LEAF, 4> as Handler,
        (CONTEXT_LEAF, 0) => script_call_leaf_miss::<CONTEXT_LEAF, 0> as Handler,
        (CONTEXT_LEAF, 1) => script_call_leaf_miss::<CONTEXT_LEAF, 1> as Handler,
        (CONTEXT_LEAF, 2) => script_call_leaf_miss::<CONTEXT_LEAF, 2> as Handler,
        (CONTEXT_LEAF, 3) => script_call_leaf_miss::<CONTEXT_LEAF, 3> as Handler,
        (CONTEXT_LEAF, 4) => script_call_leaf_miss::<CONTEXT_LEAF, 4> as Handler,
        (THIS_LEAF, 0) => script_call_leaf_miss::<THIS_LEAF, 0> as Handler,
        (THIS_LEAF, 1) => script_call_leaf_miss::<THIS_LEAF, 1> as Handler,
        (THIS_LEAF, 2) => script_call_leaf_miss::<THIS_LEAF, 2> as Handler,
        (THIS_LEAF, 3) => script_call_leaf_miss::<THIS_LEAF, 3> as Handler,
        (THIS_LEAF, 4) => script_call_leaf_miss::<THIS_LEAF, 4> as Handler,
        _ => script_call_full_miss,
    }
}
