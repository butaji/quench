//! A method read through a prototype hint (`Vm::holder_hint_hit`) as a staged row: a receiver
//! of one exact shape and prototype link, under a watch on the prototype's values, reads the
//! value the hint recorded. The guard word and value are per-site operands; the watch is an
//! assumption, tested by the interpreter and proved once by a specializer.

use super::Stage;

/// The value the hint `(guard, watch, value)` records for the object part at `record`: `None`
/// (leaving at `miss`) where the record's layout (`Object::guard_word`: the interned pair of
/// its whole auxiliary word, the exact shape with its storage form, and its prototype link and
/// flags) differs, or the watch no longer holds. One compare of the layout id proves the shape
/// and the prototype together.
#[inline(always)]
pub(crate) fn holder_read<S: Stage>(stage: &mut S, record: S::Word, guard: u64, watch: u64, value: u64,
    miss: S::Miss) -> Option<S::Word> {
    let words = stage.load_u32(super::region::Region::ShapeWord.mutable(), record,
        crate::heap::NATIVE_OBJECT_LAYOUT.layout_offset as i32);
    let guard = stage.site(guard);
    let same = stage.eq_imm(words, guard);
    stage.guard(same, miss)?;
    let watch = stage.site(watch);
    let holds = stage.assumption(watch);
    stage.guard(holds, miss)?;
    let zero = stage.constant(0);
    let value = stage.site(value);
    Some(stage.add_imm(zero, value))
}

/// The value a method read of a string receiver finds on `String.prototype`: every direct string
/// has the one prototype, so the family test replaces the shape and link guards, and the watch on
/// the prototype's values is the assumption. `None` (leaving at `miss`) for any other receiver or
/// once the watch is retired.
// Read by the u-jit and b-jit projections of GetField on string receivers (threads 13 and 7).
#[allow(dead_code)]
#[inline(always)]
pub(crate) fn string_holder_read<S: Stage>(stage: &mut S, receiver: S::Word, watch: u64, value: u64, miss: S::Miss)
    -> Option<S::Word> {
    let string = super::value::is_direct_string(stage, receiver);
    stage.guard(string, miss)?;
    let watch = stage.site(watch);
    let holds = stage.assumption(watch);
    stage.guard(holds, miss)?;
    let zero = stage.constant(0);
    let value = stage.site(value);
    Some(stage.add_imm(zero, value))
}

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
    use crate::heap::{Cell, Heap, Object, ProtoLink};
    use crate::code_dependencies::PublishedCode;
    use crate::Value;
    use std::rc::Rc;

    #[test]
    fn a_holder_read_needs_the_shape_the_link_and_the_watch() {
        let mut heap = Heap::new();
        heap.register_property_shape(1, 1, None);
        let prototype = heap.alloc(Cell::Object(Object::new(Value::NULL)));
        let object = heap.alloc(Cell::Object(Object::new(prototype)));
        heap.property_add(object, Value::integer(1), 1);
        let record = heap.object(object).unwrap();
        let guard = record.guard_word();
        let header = crate::heap::layouts::word(guard as u32);
        assert_eq!(header as u32, record.hint_shape());
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

    #[test]
    fn a_string_receiver_reads_its_prototype_method_under_the_watch() {
        let watch = Rc::new(PublishedCode::new(()));
        let method = Value::integer(9);
        let read = |receiver: Value| {
            let mut stage = crate::staged::Concrete::new(std::ptr::null());
            string_holder_read(&mut stage, receiver.to_bits(), Rc::as_ptr(&watch) as u64, method.to_bits(), ()).map(Value::from_bits)
        };
        let string = Value::direct_string(3);
        assert_eq!(read(string), Some(method));
        assert_eq!(read(Value::integer(1)), None, "not a string");
        let mut trace = crate::staged::trace::Trace::default();
        let receiver = trace.constant(0);
        string_holder_read(&mut trace, receiver, 1, 2, ());
        assert_eq!(trace.holes, [1, 2], "the watch and the value are the only operands");
    }
}
