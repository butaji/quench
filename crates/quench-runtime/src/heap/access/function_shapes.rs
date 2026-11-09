//! Which shapes live function records hold, for native plans that must not take a read
//! of such a shape for a direct object's.

use crate::bitset::BitSet;
use crate::heap::Heap;

impl Heap {
    /// The shapes live function records hold now, by shape id: a field read planned for one
    /// of them proves a readable record, not a direct object (`NativeFieldLoad::function_shape`).
    pub(crate) fn function_shapes(&self) -> BitSet {
        let mut shapes = BitSet::new(0);
        self.function_records.for_each_live(|_, function| { shapes.insert_growing(function.object.shape() as usize); });
        shapes
    }
}

#[cfg(test)]
mod tests {
    use crate::heap::{Cell, FunctionKind, FunctionRecord, Heap, Object};
    use crate::Value;

    #[test]
    fn function_shapes_name_the_shapes_of_live_function_records_only() {
        let mut heap = Heap::new();
        heap.track_slot_representations(&[(0, None), (1, None), (2, Some((1, 1)))]);
        heap.alloc_object_with_properties(Value::NULL, 1, &[Value::integer(1)]);
        let (properties, inline) = heap.initial_object_storage(2, &[Value::integer(1), Value::integer(2)], false);
        let object = Object::with_property_storage(Value::NULL, properties, inline);
        heap.alloc(Cell::Function(Box::new(FunctionRecord {
            object, realm: Value::NULL, env: Value::NULL, kind: FunctionKind::User(crate::vm::ProgramId::MAIN, 0),
        })));
        let shapes = heap.function_shapes();
        assert!(shapes.contains(2), "the function record's shape");
        assert!(!shapes.contains(1), "an object record's shape");
    }
}
