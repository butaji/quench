//! A construction through an initializer as a staged row: the receiver the plan allocates is
//! `this` of the initializer, which runs through its entry cell as an ordinary call separate from
//! the callee closure; the result is the receiver unless the initializer returned an object.
//! The allocation and the call are the stage's: the interpreter allocates from the plan and calls
//! through its lane, a native projection bumps the heap and calls the cell's entry. The row owns
//! the result rule alone, so no projection restates it.

// The native projection (thread 13) is the consumer; until it lands the row runs under the tests.
#![allow(dead_code)]

use super::value::{is_direct_array, is_direct_function, is_direct_object};
use crate::value::ValueLayout;
use super::Stage;

/// What the initializer call leaves: its status (zero where it returned) and its value.
pub(crate) struct Returned<W> {
    pub(crate) status: W,
    pub(crate) value: W,
}

/// The result of a construction whose initializer returned `value`, as far as the word decides it:
/// the value where it is a direct object, array or function, the `receiver` where it is no heap
/// reference at all; `None` (leaving at `miss`) for any other heap reference, whose cell decides
/// (an error record, a Wasm GC object, a string).
#[inline(always)]
pub(crate) fn construct_result<S: Stage>(stage: &mut S, value: S::Word, receiver: impl FnOnce(&mut S) -> S::Word, miss: S::Miss)
    -> Option<S::Word> {
    // Each arm runs only on its own path: a primitive (the common return) yields the receiver
    // after one tag test, a direct object after two, and only another heap reference reaches the rest.
    let tag = stage.and_imm(value, ValueLayout::TAG_MASK);
    let heap = stage.eq_imm(tag, ValueLayout::HEAP_TAG);
    stage.choose_leaving(heap, |stage| {
        let object = is_direct_object(stage, value);
        stage.choose_leaving(object, |_| Some(value), |stage| {
            let array = is_direct_array(stage, value);
            let function = is_direct_function(stage, value);
            let record = stage.any(array, function);
            stage.guard(record, miss)?;
            Some(value)
        })
    }, |stage| Some(receiver(stage)))
}

/// The value of `new C(...)`: allocates the receiver with `allocate`, runs the initializer with
/// `initialize(receiver)`, and yields the receiver unless the initializer returned an object
/// (`construct_result`). Leaving at `miss` where the initializer did not return (a throw or an exit).
#[inline(always)]
pub(crate) fn construct<S: Stage>(stage: &mut S, miss: S::Miss, allocate: impl FnOnce(&mut S) -> S::Word,
    initialize: impl FnOnce(&mut S, S::Word) -> Returned<S::Word>) -> Option<S::Word> {
    let receiver = allocate(stage);
    let Returned { status, value } = initialize(stage, receiver);
    let returned = stage.eq_imm(status, 0u32);
    stage.guard(returned, miss)?;
    construct_result(stage, value, |_| receiver, miss)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::staged::Concrete;
    use crate::Value;

    fn run(status: u64, returned: Value) -> Option<u64> {
        let mut stage = Concrete::new(std::ptr::null());
        let receiver = Value::integer(5).to_bits();
        construct(&mut stage, (), |stage| stage.constant(receiver), |stage, _| {
            Returned { status: stage.constant(status), value: stage.constant(returned.to_bits()) }
        })
    }

    #[test]
    fn the_receiver_is_the_result_unless_an_object_is_returned() {
        let mut heap = crate::heap::Heap::new();
        let object = heap.alloc(crate::heap::Cell::Object(crate::heap::Object::new(Value::NULL)));
        let receiver = Value::integer(5).to_bits();
        assert_eq!(run(0, Value::UNDEFINED), Some(receiver));
        assert_eq!(run(0, object), Some(object.to_bits()));
        assert_eq!(run(1, Value::UNDEFINED), None, "a throw leaves");
        assert_eq!(run(0, Value::integer(1)), Some(receiver), "a primitive yields the receiver");
        let string = heap.alloc(crate::heap::Cell::Error(Default::default()));
        assert_eq!(run(0, string), None, "another heap reference is the cell's to decide");
    }

    #[test]
    fn the_row_has_no_per_site_hole() {
        let mut trace = crate::staged::trace::Trace::default();
        construct(&mut trace, (), |stage| stage.constant(0), |stage, receiver| Returned { status: stage.constant(1), value: receiver });
        assert!(trace.holes.is_empty());
    }
}
