//! A construction through an initializer as a staged row: the receiver the plan allocates is
//! `this` of the initializer, which runs through its entry cell as an ordinary call separate from
//! the callee closure; the result is the receiver unless the initializer returned an object.
//! The allocation and the call are the stage's: the interpreter allocates from the plan and calls
//! through its lane, a native projection bumps the heap and calls the cell's entry. The row owns
//! the result rule alone, so no projection restates it.

// The native projection (thread 13) is the consumer; until it lands the row runs under the tests.
#![allow(dead_code)]

use super::value::is_direct_object;
use super::Stage;

/// What the initializer call leaves: its status (zero where it returned) and its value.
pub(crate) struct Returned<W> {
    pub(crate) status: W,
    pub(crate) value: W,
}

/// The value of `new C(...)`: allocates the receiver with `allocate`, runs the initializer with
/// `initialize(receiver)`, and yields the receiver unless the initializer returned an object.
/// Leaving at `miss` where the initializer did not return (a throw or an exit) or returned some
/// other value than undefined or a direct object, which the general construction decides.
#[inline(always)]
pub(crate) fn construct<S: Stage>(stage: &mut S, miss: S::Miss, allocate: impl FnOnce(&mut S) -> S::Word,
    initialize: impl FnOnce(&mut S, S::Word) -> Returned<S::Word>) -> Option<S::Word> {
    let receiver = allocate(stage);
    let Returned { status, value } = initialize(stage, receiver);
    let returned = stage.eq_imm(status, 0u32);
    stage.guard(returned, miss)?;
    let object = is_direct_object(stage, value);
    let undefined = stage.eq_imm(value, crate::Value::UNDEFINED.to_bits());
    let known = stage.any(object, undefined);
    stage.guard(known, miss)?;
    Some(stage.select(object, value, receiver))
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
        assert_eq!(run(0, Value::integer(1)), None, "another value is the general path's");
    }

    #[test]
    fn the_row_has_no_per_site_hole() {
        let mut trace = crate::staged::trace::Trace::default();
        construct(&mut trace, (), |stage| stage.constant(0), |stage, receiver| Returned { status: stage.constant(1), value: receiver });
        assert!(trace.holes.is_empty());
    }
}
