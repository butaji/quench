//! The staged `typeof`: the name index of a value its own word decides, written once for the
//! interpreter and every tier.

use super::*;

use super::value::{TYPEOF_BOOLEAN, TYPEOF_NUMBER, TYPEOF_OBJECT, TYPEOF_UNDEFINED, typeof_classes};

/// The `typeof` name index (int32) of `source`, into `destination`, where the value's own
/// word decides it: a Number, a Boolean, `null`, `undefined` or the deleted marker. A heap
/// value's cell decides its name (a string, a function, the IsHTMLDDA object), so it leaves
/// at the frame's miss edge.
#[inline(always)]
pub(super) fn typeof_index<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    destination: Register, source: Register) -> Step<S::Word, F::Control> {
    let miss = frame.miss();
    let bits = frame.tagged(stage, source);
    let heap = super::value::is_heap(stage, bits);
    if stage.guard_false(heap, miss).is_none() {
        return Step::Unresolved;
    }
    let name = immediate_name(stage, bits);
    let result = frame.int_result(stage, name);
    let fact = frame.int_fact();
    frame.write(stage, destination, result, fact);
    Step::Value(result)
}

/// The name index of the immediate `bits`: every immediate that is no Number,
/// Boolean or `null` is `undefined` or the deleted marker, whose name it is.
#[inline(always)]
fn immediate_name<S: NumericStage>(stage: &mut S, bits: S::Word) -> S::Word {
    let (number, boolean, null) = typeof_classes(stage, bits);
    let mut name = stage.i32_constant(TYPEOF_UNDEFINED as i32);
    for (holds, index) in [(null, TYPEOF_OBJECT), (boolean, TYPEOF_BOOLEAN), (number, TYPEOF_NUMBER)] {
        let named = stage.i32_constant(index as i32);
        name = stage.select(holds, named, name);
    }
    name
}
