//! `LaneStack::note_site_callee`'s history order.
use super::*;
use super::super::compiled::{CallTarget, CalleeBinding};

#[test]
fn a_call_sites_history_keeps_each_callee_once_in_first_seen_order() {
    // A callee met again leaves the record as it is; a further one evicts the oldest
    // (`LaneStack::note_site_callee`).
    let stack = LaneStack::default();
    let link = std::cell::Cell::new(NO_CALL_LINK);
    let target = |k: i32| CallTarget { callee: CalleeBinding::Identity(Value::integer(k)), function: k as u32 };
    let history = || {
        stack.call_sites.borrow()[&std::ptr::from_ref(&link).addr()].iter().map(|target| target.function).collect::<Vec<_>>()
    };
    for k in [1, 2, 3, 2, 2] {
        stack.note_site_callee(&link, target(k));
    }
    assert_eq!(history(), [1, 2, 3]);
    for k in [4, 1, 5] {
        stack.note_site_callee(&link, target(k));
    }
    assert_eq!(history(), [2, 3, 4, 5]);
}
