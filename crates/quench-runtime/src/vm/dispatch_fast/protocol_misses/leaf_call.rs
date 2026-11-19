//! Protocol misses of leaf calls (`protocol_family::LEAF_CALL`).
//!
//! The parameters (low byte of `protocol_row`'s `MISS`) are a kind in the top two bits and a
//! payload in the rest: a leaf call's kind is 0 and its payload its leaf form (`leaf << 3`) and
//! argument count. The builtin, Math and `Reflect.get` kinds of the Derive thread's table keep
//! their own handlers on this branch, so only the leaf kind is named here.

use super::super::*;
use super::super::leaf_calls::script_call_leaf_miss;

/// The kind of a parameter byte: a leaf call (`leaf_parameters`).
const KIND_MASK: u8 = 0xc0;
const LEAF_KIND: u8 = 0;
const PAYLOAD_MASK: u8 = !KIND_MASK;

/// The parameters of a leaf call of `leaf` form (`PLAIN_LEAF`..`THIS_LEAF`) and argument count
/// `argc` (at most `FIXED_ARGUMENT_COPIES`).
pub(in crate::vm::dispatch_fast) const fn leaf_parameters(leaf: u8, argc: u8) -> u8 {
    LEAF_KIND | (leaf << 3) | argc
}

/// The leaf form and argument count of a leaf call's `parameters` (`leaf_parameters`); `None` for
/// another kind. The lane frame's leaf call (`LaneFrame::call_entry`) reads its form from here.
pub(in crate::vm::dispatch_fast) const fn leaf_of(parameters: u8) -> Option<(u8, u16)> {
    if parameters & KIND_MASK != LEAF_KIND {
        return None;
    }
    let payload = parameters & PAYLOAD_MASK;
    Some((payload >> 3, (payload & 7) as u16))
}

/// The miss body of a record of this family with static parameters `parameters`
/// (`protocol_row::<H, OP, ACC, { protocol_family::LEAF_CALL | parameters }>`): the proved leaf call
/// of its form and count (`script_call_leaf_miss`), which proves the link where it is not.
pub(in crate::vm::dispatch_fast) const fn leaf_call_miss_of<H: Host>(parameters: u8) -> Handler {
    match leaf_of(parameters) {
        Some((PLAIN_LEAF, 0)) => script_call_leaf_miss::<H, PLAIN_LEAF, 0> as Handler,
        Some((PLAIN_LEAF, 1)) => script_call_leaf_miss::<H, PLAIN_LEAF, 1> as Handler,
        Some((PLAIN_LEAF, 2)) => script_call_leaf_miss::<H, PLAIN_LEAF, 2> as Handler,
        Some((PLAIN_LEAF, 3)) => script_call_leaf_miss::<H, PLAIN_LEAF, 3> as Handler,
        Some((PLAIN_LEAF, 4)) => script_call_leaf_miss::<H, PLAIN_LEAF, 4> as Handler,
        Some((CAPTURE_LEAF, 0)) => script_call_leaf_miss::<H, CAPTURE_LEAF, 0> as Handler,
        Some((CAPTURE_LEAF, 1)) => script_call_leaf_miss::<H, CAPTURE_LEAF, 1> as Handler,
        Some((CAPTURE_LEAF, 2)) => script_call_leaf_miss::<H, CAPTURE_LEAF, 2> as Handler,
        Some((CAPTURE_LEAF, 3)) => script_call_leaf_miss::<H, CAPTURE_LEAF, 3> as Handler,
        Some((CAPTURE_LEAF, 4)) => script_call_leaf_miss::<H, CAPTURE_LEAF, 4> as Handler,
        Some((CONTEXT_LEAF, 0)) => script_call_leaf_miss::<H, CONTEXT_LEAF, 0> as Handler,
        Some((CONTEXT_LEAF, 1)) => script_call_leaf_miss::<H, CONTEXT_LEAF, 1> as Handler,
        Some((CONTEXT_LEAF, 2)) => script_call_leaf_miss::<H, CONTEXT_LEAF, 2> as Handler,
        Some((CONTEXT_LEAF, 3)) => script_call_leaf_miss::<H, CONTEXT_LEAF, 3> as Handler,
        Some((CONTEXT_LEAF, 4)) => script_call_leaf_miss::<H, CONTEXT_LEAF, 4> as Handler,
        Some((THIS_LEAF, 0)) => script_call_leaf_miss::<H, THIS_LEAF, 0> as Handler,
        Some((THIS_LEAF, 1)) => script_call_leaf_miss::<H, THIS_LEAF, 1> as Handler,
        Some((THIS_LEAF, 2)) => script_call_leaf_miss::<H, THIS_LEAF, 2> as Handler,
        Some((THIS_LEAF, 3)) => script_call_leaf_miss::<H, THIS_LEAF, 3> as Handler,
        Some((THIS_LEAF, 4)) => script_call_leaf_miss::<H, THIS_LEAF, 4> as Handler,
        _ => script_call_full_miss::<H>,
    }
}
