//! A method read through a prototype hint (`Vm::holder_hint_hit`) as a staged row: a receiver
//! of one exact shape and prototype link, under a watch on the prototype's values, reads the
//! value the hint recorded. The guard word and value are per-site operands; the watch is an
//! assumption, tested by the interpreter and proved once by a specializer.


/// The row is defined with the inline caches' ops (`IcOp::Held`), whose home is `ic`.
pub(crate) use super::ic::holder_read;

/// The value a method read of a string receiver finds on `String.prototype`: every direct string
/// has the one prototype, so the family test replaces the shape and link guards, and the watch on
/// the prototype's values is the assumption. `None` (leaving at `miss`) for any other receiver or
/// once the watch is retired.
// Read by the u-jit and b-jit projections of GetField on string receivers (threads 13 and 7).

/// The interpreter's reading of `holder_read`.
///
/// # Safety
/// `watch` is the address of a live watch (`Rc::as_ptr` of a hint's), which outlives the call.
#[inline(always)]
pub(crate) unsafe fn concrete_holder_read(record: &crate::heap::Object, guard: u64,
    watch: *const crate::code_dependencies::PublishedCode<()>, value: crate::Value) -> Option<crate::Value> {
    let mut stage = super::Concrete::new(std::ptr::null());
    holder_read(&mut stage, record as *const crate::heap::Object as u64, guard, watch as u64, value.to_bits(), ())
        .map(crate::Value::from_bits)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::staged::stage::Stage;
    use crate::heap::{Heap, Object, ProtoLink};
    use crate::code_dependencies::PublishedCode;
    use crate::Value;
    use std::rc::Rc;

    #[test]
    fn a_holder_read_needs_the_shape_the_link_and_the_watch() {
        let mut heap = Heap::new();
        heap.register_property_shape(1, 1, None);
        let prototype = heap.alloc_object(Object::new(Value::NULL));
        let object = heap.alloc_object(Object::new(prototype));
        heap.property_add(object, Value::integer(1), 1);
        let record = heap.object(object).unwrap();
        let guard = record.guard_word();
        let header = crate::heap::layouts::word(guard as u32);
        assert_eq!(guard as u32, record.hint_shape());
        assert_eq!(header as u32 & !crate::staged::ic::FORM_BITS, record.exact_shape());
        assert_eq!((header >> u32::BITS) as u32 & ProtoLink::of(prototype).bits(), ProtoLink::of(prototype).bits());
        let watch = Rc::new(PublishedCode::new(()));
        let hint = Value::integer(42);
        let read = |guard, watch: &Rc<PublishedCode<()>>| unsafe { concrete_holder_read(record, guard, Rc::as_ptr(watch), hint) };
        assert_eq!(read(guard, &watch), Some(hint));
        assert_eq!(read(guard + (1 << u32::BITS), &watch), None, "another shape");
        assert_eq!(read(guard ^ 1, &watch), None, "another prototype");
        watch.retire();
        assert_eq!(read(guard, &watch), None, "a retired watch");
    }

    /// The row's operands are holes in a fixed order: guard, watch, value.
    #[test]
    fn the_holder_read_leaves_its_operands_as_holes() {
        let run = |guard, watch, value| {
            let mut stage = crate::staged::trace::Trace::default();
            let record = stage.constant(0);
            holder_read(&mut stage, record, guard, watch, value, ());
            (stage.ops, stage.holes)
        };
        let (first, holes) = run(3, 100, 7);
        let (second, other) = run(4, 200, 8);
        assert_eq!(first, second);
        assert_eq!(holes, [3, 100, 7]);
        assert_eq!(other, [4, 200, 8]);
    }
}
