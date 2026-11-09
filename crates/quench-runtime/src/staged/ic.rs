//! Inline caches as data (CacheIR): a property read a site proved, as a short list of ops
//! that every tier runs from this one definition, the interpreter through `Concrete` and the
//! native tiers through `Emit`. A tier records a site's state as its op list instead of
//! writing the read again.

use super::Stage;
use super::object::{arena_run, extras_pointer, has_extras, inline_storage, object_record_at, proto_link, proto_object_record_id};
use crate::heap::NATIVE_OBJECT_LAYOUT as OBJECT;

/// One step of a proved property read, over the record the read has reached.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum IcOp {
    /// The record's exact shape (`Object::exact_shape`) is this one, else the read misses.
    GuardShape(u32),
    /// The read moves to the record's prototype, an object record, else misses.
    Prototype,
    /// The value at `slot` of a prototype record, which keeps its values in the arena past its
    /// extras (`Object::extras_storage_start`), else the read misses; it ends the read.
    LoadPrototypeSlot(u16),
}

/// The value `ops` read from the object record at `record`, prototypes found in the record
/// table at `records`, or `miss` where a guard fails. `None` where the stage leaves at
/// `miss`, or the list ends before a load.
pub(crate) fn run<S: Stage>(stage: &mut S, ops: &[IcOp], mut record: S::Word, records: S::Word, miss: S::Miss) -> Option<S::Word> {
    for op in ops {
        match *op {
            IcOp::GuardShape(shape) => {
                let exact = stage.load_u32(record, OBJECT.auxiliary_offset as i32);
                let same = stage.eq_imm(exact, u64::from(shape));
                stage.guard(same, miss)?;
            }
            IcOp::Prototype => {
                let link = proto_link(stage, record);
                let id = proto_object_record_id(stage, link, miss)?;
                record = object_record_at(stage, records, id);
            }
            IcOp::LoadPrototypeSlot(slot) => return prototype_slot(stage, record, slot, miss),
        }
    }
    None
}

/// `IcOp::LoadPrototypeSlot` of the record at `record`.
fn prototype_slot<S: Stage>(stage: &mut S, record: S::Word, slot: u16, miss: S::Miss) -> Option<S::Word> {
    let inline = inline_storage(stage, record);
    let in_arena = stage.not(inline);
    stage.guard(in_arena, miss)?;
    let payload = stage.load_u64(record, OBJECT.payload_offset as i32);
    let extras = has_extras(stage, payload);
    stage.guard(extras, miss)?;
    let pointer = extras_pointer(stage, payload);
    let present = stage.ne_imm(pointer, 0);
    stage.guard(present, miss)?;
    let start = stage.load_u32(pointer, OBJECT.extras_arena_start_offset as i32);
    let run = arena_run(stage, start);
    Some(stage.load_u64(run, i32::from(slot) * OBJECT.property_value_size as i32))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::heap::{Cell, Heap, Object};
    use crate::staged::Concrete;
    use crate::Value;

    /// The interpreter's reading of `run`: `None` for a miss.
    fn concrete(heap: &Heap, ops: &[IcOp], object: Value) -> Option<Value> {
        let mut stage = Concrete::new(heap.property_values_base());
        let record = heap.object(object)? as *const Object as u64;
        let records = heap.object_record_base()? as u64;
        run(&mut stage, ops, record, records, ()).map(Value::from_bits)
    }

    /// A prototype's read guards the receiver's and the prototype's shapes; a receiver of
    /// another shape, a prototype of another, or a record with no object prototype misses.
    #[test]
    fn an_op_list_reads_a_prototype_slot_and_misses_past_its_guards() {
        let mut heap = Heap::new();
        for shape in 1..=2_u32 {
            heap.register_property_shape(shape, shape as usize, None);
        }
        let prototype = heap.alloc(Cell::Object(Object::new(Value::NULL)));
        heap.property_add(prototype, Value::number(1.5), 1);
        heap.object_record_mut(prototype).unwrap().mark_as_prototype();
        let object = heap.alloc(Cell::Object(Object::new(prototype)));
        heap.property_add(object, Value::number(7.0), 1);
        heap.property_add(object, Value::number(8.0), 2);
        let (own, holder) = (heap.object(object).unwrap().exact_shape(), heap.object(prototype).unwrap().exact_shape());
        let read = |receiver, holder| [IcOp::GuardShape(receiver), IcOp::Prototype, IcOp::GuardShape(holder), IcOp::LoadPrototypeSlot(0)];
        assert_eq!(concrete(&heap, &read(own, holder), object), Some(Value::number(1.5)));
        assert_eq!(concrete(&heap, &read(holder, holder), object), None);
        assert_eq!(concrete(&heap, &read(own, own), object), None);
        assert_eq!(concrete(&heap, &read(holder, holder), prototype), None);
    }
}
